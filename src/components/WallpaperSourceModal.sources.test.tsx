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
import type { GrokAlbumStatus } from "@/lib/grokAlbum";
import { wallpaperImagine } from "@/lib/api";
import type {
  WallpaperGalleryItem,
  WallpaperLibraryEntry,
  WallpaperSourceErrorCode,
} from "@/lib/wallpaperSource";

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
const clearRemote = vi.hoisted(() => vi.fn());
const wallpaperLibraryList = vi.hoisted(() =>
  vi.fn(async (): Promise<WallpaperLibraryEntry[]> => []),
);
const secretsGetMasked = vi.hoisted(() =>
  vi.fn(async () => ({ hasPexelsKey: false })),
);
const secretsSet = vi.hoisted(() => vi.fn(async () => undefined));
const fetchRemoteWallpaperMedia = vi.hoisted(() => vi.fn());
const wallpaperImageToVideo = vi.hoisted(() => vi.fn());
const wallpaperImageToVideoCancel = vi.hoisted(() => vi.fn(async () => true));
const openViewer = vi.hoisted(() => vi.fn());
const tauriInvoke = vi.hoisted(() => vi.fn());
const remoteControllerState = vi.hoisted(() => ({
  busy: false,
  loadingMore: false,
  progress: null as string | null,
  canLoadMore: false,
  searchItems: [] as WallpaperGalleryItem[],
  searchErrorCode: null as WallpaperSourceErrorCode | null,
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
  historyRevision: 0,
  status: "closed" as GrokAlbumStatus,
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
    setError: (value: string | null) => void;
    setErrorCode: (value: WallpaperSourceErrorCode | null) => void;
  }) => ({
    ...remoteControllerState,
    search: async () => {
      const result = await searchRemote();
      if (remoteControllerState.searchErrorCode) {
        options.setHasSearched(true);
        options.setErrorCode(remoteControllerState.searchErrorCode);
        options.setError("provider error");
      }
      if (remoteControllerState.searchItems.length > 0) {
        options.setItems(remoteControllerState.searchItems);
        options.setHasSearched(true);
      }
      return result;
    },
    loadMore: loadMoreRemote,
    cancel: cancelRemote,
    clear: () => {
      clearRemote();
      options.setItems([]);
      options.setHasSearched(false);
      options.setError(null);
      options.setErrorCode(null);
    },
  }),
}));

