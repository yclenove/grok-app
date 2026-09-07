/**
 * @vitest-environment jsdom
 */
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import "@/test/jsdomStubs";

vi.mock("@/lib/imageLightboxFit", async () => {
  const actual = await vi.importActual<typeof import("@/lib/imageLightboxFit")>(
    "@/lib/imageLightboxFit",
  );
  return {
    ...actual,
    loadImageNaturalSize: vi.fn(async () => ({ width: 1920, height: 1080 })),
  };
});

const fetchAlbumMedia = vi.hoisted(() =>
  vi.fn(async () => ({
    path: "H:\\wallpapers\\integration-video.mp4",
    name: "integration-video.mp4",
    mime: "video/mp4",
    bytes: 1024,
  })),
);
const xSearchState = vi.hoisted(() => ({
  search: vi.fn(),
  loadMore: vi.fn(),
  cancel: vi.fn(),
}));
const remoteControllerState = vi.hoisted(() => ({
  busy: false,
  loadingMore: false,
  progress: null as string | null,
  canLoadMore: true,
}));
const webItems = vi.hoisted(() => [
  {
    id: "web-integration-image",
    thumbUrl: "https://images.example.test/wallpaper.jpg",
    fullUrl: "https://images.example.test/wallpaper.jpg",
    kind: "image" as const,
    source: "web" as const,
    width: 1920,
    height: 1080,
    sourceUrl: "https://photos.example.test/wallpaper",
    sourceName: "photos.example.test",
  },
]);
const albumState = vi.hoisted(() => {
  const albumUrl =
    "https://assets.grok.com/users/test/generated/fake/integration-video.mp4";
  return {
    items: [
      {
        id: "album-integration-video",
        thumbUrl: albumUrl,
        fullUrl: albumUrl,
        kind: "video" as const,
        source: "grok_album" as const,
        width: 1280,
        height: 720,
      },
    ],
    open: vi.fn(),
    sync: vi.fn(),
    refresh: vi.fn(),
    loadMore: vi.fn(),
  };
});

vi.mock("@/hooks/useWallpaperXSearch", () => ({
  useWallpaperXSearch: () => ({
    busy: false,
    requestId: null,
    stage: null,
    progressiveItems: [],
    search: xSearchState.search,
    loadMore: xSearchState.loadMore,
    cancel: xSearchState.cancel,
  }),
}));

vi.mock("@/hooks/useWallpaperGrokAlbum", () => ({
  useWallpaperGrokAlbum: () => ({
    items: albumState.items,
    status: "ready",
    cachedCount: 1,
    visibleCount: 1,
    busy: false,
    syncing: false,
    loadingMore: false,
    hasSynced: true,
    errorCode: null,
    canLoadMore: false,
    open: albumState.open,
    sync: albumState.sync,
    refresh: albumState.refresh,
    loadMore: albumState.loadMore,
  }),
}));

vi.mock("@/hooks/useWallpaperRemoteSourceController", () => ({
  useWallpaperRemoteSourceController: (options: {
    setItems: (items: typeof webItems) => void;
    setHasSearched: (value: boolean) => void;
  }) => ({
    ...remoteControllerState,
    search: async () => {
      options.setItems(webItems);
      options.setHasSearched(true);
    },
    loadMore: vi.fn(),
    cancel: vi.fn(async () => false),
    clear: vi.fn(),
  }),
}));

vi.mock("@/lib/api", () => ({
  isDesktopHost: () => true,
  isTauri: () => false,
  wallpaperFetchMedia: vi.fn(),
  wallpaperLibraryRemember: vi.fn(),
  wallpaperLibraryLookup: vi.fn(async () => []),
  wallpaperRemoteFetchMedia: vi.fn(),
  wallpaperGrokAlbumFetchMedia: fetchAlbumMedia,
  wallpaperGrokAlbumThumbnail: vi.fn(async () => ({
    dataUrl: "data:image/jpeg;base64,YWJj",
    width: 32,
    height: 18,
  })),
  wallpaperGrokAlbumCancelRequests: vi.fn(async () => 0),
  wallpaperGrokAlbumCancelAllRequests: vi.fn(async () => 0),
  wallpaperRemoteCancelMediaRequests: vi.fn(async () => 0),
  wallpaperRemoteCancelAllMediaRequests: vi.fn(async () => 0),
  wallpaperImagine: vi.fn(),
  wallpaperLibraryList: vi.fn(),
  wallpaperLibraryPage: vi.fn(async () => ({ items: [], nextCursor: null, total: 0, kindCounts: { all: 0, image: 0, video: 0 } })),
  wallpaperLibraryDelete: vi.fn(),
  openExternalUrl: vi.fn(),
}));

vi.mock("@/components/Select", () => ({
  Select: ({ value }: { value: string }) => <span>{value}</span>,
}));

