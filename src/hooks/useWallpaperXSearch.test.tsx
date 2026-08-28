/**
 * @vitest-environment jsdom
 */
import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  useWallpaperXSearch,
  type WallpaperXSearchClient,
} from "./useWallpaperXSearch";
import type {
  WallpaperGalleryItem,
  WallpaperSearchResult,
} from "@/lib/wallpaperSource";
import type {
  WallpaperXSearchBatch,
  WallpaperXSearchProgress,
} from "@/lib/wallpaperXSearch";

afterEach(() => cleanup());

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, resolve, reject };
}

function searchResult(requestId: string): WallpaperSearchResult {
  return {
    items: [],
    meta: {
      requestId,
      requestedMode: "cli",
      routeUsed: "cli",
      durationMs: 1,
      cacheHit: false,
      candidateCount: 0,
      validCount: 0,
    },
  };
}

function galleryItem(
  id: string,
  fullUrl = `https://example.test/${id}.jpg`,
): WallpaperGalleryItem {
  return {
    id,
    thumbUrl: fullUrl,
    fullUrl,
    kind: "image",
    source: "x",
  };
}

function clientHarness() {
  let progressHandler: ((progress: WallpaperXSearchProgress) => void) | null =
    null;
  let batchHandler: ((batch: WallpaperXSearchBatch) => void) | null = null;
  const unlisten = vi.fn();
  const client: WallpaperXSearchClient = {
    search: vi.fn(),
    searchMore: vi.fn(),
    cancel: vi.fn(async () => true),
    listenProgress: vi.fn(async (handler) => {
      progressHandler = handler;
      return unlisten;
    }),
    listenBatch: vi.fn(async (handler) => {
      batchHandler = handler;
      return unlisten;
    }),
  };
  return {
    client,
    emit(progress: WallpaperXSearchProgress) {
      progressHandler?.(progress);
    },
    emitBatch(batch: WallpaperXSearchBatch) {
      batchHandler?.(batch);
    },
    unlisten,
  };
}

