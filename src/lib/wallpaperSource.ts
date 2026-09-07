/**
 * Wallpaper source helpers — X search + Imagine gallery types and pure logic.
 */

import type { WallpaperXSearchMeta } from "./wallpaperXSearch";

export type WallpaperSourceKind =
  | "x"
  | "imagine"
  | "grok_album"
  | "web"
  | "openverse"
  | "pexels"
  | "library";

export type WallpaperGalleryItem = {
  metadata?: WallpaperMediaRecord | null;
  id: string;
  thumbUrl: string;
  fullUrl: string;
  kind: "image" | "video" | string;
  width?: number | null;
  height?: number | null;
  source: WallpaperSourceKind | string;
  username?: string | null;
  postUrl?: string | null;
  textPreview?: string | null;
  likes?: number | null;
  localPath?: string | null;
  prompt?: string | null;
  sourceUrl?: string | null;
  sourceName?: string | null;
  authorName?: string | null;
  authorUrl?: string | null;
  license?: string | null;
  licenseUrl?: string | null;
};

export type WallpaperSearchResult = {
  items: WallpaperGalleryItem[];
  errorCode?: string | null;
  message?: string | null;
  meta?: WallpaperXSearchMeta | null;
};

export type WallpaperFetchResult = {
  path: string;
  mime: string;
  bytes: number;
  name: string;
};

export type WallpaperLibraryEntry = {
  metadata?: WallpaperMediaRecord | null;
  path: string;
  name: string;
  source: string;
  kind: string;
  bytes: number;
  modifiedMs: number;
};

export type WallpaperMediaRecord = {
  id: string;
  source: string;
  sourceUrl: string | null;
  sourceName?: string | null;
  authorName?: string | null;
  authorUrl?: string | null;
  license: string | null;
  licenseUrl: string | null;
  title: string | null;
  width: number | null;
  height: number | null;
  prompt: string | null;
  generation: { operation: string; aspectRatio: string | null; resolution: string | null; duration: number | null; requestedModel: string | null } | null;
  parentId: string | null;
  favorite: boolean;
  purpose: "cache" | "generated";
  bytes: number;
  modifiedMs: number;
};

export type WallpaperLibraryPurpose = "all" | "favorites" | "generated" | "cache";

export type WallpaperSourceErrorCode =
  | "catalog_write_failed"
  | "auth_required"
  | "cli_missing"
  | "pexels_key_required"
  | "pexels_key_invalid"
  | "search_failed"
  | "service_unavailable"
  | "empty"
  | "download_failed"
  | "url_blocked"
  | "imagine_source_invalid"
  | "imagine_failed"
  | "imagine_access_denied"
  | "imagine_rate_limited"
  | "imagine_request_rejected"
  | "imagine_upstream_failed"
  | "imagine_network_failed"
  | "imagine_result_invalid"
  | "rate_limited"
  | "timeout"
  | "generic";

const imagineErrorCodes = new Set<WallpaperSourceErrorCode>([
  "imagine_access_denied", "imagine_rate_limited", "imagine_request_rejected",
  "imagine_upstream_failed", "imagine_network_failed", "imagine_result_invalid",
]);

function imagineErrorCode(raw: string): WallpaperSourceErrorCode | null {
  const code = raw.trim() as WallpaperSourceErrorCode;
  return imagineErrorCodes.has(code) ? code : null;
}

function hostSearchErrorCode(raw: string): WallpaperSourceErrorCode | null {
  const code = raw.toLowerCase();
  if (
    code.includes("oauth_unavailable") ||
    code.includes("oauth_expired") ||
    code.includes("responses_unauthorized")
  ) {
    return "auth_required";
  }
  if (
    code.includes("responses_rate_limited") ||
    code.includes("provider_rate_limited")
  ) {
    return "rate_limited";
  }
  if (
    code.includes("responses_timeout") ||
    code.includes("provider_timeout")
  ) {
    return "timeout";
  }
  if (
    code.includes("responses_network") ||
    code.includes("responses_tls") ||
    code.includes("provider_network")
  ) {
    return "search_failed";
  }
  if (
    code.includes("responses_server_error") ||
    code.includes("responses_bad_request") ||
    code.includes("responses_invalid_json") ||
    code.includes("responses_protocol") ||
    code.includes("responses_tool_not_called") ||
    code.includes("responses_search_budget_exceeded") ||
    code.includes("responses_tool_budget_exceeded") ||
    code.includes("responses_circuit_open") ||
    code.includes("provider_service_unavailable") ||
    code.includes("provider_protocol") ||
    code.includes("load_more_unavailable")
  ) {
    return "service_unavailable";
  }
  if (code === "empty" || code.includes("responses_empty")) return "empty";
  return null;
}

