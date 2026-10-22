import { useCallback, useLayoutEffect, useRef, useState } from "react";
import { sessionImportSourceComparison, type SourceComparisonSide } from "@/lib/api/sessionMigrationComparison";
import type { ImportGroupScope } from "@/lib/sessionImport/groupSelectionState";
import {
  COMPARISON_UI_PAGE_SIZE, copyComparisonOwner, emptySourceComparison, validateComparisonOwner,
  validateComparisonPage, type SourceComparisonState,
} from "@/lib/sessionImport/sourceComparisonState";

export type SourceComparisonClient = { page: typeof sessionImportSourceComparison };
const liveClient: SourceComparisonClient = { page: sessionImportSourceComparison };
type PageRequest = { cursor?: string; start: number };
type Options = { onError?: (message: string) => void };

/** Owns comparison pages only. The paired-view owner must separately release its previews. */
export function useSessionImportSourceComparison(
  scope: ImportGroupScope | null, client: SourceComparisonClient = liveClient, options: Options = {},
) {
  const [state, setState] = useState<SourceComparisonState>(emptySourceComparison);
  const current = useRef(state);
  const activeScope = useRef<ImportGroupScope | null>(null);
  const lifetime = useRef(0);
  const requestSerial = useRef(0);
  const pages = useRef<PageRequest[]>([{ start: 0 }]);
  const lastAttempt = useRef(0);
  const commit = useCallback((next: SourceComparisonState) => { current.current = next; setState(next); }, []);
  const close = useCallback(() => {
    ++lifetime.current;
    ++requestSerial.current;
    pages.current = [{ start: 0 }];
    lastAttempt.current = 0;
    commit(emptySourceComparison());
  }, [commit]);
  const scanId = scope?.scanId;
  const epoch = scope?.epoch;
  const generation = scope?.generation;
  useLayoutEffect(() => {
    close();
    activeScope.current = scanId !== undefined && epoch !== undefined && generation !== undefined
      ? { scanId, epoch, generation } : null;
    return () => {
      ++lifetime.current;
      ++requestSerial.current;
      activeScope.current = null;
      current.current = emptySourceComparison();
    };
  }, [scanId, epoch, generation, close]);

  const readPage = useCallback(async (index: number): Promise<boolean> => {
    const snapshot = current.current;
    const { owner } = snapshot;
    const attempt = pages.current[index];
    if (!owner || !attempt || snapshot.loading) return false;
    const generation = lifetime.current;
    const serial = ++requestSerial.current;
    const owns = () => generation === lifetime.current && serial === requestSerial.current;
    lastAttempt.current = index;
    commit({ ...snapshot, loading: true, error: null });
    try {
      const page = await client.page({ version: 1, left: { ...owner.left }, right: { ...owner.right },
        limit: COMPARISON_UI_PAGE_SIZE, ...(attempt.cursor !== undefined ? { cursor: attempt.cursor } : {}) });
      if (!owns()) return false;
      validateComparisonPage(owner, page, attempt.start, snapshot.page);
      commit({ ...current.current, page, pageIndex: index, loading: false, error: null });
      // A newly read earlier page owns the next cursor; discard its old tail.
      pages.current.length = index + 1;
      return true;
    } catch (cause) {
      if (owns()) {
        const message = String(cause);
        commit({ ...current.current, error: message });
        options.onError?.(message);
      }
      return false;
    } finally {
      if (owns()) commit({ ...current.current, loading: false });
    }
  }, [client, commit, options.onError]);

  const open = useCallback(async (groupId: string, left: SourceComparisonSide, right: SourceComparisonSide) => {
    close();
    const scope = activeScope.current;
    if (!scope) return false;
    const owner = copyComparisonOwner({ scope, groupId, left, right });
    try { validateComparisonOwner(owner); } catch (cause) {
      const message = String(cause);
      commit({ ...emptySourceComparison(), error: message });
      options.onError?.(message);
      return false;
    }
    commit({ ...emptySourceComparison(), owner });
    return readPage(0);
  }, [close, commit, options.onError, readPage]);
  const retry = useCallback(() => readPage(lastAttempt.current), [readPage]);
  const nextPage = useCallback(() => {
    const snapshot = current.current;
    if (snapshot.loading || snapshot.error || !snapshot.page?.nextCursor) return Promise.resolve(false);
    const index = snapshot.pageIndex + 1;
    const rows = snapshot.page.rows;
    pages.current[index] = { cursor: snapshot.page.nextCursor, start: rows.at(-1)!.position + 1 };
    return readPage(index);
  }, [readPage]);
  const previousPage = useCallback(() => {
    const snapshot = current.current;
    if (snapshot.loading || snapshot.error || !snapshot.page || snapshot.pageIndex === 0) return Promise.resolve(false);
    return readPage(snapshot.pageIndex - 1);
  }, [readPage]);
  return { ...state, open, close, retry, nextPage, previousPage };
}
