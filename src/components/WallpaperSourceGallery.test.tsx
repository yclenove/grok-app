/**
 * @vitest-environment jsdom
 */
import { render } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { MessageKey } from "@/i18n";
import { WallpaperSourceGallery } from "./WallpaperSourceGallery";

vi.mock("@/lib/imageSrc", () => ({
  resolveImageSrcSync: (path: string) => `http://127.0.0.1/media/${encodeURIComponent(path)}`,
}));

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
});