/** Map host error strings / codes to a stable UI code. */
export function parseWallpaperSourceError(err: unknown): WallpaperSourceErrorCode {
  const raw =
    typeof err === "string"
      ? err
      : err instanceof Error
        ? err.message
        : err && typeof err === "object" && "message" in err
          ? String((err as { message: unknown }).message)
          : "";
  const s = raw.toLowerCase();
  const imagineError = imagineErrorCode(s);
  if (imagineError) return imagineError;
  if (s.includes("catalog_")) return "catalog_write_failed";
  if (s.includes("auth_required")) return "auth_required";
  if (s.includes("cli_missing")) return "cli_missing";
  if (s.includes("pexels_key_missing")) return "pexels_key_required";
  if (s.includes("pexels_key_invalid")) return "pexels_key_invalid";
  if (
    s.includes("url_blocked") ||
    s.includes("path_not_allowed") ||
    s.includes("path_denied")
  ) {
    return "url_blocked";
  }
  // Local media:// / path reads used when applying Imagine / library items
  if (
    s.includes("download_failed") ||
    s.includes("read_failed") ||
    s.includes("short read") ||
    (s.includes("download") && !s.includes("delete"))
  ) {
    return "download_failed";
  }
  if (s.includes("desktop_only")) return "generic";
  const searchError = hostSearchErrorCode(s);
  if (searchError) return searchError;
  if (s.includes("service_unavailable")) return "service_unavailable";
  if (s.includes("imagine_source_invalid")) return "imagine_source_invalid";
  // timeout before imagine so "imagine timeout" is not swallowed as imagine_failed
  if (s.includes("timeout") || s.includes("timed out")) return "timeout";
  if (s.includes("imagine_failed")) return "imagine_failed";
  // wallpaper_imagine command path failures (not every string with "imagine")
  if (s.includes("wallpaper_imagine") || /\bimagine\b/.test(s)) {
    return "imagine_failed";
  }
  if (s.includes("empty")) return "empty";
  if (s.includes("search_failed") || s.includes("search")) return "search_failed";
  // delete soft-fail stays generic — UI shows warn without inventing success
  if (s.includes("delete_failed")) return "generic";
  return "generic";
}

export function errorCodeFromSearchResult(
  result: WallpaperSearchResult,
): WallpaperSourceErrorCode | null {
  if (result.items.length > 0) return null;
  const code = (result.errorCode || "").toLowerCase();
  if (!code) return "empty";
  const imagineError = imagineErrorCode(code);
  if (imagineError) return imagineError;
  if (code.startsWith("catalog_")) return "catalog_write_failed";
  if (code === "auth_required") return "auth_required";
  if (code === "cli_missing") return "cli_missing";
  if (code === "pexels_key_missing") return "pexels_key_required";
  if (code === "pexels_key_invalid") return "pexels_key_invalid";
  if (code === "search_failed") return "search_failed";
  if (code === "service_unavailable") return "service_unavailable";
  if (code === "imagine_source_invalid") return "imagine_source_invalid";
  if (code === "imagine_failed") return "imagine_failed";
  const searchError = hostSearchErrorCode(code);
  if (searchError) return searchError;
  if (code === "timeout") return "timeout";
  return "generic";
}

function galleryItemAliases(item: WallpaperGalleryItem): string[] {
  return [item.localPath, item.fullUrl, item.id]
    .map((value) => value?.trim() || "")
    .filter(Boolean);
}

/** Deduplicate gallery items by every stable path, URL, and id alias. */
export function dedupeGalleryItems(
  items: WallpaperGalleryItem[],
): WallpaperGalleryItem[] {
  const seen = new Set<string>();
  const out: WallpaperGalleryItem[] = [];
  for (const it of items) {
    const aliases = galleryItemAliases(it);
    if (aliases.length === 0) continue;
    const duplicate = aliases.some((alias) => seen.has(alias));
    // Register aliases from duplicate bridge rows too. A later remote row may
    // only share the URL of an item already matched through its local path.
    aliases.forEach((alias) => seen.add(alias));
    if (duplicate) continue;
    out.push(it);
  }
  return out;
}

