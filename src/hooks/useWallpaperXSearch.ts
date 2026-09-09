import { useCallback, useEffect, useReducer, useRef, useState } from "react";
import * as api from "@/lib/api";
import { createWallpaperXSearchRequestId, isWallpaperXSearchBatch, type WallpaperXSearchBatch, isWallpaperXSearchProgress, type WallpaperXSearchProgress, type WallpaperXSearchStage } from "@/lib/wallpaperXSearch";
import { dedupeGalleryItems, type WallpaperGalleryItem, type WallpaperSearchResult } from "@/lib/wallpaperSource";

export type WallpaperXSearchClient = {
  loadMore: (continuationId: string, requestId: string) => Promise<WallpaperSearchResult>;
  search: (query: string, sort: "top" | "latest", requestId: string) => Promise<WallpaperSearchResult>;
  listenBatch: (handler: (batch: WallpaperXSearchBatch) => void) => Promise<() => void>;
  cancel: (requestId: string) => Promise<boolean>;
  listenProgress: (handler: (progress: WallpaperXSearchProgress) => void) => Promise<() => void>;
};

const DEFAULT_CLIENT: WallpaperXSearchClient = {
  search: api.wallpaperXSearch,
  loadMore: api.wallpaperXSearchMore,
  cancel: api.wallpaperXSearchCancel,
  listenProgress: api.listenWallpaperXSearchProgress,
  listenBatch: api.listenWallpaperXSearchBatch,
};

type ProgressiveState = {
  items: WallpaperGalleryItem[];
  accumulatedCount: number;
  done: boolean;
  seenBatchIndexes: ReadonlySet<number>;
};

type ProgressiveAction =
  | { type: "reset" }
  | { type: "batch"; batch: WallpaperXSearchBatch };

type PrefetchEntry = {
  continuationId: string;
  promise: Promise<WallpaperSearchResult | null>;
};

const EMPTY_PROGRESSIVE_STATE: ProgressiveState = {
  items: [],
  accumulatedCount: 0,
  done: false,
  seenBatchIndexes: new Set(),
};

function progressiveReducer(
  state: ProgressiveState,
  action: ProgressiveAction,
): ProgressiveState {
  if (action.type === "reset") return EMPTY_PROGRESSIVE_STATE;
  const { batch } = action;
  if (state.seenBatchIndexes.has(batch.batchIndex)) {
    const done = state.done || batch.done;
    return done === state.done ? state : { ...state, done };
  }
  const items = dedupeGalleryItems([...state.items, ...batch.items]);
  const seenBatchIndexes = new Set(state.seenBatchIndexes);
  seenBatchIndexes.add(batch.batchIndex);
  return {
    items,
    accumulatedCount: Math.max(
      state.accumulatedCount,
      batch.accumulatedCount,
      items.length,
    ),
    done: state.done || batch.done,
    seenBatchIndexes,
  };
}

