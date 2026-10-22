import { useCallback, useEffect, useRef, useState } from "react";
import { sessionImportRevisions, type ImportedRevisionPage } from "@/lib/api/sessionMigration";

type Client = (importId: string, cursor: string | null) => Promise<ImportedRevisionPage>;
const liveClient: Client = (importId, cursor) => sessionImportRevisions(importId, cursor, 50);
type Owner = { importId: string; flight: boolean; failed: Request | null };
type Request = { cursor: string | null; snapshot: string | null };
type State = { owner: Owner; page: ImportedRevisionPage | null; busy: boolean; error: string | null };

function validate(page: ImportedRevisionPage, importId: string, snapshot: string | null) {
  if (page.importId !== importId || (snapshot && page.snapshot !== snapshot)
    || !/^[a-f0-9]{64}$/.test(page.snapshot) || page.items.length > 50
    || !Number.isSafeInteger(page.total) || page.total < page.items.length
    || !page.items.length || new Set(page.items.map((item) => item.revisionId)).size !== page.items.length
    || ![page.currentRevision, ...page.items.map((item) => item.revisionId)].every((id) => /^[a-f0-9-]{36}$/i.test(id))) {
    throw new Error("PLAN_STALE: revision page does not match this history");
  }
}

/** One metadata page; closing or changing history cancels ownership, not the reader. */
export function useImportedRevisions(importId: string, client: Client = liveClient) {
  const [state, setState] = useState<State | null>(null);
  const owner = useRef<Owner | null>(null);
  const identity = useRef(importId);
  identity.current = importId;
  const current = state?.owner === owner.current && state?.owner.importId === importId ? state : null;

  const load = useCallback(async (captured: Owner, request: Request) => {
    const owns = () => owner.current === captured && identity.current === captured.importId;
    if (!owns() || captured.flight) return;
    captured.flight = true;
    setState((previous) => ({ owner: captured, page: previous?.owner === captured ? previous.page : null, busy: true, error: null }));
    try {
      const page = await client(captured.importId, request.cursor);
      if (!owns()) return;
      validate(page, captured.importId, request.snapshot);
      captured.failed = null;
      setState({ owner: captured, page, busy: false, error: null });
    } catch (cause) {
      if (!owns()) return;
      captured.failed = request;
      setState((previous) => ({ owner: captured, page: previous?.owner === captured ? previous.page : null, busy: false, error: String(cause) }));
    } finally {
      if (owns()) captured.flight = false;
    }
  }, [client]);

  useEffect(() => {
    const captured: Owner = { importId, flight: false, failed: null };
    owner.current = captured;
    void load(captured, { cursor: null, snapshot: null });
    return () => { if (owner.current === captured) owner.current = null; };
  }, [importId, load]);

  const request = (cursor: string | null, snapshot: string | null) => {
    const captured = owner.current;
    if (captured?.importId === importId) void load(captured, { cursor, snapshot });
  };
  return {
    page: current?.page ?? null,
    busy: current?.busy ?? true,
    error: current?.error ?? null,
    refresh: () => request(null, null),
    retry: () => {
      const captured = owner.current;
      if (captured?.importId === importId && captured.failed) void load(captured, captured.failed);
    },
    move: (direction: "previous" | "next") => {
      const page = current?.page;
      const cursor = direction === "previous" ? page?.previousCursor : page?.nextCursor;
      if (cursor && page) request(cursor, page.snapshot);
    },
    canChoose: (revision: string) => !owner.current?.flight && !current?.error
      && !!current?.page?.items.some((item) => item.revisionId === revision),
  };
}
