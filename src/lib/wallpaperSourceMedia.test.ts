import { beforeEach, describe, expect, it, vi } from "vitest";

const fetchAlbumMedia = vi.hoisted(() => vi.fn());
const cancelAlbumRequests = vi.hoisted(() => vi.fn());
const cancelAllAlbumRequests = vi.hoisted(() => vi.fn());
const fetchMedia = vi.hoisted(() => vi.fn());
const rememberMedia = vi.hoisted(() => vi.fn());
const fetchRemoteMedia = vi.hoisted(() => vi.fn());
const cancelRemoteRequests = vi.hoisted(() => vi.fn());
const cancelAllRemoteRequests = vi.hoisted(() => vi.fn());

vi.mock("@/lib/api", () => ({
  wallpaperGrokAlbumFetchMedia: fetchAlbumMedia,
  wallpaperGrokAlbumCancelRequests: cancelAlbumRequests,
  wallpaperGrokAlbumCancelAllRequests: cancelAllAlbumRequests,
  wallpaperRemoteFetchMedia: fetchRemoteMedia,
  wallpaperRemoteCancelMediaRequests: cancelRemoteRequests,
  wallpaperRemoteCancelAllMediaRequests: cancelAllRemoteRequests,
  wallpaperFetchMedia: fetchMedia,
  wallpaperLibraryRemember: rememberMedia,
}));

import {
  cancelGrokAlbumMediaRequests,
  cancelRemoteWallpaperMediaRequests,
  EMPTY_WALLPAPER_IMAGE_PLACEHOLDER,
  ensureLocalWallpaperMedia,
} from "./wallpaperSourceMedia";

const ALBUM_URL =
  "https://assets.grok.com/users/test/generated/example/image.jpg";

beforeEach(() => {
  fetchAlbumMedia.mockReset();
  cancelAlbumRequests.mockReset();
  cancelAlbumRequests.mockResolvedValue(0);
  cancelAllAlbumRequests.mockReset();
  cancelAllAlbumRequests.mockResolvedValue(0);
  fetchMedia.mockReset();
  rememberMedia.mockReset();
  fetchRemoteMedia.mockReset();
  cancelRemoteRequests.mockReset();
  cancelRemoteRequests.mockResolvedValue(0);
  cancelAllRemoteRequests.mockReset();
  cancelAllRemoteRequests.mockResolvedValue(0);
});

