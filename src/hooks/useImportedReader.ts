import { useCallback, useEffect, useRef, useState } from "react";
import {
  sessionImportEventContext,
  sessionImportEvents,
  sessionImportExportMarkdown,
  sessionImportSearch,
  type ImportedRevisionHealth,
} from "@/lib/api/sessionMigration";
import { importedEventId } from "@/lib/sessionImport/eventView";
import { retainRecentEvents } from "@/lib/sessionImport/readerBudget";
import { capReaderHits } from "@/lib/sessionImport/readerWindow";
import { acceptReaderScope, type ReaderScope } from "@/lib/sessionImport/readerScope";

export type ReaderHit = {
  eventId: string;
  ordinal?: number;
  snippet: string;
  kind: string;
};

export type ImportedReaderClient = {
  events: typeof sessionImportEvents;
  context: typeof sessionImportEventContext;
  search: typeof sessionImportSearch;
  exportMarkdown: typeof sessionImportExportMarkdown;
};

const liveReaderClient: ImportedReaderClient = {
  events: sessionImportEvents,
  context: sessionImportEventContext,
  search: sessionImportSearch,
  exportMarkdown: sessionImportExportMarkdown,
};

type ReaderOwner = {
  importId: string;
  revision: string | null;
  leaf: string | null;
  gen: number;
  scope: ReaderScope | null;
};

type SearchToken = {
  identityGen: number;
  searchGen: number;
  query: string;
};

type FlightToken = {
  identityGen: number;
  query: string;
  cursor: string | null;
};

type JumpFlight = {
  owner: ReaderOwner;
  eventId: string;
  generation: number;
  promise: Promise<boolean>;
};

function uniqueHits(hits: ReaderHit[]): ReaderHit[] {
  const seen = new Set<string>();
  const out: ReaderHit[] = [];
  for (const hit of hits) {
    if (seen.has(hit.eventId)) continue;
    seen.add(hit.eventId);
    out.push(hit);
  }
  return out;
}

