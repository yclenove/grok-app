import { useCallback, useEffect, useRef, useState } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { sessionImportEntries, type CatalogEntry } from "@/lib/api/sessionMigration";
import { appendImportedWindow, readCatalogView, writeCatalogView, type CatalogView } from "@/lib/sessionImport/catalogView";
import { createImportRefreshQueue, type ImportRefreshQueue } from "@/lib/sessionImport/refreshQueue";

/** Bounded catalog windows; off-page reader metadata uses an exact lookup. */
export function useImportedCatalog(scope?: "sidebar" | "settings") {
  const [filter, setFilter] = useState(() => readCatalogView(scope));
  const [entries, setEntries] = useState<CatalogEntry[]>([]);
  const [refreshVersion, setRefreshVersion] = useState(0);
  const [nextCursor, setNextCursor] = useState<string | null>(null);
  const [total, setTotal] = useState(0);
  const [providers, setProviders] = useState<string[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [queryPending, setQueryPending] = useState(false);
  const [windowed, setWindowed] = useState(false);
  const seqRef = useRef(0);
  const inflightRef = useRef(false);
  const cursorRef = useRef<string | null>(null);
  const filterRef = useRef(filter);
  const entriesRef = useRef(entries);
  const mounted = useRef(true);
  const refreshQueue = useRef<ImportRefreshQueue | null>(null);
  const debounceQuery = useRef(false);
  const queryTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  const load = useCallback(async () => {
    if (!mounted.current) return;
    if (queryTimer.current !== null) clearTimeout(queryTimer.current);
    queryTimer.current = null;
    debounceQuery.current = false;
    setQueryPending(false);
    const seq = ++seqRef.current;
    inflightRef.current = true;
    cursorRef.current = null;
    setNextCursor(null);
    setBusy(true);
    setError(null);
    try {
      const page = await sessionImportEntries(null, 50, filterRef.current);
      if (seq !== seqRef.current || !mounted.current) return;
      entriesRef.current = page.items;
      setEntries(page.items);
      setRefreshVersion((version) => version + 1);
      setNextCursor(page.nextCursor);
      cursorRef.current = page.nextCursor;
      setTotal(page.total);
      setProviders(page.providers ?? []);
      setWindowed(false);
    } catch (cause) {
      if (seq !== seqRef.current || !mounted.current) return;
      setError(String(cause));
    } finally {
      if (seq === seqRef.current && mounted.current) {
        inflightRef.current = false;
        setBusy(false);
      }
    }
  }, []);

  const loadMore = useCallback(async () => {
    const cursor = cursorRef.current;
    if (!cursor || inflightRef.current || !mounted.current) return;
    const seq = seqRef.current;
    inflightRef.current = true;
    setBusy(true);
    setError(null);
    try {
      const page = await sessionImportEntries(cursor, 50, filterRef.current);
      if (seq !== seqRef.current || !mounted.current) return;
      const next = appendImportedWindow(entriesRef.current, page.items, (entry) => entry.importId);
      entriesRef.current = next.items;
      setEntries(next.items);
      if (next.trimmed) setWindowed(true);
      setNextCursor(page.nextCursor);
      cursorRef.current = page.nextCursor;
      setTotal(page.total);
      setProviders(page.providers ?? []);
    } catch (cause) {
      if (seq === seqRef.current && mounted.current) setError(String(cause));
    } finally {
      if (seq === seqRef.current && mounted.current) {
        inflightRef.current = false;
        setBusy(false);
      }
    }
  }, []);

  const change = useCallback((patch: Partial<CatalogView>) => {
    const next = { ...filterRef.current, ...patch };
    if (!next.query.trim()) next.query = "";
    if (JSON.stringify(next) === JSON.stringify(filterRef.current)) return;
    refreshQueue.current?.reset();
    seqRef.current += 1;
    cursorRef.current = null;
    // A pending query owns the busy lane too, so hints cannot bypass its pause.
    inflightRef.current = true;
    debounceQuery.current = patch.query !== undefined && next.query.trim().length > 0;
    setQueryPending(debounceQuery.current);
    filterRef.current = next;
    entriesRef.current = [];
    setFilter(next);
    setEntries([]);
    setTotal(0);
    setNextCursor(null);
    setWindowed(false);
    setError(null);
    setBusy(true);
    writeCatalogView(scope, next);
  }, [scope]);

  const refresh = useCallback(() => {
    refreshQueue.current?.reset();
    return load();
  }, [load]);

  useEffect(() => {
    mounted.current = true;
    if (!debounceQuery.current) { void load(); return; }
    const timer = setTimeout(() => { refreshQueue.current?.reset(); void load(); }, 250);
    queryTimer.current = timer;
    return () => {
      clearTimeout(timer);
      if (queryTimer.current === timer) queryTimer.current = null;
    };
  }, [filter, load]);
  useEffect(() => {
    mounted.current = true;
    const stops: UnlistenFn[] = [];
    let cancelled = false;
    const queue = createImportRefreshQueue(load, () => inflightRef.current);
    refreshQueue.current = queue;
    for (const event of ["session-import://changed", "session-import://workspace-changed"]) {
      void listen(event, () => { if (!cancelled) queue.request(); })
        .then((stop) => { if (cancelled) stop(); else stops.push(stop); })
        .catch(() => { /* Explicit Refresh remains available. */ });
    }
    return () => {
      cancelled = true;
      queue.dispose();
      if (refreshQueue.current === queue) refreshQueue.current = null;
      mounted.current = false;
      seqRef.current += 1;
      stops.forEach((stop) => stop());
    };
  }, [load]);

  return {
    entries, visible: entries, refreshVersion, ...filter,
    setQuery: (query: string) => change({ query }),
    setProvider: (provider: string | null) => change({ provider }),
    setSort: (sort: CatalogView["sort"]) => change({ sort }),
    setWorkspace: (workspace: CatalogView["workspace"]) => change({ workspace }),
    providers, load: refresh, loadMore, nextCursor, total, error, busy, queryPending, windowed,
  };
}
