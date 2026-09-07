/**
 * @vitest-environment jsdom
 */
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { MessageKey } from "@/i18n";
import { ensureMediaEndpoint, resolveImageSrcSync } from "@/lib/imageSrc";
import {
  WallpaperSourceGallery,
  type WallpaperSourceGalleryProps,
} from "./WallpaperSourceGallery";

vi.mock("@/lib/imageSrc", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/imageSrc")>();
  return {
    ...actual,
    ensureMediaEndpoint: vi.fn(() => Promise.resolve()),
    resolveImageSrcSync: vi.fn((path: string) =>
      `http://127.0.0.1/media/${encodeURIComponent(path)}`),
  };
});

afterEach(cleanup);

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
    onGenerateVideo: vi.fn(),
    onEditImage: vi.fn(),
    onDropItem: vi.fn(),
    onOpenXStatus: vi.fn(),
    onOpenSource: vi.fn(),
    onDeleteLibraryItem: vi.fn(),
    onLoadMore: vi.fn(),
    ...overrides,
  };
}

describe("WallpaperSourceGallery", () => {
  it("closes details when a filter removes its item and does not reopen it when rows return", () => {
    const item = { id: "details", source: "library", kind: "image", fullUrl: "file:///test.png", thumbUrl: "", localPath: "/test.png", textPreview: "Lake" };
    const props = galleryProps({ tab: "library", visibleItems: [item] });
    const view = render(<WallpaperSourceGallery {...props} />);
    fireEvent.click(screen.getByRole("button", { name: "settings.wallpaperSource.details.title: Lake" }));
    expect(screen.getByRole("dialog")).toBeTruthy();
    expect(props.onPreview).not.toHaveBeenCalled();
    view.rerender(<WallpaperSourceGallery {...props} visibleItems={[]} />);
    expect(screen.queryByRole("dialog")).toBeNull();
    view.rerender(<WallpaperSourceGallery {...props} />);
    expect(screen.queryByRole("dialog")).toBeNull();
  });
  it("toggles a favorite without opening preview and shows its saved state", () => {
    const item = { id: "favorite-image", source: "library", kind: "image", localPath: "/test.png", fullUrl: "file:///test.png", thumbUrl: "", textPreview: "Lake" };
    const toggle = vi.fn();
    const props = galleryProps({ tab: "library", visibleItems: [item], onToggleFavorite: toggle });
    const view = render(<WallpaperSourceGallery {...props} />);
    fireEvent.click(screen.getByRole("button", { name: "settings.wallpaperSource.library.favorite: Lake" }));
    expect(toggle).toHaveBeenCalledWith(item);
    expect(props.onPreview).not.toHaveBeenCalled();
    view.rerender(<WallpaperSourceGallery {...props} visibleItems={[{ ...item, metadata: { favorite: true } as never }]} />);
    expect(screen.getByRole("button", { name: "settings.wallpaperSource.library.unfavorite: Lake" }).getAttribute("aria-pressed")).toBe("true");
  });

  it("retains a failed local card and retries only its media", () => {
    const item = { id: "failed-image", source: "library", kind: "image", fullUrl: "file:///test.png", thumbUrl: "", localPath: "/test.png", textPreview: "Lake", width: 600, height: 400 };
    const props = galleryProps({ tab: "library", visibleItems: [item, { ...item, id: "good-image", localPath: "/good.png", textPreview: "Mountain" }] });
    const { container } = render(<WallpaperSourceGallery {...props} />);
    const original = container.querySelectorAll("img")[0];
    const unchanged = container.querySelectorAll("img")[1];
    fireEvent.error(original);
    expect(props.onDropItem).not.toHaveBeenCalled();
    expect(screen.getAllByRole("listitem")).toHaveLength(2);
    fireEvent.click(screen.getByRole("button", { name: "ui.errorBoundary.retry: Lake" }));
    expect(container.querySelectorAll("img")).toHaveLength(2);
    expect(container.querySelectorAll("img")[0]).not.toBe(original);
    expect(container.querySelectorAll("img")[1]).toBe(unchanged);
    expect(props.onPreview).not.toHaveBeenCalled();
  });

  it.each(["library", "imagine", "x", "grok_album", "web", "openverse", "pexels"] as const)(
    "hands the original image to edit from %s without triggering preview", (tab) => {
      const image = { id: "image", kind: "image", source: tab, localPath: "/source.png", fullUrl: "file:///source.png", thumbUrl: "" };
      const props = galleryProps({ tab, visibleItems: [image, { ...image, id: "video", kind: "video" }] });
      render(<WallpaperSourceGallery {...props} />);
      const buttons = screen.getAllByRole("button", { name: /settings.wallpaperSource.editImage/ });
      expect(buttons).toHaveLength(1);
      fireEvent.click(buttons[0]);
      expect(props.onEditImage).toHaveBeenCalledWith(image);
      expect(props.onPreview).not.toHaveBeenCalled();
    },
  );
  it.each(["library", "imagine"] as const)(
    "refreshes a local video in %s after the media endpoint becomes ready",
    async (tab) => {
      let finishEndpoint!: () => void;
      vi.mocked(ensureMediaEndpoint).mockReturnValueOnce(
        new Promise((resolve) => {
          finishEndpoint = () => resolve(null);
        }),
      );
      vi.mocked(resolveImageSrcSync).mockReturnValueOnce(null);
      const path = "H:/wallpapers/imagine/result.mp4";
      const { container } = render(
        <WallpaperSourceGallery
          {...galleryProps({
            tab,
            visibleItems: [{
              id: "generated-video",
              kind: "video",
              source: "imagine",
              localPath: path,
              fullUrl: `file://${path}`,
              thumbUrl: "",
            }],
          })}
        />,
      );
      const video = container.querySelector("video");
      expect(video?.getAttribute("src")).toBe(`file://${path}`);
      await act(async () => finishEndpoint());
      expect(video?.getAttribute("src")).toBe(
        `http://127.0.0.1/media/${encodeURIComponent(path)}`,
      );
    },
  );

  it("gives each preview control an item-specific accessible name", () => {
    render(
      <WallpaperSourceGallery
        {...galleryProps({
          visibleItems: [
            {
              id: "first",
              thumbUrl: "https://images.example.test/first.jpg",
              fullUrl: "https://images.example.test/first.jpg",
              kind: "image",
              source: "openverse",
              textPreview: "Misty ridge",
            },
            {
              id: "second",
              thumbUrl: "https://images.example.test/second.jpg",
              fullUrl: "https://images.example.test/second.jpg",
              kind: "image",
              source: "openverse",
              textPreview: "Night coast",
            },
          ],
          kindCounts: { all: 2, image: 2, video: 0 },
          selectedId: "first",
        })}
      />,
    );

    const firstPreview = screen.getByRole("button", {
      name: "settings.wallpaperSource.openPreview: Misty ridge",
    });
    const secondPreview = screen.getByRole("button", {
      name: "settings.wallpaperSource.openPreview: Night coast",
    });
    expect(firstPreview.getAttribute("aria-pressed")).toBe("true");
    expect(secondPreview.getAttribute("aria-pressed")).toBe("false");
    expect(
      screen.getByRole("button", {
        name: "settings.wallpaperSource.generateVideoFromImage: Misty ridge",
      }),
    ).toBeTruthy();
    expect(
      screen.getByRole("button", {
        name: "settings.wallpaperSource.generateVideoFromImage: Night coast",
      }),
    ).toBeTruthy();
  });

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
        onGenerateVideo={vi.fn()}
        onEditImage={vi.fn()}
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
    "keeps %s provenance accessible without repeating the provider label",
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

      const sourceAction = screen.getByRole("button", { name: sourceName });
      expect(sourceAction.querySelector("svg")).not.toBeNull();
      expect(screen.queryByText(sourceName)).toBeNull();
      expect(container.querySelector(".wallpaper-masonry__meta")).toBeNull();
      expect(container.querySelector(".wallpaper-attribution")).not.toBeNull();
      expect(
        container.querySelector(".wallpaper-masonry__media img"),
      ).not.toBeNull();
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

  it("renders only one clear action when filters hide the collection", () => {
    render(
      <WallpaperSourceGallery
        {...galleryProps({
          kindCounts: { all: 3, image: 3, video: 0 },
          galleryFilter: "no-match",
          filtersActive: true,
          showFilters: true,
          showTextFilter: true,
          emptyState: {
            kind: "filter_empty",
            titleKey: "settings.wallpaperSource.empty.filterEmpty",
            hintKey: "settings.wallpaperSource.empty.filterEmptyHint",
            showClearFilters: true,
            softFail: true,
          },
          showEmptyBlock: true,
        })}
      />,
    );

    expect(
      screen.getAllByRole("button", {
        name: "settings.wallpaperSource.clearFilters",
      }),
    ).toHaveLength(1);
    expect(
      screen.getByText("settings.wallpaperSource.empty.filterEmpty"),
    ).toBeTruthy();
  });

  it("routes an image play action without opening the preview", () => {
    const image = {
      id: "search-image",
      thumbUrl: "https://images.example.test/image.jpg",
      fullUrl: "https://images.example.test/image.jpg",
      kind: "image",
      source: "openverse",
    } satisfies WallpaperSourceGalleryProps["visibleItems"][number];
    const video = {
      ...image,
      id: "search-video",
      kind: "video",
      fullUrl: "https://images.example.test/video.mp4",
    };
    const onPreview = vi.fn();
    const onGenerateVideo = vi.fn();
    render(
      <WallpaperSourceGallery
        {...galleryProps({
          visibleItems: [image, video],
          kindCounts: { all: 2, image: 1, video: 1 },
          onPreview,
          onGenerateVideo,
        })}
      />,
    );

    const action = screen.getByRole("button", {
      name: "settings.wallpaperSource.generateVideoFromImage: search-image",
    });
    expect(
      screen.getAllByRole("button", {
        name: /^settings\.wallpaperSource\.openPreview/,
      }),
    ).toHaveLength(2);
    fireEvent.click(action);

    expect(onGenerateVideo).toHaveBeenCalledWith(image);
    expect(onPreview).not.toHaveBeenCalled();
  });

  it("locks the image play action with the rest of the card", () => {
    render(
      <WallpaperSourceGallery
        {...galleryProps({
          locked: true,
          visibleItems: [
            {
              id: "locked-image",
              thumbUrl: "https://images.example.test/locked.jpg",
              fullUrl: "https://images.example.test/locked.jpg",
              kind: "image",
              source: "x",
            },
          ],
          kindCounts: { all: 1, image: 1, video: 0 },
        })}
      />,
    );

    expect(
      screen.getByRole<HTMLButtonElement>("button", {
        name: "settings.wallpaperSource.generateVideoFromImage: locked-image",
      }).disabled,
    ).toBe(true);
  });
});
