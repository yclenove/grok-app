import * as api from "@/lib/api";
import {
  resolveApplySource,
  type WallpaperFetchResult,
  type WallpaperGalleryItem,
} from "@/lib/wallpaperSource";
import { createWallpaperRequestId } from "@/lib/wallpaperRequest";
import {
  isWallpaperRemoteSource,
  type WallpaperRemoteSource,
} from "@/lib/wallpaperRemoteSearch";
import { clearRemoteWallpaperThumbnailCache } from "@/lib/remoteWallpaperThumbnail";

export const EMPTY_WALLPAPER_IMAGE_PLACEHOLDER =
  "data:image/gif;base64,R0lGODlhAQABAIAAAAAAAP///ywAAAAAAQABAAACAUwAOw==";

const activeAlbumMediaRequests = new Set<string>();
const pendingAlbumMedia = new Map<string, Promise<WallpaperFetchResult>>();
const activeRemoteMediaRequests = new Set<string>();
const pendingRemoteMedia = new Map<string, Promise<WallpaperFetchResult>>();

export function cancelGrokAlbumMediaRequests(): void {
  const requestIds = Array.from(activeAlbumMediaRequests);
  activeAlbumMediaRequests.clear();
  pendingAlbumMedia.clear();
  const targeted =
    requestIds.length > 0
      ? api.wallpaperGrokAlbumCancelRequests(requestIds)
      : Promise.resolve(0);
  // Host-side cancellation is the lifecycle authority. The targeted request
  // ids retain pre-cancel semantics for IPC reordering, while cancel-all also
  // catches an active Viewer request if frontend module state was replaced.
  void Promise.allSettled([
    targeted,
    api.wallpaperGrokAlbumCancelAllRequests(),
  ]);
}

export function cancelRemoteWallpaperMediaRequests(): void {
  const requestIds = Array.from(activeRemoteMediaRequests);
  activeRemoteMediaRequests.clear();
  pendingRemoteMedia.clear();
  clearRemoteWallpaperThumbnailCache();
  const targeted =
    requestIds.length > 0
      ? api.wallpaperRemoteCancelMediaRequests(requestIds)
      : Promise.resolve(0);
  void Promise.allSettled([
    targeted,
    api.wallpaperRemoteCancelAllMediaRequests(),
  ]);
}

/**
 * Resolve a local path for media preview or wallpaper application. Remote
 * originals enter the source-specific, validated wallpaper library first.
 */
export async function ensureLocalWallpaperMedia(
  item: WallpaperGalleryItem,
): Promise<{ path: string; name?: string; mime?: string }> {
  const source = resolveApplySource(item);
  if (source.kind === "path") return { path: source.path };

  let fetched;
  if (item.source === "grok_album") {
    const key = source.url.trim();
    let request = pendingAlbumMedia.get(key);
    if (!request) {
      const requestId = createWallpaperRequestId();
      activeAlbumMediaRequests.add(requestId);
      request = api.wallpaperGrokAlbumFetchMedia(key, requestId).finally(() => {
        activeAlbumMediaRequests.delete(requestId);
        if (pendingAlbumMedia.get(key) === request) {
          pendingAlbumMedia.delete(key);
        }
      });
      pendingAlbumMedia.set(key, request);
    }
    fetched = await request;
  } else if (isWallpaperRemoteSource(item.source)) {
    const remoteSource: WallpaperRemoteSource = item.source;
    const key = `${remoteSource}:${source.url.trim()}`;
    let request = pendingRemoteMedia.get(key);
    if (!request) {
      const requestId = createWallpaperRequestId();
      activeRemoteMediaRequests.add(requestId);
      request = api
        .wallpaperRemoteFetchMedia(remoteSource, source.url, requestId)
        .finally(() => {
          activeRemoteMediaRequests.delete(requestId);
          if (pendingRemoteMedia.get(key) === request) {
            pendingRemoteMedia.delete(key);
          }
        });
      pendingRemoteMedia.set(key, request);
    }
    fetched = await request;
  } else {
    fetched = await api.wallpaperFetchMedia(
      source.url,
      item.source === "imagine" ? "imagine" : "x",
    );
  }
  return { path: fetched.path, name: fetched.name, mime: fetched.mime };
}
