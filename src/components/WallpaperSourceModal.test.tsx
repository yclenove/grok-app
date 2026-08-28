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
    cancel: cancelSearch,
  }),
}));

vi.mock("@/lib/api", () => ({
  isDesktopHost: () => true,
  wallpaperFetchMedia: vi.fn(),
  wallpaperImagine: vi.fn(),
  wallpaperLibraryList: vi.fn(),
  wallpaperLibraryDelete: vi.fn(),
  openExternalUrl: vi.fn(),
}));

vi.mock("@/components/ImageViewer", () => ({
  useImageViewerOptional: () => ({ open: vi.fn() }),
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
});
