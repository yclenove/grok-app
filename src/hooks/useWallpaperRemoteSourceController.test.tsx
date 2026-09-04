/**
 * @vitest-environment jsdom
 */
import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { useState } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { MessageKey } from "@/i18n";
import type {
  WallpaperRemoteSearchResult,
  WallpaperRemoteSearchStage,
  WallpaperRemoteSource,
} from "@/lib/wallpaperRemoteSearch";
import type {
  WallpaperGalleryItem,
  WallpaperSourceErrorCode,
} from "@/lib/wallpaperSource";
import type { WallpaperGalleryKindFilter } from "@/lib/wallpaperGalleryPro";

const remote = vi.hoisted(() => ({
  busy: false,
  source: null as WallpaperRemoteSource | null,
  requestId: null as string | null,
  stage: null as WallpaperRemoteSearchStage | null,
  progressiveItems: [] as WallpaperGalleryItem[],
  progressiveCount: 0,
  progressiveDone: false,
  search: vi.fn(),
  loadMore: vi.fn(),
  cancel: vi.fn(async () => true),
}));

vi.mock("@/hooks/useWallpaperRemoteSearch", () => ({
  useWallpaperRemoteSearch: () => remote,
}));

vi.mock("@/lib/api", () => ({
  isDesktopHost: () => true,
  wallpaperRemoteCancelMediaRequests: vi.fn(async () => 0),
}));

import { useWallpaperRemoteSourceController } from "./useWallpaperRemoteSourceController";

type HarnessOptions = {
  enabled?: boolean;
  source?: WallpaperRemoteSource | null;
  query?: string;
};

function galleryItem(id: string, source: WallpaperRemoteSource = "web") {
  return {
    id,
    thumbUrl: `https://images.example.test/${id}.jpg`,
    fullUrl: `https://images.example.test/${id}.jpg`,
    kind: "image",
    source,
    sourceUrl: `https://example.test/${id}`,
    sourceName: "example.test",
  } satisfies WallpaperGalleryItem;
}

function result(
  items: WallpaperGalleryItem[],
  overrides: Partial<WallpaperRemoteSearchResult> = {},
): WallpaperRemoteSearchResult {
  return {
    source: "web",
    items,
    hasMore: false,
    cacheHit: false,
    durationMs: 1_000,
    ...overrides,
  };
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((next) => {
    resolve = next;
  });
  return { promise, resolve };
}

function translate(
  key: MessageKey,
  vars?: Record<string, string | number | null | undefined>,
) {
  if (key === "settings.wallpaperSource.err.auth_required") {
    return "Sign-in required";
  }
  const suffix = vars
    ? Object.entries(vars)
        .map(([name, value]) => `${name}=${value}`)
        .join(",")
    : "";
  return suffix ? `${key}:${suffix}` : key;
}

function useHarness({
  enabled = true,
  source = "web",
  query = "mountains",
}: HarnessOptions) {
  const [items, setItems] = useState<WallpaperGalleryItem[]>([]);
  const [hasSearched, setHasSearched] = useState(false);
  const [selectedId, setSelectedId] = useState<string | null>("old");
  const [error, setError] = useState<string | null>(null);
  const [errorCode, setErrorCode] =
    useState<WallpaperSourceErrorCode | null>(null);
  const [statusHint, setStatusHint] = useState<string | null>(null);
  const [galleryFilter, setGalleryFilter] = useState("old filter");
  const [kindFilter, setKindFilter] =
    useState<WallpaperGalleryKindFilter>("image");
  const controller = useWallpaperRemoteSourceController({
    enabled,
    source,
    query,
    items,
    t: translate,
    setItems,
    setHasSearched,
    setSelectedId,
    setError,
    setErrorCode,
    setStatusHint,
    setGalleryFilter,
    setKindFilter,
  });
  return {
    controller,
    items,
    setItems,
    hasSearched,
    selectedId,
    error,
    errorCode,
    statusHint,
    galleryFilter,
    kindFilter,
  };
}

afterEach(() => {
  cleanup();
  remote.busy = false;
  remote.source = null;
  remote.requestId = null;
  remote.stage = null;
  remote.progressiveItems = [];
  remote.progressiveCount = 0;
  remote.progressiveDone = false;
  remote.search.mockReset();
  remote.loadMore.mockReset();
  remote.cancel.mockClear();
});