import { ImageViewerProvider } from "./ImageViewer";
import { WallpaperSourceModal } from "./WallpaperSourceModal";
import { setMediaEndpoint } from "@/lib/imageSrc";

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  setMediaEndpoint(null);
  fetchAlbumMedia.mockClear();
  remoteControllerState.busy = false;
  remoteControllerState.loadingMore = false;
  remoteControllerState.progress = null;
  remoteControllerState.canLoadMore = true;
});

describe("WallpaperSourceModal image viewer integration", () => {
  it("retries a failed original inside the real preview without dropping the card or leaving a gallery error", async () => {
    setMediaEndpoint({ baseUrl: "http://127.0.0.1:19200", token: "test-only" });
    vi.spyOn(HTMLMediaElement.prototype, "play").mockResolvedValue();
    vi.spyOn(HTMLMediaElement.prototype, "pause").mockImplementation(() => {});
    fetchAlbumMedia.mockRejectedValueOnce(new Error("download_failed: private diagnostic"));
    render(
      <ImageViewerProvider locale="en">
        <WallpaperSourceModal open initialTab="grok_album" t={(key) => key === "settings.wallpaperSource.err.download_failed" ? "Download failed" : key} onClose={vi.fn()} onPickFile={vi.fn()} />
      </ImageViewerProvider>,
    );
    fireEvent.click(await screen.findByRole("button", { name: /^settings\.wallpaperSource\.openPreview/ }));
    const retry = await screen.findByRole("button", { name: "Retry" });
    const portal = document.querySelector<HTMLElement>(".yarl__portal")!;
    expect(within(portal).getByRole("status").textContent).toContain("Download failed");
    expect(document.body.textContent).not.toContain("private diagnostic");
    expect(screen.getAllByRole("listitem", { hidden: true })).toHaveLength(1);
    fireEvent.click(retry);
    await waitFor(() => expect(portal.querySelector("video")).not.toBeNull());
    expect(fetchAlbumMedia).toHaveBeenCalledTimes(2);
    expect(screen.queryByRole("button", { name: "Retry" })).toBeNull();
    expect(document.body.textContent).not.toContain("Download failed");
    expect(screen.getAllByRole("listitem", { hidden: true })).toHaveLength(1);
  });

  it("opens the real lightbox portal for a lazy Grok album card", async () => {
    const onClose = vi.fn();
    render(
      <ImageViewerProvider locale="en">
        <WallpaperSourceModal
          open
          initialTab="grok_album"
          t={(key) => key}
          onClose={onClose}
          onPickFile={vi.fn()}
        />
      </ImageViewerProvider>,
    );

    fireEvent.click(
      await screen.findByRole("button", {
        name: /^settings\.wallpaperSource\.openPreview/,
      }),
    );

    await waitFor(() => {
      expect(document.querySelector(".yarl__portal")).not.toBeNull();
    });
    const portal = document.querySelector<HTMLElement>(".yarl__portal");
    expect(portal).not.toBeNull();
    expect(portal?.style.getPropertyValue("--yarl__portal_zindex")).toBe(
      "14000",
    );
    expect(document.querySelector(".yarl__root")).not.toBeNull();

    // The card keeps focus when the portal mounts. Exercise the real keyboard
    // path through the parent dialog instead of dispatching inside Lightbox.
    fireEvent.keyDown(document.activeElement ?? document, {
      key: "Escape",
    });
    await waitFor(() => {
      expect(document.querySelector(".yarl__portal")).toBeNull();
    });
    expect(onClose).not.toHaveBeenCalled();
    expect(
      screen.getByRole("dialog", { name: "settings.wallpaperSource.title" }),
    ).toBeTruthy();

    fireEvent.keyDown(document, { key: "Escape" });
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("opens the real lightbox while Web load more is still running", async () => {
    const view = render(
      <ImageViewerProvider locale="en">
        <WallpaperSourceModal
          open
          initialTab="web"
          t={(key) => key}
          onClose={vi.fn()}
          onPickFile={vi.fn()}
        />
      </ImageViewerProvider>,
    );

    fireEvent.change(
      screen.getByPlaceholderText("settings.wallpaperSource.web.placeholder"),
      { target: { value: "night skyline" } },
    );
    fireEvent.click(
      screen.getByRole("button", { name: "settings.wallpaperSource.search" }),
    );
    const card = await screen.findByRole("button", {
      name: /^settings\.wallpaperSource\.openPreview/,
    });

    remoteControllerState.busy = true;
    remoteControllerState.loadingMore = true;
    view.rerender(
      <ImageViewerProvider locale="en">
        <WallpaperSourceModal
          open
          initialTab="web"
          t={(key) => key}
          onClose={vi.fn()}
          onPickFile={vi.fn()}
        />
      </ImageViewerProvider>,
    );

    expect((card as HTMLButtonElement).disabled).toBe(false);
    fireEvent.click(card);
    await waitFor(() => {
      expect(document.querySelector(".yarl__portal")).not.toBeNull();
    });
  });
});