/**
 * Reconcile an authoritative page with cards already visible in the gallery.
 * Incoming metadata wins, while a local file materialized during the request
 * remains attached to the matching remote item.
 */
export function mergeAuthoritativeGalleryItems(
  current: readonly WallpaperGalleryItem[],
  incoming: readonly WallpaperGalleryItem[],
): WallpaperGalleryItem[] {
  const localPathByAlias = new Map<string, string>();
  for (const item of current) {
    const localPath = item.localPath?.trim();
    if (!localPath) continue;
    for (const alias of galleryItemAliases(item)) {
      if (!localPathByAlias.has(alias)) localPathByAlias.set(alias, localPath);
    }
  }

  return dedupeGalleryItems([...incoming]).map((item) => {
    const localPath =
      item.localPath?.trim() ||
      galleryItemAliases(item)
        .map((alias) => localPathByAlias.get(alias))
        .find(Boolean);
    return localPath ? { ...item, localPath } : item;
  });
}

/**
 * Append a new page while allowing its metadata to refresh matching cards.
 * Existing order and membership remain intact; genuinely new cards follow it.
 */
export function appendGalleryItems(
  current: readonly WallpaperGalleryItem[],
  incoming: readonly WallpaperGalleryItem[],
): WallpaperGalleryItem[] {
  const merged: WallpaperGalleryItem[] = [];

  for (const item of current) {
    const aliases = galleryItemAliases(item);
    if (aliases.length === 0) continue;
    const match = merged.findIndex((candidate) => {
      const candidateAliases = new Set(galleryItemAliases(candidate));
      return aliases.some((alias) => candidateAliases.has(alias));
    });
    if (match < 0) {
      merged.push(item);
      continue;
    }
    if (!merged[match]?.localPath && item.localPath?.trim()) {
      merged[match] = { ...merged[match]!, localPath: item.localPath };
    }
  }

  for (const item of incoming) {
    const aliases = galleryItemAliases(item);
    if (aliases.length === 0) continue;
    const matches: number[] = [];
    for (let index = 0; index < merged.length; index += 1) {
      const candidateAliases = new Set(galleryItemAliases(merged[index]!));
      if (aliases.some((alias) => candidateAliases.has(alias))) {
        matches.push(index);
      }
    }
    if (matches.length === 0) {
      merged.push(item);
      continue;
    }

    const target = matches[0]!;
    const localPath =
      item.localPath?.trim() ||
      matches
        .map((index) => merged[index]?.localPath?.trim())
        .find(Boolean) ||
      null;
    merged[target] = { ...item, localPath };
    for (let index = matches.length - 1; index >= 1; index -= 1) {
      merged.splice(matches[index]!, 1);
    }
  }

  return merged;
}

function mimeFromName(name: string): string {
  const lower = name.toLowerCase();
  if (lower.endsWith(".png")) return "image/png";
  if (lower.endsWith(".webp")) return "image/webp";
  if (lower.endsWith(".avif")) return "image/avif";
  if (lower.endsWith(".gif")) return "image/gif";
  if (lower.endsWith(".mp4") || lower.endsWith(".m4v")) return "video/mp4";
  if (lower.endsWith(".webm")) return "video/webm";
  if (lower.endsWith(".jpg") || lower.endsWith(".jpeg")) return "image/jpeg";
  return "application/octet-stream";
}

/**
 * Loopback media HTTP (and legacy media://) answers bare GETs of large files
 * with **206 + first 2 MiB only** (video Range streaming). A naive
 * `fetch(url).blob()` therefore truncates Imagine PNGs / large X downloads and
 * `prepareWallpaperFromFile` fails with a cryptic decode error. Always
 * reassemble via Range (or accept a true 200).
 *
 * Exported for unit tests.
 */
export const MEDIA_PROTO_CHUNK = 2 * 1024 * 1024;

/** Parse `Content-Range: bytes start-end/total` → total length. */
export function parseContentRangeTotal(header: string | null): number | null {
  if (!header) return null;
  const m = /bytes\s+\d+-\d+\/(\d+)\s*$/i.exec(header.trim());
  if (!m) return null;
  const n = Number(m[1]);
  return Number.isFinite(n) && n >= 0 ? n : null;
}