vi.mock("@/lib/api", () => ({
  isDesktopHost: () => true,
  isTauri: () => false,
  wallpaperFetchMedia: vi.fn(),
  wallpaperLibraryRemember: vi.fn(),
  wallpaperLibraryLookup: vi.fn(async () => []),
  wallpaperRemoteFetchMedia: fetchRemoteWallpaperMedia,
  wallpaperGrokAlbumFetchMedia: vi.fn(),
  wallpaperGrokAlbumThumbnail: vi.fn(),
  wallpaperGrokAlbumCancelRequests: vi.fn(async () => 0),
  wallpaperGrokAlbumCancelAllRequests: vi.fn(async () => 0),
  wallpaperRemoteCancelMediaRequests: vi.fn(async () => 0),
  wallpaperRemoteCancelAllMediaRequests: vi.fn(async () => 0),
  wallpaperImagine: vi.fn(),
  wallpaperImageToVideo,
  wallpaperImageToVideoCancel,
  wallpaperLibraryList,
  wallpaperLibraryPage: vi.fn(async () => {
    const items = await wallpaperLibraryList();
    return { items, nextCursor: null, total: items.length, kindCounts: { all: items.length, image: items.filter((item) => item.kind === "image").length, video: items.filter((item) => item.kind === "video").length } };
  }),
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

vi.mock("@tauri-apps/api/core", () => ({ invoke: tauriInvoke }));

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
  clearRemote.mockClear();
  wallpaperLibraryList.mockClear();
  wallpaperLibraryList.mockResolvedValue([]);
  secretsGetMasked.mockClear();
  secretsGetMasked.mockResolvedValue({ hasPexelsKey: false });
  secretsSet.mockClear();
  fetchRemoteWallpaperMedia.mockReset();
  wallpaperImageToVideo.mockReset();
  wallpaperImageToVideoCancel.mockClear();
  openViewer.mockReset();
  tauriInvoke.mockReset();
  delete (window as typeof window & { __TAURI_INTERNALS__?: unknown })
    .__TAURI_INTERNALS__;
  remoteControllerState.busy = false;
  remoteControllerState.loadingMore = false;
  remoteControllerState.progress = null;
  remoteControllerState.canLoadMore = false;
  remoteControllerState.searchItems = [];
  remoteControllerState.searchErrorCode = null;
  grokAlbumState.status = "closed";
  grokAlbumState.historyRevision = 0;
  grokAlbumState.cachedCount = 0;
  grokAlbumState.visibleCount = 0;
  grokAlbumState.busy = false;
  grokAlbumState.syncing = false;
  grokAlbumState.loadingMore = false;
  grokAlbumState.hasSynced = false;
  grokAlbumState.errorCode = null;
  grokAlbumState.canLoadMore = false;
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
  it("reuses a library prompt in Imagine and preserves the library when returning", async () => {
    wallpaperLibraryList.mockResolvedValueOnce([{
      path: "/saved.png", name: "saved.png", source: "imagine", kind: "image", bytes: 2000, modifiedMs: 1,
      metadata: { id: "saved", title: "Saved artwork", prompt: "A quiet mountain lake", generation: { aspectRatio: "9:16" } } as never,
    }]);
    render(<WallpaperSourceModal open initialTab="library" t={t as never} onClose={vi.fn()} onPickFile={vi.fn()} />);
    fireEvent.click(await screen.findByRole("button", { name: "settings.wallpaperSource.details.title: Saved artwork" }));
    fireEvent.click(screen.getByRole("button", { name: "settings.wallpaperSource.details.reuse" }));
    expect(screen.getByRole("tabpanel").getAttribute("aria-labelledby")).toBe("wallpaper-source-tab-imagine");
    expect(screen.getByDisplayValue("A quiet mountain lake")).toBeTruthy();
    expect(screen.getByText("9:16")).toBeTruthy();
    expect(wallpaperImagine).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("tab", { name: "settings.wallpaperLibrary" }));
    await screen.findByRole("button", { name: "settings.wallpaperSource.details.title: Saved artwork" });
    expect(wallpaperLibraryList).toHaveBeenCalledTimes(1);
  });
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
      groups.map((group) => group.getAttribute("data-source-group")),
    ).toEqual(["discovery", "create", "personal"]);
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
    expect(
      screen.queryByText("settings.wallpaperSource.xHint"),
    ).toBeNull();
    expect(
      screen.queryByText("settings.wallpaperSource.imagineHint"),
    ).toBeNull();
    expect(
      screen.queryByText("settings.wallpaperSource.libraryHint"),
    ).toBeNull();
    expect(
      screen.queryByText("settings.wallpaperSource.web.hint"),
    ).toBeNull();
    expect(
      screen.queryByText("settings.wallpaperSource.grokAlbum.hint"),
    ).toBeNull();
    expect(
      screen.queryByText("settings.wallpaperSource.footerHint"),
    ).toBeNull();

    const layout = view.container.querySelector(".wallpaper-source-layout");
    expect(layout?.firstElementChild).toBe(tablist);
    expect(layout?.lastElementChild).toBe(panel);
  });

  it("cancels outgoing requests and restores the provider's query, rows and scroll", async () => {
    remoteControllerState.searchItems = [galleryItem("saved", { source: "openverse" })];
    render(
      <WallpaperSourceModal
        open
        initialTab="openverse"
        t={t as never}
        onClose={vi.fn()}
        onPickFile={vi.fn()}
      />,
    );

    fireEvent.change(screen.getByPlaceholderText("settings.wallpaperSource.openverse.placeholder"), { target: { value: "misty coast" } });
    fireEvent.click(screen.getByRole("button", { name: "settings.wallpaperSource.search" }));
    await screen.findByRole("button", { name: /^settings\.wallpaperSource\.openPreview/ });
    const scroller = document.querySelector<HTMLDivElement>(".wallpaper-masonry")!.parentElement!;
    scroller.scrollTop = 420;
    fireEvent.click(
      screen.getByRole("tab", { name: "settings.wallpaperPexels" }),
    );

    expect(cancelRemote).toHaveBeenCalledTimes(1);
    expect(clearRemote).not.toHaveBeenCalled();
    expect(screen.queryByRole("button", { name: /^settings\.wallpaperSource\.openPreview/ })).toBeNull();
    fireEvent.click(screen.getByRole("tab", { name: "settings.wallpaperOpenverse" }));
    expect(screen.getByPlaceholderText<HTMLInputElement>("settings.wallpaperSource.openverse.placeholder").value).toBe("misty coast");
    expect(screen.getByRole("button", { name: /^settings\.wallpaperSource\.openPreview/ })).toBeTruthy();
    expect(searchRemote).toHaveBeenCalledTimes(1);
    expect(scroller.scrollTop).toBe(420);
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

    await waitFor(() => expect(wallpaperLibraryList).toHaveBeenCalled());
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

  it("ignores a late library list after reopening another source", async () => {
    let resolveLibrary!: (entries: Array<{
      path: string;
      name: string;
      source: string;
      kind: string;
      bytes: number;
      modifiedMs: number;
    }>) => void;
    wallpaperLibraryList.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          resolveLibrary = resolve;
        }),
    );
    const props = {
      t: t as never,
      onClose: vi.fn(),
      onPickFile: vi.fn(),
    };
    const view = render(
      <WallpaperSourceModal open initialTab="library" {...props} />,
    );

    await waitFor(() => expect(wallpaperLibraryList).toHaveBeenCalled());
    view.rerender(
      <WallpaperSourceModal open={false} initialTab="library" {...props} />,
    );
    view.rerender(<WallpaperSourceModal open initialTab="x" {...props} />);

    await act(async () => {
      resolveLibrary([
        {
          path: "C:\\wallpapers\\late-library.jpg",
          name: "late-library.jpg",
          source: "openverse",
          kind: "image",
          bytes: 1024,
          modifiedMs: 1,
        },
      ]);
    });

    expect(screen.getByRole("tabpanel").getAttribute("aria-labelledby")).toBe(
      "wallpaper-source-tab-x",
    );
    expect(
      screen.queryByRole("button", {
        name: /^settings\.wallpaperSource\.openPreview/,
      }),
    ).toBeNull();
  });

  it("restores album filters and scroll after revalidation, then clears them on a changed ready page", async () => {
    const albumItems = [galleryItem("still", { source: "grok_album", kind: "image" }), galleryItem("clip", { source: "grok_album", kind: "video" })];
    Object.assign(grokAlbumState, { status: "ready", hasSynced: true, items: albumItems, cachedCount: 2, visibleCount: 2 });
    const props = { open: true, initialTab: "grok_album" as const, t: t as never, onClose: vi.fn(), onPickFile: vi.fn() };
    const view = render(<WallpaperSourceModal {...props} />);
    fireEvent.click(screen.getByRole("button", { name: /^settings\.wallpaperSource\.kind\.video/ }));
    const scroller = view.container.querySelector<HTMLDivElement>(".wallpaper-masonry-scroll")!;
    scroller.scrollTop = 320;
    fireEvent.click(screen.getByRole("tab", { name: "settings.wallpaperFromX" }));
    Object.assign(grokAlbumState, { status: "loading", busy: true, hasSynced: false, items: [], cachedCount: 0, visibleCount: 0 });
    fireEvent.click(screen.getByRole("tab", { name: "settings.wallpaperGrokAlbum" }));
    scroller.scrollTop = 0;
    expect(screen.getByRole("button", { name: "settings.wallpaperSource.clearFilters" })).toBeTruthy();
    Object.assign(grokAlbumState, { status: "ready", busy: false, hasSynced: true, items: albumItems, cachedCount: 2, visibleCount: 2 });
    view.rerender(<WallpaperSourceModal {...props} />);
    await waitFor(() => expect(scroller.scrollTop).toBe(320));
    expect(screen.getAllByRole("button", { name: /^settings\.wallpaperSource\.openPreview/ })).toHaveLength(1);
    grokAlbumState.historyRevision += 1;
    view.rerender(<WallpaperSourceModal {...props} />);
    await waitFor(() => expect(screen.queryByRole("button", { name: "settings.wallpaperSource.clearFilters" })).toBeNull());
    expect(screen.getAllByRole("button", { name: /^settings\.wallpaperSource\.openPreview/ })).toHaveLength(2);
  });

  it("clears stale album filters when the official page leaves ready state", async () => {
    grokAlbumState.status = "ready";
    grokAlbumState.hasSynced = true;
    grokAlbumState.cachedCount = 2;
    grokAlbumState.visibleCount = 2;
    grokAlbumState.items = [
      galleryItem("album-image", { source: "grok_album", kind: "image" }),
      galleryItem("album-video", { source: "grok_album", kind: "video" }),
    ];
    const props = {
      open: true,
      initialTab: "grok_album" as const,
      t: t as never,
      onClose: vi.fn(),
      onPickFile: vi.fn(),
    };
    const view = render(<WallpaperSourceModal {...props} />);

    fireEvent.click(
      screen.getByRole("button", {
        name: /^settings\.wallpaperSource\.kind\.video/,
      }),
    );
    expect(
      screen.getByRole("button", {
        name: "settings.wallpaperSource.clearFilters",
      }),
    ).toBeTruthy();

    grokAlbumState.status = "verification";
    grokAlbumState.hasSynced = false;
    grokAlbumState.cachedCount = 0;
    grokAlbumState.visibleCount = 0;
    grokAlbumState.items = [];
    view.rerender(<WallpaperSourceModal {...props} />);

    await waitFor(() =>
      expect(
        screen.queryByRole("button", {
          name: "settings.wallpaperSource.clearFilters",
        }),
      ).toBeNull(),
    );
    expect(
      screen.queryByText("settings.wallpaperSource.empty.filterEmpty"),
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

  it("recovers Pexels search after replacing a rejected key", async () => {
    secretsGetMasked.mockResolvedValue({ hasPexelsKey: true });
    remoteControllerState.searchErrorCode = "pexels_key_invalid";
    render(
      <WallpaperSourceModal
        open
        initialTab="pexels"
        t={t as never}
        onClose={vi.fn()}
        onPickFile={vi.fn()}
      />,
    );

    expect(
      await screen.findByText("settings.wallpaperSource.pexels.keySaved"),
    ).toBeTruthy();
    fireEvent.change(
      screen.getByPlaceholderText("settings.wallpaperSource.pexels.placeholder"),
      { target: { value: "mountain lake" } },
    );
    fireEvent.click(
      screen.getByRole("button", { name: "settings.wallpaperSource.search" }),
    );

    expect(
      await screen.findByText("settings.wallpaperSource.pexels.keyInvalid"),
    ).toBeTruthy();
    fireEvent.change(
      await screen.findByLabelText(
        "settings.wallpaperSource.pexels.keyPlaceholder",
      ),
      { target: { value: "replacement-pexels-key" } },
    );
    fireEvent.click(
      screen.getByRole("button", {
        name: "settings.wallpaperSource.pexels.keySave",
      }),
    );

    await waitFor(() =>
      expect(secretsSet).toHaveBeenCalledWith({
        pexelsApiKey: "replacement-pexels-key",
      }),
    );
    expect(cancelRemote).toHaveBeenCalledTimes(1);
    expect(clearRemote).toHaveBeenCalledTimes(1);
    expect(
      await screen.findByText("settings.wallpaperSource.pexels.keySaved"),
    ).toBeTruthy();
    expect(
      screen.queryByText("settings.wallpaperSource.pexels.keyInvalid"),
    ).toBeNull();
    expect(
      screen.getByRole<HTMLButtonElement>("button", {
        name: "settings.wallpaperSource.search",
      }).disabled,
    ).toBe(false);

    remoteControllerState.searchErrorCode = null;
    fireEvent.click(
      screen.getByRole("button", { name: "settings.wallpaperSource.search" }),
    );
    await waitFor(() => expect(searchRemote).toHaveBeenCalledTimes(2));
    expect(
      screen.queryByText("settings.wallpaperSource.pexels.keyInvalid"),
    ).toBeNull();
  });

  it("confirms Pexels key removal and clears its active result state", async () => {
    secretsGetMasked.mockResolvedValue({ hasPexelsKey: true });
    remoteControllerState.searchItems = [
      galleryItem("pexels-existing", { source: "pexels" }),
    ];
    render(
      <WallpaperSourceModal
        open
        initialTab="pexels"
        t={t as never}
        onClose={vi.fn()}
        onPickFile={vi.fn()}
      />,
    );

    await screen.findByText("settings.wallpaperSource.pexels.keySaved");
    fireEvent.change(
      screen.getByPlaceholderText("settings.wallpaperSource.pexels.placeholder"),
      { target: { value: "mountain lake" } },
    );
    fireEvent.click(
      screen.getByRole("button", { name: "settings.wallpaperSource.search" }),
    );
    await screen.findByRole("button", {
      name: /^settings\.wallpaperSource\.openPreview/,
    });

    fireEvent.click(
      screen.getByRole("button", {
        name: "settings.wallpaperSource.pexels.keyDelete",
      }),
    );
    expect(
      screen.getByText("settings.wallpaperSource.pexels.keyDeleteBody"),
    ).toBeTruthy();
    fireEvent.click(
      screen.getByRole("button", {
        name: "settings.wallpaperSource.pexels.keyDeleteConfirm",
      }),
    );

    await waitFor(() =>
      expect(secretsSet).toHaveBeenCalledWith({ pexelsApiKey: "" }),
    );
    expect(cancelRemote).toHaveBeenCalledTimes(1);
    expect(clearRemote).toHaveBeenCalledTimes(1);
    expect(
      await screen.findByText("settings.wallpaperSource.pexels.keyRequired"),
    ).toBeTruthy();
    expect(
      screen.queryByRole("button", {
        name: /^settings\.wallpaperSource\.openPreview/,
      }),
    ).toBeNull();
    expect(
      screen.getByRole<HTMLButtonElement>("button", {
        name: "settings.wallpaperSource.search",
      }).disabled,
    ).toBe(true);
  });
});

describe("WallpaperSourceModal remote source wiring", () => {
  it("routes an image card into the Imagine video workflow", async () => {
    remoteControllerState.searchItems = [
      galleryItem("web-video-source", {
        source: "web",
        sourceUrl: "https://photos.example.test/page",
        sourceName: "photos.example.test",
      }),
    ];
    fetchRemoteWallpaperMedia.mockResolvedValue({
      path: "H:\\wallpapers\\web\\video-source.jpg",
      name: "video-source.jpg",
      mime: "image/jpeg",
      bytes: 128,
    });
    wallpaperImageToVideo.mockResolvedValue({
      items: [
        galleryItem("generated-video", {
          source: "imagine",
          kind: "video",
          localPath: "H:\\wallpapers\\imagine\\generated-video.mp4",
          fullUrl: "file:///H:/wallpapers/imagine/generated-video.mp4",
          thumbUrl: "file:///H:/wallpapers/imagine/generated-video.mp4",
        }),
      ],
    });
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
    fireEvent.click(
      await screen.findByRole("button", {
        name: "settings.wallpaperSource.generateVideoFromImage: photos.example.test",
      }),
    );

    await waitFor(() =>
      expect(screen.getByRole("tabpanel").getAttribute("aria-labelledby")).toBe(
        "wallpaper-source-tab-imagine",
      ),
    );
    await screen.findByText("photos.example.test");
    expect(fetchRemoteWallpaperMedia).toHaveBeenCalledWith(
      "web",
      "https://example.test/web-video-source.jpg",
      expect.stringMatching(/^[0-9a-f-]{36}$/),
    );

    expect(
      screen.getByRole<HTMLButtonElement>("button", {
        name: "settings.wallpaperSource.generateVideo",
      }).disabled,
    ).toBe(false);

    fireEvent.click(
      screen.getByRole("button", {
        name: "settings.wallpaperSource.generateVideo",
      }),
    );
    await waitFor(() =>
      expect(wallpaperImageToVideo).toHaveBeenCalledWith(
        "H:\\wallpapers\\web\\video-source.jpg",
        "settings.wallpaperSource.videoPromptDefault",
        6,
        "480p",
        expect.stringMatching(/^[0-9a-f-]{36}$/),
      ),
    );
    await waitFor(() =>
      expect(
        document.querySelector("video.wallpaper-masonry__img"),
      ).not.toBeNull(),
    );
    expect(
      screen.queryByRole("button", {
        name: /^settings\.wallpaperSource\.generateVideoFromImage:/,
      }),
    ).toBeNull();
  });

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

  it("does not apply a late remote file after the source modal closes", async () => {
    let resolveFetch:
      | ((value: {
          path: string;
          name: string;
          mime: string;
          bytes: number;
        }) => void)
      | null = null;
    fetchRemoteWallpaperMedia.mockImplementation(
      () =>
        new Promise((resolve) => {
          resolveFetch = resolve;
        }),
    );
    tauriInvoke.mockImplementation(async (command: string) => {
      if (command === "media_file_info") {
        return { bytes: 1, mime: "image/jpeg", name: "late.jpg" };
      }
      if (command === "media_read_file_chunk") {
        return Uint8Array.from([0xff]).buffer;
      }
      throw new Error(`unexpected command: ${command}`);
    });
    Object.defineProperty(window, "__TAURI_INTERNALS__", {
      configurable: true,
      value: {},
    });
    remoteControllerState.searchItems = [
      galleryItem("late-web", {
        source: "web",
        sourceUrl: "https://photos.example.test/page",
        sourceName: "photos.example.test",
      }),
    ];
    const onClose = vi.fn();
    const onPickFile = vi.fn(async () => undefined);
    render(
      <WallpaperSourceModal
        open
        initialTab="web"
        t={t as never}
        onClose={onClose}
        onPickFile={onPickFile}
      />,
    );

    fireEvent.change(
      screen.getByPlaceholderText("settings.wallpaperSource.web.placeholder"),
      { target: { value: "night skyline" } },
    );
    fireEvent.click(
      screen.getByRole("button", { name: "settings.wallpaperSource.search" }),
    );
    fireEvent.click(
      await screen.findByRole("button", {
        name: /^settings\.wallpaperSource\.openPreview/,
      }),
    );
    const apply = screen.getByRole("button", {
      name: "settings.wallpaperSource.apply",
    });
    await waitFor(() => expect((apply as HTMLButtonElement).disabled).toBe(false));
    fireEvent.click(apply);
    await waitFor(() => expect(fetchRemoteWallpaperMedia).toHaveBeenCalledTimes(1));
    fireEvent.click(screen.getByRole("button", { name: "modal-close" }));

    await act(async () => {
      resolveFetch?.({
        path: "H:\\wallpapers\\web\\late.jpg",
        name: "late.jpg",
        mime: "image/jpeg",
        bytes: 1,
      });
      await Promise.resolve();
    });
    expect(onClose).toHaveBeenCalledTimes(1);
    expect(tauriInvoke).not.toHaveBeenCalled();
    expect(onPickFile).not.toHaveBeenCalled();
  });

  it("opens an existing Web image while load more is running", async () => {
    remoteControllerState.searchItems = [
      galleryItem("web-existing", {
        source: "web",
        sourceUrl: "https://photos.example.test/page",
        sourceName: "photos.example.test",
      }),
    ];
    fetchRemoteWallpaperMedia.mockResolvedValue({
      path: "H:\\wallpapers\\web-existing.jpg",
      name: "web-existing.jpg",
      mime: "image/jpeg",
      bytes: 128,
    });
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
      name: /^settings\.wallpaperSource\.openPreview/,
    });
    remoteControllerState.busy = true;
    remoteControllerState.loadingMore = true;
    remoteControllerState.canLoadMore = true;
    view.rerender(<WallpaperSourceModal {...props} />);

    expect((card as HTMLButtonElement).disabled).toBe(false);
    fireEvent.click(card);
    await waitFor(() => expect(openViewer).toHaveBeenCalledTimes(1));
    expect(fetchRemoteWallpaperMedia).not.toHaveBeenCalled();
    const slide = openViewer.mock.calls[0]?.[0]?.[0] as {
      src: string;
      kind: string;
      loadOriginal?: () => Promise<{ src: string; mime?: string } | null>;
    };
    expect(slide).toMatchObject({
      src: "https://example.test/web-existing.jpg",
      kind: "image",
    });
    expect(slide.loadOriginal).toBeTypeOf("function");
    await act(async () => {
      await slide.loadOriginal?.();
    });
    expect(fetchRemoteWallpaperMedia).toHaveBeenCalledWith(
      "web",
      "https://example.test/web-existing.jpg",
      expect.stringMatching(/^[0-9a-f-]{36}$/),
    );
  });

  it("ignores a previous Viewer original failure after a same-source search", async () => {
    let rejectFetch: ((reason?: unknown) => void) | null = null;
    fetchRemoteWallpaperMedia.mockImplementation(
      () =>
        new Promise((_resolve, reject) => {
          rejectFetch = reject;
        }),
    );
    remoteControllerState.searchItems = [
      galleryItem("old-web", {
        source: "web",
        sourceName: "old.example.test",
      }),
    ];
    render(
      <WallpaperSourceModal
        open
        initialTab="web"
        t={t as never}
        onClose={vi.fn()}
        onPickFile={vi.fn()}
      />,
    );

    const query = screen.getByPlaceholderText(
      "settings.wallpaperSource.web.placeholder",
    );
    fireEvent.change(query, { target: { value: "old query" } });
    fireEvent.click(
      screen.getByRole("button", { name: "settings.wallpaperSource.search" }),
    );
    fireEvent.click(
      await screen.findByRole("button", {
        name: /settings\.wallpaperSource\.openPreview: old\.example\.test/,
      }),
    );
    const oldSlide = openViewer.mock.calls[0]?.[0]?.[0] as {
      loadOriginal?: () => Promise<unknown>;
    };
    let oldLoad: Promise<unknown> | undefined;
    await act(async () => {
      oldLoad = oldSlide.loadOriginal?.();
      await Promise.resolve();
    });

    remoteControllerState.searchItems = [
      galleryItem("new-web", {
        source: "web",
        sourceName: "new.example.test",
      }),
    ];
    fireEvent.change(query, { target: { value: "new query" } });
    fireEvent.click(
      screen.getByRole("button", { name: "settings.wallpaperSource.search" }),
    );
    await screen.findByRole("button", {
      name: /settings\.wallpaperSource\.openPreview: new\.example\.test/,
    });

    await act(async () => {
      rejectFetch?.(new Error("old original failed"));
      await oldLoad;
    });
    expect(screen.queryByRole("alert")).toBeNull();
  });
});