describe("useWallpaperRemoteSourceController", () => {
  it("keeps paging available when an initial validated page is empty", async () => {
    remote.search.mockResolvedValue(
      result([], { errorCode: "empty", hasMore: true }),
    );
    remote.loadMore.mockResolvedValue(result([galleryItem("second-page")]));
    const hook = renderHook(() => useHarness({ query: "rare subject" }));

    await act(async () => hook.result.current.controller.search());

    expect(hook.result.current.items).toEqual([]);
    expect(hook.result.current.errorCode).toBe("empty");
    expect(hook.result.current.controller.canLoadMore).toBe(true);
    await act(async () => hook.result.current.controller.loadMore());
    expect(remote.loadMore).toHaveBeenCalledWith("web", "rare subject");
    expect(hook.result.current.items.map((item) => item.id)).toEqual([
      "second-page",
    ]);
  });

  it("isolates paging state by provider and normalized query", async () => {
    remote.search.mockResolvedValue(
      result([galleryItem("openverse-first", "openverse")], {
        source: "openverse",
        hasMore: true,
      }),
    );
    remote.loadMore.mockResolvedValue(
      result([galleryItem("openverse-second", "openverse")], {
        source: "openverse",
        hasMore: true,
      }),
    );
    const hook = renderHook(
      ({ source, query }) => useHarness({ source, query }),
      {
        initialProps: {
          source: "openverse" as WallpaperRemoteSource,
          query: "misty   coast",
        },
      },
    );

    await act(async () => hook.result.current.controller.search());
    await waitFor(() => expect(remote.loadMore).toHaveBeenCalledTimes(1));
    expect(hook.result.current.controller.canLoadMore).toBe(true);

    hook.rerender({ source: "pexels", query: "misty   coast" });
    expect(hook.result.current.controller.canLoadMore).toBe(false);

    hook.rerender({ source: "openverse", query: "different coast" });
    await waitFor(() =>
      expect(hook.result.current.controller.canLoadMore).toBe(false),
    );
    hook.rerender({ source: "openverse", query: "misty coast" });
    expect(hook.result.current.controller.canLoadMore).toBe(false);
  });

  it("normalizes a search, resets stale UI, and preserves paging state", async () => {
    remote.search.mockResolvedValue(
      result([galleryItem("first")], { hasMore: true }),
    );
    remote.loadMore.mockResolvedValue(result([galleryItem("second")]));
    const hook = renderHook(
      ({ query }) => useHarness({ query }),
      { initialProps: { query: "  misty   mountains  " } },
    );

    await act(async () => hook.result.current.controller.search());

    expect(remote.search).toHaveBeenCalledWith("web", "misty mountains");
    await waitFor(() => expect(remote.loadMore).toHaveBeenCalledTimes(1));
    expect(hook.result.current.items.map((item) => item.id)).toEqual(["first"]);
    expect(hook.result.current.hasSearched).toBe(true);
    expect(hook.result.current.selectedId).toBeNull();
    expect(hook.result.current.galleryFilter).toBe("");
    expect(hook.result.current.kindFilter).toBe("all");
    expect(hook.result.current.controller.canLoadMore).toBe(true);
    expect(hook.result.current.statusHint).toContain("count=1");

    await act(async () => hook.result.current.controller.loadMore());

    expect(remote.loadMore).toHaveBeenCalledTimes(1);
    expect(remote.loadMore).toHaveBeenCalledWith("web", "misty mountains");
    expect(hook.result.current.items.map((item) => item.id)).toEqual([
      "first",
      "second",
    ]);
    expect(hook.result.current.controller.canLoadMore).toBe(false);
    expect(hook.result.current.statusHint).toContain("total=2");
  });

  it("keeps one hidden result page ahead after each load-more click", async () => {
    const firstPrefetch = deferred<WallpaperRemoteSearchResult>();
    const secondPrefetch = deferred<WallpaperRemoteSearchResult>();
    remote.search.mockResolvedValue(
      result([galleryItem("first")], { hasMore: true }),
    );
    remote.loadMore
      .mockReturnValueOnce(firstPrefetch.promise)
      .mockReturnValueOnce(secondPrefetch.promise);
    const hook = renderHook(() => useHarness({ query: "aurora" }));

    await act(async () => hook.result.current.controller.search());
    await waitFor(() => expect(remote.loadMore).toHaveBeenCalledTimes(1));
    expect(hook.result.current.items.map((item) => item.id)).toEqual(["first"]);

    await act(async () => {
      firstPrefetch.resolve(
        result([galleryItem("second")], { hasMore: true }),
      );
      await firstPrefetch.promise;
    });
    await act(async () => hook.result.current.controller.loadMore());

    expect(hook.result.current.items.map((item) => item.id)).toEqual([
      "first",
      "second",
    ]);
    await waitFor(() => expect(remote.loadMore).toHaveBeenCalledTimes(2));
    expect(hook.result.current.controller.canLoadMore).toBe(true);

    await act(async () => {
      secondPrefetch.resolve(result([], { errorCode: "empty" }));
      await secondPrefetch.promise;
    });
  });

  it("issues a fresh foreground request after a hidden prefetch failure", async () => {
    const foreground = deferred<WallpaperRemoteSearchResult>();
    remote.search.mockResolvedValue(
      result([galleryItem("first")], { hasMore: true }),
    );
    remote.loadMore
      .mockResolvedValueOnce(result([], { errorCode: "provider_timeout" }))
      .mockReturnValueOnce(foreground.promise);
    const hook = renderHook(() => useHarness({ query: "rainforest" }));

    await act(async () => hook.result.current.controller.search());
    await waitFor(() => expect(remote.loadMore).toHaveBeenCalledTimes(1));
    expect(hook.result.current.items.map((item) => item.id)).toEqual(["first"]);
    expect(hook.result.current.error).toBeNull();

    let paging!: Promise<void>;
    act(() => {
      paging = hook.result.current.controller.loadMore();
    });

    expect(remote.loadMore).toHaveBeenCalledTimes(2);
    expect(remote.loadMore).toHaveBeenLastCalledWith("web", "rainforest");
    await waitFor(() =>
      expect(hook.result.current.controller.loadingMore).toBe(true),
    );
    expect(hook.result.current.items.map((item) => item.id)).toEqual(["first"]);
    expect(hook.result.current.error).toBeNull();

    await act(async () => {
      foreground.resolve(result([galleryItem("second")]));
      await paging;
    });

    expect(hook.result.current.items.map((item) => item.id)).toEqual([
      "first",
      "second",
    ]);
    expect(hook.result.current.error).toBeNull();
    expect(hook.result.current.controller.canLoadMore).toBe(false);
  });

  it("also retries when the hidden prefetch rejects", async () => {
    remote.search.mockResolvedValue(
      result([galleryItem("first")], { hasMore: true }),
    );
    remote.loadMore
      .mockRejectedValueOnce(new Error("provider_timeout"))
      .mockResolvedValueOnce(result([galleryItem("second")]));
    const hook = renderHook(() => useHarness({ query: "waterfall" }));

    await act(async () => hook.result.current.controller.search());
    await waitFor(() => expect(remote.loadMore).toHaveBeenCalledTimes(1));
    await act(async () => hook.result.current.controller.loadMore());

    expect(remote.loadMore).toHaveBeenCalledTimes(2);
    expect(hook.result.current.items.map((item) => item.id)).toEqual([
      "first",
      "second",
    ]);
    expect(hook.result.current.error).toBeNull();
  });

  it("preserves existing cards when the fresh foreground request also fails", async () => {
    remote.search.mockResolvedValue(
      result([galleryItem("survivor")], { hasMore: true }),
    );
    remote.loadMore
      .mockResolvedValueOnce(result([], { errorCode: "provider_timeout" }))
      .mockResolvedValueOnce(result([], { errorCode: "provider_timeout" }));
    const hook = renderHook(() => useHarness({ query: "coast" }));

    await act(async () => hook.result.current.controller.search());
    await waitFor(() => expect(remote.loadMore).toHaveBeenCalledTimes(1));
    await act(async () => hook.result.current.controller.loadMore());

    expect(remote.loadMore).toHaveBeenCalledTimes(2);
    expect(hook.result.current.items.map((item) => item.id)).toEqual([
      "survivor",
    ]);
    expect(hook.result.current.error).not.toBeNull();
    expect(hook.result.current.controller.canLoadMore).toBe(true);
  });

  it("appends each progressive item once and exposes real progress", async () => {
    remote.busy = true;
    remote.source = "web";
    remote.requestId = "web-progress";
    remote.stage = "validating_images";
    remote.progressiveItems = [galleryItem("first")];
    const hook = renderHook(() => useHarness({}));

    await waitFor(() => expect(hook.result.current.items).toHaveLength(1));
    expect(hook.result.current.controller.progress).toBe(
      "settings.wallpaperSource.remote.progress.validatingImages",
    );

    remote.progressiveItems = [galleryItem("first"), galleryItem("second")];
    hook.rerender();
    await waitFor(() => expect(hook.result.current.items).toHaveLength(2));

    hook.rerender();
    expect(hook.result.current.items.map((item) => item.id)).toEqual([
      "first",
      "second",
    ]);
  });

  it("preserves a local path materialized before the final search result", async () => {
    const pending = deferred<WallpaperRemoteSearchResult>();
    remote.search.mockReturnValue(pending.promise);
    const hook = renderHook(() => useHarness({}));
    let searching!: Promise<void>;

    act(() => {
      searching = hook.result.current.controller.search();
    });
    act(() => {
      hook.result.current.setItems([
        {
          ...galleryItem("materialized"),
          localPath: "H:\\wallpapers\\materialized.jpg",
        },
      ]);
    });
    await act(async () => {
      pending.resolve(result([galleryItem("materialized")]));
      await searching;
    });

    expect(hook.result.current.items).toHaveLength(1);
    expect(hook.result.current.items[0]?.localPath).toBe(
      "H:\\wallpapers\\materialized.jpg",
    );
  });

  it("rebuilds a loaded page from the final result and preserves materialized paths", async () => {
    const pending = deferred<WallpaperRemoteSearchResult>();
    remote.search.mockResolvedValue(
      result([galleryItem("baseline")], { hasMore: true }),
    );
    remote.loadMore
      .mockResolvedValueOnce(result([], { errorCode: "provider_timeout" }))
      .mockReturnValueOnce(pending.promise);
    const hook = renderHook(() => useHarness({}));

    await act(async () => hook.result.current.controller.search());
    await waitFor(() => expect(remote.loadMore).toHaveBeenCalledTimes(1));
    let paging!: Promise<void>;
    act(() => {
      paging = hook.result.current.controller.loadMore();
    });
    await waitFor(() => expect(remote.loadMore).toHaveBeenCalledTimes(2));

    remote.busy = true;
    remote.source = "web";
    remote.requestId = "web-authoritative-more";
    remote.progressiveItems = [
      galleryItem("first"),
      galleryItem("second"),
      galleryItem("progressive-only"),
    ];
    hook.rerender();
    await waitFor(() => expect(hook.result.current.items).toHaveLength(4));
    act(() => {
      hook.result.current.setItems((current) =>
        current.map((item) =>
          item.id === "baseline" || item.id === "second"
            ? { ...item, localPath: `H:\\wallpapers\\${item.id}.jpg` }
            : item,
        ),
      );
    });

    await act(async () => {
      pending.resolve(
        result([
          { ...galleryItem("second"), sourceName: "final-second" },
          { ...galleryItem("first"), sourceName: "final-first" },
        ]),
      );
      await paging;
    });

    expect(hook.result.current.items.map((item) => item.id)).toEqual([
      "baseline",
      "second",
      "first",
    ]);
    expect(hook.result.current.items[0]?.localPath).toBe(
      "H:\\wallpapers\\baseline.jpg",
    );
    expect(hook.result.current.items[1]).toMatchObject({
      sourceName: "final-second",
      localPath: "H:\\wallpapers\\second.jpg",
    });
    expect(hook.result.current.items[2]?.sourceName).toBe("final-first");
  });

  it("keeps prefetched paging items hidden when load more is empty", async () => {
    remote.search.mockResolvedValue(
      result([galleryItem("survivor")], { hasMore: true }),
    );
    const pending = deferred<WallpaperRemoteSearchResult>();
    remote.loadMore.mockReturnValue(pending.promise);
    const hook = renderHook(() => useHarness({}));

    await act(async () => hook.result.current.controller.search());
    let paging!: Promise<void>;
    act(() => {
      paging = hook.result.current.controller.loadMore();
    });

    remote.busy = true;
    remote.source = "web";
    remote.requestId = "web-more";
    remote.progressiveItems = [galleryItem("progressive-duplicate")];
    hook.rerender();
    expect(hook.result.current.items).toHaveLength(1);

    await act(async () => {
      pending.resolve(result([], { errorCode: "empty", hasMore: false }));
      await paging;
    });

    expect(hook.result.current.items.map((item) => item.id)).toEqual([
      "survivor",
    ]);
    expect(hook.result.current.error).toBeNull();
    expect(hook.result.current.statusHint).toBe(
      "settings.wallpaperSource.noMore",
    );
    expect(hook.result.current.controller.canLoadMore).toBe(false);
  });

  it("preserves cards and the continuation across an empty intermediate page", async () => {
    remote.search.mockResolvedValue(
      result([galleryItem("survivor")], { hasMore: true }),
    );
    remote.loadMore.mockResolvedValue(
      result([], { errorCode: "empty", hasMore: true }),
    );
    const hook = renderHook(() => useHarness({ query: "rare coast" }));

    await act(async () => hook.result.current.controller.search());
    await waitFor(() => expect(remote.loadMore).toHaveBeenCalledTimes(1));
    await act(async () => hook.result.current.controller.loadMore());

    expect(hook.result.current.items.map((item) => item.id)).toEqual([
      "survivor",
    ]);
    expect(hook.result.current.errorCode).toBe("empty");
    expect(hook.result.current.controller.canLoadMore).toBe(true);
  });

  it("keeps loading-more state for the entire paging request", async () => {
    remote.search.mockResolvedValue(
      result([galleryItem("first")], { hasMore: true }),
    );
    const pending = deferred<WallpaperRemoteSearchResult>();
    remote.loadMore.mockReturnValue(pending.promise);
    const hook = renderHook(() => useHarness({}));

    await act(async () => hook.result.current.controller.search());
    let paging!: Promise<void>;
    act(() => {
      paging = hook.result.current.controller.loadMore();
    });

    await waitFor(() =>
      expect(hook.result.current.controller.loadingMore).toBe(true),
    );
    remote.stage = "validating_images";
    hook.rerender();
    expect(hook.result.current.controller.loadingMore).toBe(true);

    await act(async () => {
      pending.resolve(result([galleryItem("second")]));
      await paging;
    });
    expect(hook.result.current.controller.loadingMore).toBe(false);
  });

  it("maps Host auth errors and cancels when the source is disabled", async () => {
    remote.search.mockResolvedValue(
      result([], { errorCode: "oauth_expired" }),
    );
    const hook = renderHook(
      ({ enabled }) => useHarness({ enabled }),
      { initialProps: { enabled: true } },
    );

    await act(async () => hook.result.current.controller.search());
    expect(hook.result.current.errorCode).toBe("auth_required");
    expect(hook.result.current.error).toBe("Sign-in required");

    hook.rerender({ enabled: false });
    await waitFor(() => expect(remote.cancel).toHaveBeenCalled());
    expect(hook.result.current.controller.canLoadMore).toBe(false);
  });

  it("cancels an in-flight hidden prefetch when disabled", async () => {
    const pending = deferred<WallpaperRemoteSearchResult | null>();
    remote.search.mockResolvedValue(
      result([galleryItem("first")], { hasMore: true }),
    );
    remote.loadMore.mockReturnValue(pending.promise);
    const hook = renderHook(
      ({ enabled }) => useHarness({ enabled }),
      { initialProps: { enabled: true } },
    );

    await act(async () => hook.result.current.controller.search());
    await waitFor(() => expect(remote.loadMore).toHaveBeenCalledTimes(1));
    hook.rerender({ enabled: false });
    await waitFor(() => expect(remote.cancel).toHaveBeenCalled());

    await act(async () => {
      pending.resolve(null);
      await pending.promise;
    });
    expect(hook.result.current.items.map((item) => item.id)).toEqual(["first"]);
  });
});