export function useWallpaperXSearch(
  client: WallpaperXSearchClient = DEFAULT_CLIENT,
  requestIdFactory: () => string = createWallpaperXSearchRequestId,
) {
  const [loadingMore, setLoadingMore] = useState(false);
  const [busy, setBusy] = useState(false);
  const [stage, setStage] = useState<WallpaperXSearchStage | null>(null);
  const [progressive, dispatchProgressive] = useReducer(progressiveReducer, EMPTY_PROGRESSIVE_STATE);
  const active = useRef<string | null>(null);
  const activeKind = useRef<"foreground" | "prefetch" | null>(null);
  const prefetched = useRef<PrefetchEntry | null>(null);
  const generation = useRef(0);
  const mounted = useRef(true);

  useEffect(() => {
    mounted.current = true;
    let disposed = false;
    let unlisten: (() => void) | undefined;
    let unlistenBatch: (() => void) | undefined;
    void client.listenBatch((event) => {
      if (
        !disposed &&
        mounted.current &&
        activeKind.current === "foreground" &&
        isWallpaperXSearchBatch(event) &&
        event.requestId === active.current
      ) {
        dispatchProgressive({ type: "batch", batch: event });
      }
    }).then((cleanup) => { if (disposed) cleanup(); else unlistenBatch = cleanup; })
      .catch(() => { /* Invoke result remains authoritative if event registration fails. */ });
    void client.listenProgress((event) => {
      if (
        !disposed &&
        mounted.current &&
        activeKind.current === "foreground" &&
        isWallpaperXSearchProgress(event) &&
        event.requestId === active.current
      ) {
        setStage(event.stage);
      }
    }).then((cleanup) => {
      if (disposed) cleanup();
      else unlisten = cleanup;
    }).catch(() => { /* Progress is advisory; the invoke result is authoritative. */ });
    return () => {
      disposed = true;
      mounted.current = false;
      generation.current += 1;
      const id = active.current;
      active.current = null;
      activeKind.current = null;
      prefetched.current = null;
      unlisten?.();
      unlistenBatch?.();
      if (id) void client.cancel(id).catch(() => false);
    };
  }, [client]);

  const cancel = useCallback((): Promise<boolean> => {
    const id = active.current;
    generation.current += 1;
    active.current = null;
    activeKind.current = null;
    prefetched.current = null;
    if (mounted.current) {
      dispatchProgressive({ type: "reset" });
      setLoadingMore(false);
      setBusy(false);
      setStage(null);
    }
    return id ? client.cancel(id).catch(() => false) : Promise.resolve(false);
  }, [client]);

  const run = useCallback(async (query: string, sort: "top" | "latest", continuationId?: string) => {
    if (!mounted.current) return null;
    // Invalidate synchronously: a slow cancel acknowledgement must not delay
    // replacement or let a third request be overwritten by a waiting second.
    void cancel();
    const id = requestIdFactory();
    const revision = ++generation.current;
    active.current = id;
    activeKind.current = "foreground";
    setLoadingMore(continuationId !== undefined);
    setBusy(true);
    setStage("preparing");
    const current = () => mounted.current && generation.current === revision && active.current === id;
    try {
      const result = continuationId === undefined ? await client.search(query, sort, id) : await client.loadMore(continuationId, id);
      return current() && result.errorCode !== "cancelled" ? result : null;
    } catch (error) {
      if (!current()) return null;
      throw error;
    } finally {
      if (current()) {
        active.current = null;
        activeKind.current = null;
        setLoadingMore(false);
        setBusy(false);
        setStage(null);
      }
    }
  }, [cancel, client, requestIdFactory]);

  const search = useCallback((query: string, sort: "top" | "latest") => run(query, sort), [run]);
  const prefetchMore = useCallback(
    (continuationId: string): Promise<WallpaperSearchResult | null> => {
      if (!mounted.current) return Promise.resolve(null);
      const existing = prefetched.current;
      if (existing?.continuationId === continuationId) return existing.promise;
      if (activeKind.current === "foreground") return Promise.resolve(null);

      // A continuation is an exclusive Host lease. A different prefetch must
      // cancel the old request before it can claim a new request ID.
      const previousId = active.current;
      prefetched.current = null;
      const id = requestIdFactory();
      const revision = ++generation.current;
      active.current = id;
      activeKind.current = "prefetch";
      if (previousId) void client.cancel(previousId).catch(() => false);
      const current = () =>
        mounted.current &&
        generation.current === revision &&
        active.current === id &&
        activeKind.current === "prefetch";

      let entry!: PrefetchEntry;
      const promise = (async (): Promise<WallpaperSearchResult | null> => {
        let reusable = false;
        try {
          const result = await client.loadMore(continuationId, id);
          if (!current() || result.errorCode === "cancelled") return null;
          // Only a successful or confirmed-empty response consumes the Host
          // continuation. Background failures stay invisible and are retried
          // once as a foreground request when the user asks for more.
          reusable = !result.errorCode || result.errorCode === "empty";
          return reusable ? result : null;
        } catch {
          return null;
        } finally {
          if (current()) {
            active.current = null;
            activeKind.current = null;
          }
          if (!reusable && prefetched.current === entry) {
            prefetched.current = null;
          }
        }
      })();
      entry = { continuationId, promise };
      prefetched.current = entry;
      return promise;
    },
    [client, requestIdFactory],
  );

  const loadMore = useCallback(
    async (continuationId: string): Promise<WallpaperSearchResult | null> => {
      const entry = prefetched.current;
      if (!entry || entry.continuationId !== continuationId) {
        return run("", "top", continuationId);
      }

      // Consume the prepared page exactly once. If it is still running, the
      // foreground action waits for this same promise instead of opening a
      // second Responses request.
      prefetched.current = null;
      const revision = generation.current;
      setLoadingMore(true);
      setBusy(true);
      let result: WallpaperSearchResult | null = null;
      try {
        result = await entry.promise;
      } finally {
        if (mounted.current && generation.current === revision) {
          setLoadingMore(false);
          setBusy(false);
          setStage(null);
        }
      }
      if (!mounted.current || generation.current !== revision) return null;
      if (result) return result;
      return run("", "top", continuationId);
    },
    [run],
  );
  return {
    busy,
    loadingMore,
    stage,
    search,
    loadMore,
    prefetchMore,
    cancel,
    progressiveItems: progressive.items,
  };
}
