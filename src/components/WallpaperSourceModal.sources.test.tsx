/**
 * @vitest-environment jsdom
 */
import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import "@/test/jsdomStubs";
import type { WallpaperGalleryItem } from "@/lib/wallpaperSource";

const cancelSearch = vi.hoisted(() => vi.fn(async () => true));
const searchX = vi.hoisted(() => vi.fn());
const loadMoreX = vi.hoisted(() => vi.fn());
const openGrokAlbum = vi.hoisted(() => vi.fn(async () => undefined));
const syncGrokAlbum = vi.hoisted(() => vi.fn(async () => null));
const refreshGrokAlbum = vi.hoisted(() => vi.fn(async () => undefined));
const loadMoreGrokAlbum = vi.hoisted(() => vi.fn(async () => undefined));
const searchRemote = vi.hoisted(() => vi.fn(async () => null));
const loadMoreRemote = vi.hoisted(() => vi.fn(async () => null));
const cancelRemote = vi.hoisted(() => vi.fn(async () => true));
const wallpaperLibraryList = vi.hoisted(() => vi.fn(async () => []));
const secretsGetMasked = vi.hoisted(() =>
  vi.fn(async () => ({ hasPexelsKey: false })),
);
const secretsSet = vi.hoisted(() => vi.fn(async () => undefined));
const fetchRemoteWallpaperMedia = vi.hoisted(() => vi.fn());
const openViewer = vi.hoisted(() => vi.fn());
const remoteControllerState = vi.hoisted(() => ({
  busy: false,
  loadingMore: false,
  progress: null as string | null,
  canLoadMore: false,
  searchItems: [] as WallpaperGalleryItem[],
}));
const xSearchState = vi.hoisted(() => ({
  busy: false,
  requestId: null as string | null,
  stage: null,
  progressiveItems: [] as WallpaperGalleryItem[],
  progressiveCount: 0,
  progressiveDone: false,
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
  items: [] as WallpaperGalleryItem[],
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
  }),
}));

