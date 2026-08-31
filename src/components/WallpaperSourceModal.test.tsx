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
const fetchGrokAlbumMedia = vi.hoisted(() => vi.fn());
const fetchGrokAlbumThumbnail = vi.hoisted(() => vi.fn());
const cancelGrokAlbumRequests = vi.hoisted(() => vi.fn(async () => 0));
const openViewer = vi.hoisted(() => vi.fn());
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

vi.mock("@/lib/api", () => ({
  isDesktopHost: () => true,
  isTauri: () => false,
  wallpaperFetchMedia: fetchWallpaperMedia,
  wallpaperGrokAlbumFetchMedia: fetchGrokAlbumMedia,
  wallpaperGrokAlbumThumbnail: fetchGrokAlbumThumbnail,
  wallpaperGrokAlbumCancelRequests: cancelGrokAlbumRequests,
  wallpaperGrokAlbumCancelAllRequests: vi.fn(async () => 0),
  wallpaperImagine: vi.fn(),
  wallpaperLibraryList: vi.fn(),
  wallpaperLibraryDelete: vi.fn(),
  openExternalUrl: vi.fn(),
}));

vi.mock("@/components/ImageViewerContext", () => ({
  useImageViewerOptional: () => ({ open: openViewer }),
}));

vi.mock("@/components/Select", () => ({
  Select: ({ value }: { value: string }) => <span>{value}</span>,
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
  fetchGrokAlbumMedia.mockReset();
  fetchGrokAlbumThumbnail.mockReset();
  cancelGrokAlbumRequests.mockClear();
  openViewer.mockReset();
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

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, resolve, reject };
}

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

describe("WallpaperSourceModal X search lifecycle", () => {
  const t = (key: string) => key;

  it("shows real progress and exposes an explicit cancel-search action", () => {
    render(
      <WallpaperSourceModal
        open
        t={t as never}
        onClose={vi.fn()}
        onPickFile={vi.fn()}
      />,
    );

    expect(
      screen.getByText("settings.wallpaperSource.progress.validating"),
    ).toBeTruthy();
    fireEvent.click(
      screen.getByRole("button", {
        name: "settings.wallpaperSource.cancelSearch",
      }),
    );
    expect(cancelSearch).toHaveBeenCalledTimes(1);
  });

  it("cancels an active X search when switching tabs or closing", () => {
    const onClose = vi.fn();
    render(
      <WallpaperSourceModal
        open
        t={t as never}
        onClose={onClose}
        onPickFile={vi.fn()}
      />,
    );

    fireEvent.click(
      screen.getByRole("tab", { name: "settings.wallpaperImagine" }),
    );
    expect(cancelSearch).toHaveBeenCalledTimes(1);

    fireEvent.click(screen.getByRole("button", { name: "modal-close" }));
    expect(cancelSearch).toHaveBeenCalledTimes(2);
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("exposes the isolated Grok album source and opens its official window", () => {
    xSearchState.busy = false;
    xSearchState.requestId = null;
    render(
      <WallpaperSourceModal
        open
        initialTab="grok_album"
        t={t as never}
        onClose={vi.fn()}
        onPickFile={vi.fn()}
      />,
    );

    expect(
      screen
        .getByRole("tab", { name: "settings.wallpaperGrokAlbum" })
        .getAttribute("aria-selected"),
    ).toBe("true");
    expect(
      screen.getByText("settings.wallpaperSource.grokAlbum.privacy"),
    ).toBeTruthy();
    fireEvent.click(
      screen.getByRole("button", {
        name: "settings.wallpaperSource.grokAlbum.open",
      }),
    );
    expect(openGrokAlbum).toHaveBeenCalledWith("settings.wallpaperGrokAlbum");
  });

  it("uses an append-stable gallery only for Grok album results", () => {
    xSearchState.busy = false;
    xSearchState.requestId = null;
    const view = render(
      <WallpaperSourceModal
        open
        initialTab="grok_album"
        t={t as never}
        onClose={vi.fn()}
        onPickFile={vi.fn()}
      />,
    );

    expect(
      view.container.querySelector(".wallpaper-masonry")?.classList,
    ).toContain("wallpaper-masonry--stable");

    fireEvent.click(
      screen.getByRole("tab", { name: "settings.wallpaperFromX" }),
    );

    expect(
      view.container.querySelector(".wallpaper-masonry")?.classList,
    ).not.toContain("wallpaper-masonry--stable");
  });

  it("keeps honest controls visible while an album page loads more", async () => {
    xSearchState.busy = false;
    xSearchState.requestId = null;
    grokAlbumState.status = "ready";
    grokAlbumState.cachedCount = 20;
    grokAlbumState.visibleCount = 20;
    grokAlbumState.busy = true;
    grokAlbumState.loadingMore = true;
    grokAlbumState.hasSynced = true;
    grokAlbumState.canLoadMore = true;
    const url =
      "https://assets.grok.com/users/test/generated/fake/loading-more.jpg";
    grokAlbumItems.push(
      galleryItem("album-loading-more", {
        thumbUrl: url,
        fullUrl: url,
        source: "grok_album",
      }),
    );

    render(
      <WallpaperSourceModal
        open
        initialTab="grok_album"
        t={t as never}
        onClose={vi.fn()}
        onPickFile={vi.fn()}
      />,
    );

    const loadMore = await screen.findByRole("button", {
      name: "settings.wallpaperSource.loadingMore",
    });
    expect((loadMore as HTMLButtonElement).disabled).toBe(true);
    expect(loadMore.getAttribute("aria-busy")).toBe("true");
    expect(
      (
        screen.getByRole("button", {
          name: "settings.wallpaperSource.grokAlbum.sync",
        }) as HTMLButtonElement
      ).disabled,
    ).toBe(true);
    expect(
      screen.queryByRole("button", {
        name: "settings.wallpaperSource.grokAlbum.syncing",
      }),
    ).toBeNull();
  });

  it("uses the authenticated album media bridge when opening an original", async () => {
    xSearchState.busy = false;
    xSearchState.requestId = null;
    const url =
      "https://assets.grok.com/users/test/generated/fake/private-image.jpg";
    grokAlbumItems.push(
      galleryItem("album-private", {
        thumbUrl: url,
        fullUrl: url,
        source: "grok_album",
      }),
    );
    fetchGrokAlbumThumbnail.mockResolvedValue({
      dataUrl: "data:image/jpeg;base64,YWJj",
      width: 32,
      height: 32,
    });
    fetchGrokAlbumMedia.mockResolvedValue({
      path: "H:\\wallpapers\\album-private.jpg",
      name: "album-private.jpg",
      mime: "image/jpeg",
      bytes: 128,
    });

    render(
      <WallpaperSourceModal
        open
        initialTab="grok_album"
        t={t as never}
        onClose={vi.fn()}
        onPickFile={vi.fn()}
      />,
    );

    fireEvent.click(
      await screen.findByRole("button", {
        name: "settings.wallpaperSource.openPreview",
      }),
    );

    await waitFor(() => expect(openViewer).toHaveBeenCalledTimes(1));
    expect(fetchGrokAlbumMedia).not.toHaveBeenCalled();
    expect(fetchWallpaperMedia).not.toHaveBeenCalled();
    const slides = openViewer.mock.calls[0]?.[0] as Array<{
      loadOriginal?: () => Promise<{ src: string; mime?: string } | null>;
    }>;
    expect(slides[0]?.loadOriginal).toBeTypeOf("function");
    await slides[0]!.loadOriginal!();
    expect(fetchGrokAlbumMedia).toHaveBeenCalledWith(
      url,
      expect.stringMatching(/^[0-9a-f-]{36}$/),
    );
  });

  it("downloads a remote album sibling only when the lightbox navigates to it", async () => {
    xSearchState.busy = false;
    xSearchState.requestId = null;
    const firstUrl =
      "https://assets.grok.com/users/test/generated/fake/first.jpg";
    const secondUrl =
      "https://assets.grok.com/users/test/generated/fake/second.jpg";
    grokAlbumItems.push(
      galleryItem("album-first", {
        thumbUrl: firstUrl,
        fullUrl: firstUrl,
        source: "grok_album",
      }),
      galleryItem("album-second", {
        thumbUrl: secondUrl,
        fullUrl: secondUrl,
        source: "grok_album",
      }),
    );
    fetchGrokAlbumThumbnail.mockResolvedValue({
      dataUrl: "data:image/jpeg;base64,YWJj",
      width: 32,
      height: 32,
    });
    fetchGrokAlbumMedia.mockImplementation(async (url: string) => ({
      path: url === firstUrl
        ? "H:\\wallpapers\\album-first.jpg"
        : "H:\\wallpapers\\album-second.jpg",
      name: url === firstUrl ? "album-first.jpg" : "album-second.jpg",
      mime: "image/jpeg",
      bytes: 128,
    }));

    render(
      <WallpaperSourceModal
        open
        initialTab="grok_album"
        t={t as never}
        onClose={vi.fn()}
        onPickFile={vi.fn()}
      />,
    );

    fireEvent.click(
      (await screen.findAllByRole("button", {
        name: "settings.wallpaperSource.openPreview",
      }))[0]!,
    );
    await waitFor(() => expect(openViewer).toHaveBeenCalledTimes(1));

    const slides = openViewer.mock.calls[0]?.[0] as Array<{
      src: string;
      loadOriginal?: () => Promise<{
        src: string;
        kind?: "image" | "video";
        mime?: string;
      } | null>;
    }>;
    expect(slides).toHaveLength(2);
    expect(slides[1]?.loadOriginal).toBeTypeOf("function");
    expect(fetchGrokAlbumMedia).not.toHaveBeenCalled();

    let loaded: Awaited<ReturnType<NonNullable<typeof slides[1]["loadOriginal"]>>>;
    await act(async () => {
      loaded = await slides[1]!.loadOriginal!();
    });
    expect(fetchGrokAlbumMedia).toHaveBeenCalledTimes(1);
    expect(fetchGrokAlbumMedia).toHaveBeenCalledWith(
      secondUrl,
      expect.stringMatching(/^[0-9a-f-]{36}$/),
    );
    expect(loaded!).toEqual({
      src: "H:\\wallpapers\\album-second.jpg",
      kind: "image",
      mime: "image/jpeg",
      poster: undefined,
    });
  });

  it("ignores an album original that finishes after switching sources", async () => {
    xSearchState.busy = false;
    xSearchState.requestId = null;
    const url =
      "https://assets.grok.com/users/test/generated/fake/stale-image.jpg";
    const pending = deferred<{
      path: string;
      name: string;
      mime: string;
      bytes: number;
    }>();
    grokAlbumItems.push(
      galleryItem("album-stale", {
        thumbUrl: url,
        fullUrl: url,
        source: "grok_album",
      }),
    );
    fetchGrokAlbumThumbnail.mockResolvedValue({
      dataUrl: "data:image/jpeg;base64,YWJj",
      width: 32,
      height: 32,
    });
    fetchGrokAlbumMedia.mockReturnValue(pending.promise);

    render(
      <WallpaperSourceModal
        open
        initialTab="grok_album"
        t={t as never}
        onClose={vi.fn()}
        onPickFile={vi.fn()}
      />,
    );
    fireEvent.click(
      await screen.findByRole("button", {
        name: "settings.wallpaperSource.openPreview",
      }),
    );
    await waitFor(() => expect(openViewer).toHaveBeenCalledTimes(1));
    const slides = openViewer.mock.calls[0]?.[0] as Array<{
      loadOriginal?: () => Promise<{ src: string } | null>;
    }>;
    const load = slides[0]!.loadOriginal!();
    await waitFor(() => expect(fetchGrokAlbumMedia).toHaveBeenCalledTimes(1));

    fireEvent.click(
      screen.getByRole("tab", { name: "settings.wallpaperFromX" }),
    );
    pending.resolve({
      path: "H:\\wallpapers\\stale-image.jpg",
      name: "stale-image.jpg",
      mime: "image/jpeg",
      bytes: 128,
    });

    await expect(load).resolves.toBeNull();
    expect(
      screen.queryByText("settings.wallpaperSource.err.download_failed"),
    ).toBeNull();
  });

  it("cancels an active album original before closing the source modal", async () => {
    xSearchState.busy = false;
    xSearchState.requestId = null;
    const url =
      "https://assets.grok.com/users/test/generated/fake/closing-image.jpg";
    const pending = deferred<{
      path: string;
      name: string;
      mime: string;
      bytes: number;
    }>();
    const onClose = vi.fn();
    grokAlbumItems.push(
      galleryItem("album-closing", {
        thumbUrl: url,
        fullUrl: url,
        source: "grok_album",
      }),
    );
    fetchGrokAlbumThumbnail.mockResolvedValue({
      dataUrl: "data:image/jpeg;base64,YWJj",
      width: 32,
      height: 32,
    });
    fetchGrokAlbumMedia.mockReturnValue(pending.promise);

    render(
      <WallpaperSourceModal
        open
        initialTab="grok_album"
        t={t as never}
        onClose={onClose}
        onPickFile={vi.fn()}
      />,
    );
    fireEvent.click(
      await screen.findByRole("button", {
        name: "settings.wallpaperSource.openPreview",
      }),
    );
    await waitFor(() => expect(openViewer).toHaveBeenCalledTimes(1));
    const slides = openViewer.mock.calls[0]?.[0] as Array<{
      loadOriginal?: () => Promise<{ src: string } | null>;
    }>;
    const load = slides[0]!.loadOriginal!();
    await waitFor(() => expect(fetchGrokAlbumMedia).toHaveBeenCalledTimes(1));
    const requestId = fetchGrokAlbumMedia.mock.calls[0]?.[1] as string;

    fireEvent.click(screen.getByRole("button", { name: "modal-close" }));

    expect(cancelGrokAlbumRequests).toHaveBeenCalledWith([requestId]);
    expect(onClose).toHaveBeenCalledTimes(1);
    pending.resolve({
      path: "H:\\wallpapers\\closing-image.jpg",
      name: "closing-image.jpg",
      mime: "image/jpeg",
      bytes: 128,
    });
    await expect(load).resolves.toBeNull();
  });

  it("opens a downloaded album video as a video slide", async () => {
    xSearchState.busy = false;
    xSearchState.requestId = null;
    const url =
      "https://assets.grok.com/users/test/generated/fake/private-video.mp4";
    grokAlbumItems.push(
      galleryItem("album-video", {
        thumbUrl: url,
        fullUrl: url,
        kind: "video",
        source: "grok_album",
      }),
    );
    fetchGrokAlbumThumbnail.mockResolvedValue({
      dataUrl: "data:image/jpeg;base64,YWJj",
      width: 32,
      height: 32,
    });
    fetchGrokAlbumMedia.mockResolvedValue({
      path: "H:\\wallpapers\\album-video.mp4",
      name: "album-video.mp4",
      mime: "video/mp4",
      bytes: 1024,
    });

    render(
      <WallpaperSourceModal
        open
        initialTab="grok_album"
        t={t as never}
        onClose={vi.fn()}
        onPickFile={vi.fn()}
      />,
    );

    fireEvent.click(
      await screen.findByRole("button", {
        name: "settings.wallpaperSource.openPreview",
      }),
    );

    await waitFor(() => expect(openViewer).toHaveBeenCalledTimes(1));
    const slides = openViewer.mock.calls[0]?.[0] as Array<{
      loadOriginal?: () => Promise<{
        kind?: "image" | "video";
        mime?: string;
      } | null>;
    }>;
    expect(slides[0]?.loadOriginal).toBeTypeOf("function");
    await expect(slides[0]!.loadOriginal!()).resolves.toMatchObject({
      kind: "video",
      mime: "video/mp4",
    });
    expect(fetchGrokAlbumMedia).toHaveBeenCalledWith(
      url,
      expect.stringMatching(/^[0-9a-f-]{36}$/),
    );
  });

  it("appends live batches and lets the final result replace sparse metadata", async () => {
    xSearchState.busy = false;
    xSearchState.requestId = null;
    xSearchState.progressiveItems = [];
    const pending = deferred<WallpaperSearchResult>();
    searchX.mockReturnValue(pending.promise);
    const props = {
      open: true,
      t: t as never,
      onClose: vi.fn(),
      onPickFile: vi.fn(),
    };
    const view = render(<WallpaperSourceModal {...props} />);

    fireEvent.change(
      screen.getByPlaceholderText("settings.wallpaperSource.xPlaceholder"),
      { target: { value: "misty mountains" } },
    );
    fireEvent.click(
      screen.getByRole("button", { name: "settings.wallpaperSource.search" }),
    );
    expect(searchX).toHaveBeenCalledWith("misty mountains", "top");

    xSearchState.busy = true;
    xSearchState.requestId = "request-progressive";
    xSearchState.progressiveItems = [galleryItem("first")];
    xSearchState.progressiveCount = 1;
    view.rerender(<WallpaperSourceModal {...props} />);
    await waitFor(() => expect(screen.getAllByRole("listitem")).toHaveLength(1));

    xSearchState.progressiveItems = [
      galleryItem("first"),
      galleryItem("second"),
    ];
    xSearchState.progressiveCount = 2;
    view.rerender(<WallpaperSourceModal {...props} />);
    await waitFor(() => expect(screen.getAllByRole("listitem")).toHaveLength(2));

    await act(async () => {
      pending.resolve({
        items: [
          galleryItem("first", { username: "alice", likes: 42 }),
          galleryItem("second", { username: "bob" }),
        ],
        meta: {
          requestId: "request-progressive",
          requestedMode: "responses_preview",
          routeUsed: "responses",
          durationMs: 25,
          cacheHit: false,
          candidateCount: 2,
          validCount: 2,
        },
      });
      await pending.promise;
    });

    await waitFor(() => expect(screen.getByText(/@alice/)).toBeTruthy());
    expect(screen.getByText("@bob")).toBeTruthy();
  });

  it("keeps validated batches visible when the final invoke rejects", async () => {
    xSearchState.busy = false;
    xSearchState.requestId = null;
    const pending = deferred<WallpaperSearchResult>();
    searchX.mockReturnValue(pending.promise);
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
    xSearchState.busy = true;
    xSearchState.requestId = "request-partial";
    xSearchState.progressiveItems = [galleryItem("survivor")];
    xSearchState.progressiveCount = 1;
    view.rerender(<WallpaperSourceModal {...props} />);
    await waitFor(() => expect(screen.getAllByRole("listitem")).toHaveLength(1));

    await act(async () => {
      pending.reject(new Error("transport failed"));
      await pending.promise.catch(() => undefined);
    });

    await waitFor(() => expect(screen.getByRole("alert")).toBeTruthy());
    expect(screen.getAllByRole("listitem")).toHaveLength(1);
  });

  it("loads one more Responses batch and lets final item metadata replace the live batch", async () => {
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
      { target: { value: "misty mountains" } },
    );
    fireEvent.click(
      screen.getByRole("button", { name: "settings.wallpaperSource.search" }),
    );
    const loadMoreButton = await screen.findByRole("button", {
      name: "settings.wallpaperSource.loadMore",
    });
    fireEvent.change(
      screen.getByPlaceholderText("settings.wallpaperSource.xPlaceholder"),
      { target: { value: "edited but not searched" } },
    );
    fireEvent.click(loadMoreButton);
    expect(loadMoreX).toHaveBeenCalledWith("misty mountains", "top");

    xSearchState.busy = true;
    xSearchState.requestId = "request-more";
    xSearchState.progressiveItems = [galleryItem("fresh")];
    xSearchState.progressiveCount = 1;
    view.rerender(<WallpaperSourceModal {...props} />);
    await waitFor(() => expect(screen.getAllByRole("listitem")).toHaveLength(2));

    await act(async () => {
      pendingMore.resolve({
        items: [
          galleryItem("initial"),
          galleryItem("fresh", { username: "final-author", likes: 9 }),
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

    await waitFor(() => expect(screen.getByText(/@final-author/)).toBeTruthy());
    expect(screen.getAllByRole("listitem")).toHaveLength(2);
    expect(
      screen.queryByRole("button", {
        name: "settings.wallpaperSource.loadMore",
      }),
    ).toBeNull();
  });

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
  });
});
