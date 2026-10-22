import { useCallback, useEffect, useRef, useState } from "react";
import * as api from "@/lib/api/sessionWorkspaceAssociation";
import { appendImportedWindow } from "@/lib/sessionImport/catalogView";

/** Metadata edits have their own lane; they never reload reader events or anchors. */
export function useImportedWorkspaceAssociation(importId: string | null) {
  const generation = useRef(0);
  const pending = useRef(false);
  const owner = useRef(importId);
  owner.current = importId;
  const request = useRef<api.ImportedWorkspaceUpdate | null>(null);
  const [association, setAssociation] = useState<api.ImportedWorkspaceAssociation | null>(null);
  const [items, setItems] = useState<api.ImportedWorkspaceChoice[]>([]);
  const [cursor, setCursor] = useState<string | null>(null);
  const [draft, setDraft] = useState<api.ImportedWorkspaceChoice | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);
  const [windowed, setWindowed] = useState(false);

  const refresh = useCallback(async () => {
    if (!importId || pending.current) return;
    const token = ++generation.current;
    pending.current = true;
    setBusy(true);
    setError(null);
    setSaved(false);
    try {
      const current = await api.importedWorkspaceGet(importId);
      if (generation.current !== token || owner.current !== importId) return;
      const page = current.sourceReady
        ? await api.importedWorkspaceOptions(importId)
        : { items: [], nextCursor: null };
      if (generation.current !== token || owner.current !== importId) return;
      setAssociation(current);
      setItems(page.items);
      setWindowed(false);
      setCursor(page.nextCursor);
      setDraft(page.items.find((item) => item.sourceWorkspaceId === current.choice?.sourceWorkspaceId) ?? current.choice);
      request.current = null;
    } catch (cause) {
      if (generation.current === token && owner.current === importId) setError(String(cause));
    } finally {
      if (generation.current === token && owner.current === importId) {
        pending.current = false;
        setBusy(false);
      }
    }
  }, [importId]);

  useEffect(() => {
    generation.current += 1;
    pending.current = false;
    request.current = null;
    setAssociation(null);
    setItems([]);
    setWindowed(false);
    setCursor(null);
    setDraft(null);
    setError(null);
    setSaved(false);
    setBusy(false);
    void refresh();
    return () => { generation.current += 1; };
  }, [refresh]);

  const loadMore = useCallback(async () => {
    if (!importId || !cursor || pending.current) return;
    const token = generation.current;
    pending.current = true;
    setBusy(true);
    setError(null);
    try {
      const page = await api.importedWorkspaceOptions(importId, cursor);
      if (generation.current !== token || owner.current !== importId) return;
      const next = appendImportedWindow(items, page.items, (item) => item.sourceWorkspaceId);
      setItems(next.items);
      if (next.trimmed) setWindowed(true);
      setCursor(page.nextCursor);
    } catch (cause) {
      if (generation.current === token && owner.current === importId) setError(String(cause));
    } finally {
      if (generation.current === token && owner.current === importId) {
        pending.current = false;
        setBusy(false);
      }
    }
  }, [cursor, importId, items]);

  const choose = useCallback((id: string) => {
    if (pending.current) return;
    const choice = items.find((item) => item.sourceWorkspaceId === id)
      ?? (association?.choice?.sourceWorkspaceId === id ? association.choice : null);
    if (id && !choice) return;
    setDraft(choice);
    request.current = null;
    setError(null);
    setSaved(false);
  }, [association, items]);

  const save = useCallback(async (clear = false) => {
    if (!importId || !association || association.importId !== importId || pending.current) return;
    const choice = clear ? null : draft;
    const token = generation.current;
    const previous = request.current;
    const next = previous && previous.importId === importId
      && previous.expectedVersion === association.version
      && JSON.stringify(previous.choice) === JSON.stringify(choice)
      ? previous
      : { importId, expectedVersion: association.version, requestId: crypto.randomUUID(), choice };
    request.current = next;
    pending.current = true;
    setBusy(true);
    setError(null);
    setSaved(false);
    try {
      const result = await api.importedWorkspaceUpdate(next);
      if (generation.current !== token || owner.current !== importId) return;
      setAssociation(result);
      setDraft(result.choice);
      request.current = null;
      setSaved(true);
    } catch (cause) {
      if (generation.current === token && owner.current === importId) setError(String(cause));
    } finally {
      if (generation.current === token && owner.current === importId) {
        pending.current = false;
        setBusy(false);
      }
    }
  }, [association, draft, importId]);

  return { association, items, cursor, draft, busy, error, saved, windowed, refresh, loadMore, choose, save };
}