vi.mock("@/lib/api", () => ({
  isDesktopHost: () => true,
  isTauri: () => false,
  wallpaperFetchMedia: vi.fn(),
  wallpaperRemoteFetchMedia: fetchRemoteWallpaperMedia,
  wallpaperGrokAlbumFetchMedia: vi.fn(),
  wallpaperGrokAlbumThumbnail: vi.fn(),
  wallpaperGrokAlbumCancelRequests: vi.fn(async () => 0),
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
  searchRemote.mockClear();
  loadMoreRemote.mockClear();
  cancelRemote.mockClear();
  wallpaperLibraryList.mockClear();
  wallpaperLibraryList.mockResolvedValue([]);
  secretsGetMasked.mockClear();
  secretsGetMasked.mockResolvedValue({ hasPexelsKey: false });
  secretsSet.mockClear();
  fetchRemoteWallpaperMedia.mockReset();
  openViewer.mockReset();
  remoteControllerState.busy = false;
  remoteControllerState.loadingMore = false;
  remoteControllerState.progress = null;
  remoteControllerState.canLoadMore = false;
  remoteControllerState.searchItems = [];
  grokAlbumState.status = "closed";
  grokAlbumState.busy = false;
  grokAlbumState.items = [];
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

const t = (key: string) => key;

describe("WallpaperSourceModal source workspace", () => {
  it("associates the source navigation with one workspace", () => {
    const view = render(
      <WallpaperSourceModal
        open
        t={t as never}
        onClose={vi.fn()}
        onPickFile={vi.fn()}
      />,
    );

    const tablist = screen.getByRole("tablist", {
      name: "settings.wallpaperSource.title",
    });
    const tabs = screen.getAllByRole("tab");
    const panel = screen.getByRole("tabpanel");
    expect(tabs).toHaveLength(7);
    expect(panel.id).toBe("wallpaper-source-panel");
    expect(panel.getAttribute("aria-labelledby")).toBe(
      "wallpaper-source-tab-x",
    );
    expect(
      tabs.every((tab) => tab.getAttribute("aria-controls") === panel.id),
    ).toBe(true);
    const groups = Array.from(
      tablist.querySelectorAll(".wallpaper-source-tabs__group"),
    );
    expect(
      groups.map((group) =>
        Array.from(group.querySelectorAll('[role="tab"]')).map((tab) =>
          tab.getAttribute("aria-label"),
        ),
      ),
    ).toEqual([
      [
        "settings.wallpaperFromX",
        "settings.wallpaperWeb",
        "settings.wallpaperOpenverse",
        "settings.wallpaperPexels",
      ],
      ["settings.wallpaperImagine"],
      ["settings.wallpaperGrokAlbum", "settings.wallpaperLibrary"],
    ]);

    const layout = view.container.querySelector(".wallpaper-source-layout");
    expect(layout?.firstElementChild).toBe(tablist);
    expect(layout?.lastElementChild).toBe(panel);
  });

  it("keeps paged search and album cards stable while results append", () => {
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
    ).toContain("wallpaper-masonry--stable");
    fireEvent.click(
      screen.getByRole("tab", { name: "settings.wallpaperImagine" }),
    );
    expect(
      view.container.querySelector(".wallpaper-masonry")?.classList,
    ).not.toContain("wallpaper-masonry--stable");
  });

  it("keeps an empty library free of zero-count filter controls", async () => {
    render(
      <WallpaperSourceModal
        open
        initialTab="library"
        t={t as never}
        onClose={vi.fn()}
        onPickFile={vi.fn()}
      />,
    );

    await waitFor(() => expect(wallpaperLibraryList).toHaveBeenCalledWith(96));
    expect(
      screen.queryByRole("button", {
        name: "settings.wallpaperSource.kind.all",
      }),
    ).toBeNull();
    expect(
      screen.queryByRole("button", {
        name: "settings.wallpaperSource.clearFilters",
      }),
    ).toBeNull();
  });

  it("saves a Host-only Pexels key before enabling search", async () => {
    render(
      <WallpaperSourceModal
        open
        t={t as never}
        onClose={vi.fn()}
        onPickFile={vi.fn()}
      />,
    );

    expect(
      screen.getByRole("tab", { name: "settings.wallpaperOpenverse" }),
    ).toBeTruthy();
    fireEvent.click(
      screen.getByRole("tab", { name: "settings.wallpaperPexels" }),
    );
    await waitFor(() => expect(secretsGetMasked).toHaveBeenCalledTimes(1));
    fireEvent.change(
      screen.getByPlaceholderText("settings.wallpaperSource.pexels.placeholder"),
      { target: { value: "mountain lake" } },
    );
    expect(
      screen.getByRole<HTMLButtonElement>("button", {
        name: "settings.wallpaperSource.search",
      }).disabled,
    ).toBe(true);

    fireEvent.change(
      await screen.findByLabelText(
        "settings.wallpaperSource.pexels.keyPlaceholder",
      ),
      { target: { value: "pexels-test-key" } },
    );
    fireEvent.click(
      screen.getByRole("button", {
        name: "settings.wallpaperSource.pexels.keySave",
      }),
    );
    await waitFor(() =>
      expect(secretsSet).toHaveBeenCalledWith({
        pexelsApiKey: "pexels-test-key",
      }),
    );
    expect(
      await screen.findByText("settings.wallpaperSource.pexels.keySaved"),
    ).toBeTruthy();
    expect(
      screen.getByRole<HTMLButtonElement>("button", {
        name: "settings.wallpaperSource.search",
      }).disabled,
    ).toBe(false);
  });
});

describe("WallpaperSourceModal remote source wiring", () => {
  it("keeps a regular Web grid free of a redundant local filter", async () => {
    remoteControllerState.searchItems = Array.from({ length: 10 }, (_, index) =>
      galleryItem(`web-${index}`, {
        source: "web",
        sourceUrl: `https://photos.example.test/page-${index}`,
        sourceName: "photos.example.test",
      }),
    );
    render(
      <WallpaperSourceModal
        open
        initialTab="web"
        t={t as never}
        onClose={vi.fn()}
        onPickFile={vi.fn()}
      />,
    );

    fireEvent.change(
      screen.getByPlaceholderText("settings.wallpaperSource.web.placeholder"),
      { target: { value: "night skyline" } },
    );
    fireEvent.click(
      screen.getByRole("button", { name: "settings.wallpaperSource.search" }),
    );
    await waitFor(() => expect(screen.getAllByRole("listitem")).toHaveLength(10));
    expect(
      screen.queryByPlaceholderText("settings.wallpaperSource.filterPlaceholder"),
    ).toBeNull();
    expect(document.querySelectorAll(".wallpaper-masonry__meta")).toHaveLength(0);
  });

  it("routes Web search, progress, cancellation, and close", () => {
    remoteControllerState.progress =
      "settings.wallpaperSource.remote.progress.fetchingSources";
    const onClose = vi.fn();
    const props = {
      open: true,
      initialTab: "web" as const,
      t: t as never,
      onClose,
      onPickFile: vi.fn(),
    };
    const view = render(<WallpaperSourceModal {...props} />);

    expect(
      screen.getByText(
        "settings.wallpaperSource.remote.progress.fetchingSources",
      ),
    ).toBeTruthy();
    fireEvent.change(
      screen.getByPlaceholderText("settings.wallpaperSource.web.placeholder"),
      { target: { value: "night skyline" } },
    );
    fireEvent.click(
      screen.getByRole("button", { name: "settings.wallpaperSource.search" }),
    );
    expect(searchRemote).toHaveBeenCalledTimes(1);

    remoteControllerState.busy = true;
    view.rerender(<WallpaperSourceModal {...props} />);
    fireEvent.click(
      screen.getByRole("button", {
        name: "settings.wallpaperSource.cancelSearch",
      }),
    );
    expect(cancelRemote).toHaveBeenCalledTimes(1);
    fireEvent.click(screen.getByRole("button", { name: "modal-close" }));
    expect(cancelRemote).toHaveBeenCalledTimes(2);
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("opens an existing Web image while load more is running", async () => {
    remoteControllerState.searchItems = [
      galleryItem("web-existing", {
        source: "web",
        sourceUrl: "https://photos.example.test/page",
        sourceName: "photos.example.test",
      }),
    ];
    const props = {
      open: true,
      initialTab: "web" as const,
      t: t as never,
      onClose: vi.fn(),
      onPickFile: vi.fn(),
    };
    const view = render(<WallpaperSourceModal {...props} />);

    fireEvent.change(
      screen.getByPlaceholderText("settings.wallpaperSource.web.placeholder"),
      { target: { value: "night skyline" } },
    );
    fireEvent.click(
      screen.getByRole("button", { name: "settings.wallpaperSource.search" }),
    );
    const card = await screen.findByRole("button", {
      name: "settings.wallpaperSource.openPreview",
    });
    remoteControllerState.busy = true;
    remoteControllerState.loadingMore = true;
    remoteControllerState.canLoadMore = true;
    view.rerender(<WallpaperSourceModal {...props} />);

    expect((card as HTMLButtonElement).disabled).toBe(false);
    fireEvent.click(card);
    await waitFor(() => expect(openViewer).toHaveBeenCalledTimes(1));
    expect(fetchRemoteWallpaperMedia).not.toHaveBeenCalled();
    expect(openViewer.mock.calls[0]?.[0]?.[0]).toMatchObject({
      src: "https://example.test/web-existing.jpg",
      kind: "image",
    });
  });
});