/**
 * Fetch an entire media HTTP (or legacy media://) resource, reassembling Range chunks.
 *
 * `opts.chunkSize` is for unit tests only (default = media protocol 2 MiB).
 */
export async function fetchEntireMediaBlob(
  url: string,
  fetchImpl: typeof fetch = fetch,
  opts?: { chunkSize?: number },
): Promise<Blob> {
  const chunkSize = opts?.chunkSize ?? MEDIA_PROTO_CHUNK;
  // Probe total size with a 1-byte Range (media protocol always supports it).
  const probe = await fetchImpl(url, {
    headers: { Range: "bytes=0-0" },
  });
  if (!(probe.ok || probe.status === 206)) {
    throw new Error(`read_failed: HTTP ${probe.status}`);
  }

  const totalFromRange = parseContentRangeTotal(
    probe.headers.get("content-range"),
  );
  // Consume probe body so the connection can close cleanly.
  try {
    await probe.arrayBuffer();
  } catch {
    /* ignore */
  }

  if (totalFromRange === 0) {
    return new Blob([]);
  }

  // No Content-Range → host returned a full 200 for the probe; just GET once.
  if (totalFromRange == null) {
    const full = await fetchImpl(url);
    if (!full.ok) {
      throw new Error(`read_failed: HTTP ${full.status}`);
    }
    return full.blob();
  }

  if (totalFromRange <= chunkSize) {
    const one = await fetchImpl(url, {
      headers: { Range: `bytes=0-${totalFromRange - 1}` },
    });
    if (!(one.ok || one.status === 206)) {
      throw new Error(`read_failed: HTTP ${one.status}`);
    }
    return one.blob();
  }

  const parts: Blob[] = [];
  let got = 0;
  for (let start = 0; start < totalFromRange; start += chunkSize) {
    const end = Math.min(start + chunkSize - 1, totalFromRange - 1);
    const part = await fetchImpl(url, {
      headers: { Range: `bytes=${start}-${end}` },
    });
    if (!(part.ok || part.status === 206)) {
      throw new Error(`read_failed: HTTP ${part.status} at ${start}-${end}`);
    }
    const blob = await part.blob();
    got += blob.size;
    parts.push(blob);
  }
  if (got < totalFromRange) {
    throw new Error(
      `read_failed: short read (${got}/${totalFromRange} bytes)`,
    );
  }
  return new Blob(parts);
}

/** Bounded raw Tauri IPC chunk size (must match Host `MAX_IPC_CHUNK`). */
export const MEDIA_IPC_CHUNK = 8 * 1024 * 1024;

/** Wallpaper video ceiling (must match Host `MAX_IPC_FILE`). */
export const MEDIA_IPC_MAX_FILE = 200 * 1024 * 1024;

type MediaFileInfo = {
  bytes: number;
  mime: string;
  name: string;
};

type MediaInvoke = (
  command: string,
  args?: Record<string, unknown>,
) => Promise<unknown>;

function ipcBytesToArrayBuffer(value: unknown): ArrayBuffer {
  if (value instanceof ArrayBuffer) return value;
  if (ArrayBuffer.isView(value)) {
    const view = new Uint8Array(value.buffer, value.byteOffset, value.byteLength);
    const copy = new Uint8Array(view.byteLength);
    copy.set(view);
    return copy.buffer;
  }
  // Normalize test doubles and bridges that serialize byte buffers as arrays.
  // Production Tauri returns an ArrayBuffer for `tauri::ipc::Response`.
  if (
    Array.isArray(value) &&
    value.every(
      (byte) => Number.isInteger(byte) && Number(byte) >= 0 && Number(byte) <= 255,
    )
  ) {
    return Uint8Array.from(value as number[]).buffer;
  }
  throw new Error("read_failed: invalid IPC byte response");
}

/**
 * Read a complete allowlisted media file through bounded raw Tauri IPC.
 *
 * WebView2 can reject page-script `fetch()` to 127.0.0.1 before the request
 * reaches the Host CORS handler. Raw IPC avoids that browser network gate
 * while the Host still enforces `path_scope` and a 200 MiB total cap.
 * `opts.chunkSize` is for unit tests only.
 */