describe("useWallpaperXSearch", () => {
  it("tracks only progress for the active request", async () => {
    const pending = deferred<WallpaperSearchResult>();
    const harness = clientHarness();
    vi.mocked(harness.client.search).mockReturnValue(pending.promise);
    const hook = renderHook(() =>
      useWallpaperXSearch(harness.client, () => "request-1"),
    );

    let searchPromise!: Promise<WallpaperSearchResult | null>;
    act(() => {
      searchPromise = hook.result.current.search("mountains", "top");
    });
    await waitFor(() => expect(hook.result.current.busy).toBe(true));
    expect(hook.result.current.stage).toBe("preparing");

    act(() => {
      harness.emit({ requestId: "other-request", stage: "supplementing" });
    });
    expect(hook.result.current.stage).toBe("preparing");
    act(() => {
      harness.emit({ requestId: "request-1", stage: "validating" });
    });
    expect(hook.result.current.stage).toBe("validating");

    pending.resolve(searchResult("request-1"));
    await act(async () => {
      expect(await searchPromise).toEqual(searchResult("request-1"));
    });
    expect(hook.result.current.busy).toBe(false);
    expect(hook.result.current.stage).toBeNull();
  });

  it("cancels promptly and discards a late result", async () => {
    const pending = deferred<WallpaperSearchResult>();
    const harness = clientHarness();
    vi.mocked(harness.client.search).mockReturnValue(pending.promise);
    const hook = renderHook(() =>
      useWallpaperXSearch(harness.client, () => "request-2"),
    );

    let searchPromise!: Promise<WallpaperSearchResult | null>;
    act(() => {
      searchPromise = hook.result.current.search("ocean", "latest");
    });
    await waitFor(() => expect(hook.result.current.busy).toBe(true));
    await waitFor(() => expect(harness.client.listenBatch).toHaveBeenCalled());
    act(() => {
      harness.emitBatch({
        requestId: "request-2",
        batchIndex: 1,
        items: [galleryItem("before-cancel")],
        accumulatedCount: 1,
        done: false,
      });
    });
    expect(hook.result.current.progressiveItems).toHaveLength(1);
    await act(async () => {
      expect(await hook.result.current.cancel()).toBe(true);
    });
    expect(harness.client.cancel).toHaveBeenCalledWith("request-2");
    expect(hook.result.current.busy).toBe(false);
    expect(hook.result.current.progressiveItems).toEqual([]);

    act(() => {
      harness.emitBatch({
        requestId: "request-2",
        batchIndex: 2,
        items: [galleryItem("after-cancel")],
        accumulatedCount: 2,
        done: true,
      });
    });
    expect(hook.result.current.progressiveItems).toEqual([]);

    pending.resolve(searchResult("request-2"));
    await expect(searchPromise).resolves.toBeNull();
    expect(hook.result.current.requestId).toBeNull();
  });

  it("accepts out-of-order active batches, dedupes items, and ignores repeats", async () => {
    const pending = deferred<WallpaperSearchResult>();
    const harness = clientHarness();
    vi.mocked(harness.client.search).mockReturnValue(pending.promise);
    const hook = renderHook(() =>
      useWallpaperXSearch(harness.client, () => "request-batches"),
    );

    let searchPromise!: Promise<WallpaperSearchResult | null>;
    act(() => {
      searchPromise = hook.result.current.search("forest", "top");
    });
    await waitFor(() => expect(harness.client.listenBatch).toHaveBeenCalled());

    act(() => {
      harness.emitBatch({
        requestId: "other-request",
        batchIndex: 1,
        items: [galleryItem("ignored")],
        accumulatedCount: 1,
        done: false,
      });
      harness.emitBatch({
        requestId: "request-batches",
        batchIndex: 2,
        items: [galleryItem("b"), galleryItem("shared")],
        accumulatedCount: 2,
        done: false,
      });
      harness.emitBatch({
        requestId: "request-batches",
        batchIndex: 1,
        items: [galleryItem("a"), galleryItem("shared")],
        accumulatedCount: 3,
        done: false,
      });
      harness.emitBatch({
        requestId: "request-batches",
        batchIndex: 2,
        items: [galleryItem("duplicate-batch")],
        accumulatedCount: 4,
        done: false,
      });
    });

    expect(hook.result.current.progressiveItems.map((item) => item.id)).toEqual([
      "b",
      "shared",
      "a",
    ]);
    expect(hook.result.current.progressiveCount).toBe(3);
    expect(hook.result.current.progressiveDone).toBe(false);

    act(() => {
      harness.emitBatch({
        requestId: "request-batches",
        batchIndex: 3,
        items: [],
        accumulatedCount: 3,
        done: true,
      });
    });
    expect(hook.result.current.progressiveDone).toBe(true);

    pending.resolve(searchResult("request-batches"));
    await act(async () => {
      expect(await searchPromise).toEqual(searchResult("request-batches"));
    });
  });

  it("cancels the prior generation before a replacement search", async () => {
    const first = deferred<WallpaperSearchResult>();
    const second = deferred<WallpaperSearchResult>();
    const harness = clientHarness();
    vi.mocked(harness.client.search)
      .mockReturnValueOnce(first.promise)
      .mockReturnValueOnce(second.promise);
    const ids = ["request-old", "request-new"];
    const hook = renderHook(() =>
      useWallpaperXSearch(harness.client, () => ids.shift() ?? "request-extra"),
    );

    let firstPromise!: Promise<WallpaperSearchResult | null>;
    let secondPromise!: Promise<WallpaperSearchResult | null>;
    act(() => {
      firstPromise = hook.result.current.search("old", "top");
    });
    await waitFor(() => expect(hook.result.current.busy).toBe(true));
    act(() => {
      secondPromise = hook.result.current.search("new", "top");
    });
    await waitFor(() =>
      expect(harness.client.cancel).toHaveBeenCalledWith("request-old"),
    );
    await waitFor(() => expect(harness.client.search).toHaveBeenCalledTimes(2));

    second.resolve(searchResult("request-new"));
    await act(async () => {
      expect(await secondPromise).toEqual(searchResult("request-new"));
    });
    first.resolve(searchResult("request-old"));
    await expect(firstPromise).resolves.toBeNull();
  });

  it("cancels the active request and removes the listener on unmount", async () => {
    const pending = deferred<WallpaperSearchResult>();
    const harness = clientHarness();
    vi.mocked(harness.client.search).mockReturnValue(pending.promise);
    const hook = renderHook(() =>
      useWallpaperXSearch(harness.client, () => "request-unmount"),
    );
    act(() => {
      void hook.result.current.search("rain", "top");
    });
    await waitFor(() => expect(hook.result.current.busy).toBe(true));
    hook.unmount();
    await waitFor(() =>
      expect(harness.client.cancel).toHaveBeenCalledWith("request-unmount"),
    );
    await waitFor(() => expect(harness.unlisten).toHaveBeenCalledTimes(2));
  });

  it("runs user-triggered load more through the same isolated lifecycle", async () => {
    const harness = clientHarness();
    vi.mocked(harness.client.searchMore).mockResolvedValue(
      searchResult("request-more"),
    );
    const hook = renderHook(() =>
      useWallpaperXSearch(harness.client, () => "request-more"),
    );

    let result: WallpaperSearchResult | null = null;
    await act(async () => {
      result = await hook.result.current.loadMore("forest", "latest");
    });

    expect(harness.client.searchMore).toHaveBeenCalledWith(
      "forest",
      "latest",
      "request-more",
    );
    expect(harness.client.search).not.toHaveBeenCalled();
    expect(result).toEqual(searchResult("request-more"));
    expect(hook.result.current.busy).toBe(false);
  });
});
