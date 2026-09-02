import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type Dispatch,
  type SetStateAction,
} from "react";
import { useWallpaperRemoteSearch } from "@/hooks/useWallpaperRemoteSearch";
import type { MessageKey } from "@/i18n";
import { isDesktopHost } from "@/lib/api";
import {
  wallpaperRemoteProgressMessageKey,
  wallpaperRemoteUiError,
  type WallpaperRemoteSearchResult,
  type WallpaperRemoteSource,
} from "@/lib/wallpaperRemoteSearch";
import {
  dedupeGalleryItems,
  parseWallpaperSourceError,
  type WallpaperGalleryItem,
  type WallpaperSourceErrorCode,
} from "@/lib/wallpaperSource";
import type { WallpaperGalleryKindFilter } from "@/lib/wallpaperGalleryPro";
import { clearRemoteWallpaperThumbnailCache } from "@/lib/remoteWallpaperThumbnail";
import { wallpaperSourceErrorMessage } from "@/lib/wallpaperSourcePresentation";

type Translate = (
  key: MessageKey,
  vars?: Record<string, string | number | undefined | null>,
) => string;

type Options = {
  enabled: boolean;
  source: WallpaperRemoteSource | null;
  query: string;
  items: WallpaperGalleryItem[];
  t: Translate;
  setItems: Dispatch<SetStateAction<WallpaperGalleryItem[]>>;
  setHasSearched: Dispatch<SetStateAction<boolean>>;
  setSelectedId: Dispatch<SetStateAction<string | null>>;
  setError: Dispatch<SetStateAction<string | null>>;
  setErrorCode: Dispatch<SetStateAction<WallpaperSourceErrorCode | null>>;
  setStatusHint: Dispatch<SetStateAction<string | null>>;
  setGalleryFilter: Dispatch<SetStateAction<string>>;
  setKindFilter: Dispatch<SetStateAction<WallpaperGalleryKindFilter>>;
};

type Continuation = {
  source: WallpaperRemoteSource;
  query: string;
};

type PrefetchOutcome = {
  continuation: Continuation;
  result: WallpaperRemoteSearchResult | null;
  error: unknown | null;
};

function sameContinuation(
  left: Continuation,
  right: Continuation,
): boolean {
  return left.source === right.source && left.query === right.query;
}

