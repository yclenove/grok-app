/**
 * @vitest-environment jsdom
 */
import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import "@/test/jsdomStubs";
import type {
  WallpaperGalleryItem,
  WallpaperSearchResult,
} from "@/lib/wallpaperSource";

const cancelSearch = vi.hoisted(() => vi.fn(async () => true));
const searchX = vi.hoisted(() => vi.fn());
const loadMoreX = vi.hoisted(() => vi.fn());
const openGrokAlbum = vi.hoisted(() => vi.fn(async () => undefined));
const syncGrokAlbum = vi.hoisted(() => vi.fn(async () => null));
const refreshGrokAlbum = vi.hoisted(() => vi.fn(async () => undefined));
const loadMoreGrokAlbum = vi.hoisted(() => vi.fn(async () => undefined));
const grokAlbumItems = vi.hoisted(() => [] as WallpaperGalleryItem[]);
const fetchWallpaperMedia = vi.hoisted(() => vi.fn());
const fetchRemoteWallpaperMedia = vi.hoisted(() => vi.fn());
const fetchGrokAlbumMedia = vi.hoisted(() => vi.fn());
const fetchGrokAlbumThumbnail = vi.hoisted(() => vi.fn());
const cancelGrokAlbumRequests = vi.hoisted(() => vi.fn(async () => 0));
const openViewer = vi.hoisted(() => vi.fn());
const searchRemote = vi.hoisted(() => vi.fn(async () => null));
const loadMoreRemote = vi.hoisted(() => vi.fn(async () => null));
const cancelRemote = vi.hoisted(() => vi.fn(async () => true));
const wallpaperLibraryList = vi.hoisted(() => vi.fn(async () => []));
const secretsGetMasked = vi.hoisted(() =>
  vi.fn(async () => ({ hasPexelsKey: false })),
);
const secretsSet = vi.hoisted(() => vi.fn(async () => undefined));
const remoteControllerState = vi.hoisted(() => ({
  busy: false,
  loadingMore: false,
  progress: null as string | null,
  canLoadMore: false,
  searchItems: [] as WallpaperGalleryItem[],
}));
const grokAlbumState = vi.hoisted(() => ({
  status: "closed" as "closed" | "ready",
  cachedCount: 0,
  visibleCount: 0,
  busy: false,
  syncing: false,
  loadingMore: false,
  hasSynced: false,
  errorCode: null,
  canLoadMore: false,
}));
const xSearchState = vi.hoisted(() => ({
  busy: true,
  requestId: "request-active" as string | null,
  stage: "validating" as const,
  progressiveItems: [] as WallpaperGalleryItem[],
  progressiveCount: 0,
  progressiveDone: false,
}));

vi.mock("@/hooks/useWallpaperXSearch", () => ({
  useWallpaperXSearch: () => ({
    ...xSearchState,
    search: searchX,
    loadMore: loadMoreX,
    cancel: cancelSearch,
  }),
}));

vi.mock("@/hooks/useWallpaperGrokAlbum", () => ({
  useWallpaperGrokAlbum: () => ({
    ...grokAlbumState,
    items: grokAlbumItems,
    open: openGrokAlbum,
    sync: syncGrokAlbum,
    refresh: refreshGrokAlbum,
    loadMore: loadMoreGrokAlbum,
  }),
}));

vi.mock("@/hooks/useWallpaperRemoteSourceController", () => ({
  useWallpaperRemoteSourceController: (options: {
    setItems: (items: WallpaperGalleryItem[]) => void;
    setHasSearched: (value: boolean) => void;
  }) => ({
    ...remoteControllerState,
    search: async () => {
      const result = await searchRemote();
      if (remoteControllerState.searchItems.length > 0) {
        options.setItems(remoteControllerState.searchItems);
        options.setHasSearched(true);
      }
      return result;
    },
    loadMore: loadMoreRemote,
    cancel: cancelRemote,
    clear: vi.fn(),
  }),
}));

vi.mock("@/lib/api", () => ({
  isDesktopHost: () => true,
  isTauri: () => false,
  wallpaperFetchMedia: fetchWallpaperMedia,
  wallpaperRemoteFetchMedia: fetchRemoteWallpaperMedia,
  wallpaperGrokAlbumFetchMedia: fetchGrokAlbumMedia,
  wallpaperGrokAlbumThumbnail: fetchGrokAlbumThumbnail,
  wallpaperGrokAlbumCancelRequests: cancelGrokAlbumRequests,
  wallpaperGrokAlbumCancelAllRequests: vi.fn(async () => 0),
  wallpaperRemoteCancelMediaRequests: vi.fn(async () => 0),
  wallpaperRemoteCancelAllMediaRequests: vi.fn(async () => 0),
  wallpaperImagine: vi.fn(),
  wallpaperLibraryList,
  wallpaperLibraryDelete: vi.fn(),
  openExternalUrl: vi.fn(),
  secretsGetMasked,
  secretsSet,
}));

