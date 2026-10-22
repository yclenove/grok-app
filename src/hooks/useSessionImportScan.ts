import { useCallback, useEffect, useRef, useState } from "react";
import * as api from "@/lib/api/sessionMigration";
import type {
  CandidateSelection,
  ScanSource,
  ScanState,
  SourceCandidate,
} from "@/lib/api/sessionMigration";

const isActive = (state: ScanState | null) =>
  !!state && ["queued", "scanning", "cancelling"].includes(state.state);

const isTerminal = (state: ScanState | null) =>
  !!state &&
  (state.complete ||
    ["completed", "failed", "cancelled", "expired"].includes(state.state));

function uniqueCandidateIds(
  candidates: SourceCandidate[],
  extras: SourceCandidate[],
): string[] {
  return Array.from(
    new Set([...candidates, ...extras].map((candidate) => candidate.candidateId)),
  );
}

/** One candidate page and its Host-owned selection membership in memory. */
export function useSessionImportScan(onError: (error: string) => void, options: { readCandidatePages?: boolean } = {}) {
  const readCandidatePages = options.readCandidatePages !== false;
  const [scan, setScan] = useState<ScanState | null>(null);
  const [candidates, setCandidates] = useState<SourceCandidate[]>([]);
  const [selection, setSelection] = useState<CandidateSelection | null>(null);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [nextCursor, setNextCursor] = useState<string | null>(null);
  const [pageIndex, setPageIndex] = useState(0);
  const [scanBusy, setScanBusy] = useState(false);
  const [selectionBusy, setSelectionBusy] = useState(false);
  const scanGeneration = useRef(0);
  const selectionGeneration = useRef(0);
  const current = useRef<ScanState | null>(null);
  const selectionRef = useRef<CandidateSelection | null>(null);
  const candidatesRef = useRef<SourceCandidate[]>([]);
  const pages = useRef<Array<string | null>>([null]);
  const pageRef = useRef(0);
  const pageGeneration = useRef(0);

  const adoptSelection = useCallback((next: CandidateSelection) => {
    const previous = selectionRef.current;
    if (
      previous?.selectionId === next.selectionId &&
      previous.stateVersion > next.stateVersion
    ) {
      return previous;
    }
    selectionRef.current = next;
    setSelection(next);
    if (current.current?.scanId === next.scanId) {
      current.current = { ...current.current, selection: next };
      setScan((old) =>
        old?.scanId === next.scanId ? { ...old, selection: next } : old,
      );
    }
    return next;
  }, []);

  const adopt = useCallback(
    (state: ScanState) => {
      const liveSelection = selectionRef.current;
      const nextSelection =
        liveSelection?.selectionId === state.selection.selectionId &&
        liveSelection.stateVersion > state.selection.stateVersion
          ? liveSelection
          : state.selection;
      adoptSelection(nextSelection);
      if (
        current.current?.scanId === state.scanId &&
        current.current.stateVersion > state.stateVersion
      ) {
        return;
      }
      const merged = { ...state, selection: nextSelection };
      current.current = merged;
      setScan(merged);
    },
    [adoptSelection],
  );

  const readMembership = useCallback(
    async (
      state: ScanState,
      descriptor: CandidateSelection,
      pageCandidates: SourceCandidate[],
    ) => {
      const ids = uniqueCandidateIds(pageCandidates, state.extraCandidates ?? []);
      if (ids.length === 0) {
        return {
          selectionId: descriptor.selectionId,
          stateVersion: descriptor.stateVersion,
          selectedIds: [] as string[],
        };
      }
      return api.sessionImportScanSelectionMembership(
        state.scanId,
        state.epoch,
        descriptor.selectionId,
        ids,
      );
    },
    [],
  );

  const readPage = useCallback(
    async (state: ScanState, index: number) => {
      const generation = scanGeneration.current;
      const request = ++pageGeneration.current;
      const page = await api.sessionImportScanPage(
        state.scanId,
        state.epoch,
        pages.current[index],
        50,
      );
      const descriptor =
        selectionRef.current?.selectionId === state.selection.selectionId &&
        selectionRef.current.stateVersion > state.selection.stateVersion
          ? selectionRef.current
          : state.selection;
      const membership = await readMembership(state, descriptor, page.candidates);
      if (
        generation !== scanGeneration.current ||
        request !== pageGeneration.current
      ) {
        return;
      }
      candidatesRef.current = page.candidates;
      setCandidates(page.candidates);
      setNextCursor(page.nextCursor);
      pageRef.current = index;
      setPageIndex(index);
      const live = selectionRef.current;
      if (
        membership.selectionId === descriptor.selectionId &&
        (!live || membership.stateVersion >= live.stateVersion)
      ) {
        setSelected(new Set(membership.selectedIds));
      }
    },
    [readMembership],
  );

  const refreshMembership = useCallback(
    async (descriptor: CandidateSelection) => {
      const state = current.current;
      if (!state || state.scanId !== descriptor.scanId) return;
      const generation = scanGeneration.current;
      const request = ++pageGeneration.current;
      const membership = await readMembership(
        state,
        descriptor,
        candidatesRef.current,
      );
      if (
        generation !== scanGeneration.current ||
        request !== pageGeneration.current ||
        membership.selectionId !== descriptor.selectionId
      ) {
        return;
      }
      const live = selectionRef.current;
      if (!live || membership.stateVersion >= live.stateVersion) {
        setSelected(new Set(membership.selectedIds));
      }
    },
    [readMembership],
  );

  const startScan = useCallback(
    async (sources?: ScanSource[]) => {
      const generation = ++scanGeneration.current;
      ++pageGeneration.current;
      ++selectionGeneration.current;
      setScanBusy(true);
      const old = current.current;
      current.current = null;
      selectionRef.current = null;
      candidatesRef.current = [];
      setScan(null);
      setSelection(null);
      setSelected(new Set());
      setCandidates([]);
      setNextCursor(null);
      pages.current = [null];
      pageRef.current = 0;
      setPageIndex(0);
      try {
        if (old && !isTerminal(old)) {
          await api.sessionImportScanCancel(old.scanId, old.epoch);
        }
        if (generation !== scanGeneration.current) return false;
        const state = await api.sessionImportScanStart(sources);
        if (generation !== scanGeneration.current) {
          await api.sessionImportScanCancel(state.scanId, state.epoch);
          return false;
        }
        adopt(state);
        return true;
      } catch (error) {
        if (generation === scanGeneration.current) onError(String(error));
        return false;
      } finally {
        if (generation === scanGeneration.current) setScanBusy(false);
      }
    },
    [adopt, onError],
  );

  useEffect(() => {
    if (!scan) return;
    let stopped = false;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const generation = scanGeneration.current;
    const id = scan.scanId;
    const epoch = scan.epoch;
    const poll = async () => {
      try {
        const state = await api.sessionImportScanGet(id, epoch);
        if (stopped || generation !== scanGeneration.current) return;
        if (readCandidatePages) await readPage(state, pageRef.current);
        if (stopped || generation !== scanGeneration.current) return;
        adopt(state);
        if (isActive(state)) timer = setTimeout(() => void poll(), 500);
      } catch (error) {
        if (!stopped && generation === scanGeneration.current) {
          onError(String(error));
        }
      }
    };
    void poll();
    return () => {
      stopped = true;
      clearTimeout(timer);
    };
  }, [scan?.scanId, scan?.state, scan?.epoch, adopt, onError, readPage, readCandidatePages]);

  useEffect(
    () => () => {
      ++scanGeneration.current;
      ++selectionGeneration.current;
      const state = current.current;
      if (state && !isTerminal(state)) {
        void api
          .sessionImportScanCancel(state.scanId, state.epoch)
          .catch(() => {});
      }
    },
    [],
  );

  const mutateSelection = useCallback(
    async (
      operation: (
        state: ScanState,
        descriptor: CandidateSelection,
      ) => Promise<CandidateSelection>,
    ) => {
      const state = current.current;
      const descriptor = selectionRef.current;
      if (!state || !descriptor || selectionBusy) return;
      const scanOwner = scanGeneration.current;
      const request = ++selectionGeneration.current;
      setSelectionBusy(true);
      try {
        const next = await operation(state, descriptor);
        if (
          scanOwner !== scanGeneration.current ||
          request !== selectionGeneration.current
        ) {
          return;
        }
        adoptSelection(next);
        await refreshMembership(next);
      } catch (error) {
        if (
          scanOwner === scanGeneration.current &&
          request === selectionGeneration.current
        ) {
          onError(String(error));
        }
      } finally {
        if (
          scanOwner === scanGeneration.current &&
          request === selectionGeneration.current
        ) {
          setSelectionBusy(false);
        }
      }
    },
    [adoptSelection, onError, refreshMembership, selectionBusy],
  );

  const toggle = useCallback(
    async (id: string) => {
      const nextValue = !selected.has(id);
      await mutateSelection((state, descriptor) =>
        api.sessionImportScanSelectionSet(
          state.scanId,
          state.epoch,
          descriptor.selectionId,
          [id],
          nextValue,
        ),
      );
    },
    [mutateSelection, selected],
  );

  const clearSelected = useCallback(async () => {
    await mutateSelection((state, descriptor) =>
      api.sessionImportScanSelectionClear(
        state.scanId,
        state.epoch,
        descriptor.selectionId,
      ),
    );
  }, [mutateSelection]);

  const selectThese = useCallback(
    async (ids: string[]) => {
      await mutateSelection((state, descriptor) =>
        api.sessionImportScanSelectionSet(
          state.scanId,
          state.epoch,
          descriptor.selectionId,
          ids,
          true,
        ),
      );
    },
    [mutateSelection],
  );

  const invertThese = useCallback(
    async (ids: string[]) => {
      await mutateSelection((state, descriptor) =>
        api.sessionImportScanSelectionInvert(
          state.scanId,
          state.epoch,
          descriptor.selectionId,
          ids,
        ),
      );
    },
    [mutateSelection],
  );

  const selectCurrent = useCallback(async () => {
    await mutateSelection((state) =>
      api.sessionImportScanSelectionFreeze(state.scanId, state.epoch),
    );
  }, [mutateSelection]);

  const changePage = useCallback(
    async (direction: -1 | 1) => {
      const state = current.current;
      if (selectionBusy || !state || scanBusy || (direction === 1 && !nextCursor)) {
        return;
      }
      const index = pageRef.current + direction;
      if (index < 0) return;
      if (direction === 1) pages.current[index] = nextCursor;
      pageRef.current = index;
      const generation = scanGeneration.current;
      setScanBusy(true);
      try {
        await readPage(state, index);
      } catch (error) {
        if (generation === scanGeneration.current) onError(String(error));
      } finally {
        if (generation === scanGeneration.current) setScanBusy(false);
      }
    },
    [nextCursor, onError, readPage, scanBusy, selectionBusy],
  );

  const controlScan = useCallback(
    async (action: "cancel" | "continue") => {
      const state = current.current;
      if (!state) return;
      const generation = scanGeneration.current;
      try {
        const updated = await (action === "cancel"
          ? api.sessionImportScanCancel
          : api.sessionImportScanContinue)(state.scanId, state.epoch);
        if (generation === scanGeneration.current) adopt(updated);
      } catch (error) {
        if (generation === scanGeneration.current) onError(String(error));
      }
    },
    [adopt, onError],
  );

  const stopIncompleteScan = useCallback(() => {
    const state = current.current;
    if (state && !isTerminal(state)) {
      void api
        .sessionImportScanCancel(state.scanId, state.epoch)
        .catch(() => {});
    }
  }, []);

  const getState = useCallback(() => ({ scan: current.current, selection: selectionRef.current }), []);
  const clearIntent = useCallback(() => {
    const prior = current.current;
    ++scanGeneration.current;
    ++selectionGeneration.current;
    ++pageGeneration.current;
    current.current = null;
    selectionRef.current = null;
    candidatesRef.current = [];
    pages.current = [null];
    pageRef.current = 0;
    setScan(null);
    setSelection(null);
    setCandidates([]);
    setSelected(new Set());
    setNextCursor(null);
    setPageIndex(0);
    setScanBusy(false);
    setSelectionBusy(false);
    if (prior && !isTerminal(prior)) void api.sessionImportScanCancel(prior.scanId, prior.epoch).catch(() => {});
  }, []);
  const refresh = useCallback(async () => {
    const state = current.current;
    if (!state || scanBusy || selectionBusy) return false;
    const generation = scanGeneration.current;
    setScanBusy(true);
    try {
      const next = await api.sessionImportScanGet(state.scanId, state.epoch);
      if (generation !== scanGeneration.current) return false;
      if (readCandidatePages) await readPage(next, pageRef.current);
      if (generation !== scanGeneration.current) return false;
      adopt(next);
      return true;
    } catch (cause) {
      if (generation === scanGeneration.current) onError(String(cause));
      return false;
    } finally {
      if (generation === scanGeneration.current) setScanBusy(false);
    }
  }, [adopt, onError, readCandidatePages, readPage, scanBusy, selectionBusy]);

  return {
    scanState: scan,
    getState,
    clearIntent,
    refresh,
    candidates,
    selection,
    selected,
    selectedCount: selection?.selectedCount ?? 0,
    nextCursor,
    pageIndex,
    scanBusy: scanBusy || selectionBusy,
    // Paging/selection also own busy, but only startup has no accepted scan yet.
    scanStarting: scanBusy && scan === null,
    scanGeneration,
    startScan,
    toggle,
    clearSelected,
    selectThese,
    invertThese,
    selectCurrent,
    changePage,
    controlScan,
    stopIncompleteScan,
    scanning: isActive(scan),
    scanPaused: scan?.state === "paused",
    scanCancelling: scan?.state === "cancelling",
    scanFailed: scan?.state === "failed",
    scanIssues: scan?.issues ?? [],
    discoveredCount: scan?.discoveredCount ?? 0,
    issueCount: scan?.issueCount ?? 0,
    extraCount: scan?.extraCount ?? 0,
  };
}
