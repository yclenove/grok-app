import { useCallback, useRef } from "react";
import type { WallpaperGalleryItem, WallpaperSourceKind } from "@/lib/wallpaperSource";
import type { WallpaperXSearchMeta } from "@/lib/wallpaperXSearch";
import type { WallpaperRemoteContinuationState } from "./useWallpaperRemoteSourceController";

export type WallpaperSourceSnapshot = {
  query: string;
  sort: "top" | "latest";
  items: WallpaperGalleryItem[];
  selectedId: string | null;
  galleryFilter: string;
  kindFilter: "all" | "image" | "video";
  hasSearched: boolean;
  routeMeta: WallpaperXSearchMeta | null;
  loadMoreAttempted: boolean;
  xContinuation: { query: string; sort: "top" | "latest" } | null;
  remoteContinuation: WallpaperRemoteContinuationState | null;
  scrollTop: number;
};

const HISTORY_TTL = 20 * 60 * 1000;
const MAX_ITEMS = 2_000;

export function useWallpaperSourceHistory() {
  const entries = useRef(new Map<WallpaperSourceKind, { value: WallpaperSourceSnapshot; time: number }>());
  const scrollRef = useRef<HTMLDivElement | null>(null);
  const save = useCallback((source: WallpaperSourceKind, value: WallpaperSourceSnapshot) => {
    // Saved album data remains owned by its authenticated controller.
    const snapshot = { ...value, items: source === "grok_album" ? [] : value.items };
    if (snapshot.items.length > MAX_ITEMS) { entries.current.delete(source); return; }
    entries.current.set(source, { value: snapshot, time: Date.now() });
  }, []);
  const get = useCallback((source: WallpaperSourceKind) => {
    const entry = entries.current.get(source);
    if (!entry || Date.now() - entry.time > HISTORY_TTL) { entries.current.delete(source); return null; }
    return entry.value;
  }, []);
  const clear = useCallback((source?: WallpaperSourceKind) => {
    if (source) entries.current.delete(source);
    else entries.current.clear();
  }, []);
  const updateItem = useCallback((item: WallpaperGalleryItem) => {
    for (const entry of entries.current.values()) {
      entry.value.items = entry.value.items.map((row) => row.id === item.id || (row.localPath && row.localPath === item.localPath) ? { ...row, localPath: item.localPath, metadata: item.metadata } : row);
    }
  }, []);
  return { save, get, clear, updateItem, scrollRef };
}
