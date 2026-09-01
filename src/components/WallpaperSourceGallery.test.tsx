/**
 * @vitest-environment jsdom
 */
import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { MessageKey } from "@/i18n";
import {
  WallpaperSourceGallery,
  type WallpaperSourceGalleryProps,
} from "./WallpaperSourceGallery";

vi.mock("@/lib/imageSrc", () => ({
  resolveImageSrcSync: (path: string) => `http://127.0.0.1/media/${encodeURIComponent(path)}`,
}));

function galleryProps(
  overrides: Partial<WallpaperSourceGalleryProps> = {},
): WallpaperSourceGalleryProps {
  return {
    t: (key: MessageKey) => key,
    tab: "openverse",
    visibleItems: [],
    selectedId: null,
    previewingId: null,
    locked: false,
    busy: false,
    kindCounts: { all: 0, image: 0, video: 0 },
    kindFilter: "all",
    galleryFilter: "",
    filtersActive: false,
    showFilters: false,
    showTextFilter: false,
    emptyState: null,
    showEmptyBlock: false,
    canLoadMore: false,
    loadingMore: false,
    onKindFilterChange: vi.fn(),
    onGalleryFilterChange: vi.fn(),
    onClearFilters: vi.fn(),
    onPreview: vi.fn(),
    onDropItem: vi.fn(),
    onOpenXStatus: vi.fn(),
    onOpenSource: vi.fn(),
    onDeleteLibraryItem: vi.fn(),
    onLoadMore: vi.fn(),
    ...overrides,
  };
}

describe("WallpaperSourceGallery", () => {
  it("renders a saved local video as video instead of a broken image", () => {
    const onDropItem = vi.fn();
    const { container } = render(
      <WallpaperSourceGallery
        t={(key: MessageKey) => key}
        tab="library"
        visibleItems={[
          {
            id: "saved-video",
            thumbUrl: "file:///H:/wallpapers/grok_album/originals/video.mp4",
            fullUrl: "file:///H:/wallpapers/grok_album/originals/video.mp4",
            kind: "video",
            source: "grok_album",
            localPath: "H:\\wallpapers\\grok_album\\originals\\video.mp4",
            textPreview: "video.mp4",
          },
        ]}
        selectedId={null}
        previewingId={null}
        locked={false}
        busy={false}
        kindCounts={{ all: 1, image: 0, video: 1 }}
        kindFilter="video"
        galleryFilter=""
        filtersActive={false}
        showFilters={false}
        showTextFilter={false}
        emptyState={null}
        showEmptyBlock={false}
        canLoadMore={false}
        loadingMore={false}
        onKindFilterChange={vi.fn()}
        onGalleryFilterChange={vi.fn()}
        onClearFilters={vi.fn()}
        onPreview={vi.fn()}
        onDropItem={onDropItem}
        onOpenXStatus={vi.fn()}
        onOpenSource={vi.fn()}
        onDeleteLibraryItem={vi.fn()}
        onLoadMore={vi.fn()}
      />,
    );

    const video = container.querySelector("video");
    expect(video).not.toBeNull();
    expect(container.querySelector("img")).toBeNull();
    expect(video?.getAttribute("preload")).toBe("metadata");
    expect(onDropItem).not.toHaveBeenCalled();
  });

  it.each([
    ["openverse", "Openverse"],
    ["pexels", "Pexels"],
  ] as const)(
    "shows %s provenance once on its own source tab",
    (source, sourceName) => {
      const { container } = render(
        <WallpaperSourceGallery
          {...galleryProps({
            tab: source,
            visibleItems: [
              {
                id: `${source}-image`,
                thumbUrl: `https://images.example.test/${source}.jpg`,
                fullUrl: `https://images.example.test/${source}.jpg`,
                kind: "image",
                source,
                sourceName,
                sourceUrl: `https://${source}.example.test/photo`,
              },
            ],
            kindCounts: { all: 1, image: 1, video: 0 },
          })}
        />,
      );

      expect(screen.getAllByText(sourceName)).toHaveLength(1);
      expect(container.querySelector(".wallpaper-masonry__meta")).toBeNull();
      expect(container.querySelector(".wallpaper-attribution")).not.toBeNull();
    },
  );

  it("keeps the provider label on imported library cards", () => {
    const { container } = render(
      <WallpaperSourceGallery
        {...galleryProps({
          tab: "library",
          visibleItems: [
            {
              id: "saved-openverse-image",
              thumbUrl: "file:///H:/wallpapers/openverse/image.jpg",
              fullUrl: "file:///H:/wallpapers/openverse/image.jpg",
              kind: "image",
              source: "openverse",
              localPath: "H:\\wallpapers\\openverse\\image.jpg",
            },
          ],
          kindCounts: { all: 1, image: 1, video: 0 },
        })}
      />,
    );

    expect(container.querySelector(".wallpaper-masonry__meta")?.textContent).toBe(
      "settings.wallpaperOpenverse",
    );
  });
});
