import { useCallback, useEffect, useReducer, useRef, useState } from "react";
import * as api from "@/lib/api";
import {
  createWallpaperXSearchRequestId,
  isWallpaperXSearchBatch,
  isWallpaperXSearchProgress,
  type WallpaperXSearchBatch,
  type WallpaperXSearchProgress,
  type WallpaperXSearchStage,
} from "@/lib/wallpaperXSearch";
import {
  dedupeGalleryItems,
  type WallpaperGalleryItem,
  type WallpaperSearchResult,
} from "@/lib/wallpaperSource";

export type WallpaperXSearchClient = {
  search: (
    query: string,
    sort: "top" | "latest",
    requestId: string,
  ) => Promise<WallpaperSearchResult>;
  cancel: (requestId: string) => Promise<boolean>;
  listenProgress: (
    handler: (progress: WallpaperXSearchProgress) => void,
  ) => Promise<() => void>;
  listenBatch: (
    handler: (batch: WallpaperXSearchBatch) => void,
  ) => Promise<() => void>;
};

const DEFAULT_CLIENT: WallpaperXSearchClient = {
  search: api.wallpaperXSearch,
  cancel: api.wallpaperXSearchCancel,
  listenProgress: api.listenWallpaperXSearchProgress,
  listenBatch: api.listenWallpaperXSearchBatch,
};

export type UseWallpaperXSearchResult = {
  busy: boolean;
  requestId: string | null;
  stage: WallpaperXSearchStage | null;
  progressiveItems: WallpaperGalleryItem[];
  progressiveCount: number;
  progressiveDone: boolean;
  search: (
    query: string,
    sort: "top" | "latest",
  ) => Promise<WallpaperSearchResult | null>;
  cancel: () => Promise<boolean>;
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

/**
 * Own one X-search generation at a time. A cancel or replacement invalidates
 * the previous generation immediately, so late IPC results cannot repopulate
 * a reopened modal even if an older Host takes time to stop.
 */
export function useWallpaperXSearch(
  client: WallpaperXSearchClient = DEFAULT_CLIENT,
  requestIdFactory: () => string = createWallpaperXSearchRequestId,
): UseWallpaperXSearchResult {
  const [busy, setBusy] = useState(false);
  const [requestId, setRequestId] = useState<string | null>(null);
  const [stage, setStage] = useState<WallpaperXSearchStage | null>(null);
  const [progressive, dispatchProgressive] = useReducer(
    progressiveReducer,
    EMPTY_PROGRESSIVE_STATE,
  );
  const activeRequestRef = useRef<string | null>(null);
  const generationRef = useRef(0);
  const mountedRef = useRef(true);

  useEffect(() => {
    let disposed = false;
    let unlistenProgress: (() => void) | null = null;
    let unlistenBatch: (() => void) | null = null;
    void client
      .listenProgress((progress) => {
        if (
          !isWallpaperXSearchProgress(progress) ||
          progress.requestId !== activeRequestRef.current ||
          !mountedRef.current
        ) {
          return;
        }
        setStage(progress.stage);
      })
      .then((cleanup) => {
        if (disposed) cleanup();
        else unlistenProgress = cleanup;
      })
      .catch(() => {
        // Progress is advisory. The invoke result remains authoritative.
      });
    void client
      .listenBatch((batch) => {
        if (
          !isWallpaperXSearchBatch(batch) ||
          batch.requestId !== activeRequestRef.current ||
          !mountedRef.current
        ) {
          return;
        }
        dispatchProgressive({ type: "batch", batch });
      })
      .then((cleanup) => {
        if (disposed) cleanup();
        else unlistenBatch = cleanup;
      })
      .catch(() => {
        // Batch delivery is advisory. The invoke result remains authoritative.
      });
    return () => {
      disposed = true;
      unlistenProgress?.();
      unlistenBatch?.();
    };
  }, [client]);

  const cancel = useCallback(async (): Promise<boolean> => {
    const activeRequest = activeRequestRef.current;
    if (!activeRequest) return false;
    generationRef.current += 1;
    activeRequestRef.current = null;
    if (mountedRef.current) {
      setBusy(false);
      setRequestId(null);
      setStage(null);
      dispatchProgressive({ type: "reset" });
    }
    try {
      return await client.cancel(activeRequest);
    } catch {
      return false;
    }
  }, [client]);

  const search = useCallback(
    async (
      query: string,
      sort: "top" | "latest",
    ): Promise<WallpaperSearchResult | null> => {
      if (activeRequestRef.current) {
        await cancel();
      }

      const nextRequestId = requestIdFactory();
      const generation = generationRef.current + 1;
      generationRef.current = generation;
      activeRequestRef.current = nextRequestId;
      if (mountedRef.current) {
        setBusy(true);
        setRequestId(nextRequestId);
        setStage("preparing");
        dispatchProgressive({ type: "reset" });
      }

      try {
        const result = await client.search(query, sort, nextRequestId);
        if (
          generationRef.current !== generation ||
          activeRequestRef.current !== nextRequestId
        ) {
          return null;
        }
        if (result.meta?.requestId && result.meta.requestId !== nextRequestId) {
          return null;
        }
        return result;
      } catch (error) {
        if (
          generationRef.current !== generation ||
          activeRequestRef.current !== nextRequestId
        ) {
          return null;
        }
        throw error;
      } finally {
        if (
          generationRef.current === generation &&
          activeRequestRef.current === nextRequestId
        ) {
          activeRequestRef.current = null;
          if (mountedRef.current) {
            setBusy(false);
            setRequestId(null);
            setStage(null);
          }
        }
      }
    },
    [cancel, client, requestIdFactory],
  );

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
      generationRef.current += 1;
      const activeRequest = activeRequestRef.current;
      activeRequestRef.current = null;
      if (activeRequest) {
        void client.cancel(activeRequest).catch(() => false);
      }
    };
  }, [client]);

  return {
    busy,
    requestId,
    stage,
    progressiveItems: progressive.items,
    progressiveCount: progressive.accumulatedCount,
    progressiveDone: progressive.done,
    search,
    cancel,
  };
}
