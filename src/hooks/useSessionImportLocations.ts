import { useCallback, useLayoutEffect, useRef, useState } from "react";
import { pickDirectory } from "@/lib/api";
import { sessionImportSourceAssociate, sessionImportSourceLocations,
  type SourceAssociationRequest, type SourceLocation, type SourceLocationPage, type SourceRegistration,
} from "@/lib/api/sessionMigrationLocations";
import { LOCATION_PAGE_LIMIT, LOCATION_SELECTION_LIMIT, validateSourceLocation,
  validateSourceLocationPage } from "@/lib/sessionImport/sourceLocationState";

export type SessionImportLocationsClient = {
  list: typeof sessionImportSourceLocations;
  associate: typeof sessionImportSourceAssociate;
  pick: typeof pickDirectory;
};
const liveClient: SessionImportLocationsClient = {
  list: sessionImportSourceLocations, associate: sessionImportSourceAssociate, pick: pickDirectory,
};
type State = {
  open: boolean; source: SourceRegistration | null; page: SourceLocationPage | null; pageIndex: number;
  phase: "read" | "pick" | "save" | null; error: string | null; path: string | null;
  attempt: SourceAssociationRequest | null; saved: SourceLocation | null; selected: SourceLocation[];
};
const empty = (): State => ({ open: false, source: null, page: null, pageIndex: 0,
  phase: null, error: null, path: null, attempt: null, saved: null, selected: [] });