export function useWallpaperRemoteSourceController({
  enabled,
  source,
  query,
  items,
  t,
  setItems,
  setHasSearched,
  setSelectedId,
  setError,
  setErrorCode,
  setStatusHint,
  setGalleryFilter,
  setKindFilter,
}: Options) {
  const remote = useWallpaperRemoteSearch();
  const [hasMore, setHasMore] = useState(false);
  const [loadingMore, setLoadingMore] = useState(false);
  const [prefetching, setPrefetching] = useState(false);
  const continuationRef = useRef<Continuation | null>(null);
  const progressiveRequestRef = useRef<string | null>(null);
  const appliedProgressiveKeysRef = useRef<Set<string>>(new Set());
  const prefetchingRef = useRef(false);
  const prefetchGenerationRef = useRef(0);
  const prefetchRequestIdRef = useRef<string | null>(null);
  const prefetchOutcomeRef = useRef<PrefetchOutcome | null>(null);
  const prefetchPromiseRef = useRef<Promise<PrefetchOutcome | null> | null>(
    null,
  );
  const itemsRef = useRef(items);
  itemsRef.current = items;

  const discardPrefetch = useCallback(() => {
    prefetchGenerationRef.current += 1;
    prefetchingRef.current = false;
    prefetchRequestIdRef.current = null;
    prefetchOutcomeRef.current = null;
    prefetchPromiseRef.current = null;
    setPrefetching(false);
  }, []);

  const cancel = useCallback(async (): Promise<boolean> => {
    discardPrefetch();
    return remote.cancel();
  }, [discardPrefetch, remote.cancel]);

  const startPrefetch = useCallback(
    (continuation: Continuation): Promise<PrefetchOutcome | null> => {
      const generation = prefetchGenerationRef.current + 1;
      prefetchGenerationRef.current = generation;
      prefetchingRef.current = true;
      prefetchOutcomeRef.current = null;
      setPrefetching(true);

      let task: Promise<PrefetchOutcome | null>;
      task = remote
        .loadMore(continuation.source, continuation.query)
        .then(
          (result): PrefetchOutcome | null => {
            if (prefetchGenerationRef.current !== generation) return null;
            const outcome = {
              continuation,
              result,
              error: null,
            } satisfies PrefetchOutcome;
            prefetchOutcomeRef.current = outcome;
            return outcome;
          },
          (error): PrefetchOutcome | null => {
            if (prefetchGenerationRef.current !== generation) return null;
            const outcome = {
              continuation,
              result: null,
              error,
            } satisfies PrefetchOutcome;
            prefetchOutcomeRef.current = outcome;
            return outcome;
          },
        )
        .finally(() => {
          if (prefetchGenerationRef.current !== generation) return;
          prefetchingRef.current = false;
          if (prefetchPromiseRef.current === task) {
            prefetchPromiseRef.current = null;
          }
          setPrefetching(false);
        });
      prefetchPromiseRef.current = task;
      return task;
    },
    [remote.loadMore],
  );

  useEffect(() => {
    if (enabled) return;
    discardPrefetch();
    continuationRef.current = null;
    progressiveRequestRef.current = null;
    appliedProgressiveKeysRef.current.clear();
    setHasMore(false);
    setLoadingMore(false);
    void remote.cancel();
  }, [discardPrefetch, enabled, remote.cancel]);

  useEffect(() => {
    if (prefetching && remote.busy && remote.requestId) {
      prefetchRequestIdRef.current = remote.requestId;
    }
  }, [prefetching, remote.busy, remote.requestId]);

  useEffect(() => {
    if (
      !enabled ||
      !source ||
      prefetchingRef.current ||
      !remote.busy ||
      remote.source !== source ||
      !remote.requestId ||
      remote.requestId === prefetchRequestIdRef.current
    ) {
      return;
    }
    if (progressiveRequestRef.current !== remote.requestId) {
      progressiveRequestRef.current = remote.requestId;
      appliedProgressiveKeysRef.current.clear();
    }
    const fresh = remote.progressiveItems.filter((item) => {
      const key = (item.localPath || item.fullUrl || item.id).trim();
      if (!key || appliedProgressiveKeysRef.current.has(key)) return false;
      appliedProgressiveKeysRef.current.add(key);
      return true;
    });
    if (fresh.length === 0) return;
    setItems((current) => dedupeGalleryItems([...current, ...fresh]));
    setHasSearched(true);
  }, [
    enabled,
    remote.busy,
    remote.progressiveItems,
    remote.requestId,
    remote.source,
    prefetching,
    setHasSearched,
    setItems,
    source,
  ]);

  const progress = useMemo(() => {
    if (prefetching || !remote.busy || remote.source !== source) return null;
    const key = wallpaperRemoteProgressMessageKey(remote.stage, source);
    return key ? t(key as MessageKey) : null;
  }, [prefetching, remote.busy, remote.source, remote.stage, source, t]);

  const resetForSearch = useCallback(() => {
    discardPrefetch();
    clearRemoteWallpaperThumbnailCache();
    setError(null);
    setErrorCode(null);
    setStatusHint(null);
    setSelectedId(null);
    setGalleryFilter("");
    setKindFilter("all");
    setHasSearched(false);
    setItems([]);
    setHasMore(false);
    setLoadingMore(false);
    continuationRef.current = null;
    progressiveRequestRef.current = null;
    appliedProgressiveKeysRef.current.clear();
  }, [
    discardPrefetch,
    setError,
    setErrorCode,
    setGalleryFilter,
    setHasSearched,
    setItems,
    setKindFilter,
    setSelectedId,
    setStatusHint,
  ]);

  const search = useCallback(async () => {
    const activeSource = source;
    const normalizedQuery = query.trim().split(/\s+/).join(" ");
    if (!activeSource || !normalizedQuery) {
      setErrorCode("empty");
      setError(wallpaperSourceErrorMessage(t, "empty"));
      return;
    }
    if (!isDesktopHost()) {
      setErrorCode("generic");
      setError(t("settings.wallpaperSource.err.desktopOnly"));
      return;
    }
    resetForSearch();
    try {
      const result = await remote.search(activeSource, normalizedQuery);
      if (!result) return;
      const list = dedupeGalleryItems(result.items);
      const code = wallpaperRemoteUiError({ ...result, items: list });
      setHasSearched(true);
      if (code) {
        setItems([]);
        setErrorCode(code);
        setError(wallpaperSourceErrorMessage(t, code));
        return;
      }
      setItems(list);
      setError(null);
      setErrorCode(null);
      setHasMore(result.hasMore);
      const continuation = result.hasMore
        ? { source: activeSource, query: normalizedQuery }
        : null;
      continuationRef.current = continuation;
      const seconds = (result.durationMs / 1000).toFixed(1);
      setStatusHint(
        t(
          result.cacheHit
            ? "settings.wallpaperSource.remote.completeCached"
            : "settings.wallpaperSource.remote.complete",
          { count: list.length, seconds },
        ),
      );
      if (continuation) void startPrefetch(continuation);
    } catch (error) {
      setHasSearched(true);
      const code = parseWallpaperSourceError(error);
      setErrorCode(code);
      setError(wallpaperSourceErrorMessage(t, code));
      continuationRef.current = null;
      setHasMore(false);
    }
  }, [
    query,
    remote.search,
    resetForSearch,
    setError,
    setErrorCode,
    setHasSearched,
    setItems,
    setStatusHint,
    source,
    startPrefetch,
    t,
  ]);

  const loadMore = useCallback(async () => {
    const continuation = continuationRef.current;
    if (!continuation || (remote.busy && !prefetchingRef.current)) return;
    const generation = prefetchGenerationRef.current;
    const initialItems = itemsRef.current;
    setError(null);
    setErrorCode(null);
    setStatusHint(null);
    progressiveRequestRef.current = null;
    appliedProgressiveKeysRef.current.clear();
    setLoadingMore(true);
    try {
      let prefetched = prefetchOutcomeRef.current;
      if (!prefetched && prefetchPromiseRef.current) {
        prefetched = await prefetchPromiseRef.current;
      }
      if (
        generation !== prefetchGenerationRef.current ||
        !continuationRef.current ||
        !sameContinuation(continuationRef.current, continuation)
      ) {
        return;
      }
      if (
        prefetched &&
        !sameContinuation(prefetched.continuation, continuation)
      ) {
        prefetched = null;
      }
      if (prefetched) prefetchOutcomeRef.current = null;
      // A hidden prefetch failure is not a user-visible paging attempt. An
      // explicit click gets one fresh foreground request; failure there is
      // surfaced normally, without an automatic retry loop.
      const prefetchedCode = prefetched?.result
        ? wallpaperRemoteUiError({
            ...prefetched.result,
            items: dedupeGalleryItems(prefetched.result.items),
          })
        : null;
      if (
        prefetched?.error ||
        (prefetchedCode !== null && prefetchedCode !== "empty")
      ) {
        prefetched = null;
      }
      const result =
        prefetched?.result ??
        (await remote.loadMore(continuation.source, continuation.query));
      if (!result) return;
      if (
        generation !== prefetchGenerationRef.current ||
        !continuationRef.current ||
        !sameContinuation(continuationRef.current, continuation)
      ) {
        return;
      }
      const extra = dedupeGalleryItems(result.items);
      const code = wallpaperRemoteUiError({ ...result, items: extra });
      if (code) {
        if (code === "empty") {
          const initialIds = new Set(initialItems.map((item) => item.id));
          setItems((current) =>
            current.filter((item) => initialIds.has(item.id)),
          );
          continuationRef.current = null;
          setHasMore(false);
          setStatusHint(t("settings.wallpaperSource.noMore"));
        } else {
          setErrorCode(code);
          setError(wallpaperSourceErrorMessage(t, code));
        }
        return;
      }
      const merged = dedupeGalleryItems([...itemsRef.current, ...extra]);
      setItems(merged);
      setError(null);
      setErrorCode(null);
      setHasMore(result.hasMore);
      const nextContinuation = result.hasMore ? continuation : null;
      continuationRef.current = nextContinuation;
      setStatusHint(
        t("settings.wallpaperSource.remote.loadedMore", {
          count: extra.length,
          total: merged.length,
        }),
      );
      if (nextContinuation) void startPrefetch(nextContinuation);
    } catch (error) {
      const code = parseWallpaperSourceError(error);
      setErrorCode(code);
      setError(wallpaperSourceErrorMessage(t, code));
    } finally {
      setLoadingMore(false);
    }
  }, [
    remote.busy,
    remote.loadMore,
    setError,
    setErrorCode,
    setItems,
    setStatusHint,
    startPrefetch,
    t,
  ]);

  return {
    busy: !prefetching && remote.busy && remote.source === source,
    loadingMore,
    progress,
    canLoadMore: hasMore && items.length > 0,
    search,
    loadMore,
    cancel,
  };
}