describe("wallpaper source media lifecycle", () => {
  it("persists source and license on original download without changing favorite state", async () => {
    const item = { id: "licensed", source: "openverse", kind: "image", thumbUrl: "https://cdn.example.test/photo.jpg", fullUrl: "https://cdn.example.test/photo.jpg", sourceUrl: "https://example.test/photo", license: "CC BY 4.0", licenseUrl: "https://creativecommons.org/licenses/by/4.0/" };
    fetchRemoteMedia.mockResolvedValue({ path: "H:\\wallpapers\\photo.jpg", name: "photo.jpg", mime: "image/jpeg" });
    const metadata = { id: "media-1", favorite: true, width: 1920, height: 1080 };
    rememberMedia.mockResolvedValue(metadata);
    const result = await ensureLocalWallpaperMedia(item);
    expect(rememberMedia).toHaveBeenCalledWith("H:\\wallpapers\\photo.jpg", item);
    expect(result.metadata).toBe(metadata);
  });

  it("reports a catalog save failure without starting another original download", async () => {
    fetchMedia.mockResolvedValue({ path: "H:\\wallpapers\\photo.jpg", name: "photo.jpg", mime: "image/jpeg" });
    rememberMedia.mockRejectedValue(new Error("catalog_write_failed"));
    await expect(ensureLocalWallpaperMedia({ id: "x", source: "x", kind: "image", thumbUrl: "https://pbs.twimg.com/media/photo.jpg", fullUrl: "https://pbs.twimg.com/media/photo.jpg" })).rejects.toThrow("catalog_write_failed");
    expect(fetchMedia).toHaveBeenCalledTimes(1);
  });

  it("uses a complete GIF for the immediate lazy-media placeholder", () => {
    const encoded = EMPTY_WALLPAPER_IMAGE_PLACEHOLDER.split(",", 2)[1] ?? "";
    const bytes = Uint8Array.from(atob(encoded), (char) => char.charCodeAt(0));

    expect(new TextDecoder("ascii").decode(bytes.slice(0, 6))).toBe("GIF89a");
    expect(bytes.length).toBeGreaterThan(20);
    expect(bytes.at(-1)).toBe(0x3b);
  });

  it("coalesces simultaneous requests for the same Grok album original", async () => {
    let release!: (value: { path: string; name: string; mime: string }) => void;
    fetchAlbumMedia.mockImplementation(
      () =>
        new Promise((resolve) => {
          release = resolve;
        }),
    );
    const item = {
      id: "album-image",
      thumbUrl: ALBUM_URL,
      fullUrl: ALBUM_URL,
      kind: "image",
      source: "grok_album",
    };

    const preview = ensureLocalWallpaperMedia(item);
    const apply = ensureLocalWallpaperMedia(item);
    await vi.waitFor(() => expect(fetchAlbumMedia).toHaveBeenCalledTimes(1));

    release({
      path: "H:\\wallpapers\\album.jpg",
      name: "album.jpg",
      mime: "image/jpeg",
    });
    await expect(Promise.all([preview, apply])).resolves.toEqual([
      {
        path: "H:\\wallpapers\\album.jpg",
        name: "album.jpg",
        mime: "image/jpeg",
      },
      {
        path: "H:\\wallpapers\\album.jpg",
        name: "album.jpg",
        mime: "image/jpeg",
      },
    ]);
  });

  it("cancels an active Grok album original with the same request id", async () => {
    let release!: (value: { path: string; name: string; mime: string }) => void;
    fetchAlbumMedia.mockImplementation(
      () =>
        new Promise((resolve) => {
          release = resolve;
        }),
    );

    const pending = ensureLocalWallpaperMedia({
      id: "album-image",
      thumbUrl: ALBUM_URL,
      fullUrl: ALBUM_URL,
      kind: "image",
      source: "grok_album",
    });
    await vi.waitFor(() => expect(fetchAlbumMedia).toHaveBeenCalledTimes(1));
    const requestId = fetchAlbumMedia.mock.calls[0]?.[1] as string;

    cancelGrokAlbumMediaRequests();
    expect(cancelAlbumRequests).toHaveBeenCalledWith([requestId]);
    expect(cancelAllAlbumRequests).toHaveBeenCalledTimes(1);
    expect(requestId).toMatch(/^[0-9a-f-]{36}$/);

    release({
      path: "H:\\wallpapers\\album.jpg",
      name: "album.jpg",
      mime: "image/jpeg",
    });
    await expect(pending).resolves.toEqual({
      path: "H:\\wallpapers\\album.jpg",
      name: "album.jpg",
      mime: "image/jpeg",
    });
  });

  it("cancels Host-side album work when local request bookkeeping is empty", () => {
    cancelGrokAlbumMediaRequests();

    expect(cancelAlbumRequests).not.toHaveBeenCalled();
    expect(cancelAllAlbumRequests).toHaveBeenCalledTimes(1);
  });

  it("keeps non-album downloads on the existing media route", async () => {
    fetchMedia.mockResolvedValue({
      path: "H:\\wallpapers\\x.jpg",
      name: "x.jpg",
      mime: "image/jpeg",
    });

    await ensureLocalWallpaperMedia({
      id: "x-image",
      thumbUrl: "https://pbs.twimg.com/media/example.jpg",
      fullUrl: "https://pbs.twimg.com/media/example.jpg",
      kind: "image",
      source: "x",
    });

    expect(fetchMedia).toHaveBeenCalledWith(
      "https://pbs.twimg.com/media/example.jpg",
      "x",
    );
    expect(cancelAlbumRequests).not.toHaveBeenCalled();
    expect(cancelAllAlbumRequests).not.toHaveBeenCalled();
  });

  it("keeps remote download headers behind the Host boundary", async () => {
    fetchRemoteMedia.mockResolvedValue({
      path: "H:\\wallpapers\\web.jpg",
      name: "web.jpg",
      mime: "image/jpeg",
    });

    await ensureLocalWallpaperMedia({
      id: "web-image",
      thumbUrl: "https://cdn.example.test/photo.jpg",
      fullUrl: "https://cdn.example.test/photo.jpg",
      kind: "image",
      source: "web",
      sourceUrl: "https://photos.example.test/item",
    });

    expect(fetchRemoteMedia).toHaveBeenCalledWith(
      "web",
      "https://cdn.example.test/photo.jpg",
      expect.stringMatching(/^[0-9a-f-]{36}$/),
    );
  });

  it("cancels both originals and thumbnail fallbacks on source exit", () => {
    cancelRemoteWallpaperMediaRequests();

    expect(cancelRemoteRequests).not.toHaveBeenCalled();
    expect(cancelAllRemoteRequests).toHaveBeenCalledTimes(1);
  });
});
