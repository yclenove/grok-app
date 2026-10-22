import { useCallback, useEffect, useRef, useState } from "react";
import { sessionImportJobHistory, type ImportJobHistoryPage } from "@/lib/api/sessionMigrationJobs";

type View = ImportJobHistoryPage & { cursors: Array<string | null>; page: number };
const emptyView = (): View => ({ items: [], nextCursor: null, cursors: [null], page: 0 });
type SelectJob = (id: string, isCurrent: () => boolean) => Promise<boolean>;

/** Own only a bounded page and cursor strings. Closing invalidates both reads
 * and job selection, including a close/reopen while the old request is pending. */
export function useSessionImportJobHistory(enabled: boolean, selectJob: SelectJob) {
  const [open, setOpen] = useState(false);
  const [view, setView] = useState<View>(emptyView);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const generation = useRef(0);
  const active = useRef(false);
  const flight = useRef(false);
  const access = useRef(enabled);
  access.current = enabled;
  const retry = useRef<(() => Promise<void>) | null>(null);

  const close = useCallback(() => {
    generation.current += 1;
    active.current = false;
    flight.current = false;
    retry.current = null;
    setOpen(false);
    setBusy(false);
    setError(null);
  }, []);

  const load = useCallback(async (cursors: Array<string | null>, page: number) => {
    if (!active.current || !access.current || flight.current) return;
    const owner = ++generation.current;
    const owns = () => active.current && access.current && owner === generation.current;
    flight.current = true;
    setBusy(true);
    setError(null);
    retry.current = () => load(cursors, page);
    try {
      const next = await sessionImportJobHistory(cursors[page]);
      if (owns()) { setView({ ...next, cursors, page }); retry.current = null; }
    } catch (cause) {
      if (owns()) setError(String(cause));
    } finally {
      if (owns()) { flight.current = false; setBusy(false); }
    }
  }, []);

  const show = useCallback(() => {
    if (!access.current || active.current) return;
    active.current = true;
    setView(emptyView());
    setOpen(true);
    void load([null], 0);
  }, [load]);

  const select = useCallback(async (id: string) => {
    if (!active.current || !access.current || flight.current) return;
    const owner = ++generation.current;
    const owns = () => active.current && access.current && owner === generation.current;
    flight.current = true;
    setBusy(true);
    setError(null);
    retry.current = null;
    try {
      if (await selectJob(id, owns)) { if (owns()) close(); }
    } catch (cause) {
      if (owns()) setError(String(cause));
    } finally {
      if (owns()) { flight.current = false; setBusy(false); }
    }
  }, [close, selectJob]);

  useEffect(() => { if (!enabled) close(); }, [enabled, close]);
  useEffect(() => () => { generation.current += 1; active.current = false; }, []);

  return {
    open: open && enabled, show, close, busy, error, ...view,
    select,
    refresh: () => void load([null], 0),
    retry: () => { if (!flight.current) void retry.current?.(); },
    canRetry: retry.current != null,
    previous: () => { if (view.page > 0) void load(view.cursors, view.page - 1); },
    next: () => {
      if (view.nextCursor) void load([...view.cursors.slice(0, view.page + 1), view.nextCursor], view.page + 1);
    },
  };
}
