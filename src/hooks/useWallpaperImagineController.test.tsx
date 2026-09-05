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

const translate = ((key: string, vars?: Record<string, unknown>) =>
  vars?.context ? `${key}:${vars.context}` : key) as never;

describe("useWallpaperImagineController", () => {
  it("prefills an editable prompt and refreshes it for each source", () => {
    const { result } = renderController();
    const first = imageItem("first-local", {
      source: "imagine",
      localPath: "C:\\wallpapers\\first.jpg",
      prompt: "Moonlit mountain lake",
    });
    const second = imageItem("second-local", {
      source: "library",
      localPath: "C:\\wallpapers\\second.jpg",
      textPreview: "second.jpg",
    });

    act(() => result.current.setPrompt("image generation prompt"));
    act(() => result.current.beginVideoFromItem(first));
    expect(result.current.prompt).toBe(
      "settings.wallpaperSource.videoPromptDefaultWithContext:Moonlit mountain lake",
    );

    act(() => result.current.setPrompt("user-edited camera orbit"));
    expect(result.current.prompt).toBe("user-edited camera orbit");
    act(() => result.current.beginVideoFromItem(second));
    expect(result.current.prompt).toBe(
      "settings.wallpaperSource.videoPromptDefault",
    );

    act(() => result.current.setMode("image"));
    expect(result.current.prompt).toBe("image generation prompt");
    act(() => result.current.setMode("video"));
    expect(result.current.prompt).toBe(
      "settings.wallpaperSource.videoPromptDefault",
    );
    expect(ensureLocalWallpaperMedia).not.toHaveBeenCalled();
  });

  it("clears the automatic prompt when its source is removed", () => {
    const { result } = renderController();

    act(() =>
      result.current.beginVideoFromItem(
        imageItem("clear-local", {
          source: "library",
          localPath: "C:\\wallpapers\\clear.jpg",
        }),
      ),
    );
    expect(result.current.prompt).toBe(
      "settings.wallpaperSource.videoPromptDefault",
    );

    act(() => result.current.clearVideoSource());
    expect(result.current.videoSource).toBeNull();
    expect(result.current.prompt).toBe("");
  });

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
    expect(cancelRemoteWallpaperMediaRequests).not.toHaveBeenCalled();

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

  it("ignores a late cancel error after leaving the Imagine source", async () => {
    const generation = deferred<WallpaperSearchResult>();
    const cancellation = deferred<boolean>();
    wallpaperImageToVideo.mockReturnValue(generation.promise);
    wallpaperImageToVideoCancel.mockReturnValue(cancellation.promise);
    const { result, setters } = renderController();

    act(() =>
      result.current.beginVideoFromItem(
        imageItem("local-switch", {
          source: "library",
          localPath: "C:\\wallpapers\\local-switch.jpg",
          fullUrl: "file:///wallpapers/local-switch.jpg",
        }),
      ),
    );
    act(() => {
      void result.current.generate();
    });
    await waitFor(() => expect(result.current.generating).toBe(true));

    let cancelAttempt: Promise<void> | null = null;
    act(() => {
      cancelAttempt = result.current.cancelGeneration();
    });
    await waitFor(() => expect(result.current.cancelling).toBe(true));
    act(() => result.current.cancelAll());
    expect(result.current.cancelling).toBe(false);
    setters.setError.mockClear();
    setters.setErrorCode.mockClear();

    await act(async () => {
      cancellation.reject(new Error("cancel transport failed"));
      await cancelAttempt;
    });

    expect(setters.setError).not.toHaveBeenCalled();
    expect(setters.setErrorCode).not.toHaveBeenCalled();
    await act(async () => generation.resolve(videoResult("late-after-switch")));
    expect(setters.setItems).not.toHaveBeenCalled();
  });

  it("cancels active generation without setting state after unmount", async () => {
    const generation = deferred<WallpaperSearchResult>();
    wallpaperImageToVideo.mockReturnValue(generation.promise);
    const { result, setters, unmount } = renderController();

    act(() =>
      result.current.beginVideoFromItem(
        imageItem("local-unmount", {
          source: "library",
          localPath: "C:\\wallpapers\\local-unmount.jpg",
          fullUrl: "file:///wallpapers/local-unmount.jpg",
        }),
      ),
    );
    act(() => {
      void result.current.generate();
    });
    await waitFor(() => expect(result.current.generating).toBe(true));
    const requestId = wallpaperImageToVideo.mock.calls[0]?.[4] as string;

    unmount();
    expect(wallpaperImageToVideoCancel).toHaveBeenCalledWith(requestId);
    await act(async () => generation.resolve(videoResult("late-unmounted")));
    expect(setters.setItems).not.toHaveBeenCalled();
  });

  it("cancels active source preparation on unmount", async () => {
    const preparation = deferred<{ path: string; mime: string }>();
    ensureLocalWallpaperMedia.mockReturnValue(preparation.promise);
    const { result, unmount } = renderController();

    act(() => result.current.beginVideoFromItem(imageItem("preparing")));
    await waitFor(() =>
      expect(ensureLocalWallpaperMedia).toHaveBeenCalledTimes(1),
    );
    unmount();

    expect(cancelRemoteWallpaperMediaRequests).toHaveBeenCalled();
    await act(async () =>
      preparation.resolve({
        path: "C:\\wallpapers\\preparing.jpg",
        mime: "image/jpeg",
      }),
    );
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
