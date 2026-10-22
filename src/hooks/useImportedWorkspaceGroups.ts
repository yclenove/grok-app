import { useCallback, useEffect, useRef, useState } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { importedWorkspaceGroups, type ImportedWorkspaceGroup } from "@/lib/api/sessionWorkspaceAssociation";
import { appendImportedWindow } from "@/lib/sessionImport/catalogView";
import { createImportRefreshQueue, type ImportRefreshQueue } from "@/lib/sessionImport/refreshQueue";

export function useImportedWorkspaceGroups(selectedId: string | null) {
  const [items, setItems] = useState<ImportedWorkspaceGroup[]>([]);
  const [selected, setSelected] = useState<ImportedWorkspaceGroup | null>(null);
  const [cursor, setCursor] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [windowed, setWindowed] = useState(false);
  const pending = useRef(false);
  const generation = useRef(0);
  const owner = useRef(selectedId);
  owner.current = selectedId;
  const itemsRef = useRef(items);
  const cursorRef = useRef(cursor);
  const refreshQueue = useRef<ImportRefreshQueue | null>(null);

  const fetchPage = useCallback(async (more: boolean) => {
    if (more && (pending.current || !cursorRef.current)) return;
    const token = ++generation.current;
    pending.current = true;
    setBusy(true);
    setError(null);
    try {
      const page = await importedWorkspaceGroups(more ? cursorRef.current : null, selectedId);
      if (generation.current !== token || owner.current !== selectedId) return;
      const next = appendImportedWindow(more ? itemsRef.current : [], page.items, (item) => item.sourceWorkspaceId);
      itemsRef.current = next.items;
      setItems(next.items);
      setSelected(page.selected);
      if (!more) setWindowed(false);
      if (next.trimmed) setWindowed(true);
      cursorRef.current = page.nextCursor;
      setCursor(page.nextCursor);
    } catch (cause) {
      if (generation.current === token && owner.current === selectedId) setError(String(cause));
    } finally {
      if (generation.current === token && owner.current === selectedId) { pending.current = false; setBusy(false); }
    }
  }, [selectedId]);

  useEffect(() => {
    void fetchPage(false);
    let cancelled = false;
    const stops: UnlistenFn[] = [];
    const queue = createImportRefreshQueue(() => fetchPage(false), () => pending.current);
    refreshQueue.current = queue;
    for (const event of ["session-import://workspace-changed", "session-import://changed"]) {
      void listen(event, () => { if (!cancelled) queue.request(); })
        .then((stop) => { if (cancelled) stop(); else stops.push(stop); })
        .catch(() => { /* Refresh is available if native events are unavailable. */ });
    }
    return () => {
      cancelled = true;
      queue.dispose();
      if (refreshQueue.current === queue) refreshQueue.current = null;
      generation.current += 1;
      stops.forEach((stop) => stop());
    };
  }, [fetchPage]);
  return { items, selected, cursor, busy, error, windowed,
    refresh: () => { refreshQueue.current?.reset(); return fetchPage(false); },
    loadMore: () => fetchPage(true) };
}
