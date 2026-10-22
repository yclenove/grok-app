import { useCallback, useEffect, useRef, useState } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { sessionImportJobGet, sessionImportJobItems, type JobActionRequest, type JobRecord } from "@/lib/api/sessionMigration";
import { importErrorCode } from "@/lib/sessionImport/errorCopy";
import { nextJobCursorHistory } from "@/lib/sessionImport/jobCursor";
import { createImportRefreshQueue, type ImportRefreshQueue } from "@/lib/sessionImport/refreshQueue";

export function importJobActive(job: JobRecord | null): boolean {
  return !!job && ["queued", "running", "cancelling"].includes(job.state);
}

type JobView = { job: JobRecord | null; cursors: Array<string | null>; page: number; stale: boolean; owner: number };
const initialView = (): JobView => ({ job: null, cursors: [null], page: 0, stale: false, owner: 0 });
type JobOperation = (id: string, request: JobActionRequest) => Promise<JobRecord>;
type PendingAction = { jobId: string; operation: JobOperation; request: JobActionRequest };

/** Keep one Host page; cursor history contains no job items. Background reads
 * refresh that page, while adopting another job invalidates every old owner. */
export function useSessionImportJob(onError: (message: string) => void) {
  const [view, setView] = useState<JobView>(initialView);
  const [pageBusy, setPageBusy] = useState(false);
  const [mutationBusy, setMutationBusy] = useState(false);
  const current = useRef(view);
  const generation = useRef(0);
  const mounted = useRef(true);
  const readFlight = useRef<symbol | null>(null);
  const pageFlight = useRef<symbol | null>(null);
  const mutationFlight = useRef<symbol | null>(null);
  const pendingAction = useRef<PendingAction | null>(null);
  const queueRef = useRef<ImportRefreshQueue | null>(null);
  const errorRef = useRef(onError);
  errorRef.current = onError;
  const job = view.job;
  const jobId = job?.jobId;
  const active = importJobActive(job);

  const publish = useCallback((next: JobView) => {
    current.current = next;
    setView(next);
  }, []);

  // Adoption is an explicit new ownership boundary, including A -> B -> A.
  const setJob = useCallback((next: JobRecord | null) => {
    if (!mounted.current) return;
    generation.current += 1;
    queueRef.current?.reset();
    readFlight.current = null;
    pageFlight.current = null;
    mutationFlight.current = null;
    pendingAction.current = null;
    setPageBusy(false);
    setMutationBusy(false);
    publish({ ...initialView(), job: next, owner: generation.current });
  }, [publish]);

  const readPage = useCallback(async (target: Pick<JobView, "cursors" | "page">, manual = false) => {
    const old = current.current.job;
    if (!mounted.current || !old) return;
    if (manual ? pageFlight.current || mutationFlight.current : readFlight.current) return;
    const owner = generation.current;
    const token = Symbol("job-page");
    readFlight.current = token;
    if (manual) { pageFlight.current = token; setPageBusy(true); }
    const owns = () => mounted.current && owner === generation.current && readFlight.current === token;
    try {
      const cursor = target.cursors[target.page];
      const next = cursor == null
        ? await sessionImportJobGet(old.jobId)
        : await sessionImportJobItems(old.jobId, cursor, 50);
      if (!owns() || next.jobId !== old.jobId) return;
      const live = current.current.job;
      if (!live || next.stateVersion < live.stateVersion) return;
      publish({ job: next, cursors: target.cursors, page: target.page, stale: false, owner });
    } catch (error) {
      if (owns()) errorRef.current(String(error));
    } finally {
      if (owns()) readFlight.current = null;
      if (mounted.current && owner === generation.current && pageFlight.current === token) {
        pageFlight.current = null;
        setPageBusy(false);
      }
    }
  }, [publish]);

  const refreshJobItems = useCallback(() => readPage(current.current, true), [readPage]);
  const loadMoreJobItems = useCallback(() => {
    const latest = current.current;
    const cursor = latest.job?.nextItemCursor;
    if (!cursor) return Promise.resolve();
    return readPage({ page: latest.page + 1, cursors: nextJobCursorHistory(latest.cursors, latest.page, cursor) }, true);
  }, [readPage]);
  const previousJobItems = useCallback(() => {
    const latest = current.current;
    if (latest.page === 0) return Promise.resolve();
    return readPage({ page: latest.page - 1, cursors: latest.cursors }, true);
  }, [readPage]);

  const mutateJob = useCallback(async (operation: JobOperation) => {
    const original = current.current.job;
    if (!mounted.current || !original || mutationFlight.current || pageFlight.current) return;
    const owner = generation.current;
    const pending = pendingAction.current;
    const intent = pending?.jobId === original.jobId && pending.operation === operation
      ? pending
      : { jobId: original.jobId, operation, request: {
        expectedStateVersion: original.stateVersion, requestId: crypto.randomUUID(),
      } };
    pendingAction.current = intent;
    const token = Symbol("job-mutation");
    mutationFlight.current = token;
    setMutationBusy(true);
    const owns = () => mounted.current && owner === generation.current && mutationFlight.current === token;
    try {
      const next = await operation(original.jobId, intent.request);
      if (!owns() || next.jobId !== original.jobId) return;
      pendingAction.current = null;
      const latest = current.current;
      if (!latest.job || next.stateVersion < latest.job.stateVersion) return;
      // A mutation reply is always page one. On later pages update the global
      // header, then re-read the current cursor before enabling row actions.
      readFlight.current = null;
      if (latest.page === 0) publish({ ...latest, job: next, stale: false });
      else {
        publish({ ...latest, job: { ...next, items: latest.job.items,
          nextItemCursor: latest.job.nextItemCursor }, stale: true });
        await readPage(latest);
      }
    } catch (error) {
      if (owns()) {
        const raw = error instanceof Error ? error.message : String(error);
        // Only identity/version refusals prove this intent was not accepted.
        // Storage errors may happen after Undo's content COMMIT; keep the same
        // identity/version for a safe explicit retry after either kind of loss.
        if (["JOB_STATE_CHANGED", "JOB_ACTION_CONFLICT"].includes(importErrorCode(raw))) pendingAction.current = null;
        errorRef.current(raw);
        // The Host may have committed before losing its reply, even if the
        // previously visible state was terminal and had no polling timer.
        readFlight.current = null;
        publish({ ...current.current, stale: true });
        await readPage(current.current);
      }
    } finally {
      if (owns()) { mutationFlight.current = null; setMutationBusy(false); }
    }
  }, [publish, readPage]);

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      generation.current += 1;
      readFlight.current = null;
      pageFlight.current = null;
      mutationFlight.current = null;
      pendingAction.current = null;
    };
  }, []);

  useEffect(() => {
    if (!jobId || (!active && !view.stale)) return;
    let disposed = false;
    let unlisten: UnlistenFn | undefined;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const owner = generation.current;
    const refresh = async () => {
      if (!disposed && owner === generation.current) await readPage(current.current);
    };
    const busy = () => !!readFlight.current || !!mutationFlight.current;
    const queue = createImportRefreshQueue(refresh, busy);
    queueRef.current = queue;
    const poll = () => {
      if (disposed) return;
      queue.request();
      timer = setTimeout(poll, 1000);
    };

    void (async () => {
      try {
        const stop = await listen<{ jobId: string }>("session-import://changed", ({ payload }) => {
          if (payload.jobId === jobId) queue.request();
        });
        if (disposed) { stop(); return; }
        unlisten = stop;
      } catch {
        // Polling also covers a dropped event or an unavailable event bridge.
      }
      if (!disposed) {
        if (busy()) queue.request();
        else void refresh();
        timer = setTimeout(poll, 1000);
      }
    })();

    return () => {
      disposed = true;
      queue.dispose();
      if (queueRef.current === queue) queueRef.current = null;
      unlisten?.();
      if (timer) clearTimeout(timer);
    };
  }, [jobId, active, view.stale, view.owner, readPage]);

  return { job, setJob, jobActive: active, jobBusy: pageBusy || mutationBusy,
    jobItemsStale: view.stale || mutationBusy, jobHasPrevious: view.page > 0, jobPageIndex: view.page,
    refreshJobItems, loadMoreJobItems, previousJobItems, mutateJob };
}