/** One installation dialog; metadata pages and explicit mutations have separate monotonic owners. */
export function useSessionImportLocations(source: SourceRegistration, client: SessionImportLocationsClient = liveClient, scopeKey = "") {
  const [state, setState] = useState<State>(empty);
  const current = useRef(state);
  const lifetime = useRef(0);
  const serial = useRef(0);
  const flight = useRef<number | null>(null);
  const cursors = useRef<Array<string | undefined>>([undefined]);
  const attemptedPage = useRef(0);
  const signature = JSON.stringify([scopeKey, source.sourceId, source.provider, source.rootPath]);
  const inputSignature = useRef(signature);
  const ownedInput = useRef<string | null>(null);
  const update = useCallback((patch: Partial<State>) => {
    current.current = { ...current.current, ...patch }; setState(current.current);
  }, []);
  const close = useCallback(() => {
    ++lifetime.current; flight.current = null; ownedInput.current = null;
    update({ open: false, phase: null });
    // An association may already have committed. Retain its exact request for
    // idempotent review on reopening; closing is not a cancellation or rollback.
  }, [update]);
  useLayoutEffect(() => () => { ++lifetime.current; flight.current = null; ownedInput.current = null; }, []);
  useLayoutEffect(() => {
    // Only committed input changes own requests; an abandoned React render
    // must not invalidate the still-visible dialog's owner.
    if (inputSignature.current !== signature) { inputSignature.current = signature; close(); }
  }, [signature, close]);
  const begin = useCallback((phase: NonNullable<State["phase"]>) => {
    if (flight.current !== null || !current.current.open || !current.current.source || ownedInput.current !== inputSignature.current) return null;
    const generation = lifetime.current;
    const request = ++serial.current;
    const signature = inputSignature.current;
    flight.current = request;
    update({ phase, error: null });
    return { source: { ...current.current.source },
      owns: () => generation === lifetime.current && flight.current === request && signature === inputSignature.current,
      finish: () => {
        if (generation === lifetime.current && flight.current === request && signature === inputSignature.current) {
          flight.current = null; update({ phase: null });
        }
      } };
  }, [update]);
  const read = useCallback(async (index: number, reset = false) => {
    const owner = begin("read");
    if (!owner) return false;
    attemptedPage.current = index;
    try {
      const cursor = reset ? undefined : cursors.current[index];
      const page = await client.list(owner.source.sourceId, owner.source.provider,
        { host: "local", limit: LOCATION_PAGE_LIMIT, ...(cursor ? { cursor } : {}) });
      if (!owner.owns()) return false;
      validateSourceLocationPage(owner.source, page);
      if (reset) cursors.current = [undefined];
      update({ page: { ...page, locations: page.locations.map((location) => ({ ...location })) }, pageIndex: index });
      return true;
    } catch (cause) { if (owner.owns()) update({ error: String(cause) }); return false; }
    finally { owner.finish(); }
  }, [begin, client, update]);
  const start = useCallback(async () => {
    if (source.provider !== "cursor" || (source.location && source.location.host !== "local") || flight.current !== null) return false;
    ++lifetime.current;
    ownedInput.current = inputSignature.current;
    const previous = current.current;
    const same = previous.source?.sourceId === source.sourceId && previous.source?.rootPath === source.rootPath;
    current.current = { ...empty(), open: true, source: { ...source },
      attempt: same ? previous.attempt : null, path: same ? previous.attempt?.path ?? null : null };
    setState(current.current); cursors.current = [undefined]; attemptedPage.current = 0;
    return read(0, true);
  }, [read, source]);
  const pick = useCallback(async () => {
    if (current.current.attempt) return false;
    const owner = begin("pick");
    if (!owner) return false;
    try {
      const path = await client.pick();
      if (!owner.owns()) return false;
      if (path) update({ path, saved: null });
      return true;
    } catch (cause) { if (owner.owns()) update({ error: String(cause) }); return false; }
    finally { owner.finish(); }
  }, [begin, client, update]);
  const save = useCallback(async () => {
    const snapshot = current.current;
    if (!snapshot.path || !snapshot.source) return false;
    const owner = begin("save");
    if (!owner) return false;
    const request = snapshot.attempt ?? { sourceId: owner.source.sourceId, provider: owner.source.provider,
      host: "local", path: snapshot.path, expectedPrimaryRoot: owner.source.rootPath };
    update({ attempt: { ...request } });
    let accepted = false;
    try {
      const result = await client.associate({ ...request });
      if (!owner.owns()) return false;
      validateSourceLocation(owner.source, result);
      if (result.kind !== "associated") throw new Error("SOURCE_CHANGED: association returned a primary location");
      update({ saved: { ...result }, attempt: null, path: null }); accepted = true;
    } catch (cause) { if (owner.owns()) update({ error: String(cause) }); }
    finally { owner.finish(); }
    if (accepted) await read(0, true);
    return accepted;
  }, [begin, client, read, update]);
  const toggle = useCallback((location: SourceLocation) => {
    const snapshot = current.current;
    if (!snapshot.open || flight.current !== null || ownedInput.current !== inputSignature.current || snapshot.error || snapshot.attempt || !snapshot.source ||
      !snapshot.page?.locations.some((item) => item.locationId === location.locationId)) return;
    validateSourceLocation(snapshot.source, location);
    const selected = snapshot.selected;
    if (!selected.some((item) => item.locationId === location.locationId) && selected.length >= LOCATION_SELECTION_LIMIT) {
      update({ error: "RESOURCE_LIMIT: too many selected history roots" }); return;
    }
    update({ selected: selected.some((item) => item.locationId === location.locationId)
      ? selected.filter((item) => item.locationId !== location.locationId) : [...selected, { ...location }] });
  }, [update]);
  const next = useCallback(() => {
    const snapshot = current.current;
    if (!snapshot.page?.nextCursor || flight.current !== null) return Promise.resolve(false);
    cursors.current[snapshot.pageIndex + 1] = snapshot.page.nextCursor;
    return read(snapshot.pageIndex + 1);
  }, [read]);
  const takeSelection = useCallback(() => {
    const snapshot = current.current;
    if (!snapshot.open || flight.current !== null || ownedInput.current !== inputSignature.current || snapshot.error || snapshot.attempt ||
      !snapshot.source || !snapshot.selected.length) return null;
    const selected = snapshot.selected.map((location) => ({ ...location }));
    for (const location of selected) validateSourceLocation(snapshot.source, location);
    const result = { source: { ...snapshot.source }, locations: selected };
    close();
    return result;
  }, [close]);
  return { ...state, busy: state.phase !== null, start, close, pick, save, toggle, next, takeSelection,
    previous: () => current.current.pageIndex > 0 ? read(current.current.pageIndex - 1) : Promise.resolve(false),
    retry: () => read(attemptedPage.current), refresh: () => read(0, true) };
}
