/**
 * @vitest-environment jsdom
 */
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import "@/test/jsdomStubs";

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

vi.mock("@/lib/api", () => ({
  isDesktopHost: () => true,
  isTauri: () => false,
  wallpaperFetchMedia: vi.fn(),
  wallpaperGrokAlbumFetchMedia: fetchAlbumMedia,
  wallpaperGrokAlbumThumbnail: vi.fn(async () => ({
    dataUrl: "data:image/jpeg;base64,YWJj",
    width: 32,
    height: 18,
  })),
  wallpaperGrokAlbumCancelRequests: vi.fn(async () => 0),
  wallpaperGrokAlbumCancelAllRequests: vi.fn(async () => 0),
  wallpaperImagine: vi.fn(),
  wallpaperLibraryList: vi.fn(),
  wallpaperLibraryDelete: vi.fn(),
  openExternalUrl: vi.fn(),
}));

vi.mock("@/components/Select", () => ({
  Select: ({ value }: { value: string }) => <span>{value}</span>,
}));

import { ImageViewerProvider } from "./ImageViewer";
import { WallpaperSourceModal } from "./WallpaperSourceModal";

afterEach(() => {
  cleanup();
  fetchAlbumMedia.mockClear();
});

describe("WallpaperSourceModal image viewer integration", () => {
  it("opens the real lightbox portal for a lazy Grok album card", async () => {
    render(
      <ImageViewerProvider locale="en">
        <WallpaperSourceModal
          open
          initialTab="grok_album"
          t={(key) => key}
          onClose={vi.fn()}
          onPickFile={vi.fn()}
        />
      </ImageViewerProvider>,
    );

    fireEvent.click(
      await screen.findByRole("button", {
        name: "settings.wallpaperSource.openPreview",
      }),
    );

    await waitFor(() => {
      expect(document.querySelector(".yarl__portal")).not.toBeNull();
    });
    expect(document.querySelector(".yarl__root")).not.toBeNull();
  });
});