export async function readLocalMediaBlobViaIpc(
  absolutePath: string,
  invokeImpl?: MediaInvoke,
  opts?: { chunkSize?: number; maxBytes?: number; signal?: AbortSignal },
): Promise<{ blob: Blob; info: MediaFileInfo }> {
  const invoke: MediaInvoke =
    invokeImpl ?? ((await import("@tauri-apps/api/core")).invoke as MediaInvoke);
  opts?.signal?.throwIfAborted();
  const rawInfo = await invoke("media_file_info", {
    path: absolutePath,
  });
  if (!rawInfo || typeof rawInfo !== "object") {
    throw new Error("read_failed: invalid media metadata");
  }

  const candidate = rawInfo as Partial<MediaFileInfo>;
  const bytes = Number(candidate.bytes);
  if (!Number.isSafeInteger(bytes) || bytes < 0 || bytes > Math.min(opts?.maxBytes ?? MEDIA_IPC_MAX_FILE, MEDIA_IPC_MAX_FILE)) {
    throw new Error("read_failed: invalid media size");
  }
  const chunkSize = opts?.chunkSize ?? MEDIA_IPC_CHUNK;
  if (
    !Number.isSafeInteger(chunkSize) ||
    chunkSize <= 0 ||
    chunkSize > MEDIA_IPC_CHUNK
  ) {
    throw new Error("read_failed: invalid IPC chunk size");
  }

  const info: MediaFileInfo = {
    bytes,
    mime:
      typeof candidate.mime === "string" && candidate.mime.trim()
        ? candidate.mime
        : "application/octet-stream",
    name:
      typeof candidate.name === "string" && candidate.name.trim()
        ? candidate.name
        : absolutePath.split(/[/\\]/).pop() || "wallpaper",
  };

  const parts: ArrayBuffer[] = [];
  let got = 0;
  for (let offset = 0; offset < bytes; offset += chunkSize) {
    opts?.signal?.throwIfAborted();
    const length = Math.min(chunkSize, bytes - offset);
    const raw = await invoke("media_read_file_chunk", {
      path: absolutePath,
      offset,
      length,
    });
    const part = ipcBytesToArrayBuffer(raw);
    if (part.byteLength !== length) {
      throw new Error(
        `read_failed: short IPC read (${part.byteLength}/${length} bytes at ${offset})`,
      );
    }
    got += part.byteLength;
    parts.push(part);
  }
  if (got !== bytes) {
    throw new Error(`read_failed: short IPC read (${got}/${bytes} bytes)`);
  }
  opts?.signal?.throwIfAborted();
  return { blob: new Blob(parts, { type: info.mime }), info };
}

/**
 * Load a local absolute path into a File for prepareWallpaperFromFile.
 * Uses bounded raw Tauri IPC so WebView2 local-network policy cannot block it.
 */
export async function fileFromAbsolutePath(
  absolutePath: string,
  opts?: { name?: string; mime?: string },
): Promise<File> {
  // Browser / unit tests: no Tauri
  if (
    typeof window === "undefined" ||
    !("__TAURI_INTERNALS__" in window || "__TAURI__" in window)
  ) {
    throw new Error("desktop_only");
  }

  let result: Awaited<ReturnType<typeof readLocalMediaBlobViaIpc>>;
  try {
    result = await readLocalMediaBlobViaIpc(absolutePath);
  } catch (e) {
    const msg = e instanceof Error ? e.message : String(e);
    if (msg.includes("path_not_allowed")) throw new Error("path_not_allowed");
    if (msg.startsWith("read_failed")) throw e instanceof Error ? e : new Error(msg);
    throw new Error(`read_failed: ${msg}`);
  }

  const { blob, info } = result;
  if (!blob.size) {
    throw new Error("read_failed: empty file");
  }

  const name =
    opts?.name ||
    info.name ||
    absolutePath.split(/[/\\]/).pop() ||
    "wallpaper.jpg";
  const fallbackMime = mimeFromName(name);
  const type =
    opts?.mime ||
    (blob.type && blob.type !== "application/octet-stream"
      ? blob.type
      : fallbackMime);
  return new File([blob], name, { type });
}

