import { describe, expect, it } from "vitest";
import {
  DEFAULT_WALLPAPER_X_SEARCH_MODE,
  normalizeWallpaperXSearchMode,
  WALLPAPER_X_SEARCH_MODES,
} from "./wallpaperXSearch";

describe("wallpaper X search settings contract", () => {
  it("keeps the stable CLI route as the default", () => {
    expect(DEFAULT_WALLPAPER_X_SEARCH_MODE).toBe("cli");
    expect(WALLPAPER_X_SEARCH_MODES).toEqual([
      "cli",
      "responses_preview",
      "auto",
    ]);
  });

  it("normalizes missing and unknown values to CLI", () => {
    for (const raw of [undefined, null, "", "responses", "future_mode", 1]) {
      expect(normalizeWallpaperXSearchMode(raw)).toBe("cli");
    }
  });

  it("preserves known preview and reserved modes", () => {
    expect(normalizeWallpaperXSearchMode("responses_preview")).toBe(
      "responses_preview",
    );
    expect(normalizeWallpaperXSearchMode("auto")).toBe("auto");
  });
});