vi.mock("@/components/ImageViewerContext", () => ({
  useImageViewerOptional: () => ({ open: openViewer }),
}));

vi.mock("@/components/Select", () => ({
  Select: ({
    value,
    onChange,
    "aria-label": ariaLabel,
  }: {
    value: string;
    onChange: (value: string) => void;
    "aria-label"?: string;
  }) => (
    <button
      type="button"
      aria-label={ariaLabel}
      onClick={() => onChange(value === "top" ? "latest" : "top")}
    >
      {value}
    </button>
  ),
}));

vi.mock("@/components/GlassModal", () => ({
  GlassModal: ({
    open,
    onClose,
    children,
    footer,
  }: {
    open: boolean;
    onClose: () => void;
    children: React.ReactNode;
    footer?: React.ReactNode;
  }) =>
    open ? (
      <div>
        <button type="button" onClick={onClose}>
          modal-close
        </button>
        {children}
        {footer}
      </div>
    ) : null,
}));

import { WallpaperSourceModal } from "./WallpaperSourceModal";

afterEach(() => {
  cleanup();
  cancelSearch.mockClear();
  searchX.mockReset();
  loadMoreX.mockReset();
  openGrokAlbum.mockClear();
  syncGrokAlbum.mockClear();
  refreshGrokAlbum.mockClear();
  loadMoreGrokAlbum.mockClear();
  fetchWallpaperMedia.mockReset();
  fetchRemoteWallpaperMedia.mockReset();
  fetchGrokAlbumMedia.mockReset();
  fetchGrokAlbumThumbnail.mockReset();
  cancelGrokAlbumRequests.mockClear();
  openViewer.mockReset();
  searchRemote.mockClear();
  loadMoreRemote.mockClear();
  cancelRemote.mockClear();
  wallpaperLibraryList.mockClear();
  wallpaperLibraryList.mockResolvedValue([]);
  secretsGetMasked.mockClear();
  secretsGetMasked.mockResolvedValue({ hasPexelsKey: false });
  secretsSet.mockClear();
  remoteControllerState.busy = false;
  remoteControllerState.loadingMore = false;
  remoteControllerState.progress = null;
  remoteControllerState.canLoadMore = false;
  remoteControllerState.searchItems = [];
  grokAlbumItems.splice(0, grokAlbumItems.length);
  grokAlbumState.status = "closed";
  grokAlbumState.cachedCount = 0;
  grokAlbumState.visibleCount = 0;
  grokAlbumState.busy = false;
  grokAlbumState.syncing = false;
  grokAlbumState.loadingMore = false;
  grokAlbumState.hasSynced = false;
  grokAlbumState.errorCode = null;
  grokAlbumState.canLoadMore = false;
  xSearchState.busy = true;
  xSearchState.requestId = "request-active";
  xSearchState.stage = "validating";
  xSearchState.progressiveItems = [];
  xSearchState.progressiveCount = 0;
  xSearchState.progressiveDone = false;
});

function galleryItem(
  id: string,
  extra: Partial<WallpaperGalleryItem> = {},
): WallpaperGalleryItem {
  return {
    id,
    thumbUrl: `https://example.test/${id}.jpg`,
    fullUrl: `https://example.test/${id}.jpg`,
    kind: "image",
    source: "x",
    ...extra,
  };
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}

