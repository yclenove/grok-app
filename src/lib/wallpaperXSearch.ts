export const WALLPAPER_X_SEARCH_MODES = [
  "cli",
  "responses_preview",
  "auto",
] as const;

export type WallpaperXSearchMode = (typeof WALLPAPER_X_SEARCH_MODES)[number];

export const DEFAULT_WALLPAPER_X_SEARCH_MODE: WallpaperXSearchMode = "cli";

/** Unknown or missing persisted values must never opt users into preview mode. */
export function normalizeWallpaperXSearchMode(
  raw: unknown,
): WallpaperXSearchMode {
  return raw === "responses_preview" || raw === "auto"
    ? raw
    : DEFAULT_WALLPAPER_X_SEARCH_MODE;
}
