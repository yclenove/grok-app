import { useCallback, useLayoutEffect, useRef, useState } from "react";
import { sessionImportSourceComparisonField } from "@/lib/api/sessionMigrationComparisonField";
import { COMPARISON_FIELD_BYTES, copyFieldContext, emptyComparisonField, fieldName, validateFieldContext,
  validateFieldPage, type ComparisonFieldContext, type ComparisonFieldState } from "@/lib/sessionImport/comparisonFieldState";

type Attempt = { cursor?: string; start: number };
const liveClient = { page: sessionImportSourceComparisonField };

/** One field segment at a time; row paging never shares this cursor or owner. */
export function useSessionImportComparisonField(context: ComparisonFieldContext | null, client = liveClient) {
  const [state, setState] = useState<ComparisonFieldState>(emptyComparisonField);
  const current = useRef(state);
  const active = useRef<ComparisonFieldContext | null>(null);
  const lifetime = useRef(0);
  const serial = useRef(0);
  const pages = useRef<Attempt[]>([{ start: 0 }]);
  const lastAttempt = useRef(0);
  const commit = useCallback((next: ComparisonFieldState) => { current.current = next; setState(next); }, []);
  const close = useCallback(() => {
    ++lifetime.current; ++serial.current;
    pages.current = [{ start: 0 }]; lastAttempt.current = 0;
    commit(emptyComparisonField());
  }, [commit]);
  const contextKey = JSON.stringify(context);
  useLayoutEffect(() => {
    close();
    active.current = copyCommittedContext(contextKey);
    return () => { ++lifetime.current; ++serial.current; active.current = null; current.current = emptyComparisonField(); };
    // Scope ownership changes only on committed renders, never abandoned ones.
  }, [contextKey, close]);

  const read = useCallback(async (index: number): Promise<boolean> => {
    const snapshot = current.current, target = active.current, attempt = pages.current[index];
    if (!target || !snapshot.field || !attempt || snapshot.loading) return false;
    const generation = lifetime.current, request = ++serial.current;
    const owns = () => generation === lifetime.current && request === serial.current;
    lastAttempt.current = index;
    commit({ ...snapshot, loading: true, error: null });
    try {
      validateFieldContext(target);
      const page = await client.page({ version: 1, left: { ...target.owner.left }, right: { ...target.owner.right },
        side: target.side, position: target.position, field: snapshot.field, expectedFieldsDigest: target.fieldsDigest,
        limitBytes: COMPARISON_FIELD_BYTES, ...(attempt.cursor !== undefined ? { cursor: attempt.cursor } : {}) });
      if (!owns()) return false;
      validateFieldPage(target, snapshot.field, page, attempt.start, snapshot.page, attempt.cursor);
      commit({ ...current.current, page, fieldNames: [...page.fieldNames], pageIndex: index, loading: false, error: null });
      pages.current.length = index + 1;
      return true;
    } catch (cause) {
      if (owns()) commit({ ...current.current, error: String(cause) });
      return false;
    } finally {
      if (owns()) commit({ ...current.current, loading: false });
    }
  }, [client, commit]);
  const open = useCallback((field: string) => {
    if (!active.current || !fieldName(field) || (current.current.loading && current.current.field === field)) return Promise.resolve(false);
    const fieldNames = current.current.fieldNames;
    close(); commit({ ...emptyComparisonField(), field, fieldNames });
    return read(0);
  }, [close, commit, read]);
  const retry = useCallback(() => read(lastAttempt.current), [read]);
  const nextPage = useCallback(() => {
    const snapshot = current.current;
    if (snapshot.loading || snapshot.error || !snapshot.page?.nextCursor) return Promise.resolve(false);
    const index = snapshot.pageIndex + 1;
    pages.current[index] = { cursor: snapshot.page.nextCursor, start: snapshot.page.endByte };
    return read(index);
  }, [read]);
  const previousPage = useCallback(() => {
    const snapshot = current.current;
    return snapshot.loading || snapshot.error || !snapshot.page || !snapshot.pageIndex ? Promise.resolve(false) : read(snapshot.pageIndex - 1);
  }, [read]);
  return { ...state, open, close, retry, nextPage, previousPage };
}

function copyCommittedContext(key: string): ComparisonFieldContext | null {
  const context = JSON.parse(key) as ComparisonFieldContext | null;
  return context ? copyFieldContext(context) : null;
}