describe("WallpaperSourceModal X search paging", () => {
  const t = (key: string) => key;

  it("keeps the existing gallery when load more finds no new images", async () => {
    xSearchState.busy = false;
    xSearchState.requestId = null;
    searchX.mockResolvedValue({
      items: [galleryItem("initial")],
      meta: {
        requestId: "request-initial",
        requestedMode: "responses_preview",
        routeUsed: "responses",
        durationMs: 20,
        cacheHit: false,
        candidateCount: 1,
        validCount: 1,
      },
    } satisfies WallpaperSearchResult);
    loadMoreX.mockResolvedValue({
      items: [],
      errorCode: "empty",
      meta: {
        requestId: "request-more",
        requestedMode: "responses_preview",
        routeUsed: "responses",
        durationMs: 15,
        cacheHit: false,
        candidateCount: 0,
        validCount: 0,
      },
    } satisfies WallpaperSearchResult);
    render(
      <WallpaperSourceModal
        open
        t={t as never}
        onClose={vi.fn()}
        onPickFile={vi.fn()}
      />,
    );

    fireEvent.change(
      screen.getByPlaceholderText("settings.wallpaperSource.xPlaceholder"),
      { target: { value: "ocean" } },
    );
    fireEvent.click(
      screen.getByRole("button", { name: "settings.wallpaperSource.search" }),
    );
    fireEvent.click(
      await screen.findByRole("button", {
        name: "settings.wallpaperSource.loadMore",
      }),
    );

    await screen.findByText("settings.wallpaperSource.noMore");
    expect(screen.getAllByRole("listitem")).toHaveLength(1);
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("does not offer load more for a CLI result", async () => {
    xSearchState.busy = false;
    xSearchState.requestId = null;
    searchX.mockResolvedValue({
      items: [galleryItem("cli-result")],
      meta: {
        requestId: "request-cli",
        requestedMode: "cli",
        routeUsed: "cli",
        durationMs: 20,
        cacheHit: false,
        candidateCount: 1,
        validCount: 1,
      },
    } satisfies WallpaperSearchResult);
    render(
      <WallpaperSourceModal
        open
        t={t as never}
        onClose={vi.fn()}
        onPickFile={vi.fn()}
      />,
    );

    fireEvent.change(
      screen.getByPlaceholderText("settings.wallpaperSource.xPlaceholder"),
      { target: { value: "forest" } },
    );
    fireEvent.click(
      screen.getByRole("button", { name: "settings.wallpaperSource.search" }),
    );

    await waitFor(() => expect(screen.getAllByRole("listitem")).toHaveLength(1));
    expect(
      screen.queryByRole("button", {
        name: "settings.wallpaperSource.loadMore",
      }),
    ).toBeNull();
  });

  it("does not offer load more for an exhausted cached Responses result", async () => {
    xSearchState.busy = false;
    xSearchState.requestId = null;
    searchX.mockResolvedValue({
      items: [galleryItem("cached-response")],
      meta: {
        requestId: "request-cached",
        requestedMode: "responses_preview",
        routeUsed: "responses",
        durationMs: 1,
        cacheHit: true,
        continuationAvailable: false,
        candidateCount: 1,
        validCount: 1,
      },
    } satisfies WallpaperSearchResult);
    render(
      <WallpaperSourceModal
        open
        t={t as never}
        onClose={vi.fn()}
        onPickFile={vi.fn()}
      />,
    );

    fireEvent.change(
      screen.getByPlaceholderText("settings.wallpaperSource.xPlaceholder"),
      { target: { value: "forest" } },
    );
    fireEvent.click(
      screen.getByRole("button", { name: "settings.wallpaperSource.search" }),
    );

    await waitFor(() => expect(screen.getAllByRole("listitem")).toHaveLength(1));
    expect(
      screen.queryByRole("button", {
        name: "settings.wallpaperSource.loadMore",
      }),
    ).toBeNull();
    expect(loadMoreX).not.toHaveBeenCalled();
  });

  it("hides Responses continuation after the query or sort changes", async () => {
    xSearchState.busy = false;
    xSearchState.requestId = null;
    searchX.mockResolvedValue({
      items: [galleryItem("initial")],
      meta: {
        requestId: "request-initial",
        requestedMode: "responses_preview",
        routeUsed: "responses",
        durationMs: 20,
        cacheHit: false,
        candidateCount: 1,
        validCount: 1,
      },
    } satisfies WallpaperSearchResult);
    render(
      <WallpaperSourceModal
        open
        t={t as never}
        onClose={vi.fn()}
        onPickFile={vi.fn()}
      />,
    );

    const queryInput = screen.getByRole("searchbox", {
      name: "settings.wallpaperSource.xPlaceholder",
    });
    fireEvent.change(queryInput, { target: { value: "ocean" } });
    fireEvent.click(
      screen.getByRole("button", { name: "settings.wallpaperSource.search" }),
    );
    await screen.findByRole("button", {
      name: "settings.wallpaperSource.loadMore",
    });

    fireEvent.change(queryInput, { target: { value: "forest" } });
    expect(
      screen.queryByRole("button", {
        name: "settings.wallpaperSource.loadMore",
      }),
    ).toBeNull();
    expect(loadMoreX).not.toHaveBeenCalled();

    fireEvent.change(queryInput, { target: { value: "ocean" } });
    expect(
      screen.getByRole("button", {
        name: "settings.wallpaperSource.loadMore",
      }),
    ).toBeTruthy();
    fireEvent.click(
      screen.getByRole("button", { name: "settings.wallpaperSource.sort" }),
    );
    expect(
      screen.queryByRole("button", {
        name: "settings.wallpaperSource.loadMore",
      }),
    ).toBeNull();
    expect(loadMoreX).not.toHaveBeenCalled();
  });

  it("preserves existing images when load more fails", async () => {
    xSearchState.busy = false;
    xSearchState.requestId = null;
    searchX.mockResolvedValue({
      items: [galleryItem("survivor")],
      meta: {
        requestId: "request-initial",
        requestedMode: "responses_preview",
        routeUsed: "responses",
        durationMs: 20,
        cacheHit: false,
        candidateCount: 1,
        validCount: 1,
      },
    } satisfies WallpaperSearchResult);
    loadMoreX.mockRejectedValue(new Error("transport failed"));
    render(
      <WallpaperSourceModal
        open
        t={t as never}
        onClose={vi.fn()}
        onPickFile={vi.fn()}
      />,
    );

    fireEvent.change(
      screen.getByPlaceholderText("settings.wallpaperSource.xPlaceholder"),
      { target: { value: "forest" } },
    );
    fireEvent.click(
      screen.getByRole("button", { name: "settings.wallpaperSource.search" }),
    );
    fireEvent.click(
      await screen.findByRole("button", {
        name: "settings.wallpaperSource.loadMore",
      }),
    );

    await waitFor(() => expect(screen.getByRole("alert")).toBeTruthy());
    expect(screen.getAllByRole("listitem")).toHaveLength(1);
    expect(
      screen.getByRole("button", {
        name: "settings.wallpaperSource.loadMore",
      }),
    ).toBeTruthy();
  });

  it("reconciles a live load-more batch to the final page order and membership", async () => {
    xSearchState.busy = false;
    xSearchState.requestId = null;
    searchX.mockResolvedValue({
      items: [galleryItem("initial")],
      meta: {
        requestId: "request-initial",
        requestedMode: "responses_preview",
        routeUsed: "responses",
        durationMs: 20,
        cacheHit: false,
        candidateCount: 1,
        validCount: 1,
      },
    } satisfies WallpaperSearchResult);
    const pendingMore = deferred<WallpaperSearchResult>();
    loadMoreX.mockReturnValue(pendingMore.promise);
    const props = {
      open: true,
      t: t as never,
      onClose: vi.fn(),
      onPickFile: vi.fn(),
    };
    const view = render(<WallpaperSourceModal {...props} />);

    fireEvent.change(
      screen.getByPlaceholderText("settings.wallpaperSource.xPlaceholder"),
      { target: { value: "ocean" } },
    );
    fireEvent.click(
      screen.getByRole("button", { name: "settings.wallpaperSource.search" }),
    );
    fireEvent.click(
      await screen.findByRole("button", {
        name: "settings.wallpaperSource.loadMore",
      }),
    );

    xSearchState.busy = true;
    xSearchState.requestId = "request-more";
    xSearchState.progressiveItems = [
      galleryItem("first"),
      galleryItem("second"),
      galleryItem("progressive-only"),
    ];
    xSearchState.progressiveCount = 3;
    view.rerender(<WallpaperSourceModal {...props} />);
    await waitFor(() => expect(screen.getAllByRole("listitem")).toHaveLength(4));

    await act(async () => {
      pendingMore.resolve({
        items: [
          galleryItem("second", { username: "rank-1" }),
          galleryItem("first", { username: "rank-2" }),
        ],
        meta: {
          requestId: "request-more",
          requestedMode: "responses_preview",
          routeUsed: "responses",
          durationMs: 18,
          cacheHit: false,
          candidateCount: 2,
          validCount: 2,
        },
      });
      await pendingMore.promise;
    });

    await waitFor(() => expect(screen.getAllByRole("listitem")).toHaveLength(3));
    expect(
      screen
        .getAllByRole("button", {
          name: /^settings\.wallpaperSource\.openPreview/,
        })
        .map((button) => button.getAttribute("aria-label")),
    ).toEqual([
      "settings.wallpaperSource.openPreview: initial",
      "settings.wallpaperSource.openPreview: rank-1",
      "settings.wallpaperSource.openPreview: rank-2",
    ]);
    expect(screen.queryByText("progressive-only")).toBeNull();
  });
});
