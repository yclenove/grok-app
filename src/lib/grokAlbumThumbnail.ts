import * as api from "@/lib/api";
import { isGrokAlbumMediaUrl } from "@/lib/grokAlbum";
import { createWallpaperRequestId } from "@/lib/wallpaperRequest";

const MAX_CONCURRENT_THUMBS = 4;
const MAX_MEMORY_THUMBS = 80;
const MAX_DATA_URL_LENGTH = 768 * 1024;
const FAILURE_COOLDOWN_MS = 15_000;

type QueueJob = {
  revision: number;
  run: () => void;
  cancel: () => void;
};

type CacheListener = () => void;

const resolved = new Map<string, string>();
const pending = new Map<string, Promise<string | null>>();
const failedUntil = new Map<string, number>();
const queue: QueueJob[] = [];
const activeRequestIds = new Set<string>();
const cacheListeners = new Map<string, Set<CacheListener>>();
let activeJobs = 0;
let revision = 0;

function notifyCacheListeners(url: string): void {
  const listeners = cacheListeners.get(url);
  if (!listeners) return;
  for (const listener of Array.from(listeners)) listener();
}

function pumpQueue(): void {
  while (activeJobs < MAX_CONCURRENT_THUMBS && queue.length > 0) {
    const job = queue.shift();
    if (!job) return;
    if (job.revision !== revision) {
      job.cancel();
      continue;
    }
    activeJobs += 1;
    job.run();
  }
}

function enqueue<T>(
  requestRevision: number,
  run: () => Promise<T>,
): Promise<T | null> {
  return new Promise<T | null>((resolve, reject) => {
    let settled = false;
    const resolveOnce = (value: T | null) => {
      if (settled) return;
      settled = true;
      resolve(value);
    };
    queue.push({
      revision: requestRevision,
      cancel: () => resolveOnce(null),
      run: () => {
        void run()
          .then(resolveOnce, reject)
          .finally(() => {
            activeJobs = Math.max(0, activeJobs - 1);
            pumpQueue();
          });
      },
    });
    pumpQueue();
  });
}

function validThumbnailResult(input: {
  dataUrl?: string;
  width?: number;
  height?: number;
}): input is { dataUrl: string; width: number; height: number } {
  return (
    typeof input.dataUrl === "string" &&
    input.dataUrl.length > "data:image/jpeg;base64,".length &&
    input.dataUrl.length <= MAX_DATA_URL_LENGTH &&
    input.dataUrl.startsWith("data:image/jpeg;base64,") &&
    Number.isFinite(input.width) &&
    Number(input.width) > 0 &&
    Number.isFinite(input.height) &&
    Number(input.height) > 0
  );
}

function remember(url: string, dataUrl: string, notify = true): void {
  const changed = resolved.get(url) !== dataUrl;
  resolved.delete(url);
  resolved.set(url, dataUrl);
  while (resolved.size > MAX_MEMORY_THUMBS) {
    const oldest = resolved.keys().next().value as string | undefined;
    if (!oldest) break;
    resolved.delete(oldest);
  }
  if (!notify) return;
  if (changed) notifyCacheListeners(url);
}

export function peekGrokAlbumThumbnail(url: string): string | null {
  const hit = resolved.get(url);
  if (!hit) return null;
  remember(url, hit, false);
  return hit;
}

export function subscribeGrokAlbumThumbnail(
  url: string,
  listener: CacheListener,
): () => void {
  const normalized = url.trim();
  let listeners = cacheListeners.get(normalized);
  if (!listeners) {
    listeners = new Set();
    cacheListeners.set(normalized, listeners);
  }
  listeners.add(listener);
  return () => {
    listeners?.delete(listener);
    if (listeners?.size === 0) cacheListeners.delete(normalized);
  };
}

export function resolveGrokAlbumThumbnail(
  url: string,
): Promise<string | null> {
  const normalized = url.trim();
  if (!isGrokAlbumMediaUrl(normalized)) return Promise.resolve(null);

  const hit = peekGrokAlbumThumbnail(normalized);
  if (hit) return Promise.resolve(hit);
  const inFlight = pending.get(normalized);
  if (inFlight) return inFlight;
  if ((failedUntil.get(normalized) || 0) > Date.now()) {
    return Promise.resolve(null);
  }

  const requestRevision = revision;
  const requestId = createWallpaperRequestId();
  const request = enqueue(requestRevision, async () => {
    if (requestRevision !== revision) return null;
    activeRequestIds.add(requestId);
    try {
      const result = await api.wallpaperGrokAlbumThumbnail(normalized, requestId);
      if (requestRevision !== revision || !validThumbnailResult(result)) {
        return null;
      }
      remember(normalized, result.dataUrl);
      failedUntil.delete(normalized);
      return result.dataUrl;
    } catch {
      if (requestRevision === revision) {
        failedUntil.set(normalized, Date.now() + FAILURE_COOLDOWN_MS);
      }
      return null;
    } finally {
      activeRequestIds.delete(requestId);
    }
  }).finally(() => {
    if (pending.get(normalized) === request) pending.delete(normalized);
  });
  pending.set(normalized, request);
  return request;
}

export async function warmGrokAlbumThumbnails(
  urls: readonly string[],
): Promise<void> {
  const unique = Array.from(new Set(urls.map((url) => url.trim()).filter(Boolean))).slice(
    0,
    40,
  );
  await Promise.all(unique.map((url) => resolveGrokAlbumThumbnail(url)));
}

export function forgetGrokAlbumThumbnail(url: string): void {
  const normalized = url.trim();
  resolved.delete(normalized);
  failedUntil.set(normalized, Date.now() + FAILURE_COOLDOWN_MS);
  notifyCacheListeners(normalized);
}

export function clearGrokAlbumThumbnailCache(): void {
  revision += 1;
  const subscribedUrls = Array.from(cacheListeners.keys());
  const requestIds = Array.from(activeRequestIds);
  if (requestIds.length > 0) {
    void api.wallpaperGrokAlbumCancelRequests(requestIds).catch(() => {
      // The revision guard still rejects late results on older Hosts.
    });
  }
  const stale = queue.splice(0);
  for (const job of stale) job.cancel();
  resolved.clear();
  pending.clear();
  failedUntil.clear();
  for (const url of subscribedUrls) notifyCacheListeners(url);
}

/** Test helper. */
export function grokAlbumThumbnailQueueState(): {
  active: number;
  queued: number;
  cached: number;
} {
  return { active: activeJobs, queued: queue.length, cached: resolved.size };
}