export function useImportedReader(
  importId: string | null,
  onError: (message: string) => void,
  opts?: { revision?: string | null; leaf?: string | null },
  client: ImportedReaderClient = liveReaderClient,
) {
  const revision = opts?.revision ?? null;
  const leaf = opts?.leaf ?? null;
  const [events, setEvents] = useState<unknown[]>([]);
  const [resolved, setResolved] = useState<{ owner: ReaderOwner; scope: ReaderScope } | null>(null);
  const [health, setHealth] = useState<ImportedRevisionHealth | null>(null);
  const [previousCursor, setPreviousCursor] = useState<string | null>(null);
  const [nextCursor, setNextCursor] = useState<string | null>(null);
  const [query, setQuery] = useState("");
  const [hits, setHits] = useState<ReaderHit[]>([]);
  const [hitsPrevious, setHitsPrevious] = useState<string | null>(null);
  const [hitsNext, setHitsNext] = useState<string | null>(null);
  const [hitsWindowed, setHitsWindowed] = useState(false);
  const [eventsWindowed, setEventsWindowed] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [searchError, setSearchError] = useState<string | null>(null);
  const [exportError, setExportError] = useState<string | null>(null);
  const exportFlightRef = useRef<ReaderOwner | null>(null);
  const ownerRef = useRef<ReaderOwner | null>(null);
  const initialFlightRef = useRef<{ owner: ReaderOwner; windowGen: number } | null>(null);
  const identityGenRef = useRef(0);
  const searchGenRef = useRef(0);
  const searchQueryRef = useRef("");
  const laneCountRef = useRef(0);
  const pageFlightRef = useRef<FlightToken | null>(null);
  const searchInitialFlightRef = useRef<FlightToken | null>(null);
  const searchPageFlightRef = useRef<FlightToken | null>(null);
  const jumpFlightRef = useRef<JumpFlight | null>(null);
  const jumpGenRef = useRef(0);
  const eventWindowGenRef = useRef(0);
  const eventsRef = useRef<unknown[]>([]);
  const hitsRef = useRef<ReaderHit[]>([]);
  const onErrorRef = useRef(onError);
  onErrorRef.current = onError;
  eventsRef.current = events;
  hitsRef.current = hits;

  const beginLane = useCallback(() => {
    laneCountRef.current += 1;
    setBusy(true);
  }, []);

  const endLane = useCallback((identityGen: number) => {
    if (ownerRef.current?.gen !== identityGen) return;
    laneCountRef.current = Math.max(0, laneCountRef.current - 1);
    setBusy(laneCountRef.current > 0);
  }, []);

  const isCurrentOwner = useCallback(
    (owner: ReaderOwner | null): owner is ReaderOwner =>
      owner != null && ownerRef.current === owner,
    [],
  );

  const isCurrentSearch = useCallback((token: SearchToken) => {
    const owner = ownerRef.current;
    return (
      owner != null &&
      owner.gen === token.identityGen &&
      searchGenRef.current === token.searchGen &&
      searchQueryRef.current === token.query
    );
  }, []);

  const captureOwner = useCallback(() => {
    const owner = ownerRef.current;
    if (
      !owner ||
      owner.importId !== importId ||
      owner.revision !== revision ||
      owner.leaf !== leaf
    ) {
      return null;
    }
    return owner;
  }, [importId, revision, leaf]);

  const pinScope = useCallback((owner: ReaderOwner, response: ReaderScope) => {
    const scope = acceptReaderScope(owner, owner.scope, response);
    if (owner.scope !== scope) {
      owner.scope = scope;
      setResolved({ owner, scope });
    }
  }, []);
  const getScope = useCallback(() => captureOwner()?.scope ?? null, [captureOwner]);

  const readInitialPage = useCallback(async (owner: ReaderOwner) => {
    if (!isCurrentOwner(owner)) return;
    const active = initialFlightRef.current;
    if (active?.owner === owner && active.windowGen === eventWindowGenRef.current) return;
    const flight = { owner, windowGen: ++eventWindowGenRef.current };
    initialFlightRef.current = flight;
    beginLane();
    try {
      const page = await client.events(owner.importId, owner.scope?.revision ?? owner.revision, null, 100, owner.scope?.leaf ?? owner.leaf);
      if (!isCurrentOwner(owner) || eventWindowGenRef.current !== flight.windowGen) return;
      pinScope(owner, page);
      setHealth(page.health ?? { outcome: "unknown", issueCount: null, issues: [], diagnosticsLimited: false });
      const retained = retainRecentEvents(page.events);
      eventsRef.current = retained;
      setEventsWindowed(retained.length < page.events.length);
      setEvents(retained);
      setPreviousCursor(page.previousCursor ?? null);
      setNextCursor(page.nextCursor);
      setError(null);
    } catch (cause) {
      if (!isCurrentOwner(owner) || eventWindowGenRef.current !== flight.windowGen) return;
      const message = String(cause);
      setError(message);
      onErrorRef.current(message);
    } finally {
      if (initialFlightRef.current === flight) initialFlightRef.current = null;
      endLane(owner.gen);
    }
  }, [beginLane, client, endLane, isCurrentOwner, pinScope]);

  useEffect(() => {
    setResolved(null);
    setEvents([]);
    setHealth(null);
    eventsRef.current = [];
    setPreviousCursor(null);
    setNextCursor(null);
    setHits([]);
    hitsRef.current = [];
    setHitsPrevious(null);
    setHitsNext(null);
    setHitsWindowed(false);
    setEventsWindowed(false);
    setQuery("");
    setNotice(null);
    setError(null);
    setSearchError(null);
    setExportError(null);
    exportFlightRef.current = null;
    initialFlightRef.current = null;
    laneCountRef.current = 0;
    pageFlightRef.current = null;
    searchInitialFlightRef.current = null;
    searchPageFlightRef.current = null;
    jumpFlightRef.current = null;
    jumpGenRef.current += 1;
    eventWindowGenRef.current += 1;
    searchGenRef.current += 1;
    searchQueryRef.current = "";
    if (!importId) {
      ownerRef.current = null;
      setBusy(false);
      return;
    }
    identityGenRef.current += 1;
    const owner: ReaderOwner = {
      importId,
      revision,
      leaf,
      gen: identityGenRef.current,
      scope: null,
    };
    ownerRef.current = owner;
    void readInitialPage(owner);
    return () => {
      if (ownerRef.current === owner) ownerRef.current = null;
    };
  }, [importId, revision, leaf, readInitialPage]);

  const reload = useCallback(async () => {
    const owner = captureOwner();
    if (owner) await readInitialPage(owner);
  }, [captureOwner, readInitialPage]);

  const search = useCallback(async () => {
    const owner = captureOwner();
    if (!owner?.scope) return;
    const inflight = searchInitialFlightRef.current;
    if (
      inflight &&
      inflight.identityGen === owner.gen &&
      inflight.query === query &&
      inflight.cursor == null
    ) {
      return;
    }
    searchGenRef.current += 1;
    if (searchQueryRef.current !== query) {
      hitsRef.current = [];
      setHits([]);
      setHitsPrevious(null);
      setHitsNext(null);
      setHitsWindowed(false);
    }
    searchQueryRef.current = query;
    searchPageFlightRef.current = null;
    searchInitialFlightRef.current = null;
    setSearchError(null);
    setNotice((current) => current === "empty" ? null : current);
    // Match the native query budget after trimming and Unicode lowercasing.
    // Validate on deliberate submission, never truncate an IME draft mid-edit.
    if (new TextEncoder().encode(query.trim().toLowerCase()).byteLength > 4096) {
      setSearchError("queryTooLong");
      return;
    }
    const token: SearchToken = {
      identityGen: owner.gen,
      searchGen: searchGenRef.current,
      query,
    };
    const flight: FlightToken = {
      identityGen: owner.gen,
      query,
      cursor: null,
    };
    searchInitialFlightRef.current = flight;
    beginLane();
    try {
      const page = await client.search(owner.importId, query, owner.scope.revision, null, owner.scope.leaf);
      if (!isCurrentSearch(token)) return;
      const capped = capReaderHits(uniqueHits(page.hits));
      hitsRef.current = capped.items;
      setHits(capped.items);
      setHitsWindowed(capped.windowed);
      setHitsPrevious(page.previousCursor ?? null);
      setHitsNext(page.nextCursor);
      setNotice(page.hits.length === 0 ? "empty" : null);
      setSearchError(null);
    } catch (cause) {
      if (!isCurrentSearch(token)) return;
      const msg = String(cause);
      setSearchError(msg);
      onErrorRef.current(msg);
    } finally {
      if (searchInitialFlightRef.current === flight) {
        searchInitialFlightRef.current = null;
      }
      endLane(token.identityGen);
    }
  }, [beginLane, captureOwner, client, endLane, isCurrentSearch, query]);

  const loadMoreHits = useCallback(async () => {
    const owner = captureOwner();
    if (!owner?.scope || !hitsNext) return;
    const inflight = searchPageFlightRef.current;
    if (
      inflight &&
      inflight.identityGen === owner.gen &&
      inflight.query === searchQueryRef.current &&
      inflight.cursor === hitsNext
    ) {
      return;
    }
    const token: SearchToken = {
      identityGen: owner.gen,
      searchGen: searchGenRef.current,
      query: searchQueryRef.current,
    };
    if (!isCurrentSearch(token)) return;
    const flight: FlightToken = {
      identityGen: owner.gen,
      query: token.query,
      cursor: hitsNext,
    };
    // The latest requested hit page owns both success and error results.
    searchPageFlightRef.current = flight;
    beginLane();
    try {
      const page = await client.search(
        owner.importId, token.query, owner.scope.revision, hitsNext, owner.scope.leaf, "forward",
      );
      if (!isCurrentSearch(token) || searchPageFlightRef.current !== flight) return;
      const capped = capReaderHits(uniqueHits(page.hits));
      hitsRef.current = capped.items;
      setHits(capped.items);
      setHitsWindowed(capped.windowed);
      setHitsPrevious(page.previousCursor ?? null);
      setHitsNext(page.nextCursor);
      setSearchError(null);
    } catch (cause) {
      if (!isCurrentSearch(token) || searchPageFlightRef.current !== flight) return;
      const msg = String(cause);
      setSearchError(msg);
      onErrorRef.current(msg);
    } finally {
      if (searchPageFlightRef.current === flight) {
        searchPageFlightRef.current = null;
      }
      endLane(token.identityGen);
    }
  }, [beginLane, captureOwner, client, endLane, hitsNext, isCurrentSearch]);

  const loadPreviousHits = useCallback(async () => {
    const owner = captureOwner();
    if (!owner?.scope || !hitsPrevious) return;
    const token: SearchToken = {
      identityGen: owner.gen,
      searchGen: searchGenRef.current,
      query: searchQueryRef.current,
    };
    if (!isCurrentSearch(token)) return;
    const inflight = searchPageFlightRef.current;
    if (
      inflight &&
      inflight.identityGen === owner.gen &&
      inflight.query === `backward:${token.query}` &&
      inflight.cursor === hitsPrevious
    ) {
      return;
    }
    const flight: FlightToken = {
      identityGen: owner.gen,
      query: `backward:${token.query}`,
      cursor: hitsPrevious,
    };
    // The latest requested hit page owns both success and error results.
    searchPageFlightRef.current = flight;
    beginLane();
    try {
      const page = await client.search(
        owner.importId,
        token.query,
        owner.scope.revision,
        hitsPrevious,
        owner.scope.leaf,
        "backward",
      );
      if (!isCurrentSearch(token) || searchPageFlightRef.current !== flight) return;
      const capped = capReaderHits(uniqueHits(page.hits));
      hitsRef.current = capped.items;
      setHits(capped.items);
      setHitsWindowed(capped.windowed);
      setHitsPrevious(page.previousCursor ?? null);
      setHitsNext(page.nextCursor);
      setSearchError(null);
    } catch (cause) {
      if (!isCurrentSearch(token) || searchPageFlightRef.current !== flight) return;
      const msg = String(cause);
      setSearchError(msg);
      onErrorRef.current(msg);
    } finally {
      if (searchPageFlightRef.current === flight) {
        searchPageFlightRef.current = null;
      }
      endLane(token.identityGen);
    }
  }, [beginLane, captureOwner, client, endLane, hitsPrevious, isCurrentSearch]);

  const loadMore = useCallback(async () => {
    const owner = captureOwner();
    if (!owner?.scope || nextCursor == null) return null;
    const inflight = pageFlightRef.current;
    if (
      inflight &&
      inflight.identityGen === owner.gen &&
      inflight.cursor === nextCursor
    ) {
      return null;
    }
    const flight: FlightToken = {
      identityGen: owner.gen,
      query: "",
      cursor: nextCursor,
    };
    pageFlightRef.current = flight;
    const eventWindowGen = ++eventWindowGenRef.current;
    beginLane();
    try {
      const page = await client.events(
        owner.importId,
        owner.scope.revision,
        nextCursor,
        100,
        owner.scope.leaf,
      );
      if (!isCurrentOwner(owner) || eventWindowGenRef.current !== eventWindowGen) return;
      pinScope(owner, page);
      setHealth(page.health ?? { outcome: "unknown", issueCount: null, issues: [], diagnosticsLimited: false });
      const merged = [...eventsRef.current, ...page.events];
      const retained = retainRecentEvents(merged);
      eventsRef.current = retained;
      setEvents(retained);
      if (retained.length < merged.length) setEventsWindowed(true);
      setPreviousCursor(page.previousCursor ?? null);
      setNextCursor(page.nextCursor);
      setError(null);
      return page.events.map(importedEventId).find((id): id is string => !!id) ?? null;
    } catch (cause) {
      if (!isCurrentOwner(owner) || eventWindowGenRef.current !== eventWindowGen) return;
      const msg = String(cause);
      setError(msg);
      onErrorRef.current(msg);
      return null;
    } finally {
      if (pageFlightRef.current === flight) {
        pageFlightRef.current = null;
      }
      endLane(owner.gen);
    }
  }, [beginLane, captureOwner, client, endLane, isCurrentOwner, nextCursor, pinScope]);

  const loadPrevious = useCallback(async () => {
    const owner = captureOwner();
    if (!owner?.scope || previousCursor == null) return null;
    const inflight = pageFlightRef.current;
    if (
      inflight &&
      inflight.identityGen === owner.gen &&
      inflight.query === "backward" &&
      inflight.cursor === previousCursor
    ) {
      return null;
    }
    const flight: FlightToken = {
      identityGen: owner.gen,
      query: "backward",
      cursor: previousCursor,
    };
    pageFlightRef.current = flight;
    const eventWindowGen = ++eventWindowGenRef.current;
    beginLane();
    try {
      const page = await client.events(
        owner.importId,
        owner.scope.revision,
        previousCursor,
        100,
        owner.scope.leaf,
        "backward",
      );
      if (!isCurrentOwner(owner) || eventWindowGenRef.current !== eventWindowGen) return;
      pinScope(owner, page);
      setHealth(page.health ?? { outcome: "unknown", issueCount: null, issues: [], diagnosticsLimited: false });
      const retained = retainRecentEvents(page.events);
      eventsRef.current = retained;
      setEvents(retained);
      setEventsWindowed(retained.length < page.events.length);
      setPreviousCursor(page.previousCursor ?? null);
      setNextCursor(page.nextCursor);
      setError(null);
      return page.events.map(importedEventId).filter((id): id is string => !!id).at(-1) ?? null;
    } catch (cause) {
      if (!isCurrentOwner(owner) || eventWindowGenRef.current !== eventWindowGen) return;
      const msg = String(cause);
      setError(msg);
      onErrorRef.current(msg);
      return null;
    } finally {
      if (pageFlightRef.current === flight) {
        pageFlightRef.current = null;
      }
      endLane(owner.gen);
    }
  }, [beginLane, captureOwner, client, endLane, isCurrentOwner, previousCursor, pinScope]);

  const ensureEvent = useCallback(
    (eventId: string) => {
      const owner = captureOwner();
      if (!owner) return Promise.resolve(false);
      const has = (list: unknown[]) => list.some((e) => importedEventId(e) === eventId);
      const inflight = jumpFlightRef.current;
      if (inflight && inflight.owner === owner && inflight.eventId === eventId) {
        return inflight.promise;
      }
      if (has(eventsRef.current)) {
        jumpGenRef.current += 1;
        eventWindowGenRef.current += 1;
        jumpFlightRef.current = null;
        return Promise.resolve(true);
      }
      const generation = ++jumpGenRef.current;
      const eventWindowGen = ++eventWindowGenRef.current;
      const promise = (async () => {
        beginLane();
        try {
          const context = await client.context(
            owner.importId,
            eventId,
            owner.scope?.revision ?? owner.revision,
            owner.scope?.leaf ?? owner.leaf,
            { before: 8, after: 8 },
          );
          if (
            !isCurrentOwner(owner) ||
            jumpGenRef.current !== generation ||
            eventWindowGenRef.current !== eventWindowGen
          ) {
            return false;
          }
          const target = context.events.some((event) => importedEventId(event) === eventId);
          if (!target) return false;
          pinScope(owner, context);
          setHealth(context.health ?? { outcome: "unknown", issueCount: null, issues: [], diagnosticsLimited: false });
          const retained = retainRecentEvents(context.events);
          eventsRef.current = retained;
          setEvents(retained);
          setEventsWindowed(false);
          setPreviousCursor(context.previousCursor);
          setNextCursor(context.nextCursor);
          setError(null);
          return true;
        } catch (cause) {
          if (
            !isCurrentOwner(owner) ||
            jumpGenRef.current !== generation ||
            eventWindowGenRef.current !== eventWindowGen
          ) {
            return false;
          }
          const msg = String(cause);
          setError(msg);
          onErrorRef.current(msg);
          return false;
        } finally {
          if (jumpFlightRef.current?.generation === generation) {
            jumpFlightRef.current = null;
          }
          endLane(owner.gen);
        }
      })();
      jumpFlightRef.current = { owner, eventId, generation, promise };
      return promise;
    },
    [beginLane, captureOwner, client, endLane, isCurrentOwner, pinScope],
  );

  const clearSearch = useCallback(() => {
    searchGenRef.current += 1;
    searchQueryRef.current = "";
    searchInitialFlightRef.current = null;
    searchPageFlightRef.current = null;
    setQuery("");
    setHits([]);
    hitsRef.current = [];
    setHitsPrevious(null);
    setHitsNext(null);
    setHitsWindowed(false);
    setSearchError(null);
    setNotice((current) => current === "empty" ? null : current);
  }, []);

  const exportMarkdown = useCallback(async () => {
    const owner = captureOwner();
    if (!owner?.scope || exportFlightRef.current?.gen === owner.gen) return;
    exportFlightRef.current = owner;
    setNotice(null);
    setExportError(null);
    beginLane();
    let copying = false;
    try {
      const md = await client.exportMarkdown(
        owner.importId,
        owner.scope.revision,
        owner.scope.leaf,
      );
      if (!isCurrentOwner(owner)) return;
      copying = true;
      await navigator.clipboard.writeText(md);
      if (!isCurrentOwner(owner)) return;
      setNotice("copied");
    } catch (cause) {
      if (!isCurrentOwner(owner)) return;
      const msg = String(cause);
      // Export is a secondary operation. A clipboard failure must not hide
      // an otherwise readable transcript or discard its cursor/diagnostics.
      if (copying) setNotice("copyFailed");
      else setExportError(msg);
      onErrorRef.current(msg);
    } finally {
      if (exportFlightRef.current === owner) exportFlightRef.current = null;
      endLane(owner.gen);
    }
  }, [beginLane, captureOwner, client, endLane, isCurrentOwner]);

  const scope = resolved && resolved.owner.importId === importId &&
    resolved.owner.revision === revision && resolved.owner.leaf === leaf ? resolved.scope : null;

  return {
    scope,
    getScope,
    events,
    health,
    previousCursor,
    nextCursor,
    query,
    setQuery,
    hits,
    hitsPrevious,
    hitsNext,
    hitsWindowed,
    eventsWindowed,
    notice,
    error,
    searchError,
    exportError,
    busy,
    reload,
    search,
    loadMore,
    loadPrevious,
    loadMoreHits,
    loadPreviousHits,
    ensureEvent,
    clearSearch,
    exportMarkdown,
  };
}
