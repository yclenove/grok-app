import type { WallpaperGalleryItem } from "./wallpaperSource";
import { fileFromAbsolutePath } from "./wallpaperSource";
import { ensureLocalWallpaperMedia } from "./wallpaperSourceMedia";
import {
  recordWallpaperXEvidencePick,
  wallpaperXEvidenceFromGalleryItem,
} from "./xEvidenceCitation";

/**
 * Materialize one selected result without allowing an invalidated source
 * generation to cross the final wallpaper-application boundary.
 */
export async function prepareWallpaperSelection(
  item: WallpaperGalleryItem,
  isCurrent: () => boolean,
): Promise<File | null> {
  const local = await ensureLocalWallpaperMedia(item);
  if (!isCurrent()) return null;

  // Local evidence ring for X picks only (path + status url meta; no cloud).
  if ((item.source || "x") === "x") {
    const pick = wallpaperXEvidenceFromGalleryItem(item, local.path);
    if (pick) recordWallpaperXEvidencePick(pick);
  }
  const file = await fileFromAbsolutePath(local.path, {
    name: local.name,
    mime: local.mime,
  });
  return isCurrent() ? file : null;
}
