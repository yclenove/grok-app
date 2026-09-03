/**
 * @vitest-environment jsdom
 */
import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type {
  WallpaperGalleryItem,
  WallpaperSearchResult,
} from "@/lib/wallpaperSource";
import { useWallpaperImagineController } from "./useWallpaperImagineController";

const wallpaperImagine = vi.hoisted(() => vi.fn());
const wallpaperImageToVideo = vi.hoisted(() => vi.fn());
const wallpaperImageToVideoCancel = vi.hoisted(() => vi.fn(async () => true));
const ensureLocalWallpaperMedia = vi.hoisted(() => vi.fn());
const cancelRemoteWallpaperMediaRequests = vi.hoisted(() =>
  vi.fn(async () => undefined),
);
const cancelGrokAlbumMediaRequests = vi.hoisted(() => vi.fn());

vi.mock("@/lib/api", () => ({
  isDesktopHost: () => true,
  wallpaperImagine,
  wallpaperImageToVideo,
  wallpaperImageToVideoCancel,
}));

vi.mock("@/lib/wallpaperSourceMedia", () => ({
  ensureLocalWallpaperMedia,
  cancelRemoteWallpaperMediaRequests,
  cancelGrokAlbumMediaRequests,
}));

vi.mock("@/lib/grokAlbumThumbnail", () => ({
  peekGrokAlbumThumbnail: () => null,
}));

vi.mock("@/lib/remoteWallpaperThumbnail", () => ({
  peekRemoteWallpaperThumbnail: () => "data:image/jpeg;base64,preview",
}));

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

function imageItem(
  id: string,
  extra: Partial<WallpaperGalleryItem> = {},
): WallpaperGalleryItem {
  return {
    id,
    thumbUrl: `https://images.example.test/${id}.jpg`,
    fullUrl: `https://images.example.test/${id}.jpg`,
    kind: "image",
    source: "web",
    ...extra,
  };
}

function videoResult(id = "generated-video"): WallpaperSearchResult {
  return {
    items: [
      {
        id,
        thumbUrl: `file:///wallpapers/${id}.mp4`,
        fullUrl: `file:///wallpapers/${id}.mp4`,
        kind: "video",
        source: "imagine",
        localPath: `C:\\wallpapers\\${id}.mp4`,
      },
    ],
  };
}

function renderController() {
  const setters = {
    setItems: vi.fn(),
    setHasSearched: vi.fn(),
    setSelectedId: vi.fn(),
    setError: vi.fn(),
    setErrorCode: vi.fn(),
    setStatusHint: vi.fn(),
    setGalleryFilter: vi.fn(),
    setKindFilter: vi.fn(),
  };
  const hook = renderHook(() =>
    useWallpaperImagineController({
      open: true,
      enabled: true,
      t: translate,
      ...setters,
    }),
  );
  return { ...hook, setters };
}

const translate = ((key: string) => key) as never;

describe("useWallpaperImagineController", () => {
  it("materializes a remote source before starting real video generation", async () => {
    ensureLocalWallpaperMedia.mockResolvedValue({
      path: "C:\\wallpapers\\web\\source.jpg",
      name: "source.jpg",
      mime: "image/jpeg",
    });
    wallpaperImageToVideo.mockResolvedValue(videoResult());
    const { result, setters } = renderController();
    const source = imageItem("source");

    act(() => result.current.beginVideoFromItem(source));
    await waitFor(() => expect(result.current.videoSourceStatus).toBe("ready"));
    expect(ensureLocalWallpaperMedia).toHaveBeenCalledWith(source);
    expect(result.current.videoSourcePreview).toBe(
      "data:image/jpeg;base64,preview",
    );

    act(() => {
      result.current.setPrompt("slow camera push");
      result.current.setVideoDuration(10);
      result.current.setVideoResolution("720p");
    });
    await act(async () => result.current.generate());

    expect(wallpaperImageToVideo).toHaveBeenCalledWith(
      "C:\\wallpapers\\web\\source.jpg",
      "slow camera push",
      10,
      "720p",
      expect.stringMatching(/^[0-9a-f-]{36}$/),
    );
    expect(setters.setItems).toHaveBeenCalledWith(videoResult().items);
    expect(setters.setSelectedId).toHaveBeenLastCalledWith("generated-video");
  });

  it("cancels an active video request and ignores its late result", async () => {
    const generation = deferred<WallpaperSearchResult>();
    wallpaperImageToVideo.mockReturnValue(generation.promise);
    const { result, setters } = renderController();

    act(() =>
      result.current.beginVideoFromItem(
        imageItem("local", {
          source: "library",
          localPath: "C:\\wallpapers\\local.jpg",
          fullUrl: "file:///wallpapers/local.jpg",
        }),
      ),
    );
    act(() => {
      void result.current.generate();
    });
    await waitFor(() => expect(result.current.generating).toBe(true));
    const requestId = wallpaperImageToVideo.mock.calls[0]?.[4] as string;

    await act(async () => result.current.cancelGeneration());
    expect(wallpaperImageToVideoCancel).toHaveBeenCalledWith(requestId);
    expect(result.current.cancelling).toBe(true);

    await act(async () => generation.resolve(videoResult("late-video")));
    await waitFor(() => expect(result.current.generating).toBe(false));
    expect(setters.setItems).not.toHaveBeenCalled();
    expect(setters.setError).toHaveBeenLastCalledWith(null);
  });

  it("does not let an older source download replace a newer selection", async () => {
    const first = deferred<{ path: string; mime: string }>();
    const second = deferred<{ path: string; mime: string }>();
    ensureLocalWallpaperMedia
      .mockReturnValueOnce(first.promise)
      .mockReturnValueOnce(second.promise);
    const { result } = renderController();

    act(() => result.current.beginVideoFromItem(imageItem("first")));
    await waitFor(() => expect(ensureLocalWallpaperMedia).toHaveBeenCalledTimes(1));
    act(() => result.current.beginVideoFromItem(imageItem("second")));
    await waitFor(() => expect(ensureLocalWallpaperMedia).toHaveBeenCalledTimes(2));

    await act(async () =>
      second.resolve({ path: "C:\\wallpapers\\second.jpg", mime: "image/jpeg" }),
    );
    await waitFor(() =>
      expect(result.current.videoSourcePath).toBe(
        "C:\\wallpapers\\second.jpg",
      ),
    );
    await act(async () =>
      first.resolve({ path: "C:\\wallpapers\\first.jpg", mime: "image/jpeg" }),
    );
    expect(result.current.videoSourcePath).toBe(
      "C:\\wallpapers\\second.jpg",
    );
  });
});