/** Prefer local path when present (Imagine / library). */
export function resolveApplySource(
  item: WallpaperGalleryItem,
): { kind: "path"; path: string } | { kind: "url"; url: string } {
  if (item.localPath && item.localPath.trim()) {
    return { kind: "path", path: item.localPath.trim() };
  }
  // file:// from host scan
  if (item.fullUrl.startsWith("file://")) {
    const p = item.fullUrl.replace(/^file:\/\//, "");
    return { kind: "path", path: decodeURIComponent(p) };
  }
  return { kind: "url", url: item.fullUrl };
}

// ── Library (X / Imagine / Grok album disk cache) ────────────────────────────

/** Stable id for a library disk row (path + mtime). */
export function libraryEntryId(entry: WallpaperLibraryEntry): string {
  const path = (entry.path || "").trim();
  const name = (entry.name || path.split(/[/\\]/).pop() || "file").trim();
  return `library-${entry.modifiedMs}-${name}-${shortHash(path || name)}`;
}

function shortHash(s: string): string {
  let h = 0;
  for (let i = 0; i < s.length; i++) {
    h = (Math.imul(31, h) + s.charCodeAt(i)) | 0;
  }
  return (h >>> 0).toString(36);
}

/** True when library entry is a still image (static-first library apply). */
export function isStaticImageLibraryEntry(
  entry: Pick<WallpaperLibraryEntry, "kind" | "name">,
): boolean {
  const kind = String(entry.kind || "").toLowerCase();
  if (kind === "video" || kind.startsWith("video/")) return false;
  const name = String(entry.name || "").toLowerCase();
  if (/\.(mp4|m4v|webm|mov)(\?|$)/i.test(name)) return false;
  return true;
}

/**
 * Sort library rows with static images first (then by modifiedMs desc).
 * Does not invent rows — only reorders / optionally drops video.
 */
export function sortLibraryEntriesStaticFirst(
  entries: readonly WallpaperLibraryEntry[],
  opts?: { imagesOnly?: boolean },
): WallpaperLibraryEntry[] {
  const imagesOnly = opts?.imagesOnly === true;
  const list = imagesOnly
    ? entries.filter(isStaticImageLibraryEntry)
    : [...entries];
  return list.sort((a, b) => {
    const av = isStaticImageLibraryEntry(a) ? 0 : 1;
    const bv = isStaticImageLibraryEntry(b) ? 0 : 1;
    if (av !== bv) return av - bv;
    return (b.modifiedMs || 0) - (a.modifiedMs || 0);
  });
}

/** Map a host library row to a gallery card (local path only). */
export function libraryEntryToGalleryItem(
  entry: WallpaperLibraryEntry,
): WallpaperGalleryItem {
  const path = (entry.path || "").trim();
  const abs = path.startsWith("file://")
    ? decodeURIComponent(path.replace(/^file:\/\//, ""))
    : path;
  const fileUrl = abs ? `file://${abs}` : "";
  const kind =
    String(entry.kind || "").toLowerCase() === "video" ||
    /\.(mp4|m4v|webm|mov)$/i.test(entry.name || abs)
      ? "video"
      : "image";
  const source = (entry.metadata?.source || entry.source || "library").trim() || "library";
  return {
    id: libraryEntryId(entry),
    thumbUrl: fileUrl,
    fullUrl: fileUrl,
    kind,
    source,
    localPath: abs || null,
    textPreview: entry.metadata?.title || entry.name || null,
    username: null,
    postUrl: null,
    prompt: entry.metadata?.prompt ?? null,
    likes: null,
    width: entry.metadata?.width ?? null,
    height: entry.metadata?.height ?? null,
    metadata: entry.metadata,
    sourceUrl: entry.metadata?.sourceUrl,
    sourceName: entry.metadata?.sourceName,
    authorName: entry.metadata?.authorName,
    authorUrl: entry.metadata?.authorUrl,
    license: entry.metadata?.license,
    licenseUrl: entry.metadata?.licenseUrl,
  };
}

/**
 * Convert host library list → gallery items.
 * `staticFirst` (default true): images before video, no invented CDN rows.
 * `imagesOnly`: drop video rows entirely (static-first library apply path).
 */
export function libraryEntriesToGalleryItems(
  entries: readonly WallpaperLibraryEntry[],
  opts?: { staticFirst?: boolean; imagesOnly?: boolean },
): WallpaperGalleryItem[] {
  const staticFirst = opts?.staticFirst !== false;
  const ordered = staticFirst
    ? sortLibraryEntriesStaticFirst(entries, { imagesOnly: opts?.imagesOnly })
    : opts?.imagesOnly
      ? entries.filter(isStaticImageLibraryEntry)
      : [...entries];
  return dedupeGalleryItems(ordered.map(libraryEntryToGalleryItem));
}
