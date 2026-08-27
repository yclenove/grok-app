import { describe, expect, it } from "vitest";
import {
  DEFAULT_WALLPAPER_X_SEARCH_MODE,
  normalizeWallpaperXSearchMode,
  wallpaperXSearchFallbackReasonKey,
  wallpaperXSearchRouteSummary,
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

  it("describes the actual route instead of inferring it from requested mode", () => {
    expect(
      wallpaperXSearchRouteSummary({
        requestedMode: "responses_preview",
        routeUsed: "responses",
        durationMs: 12_345,
        cacheHit: false,
        candidateCount: 8,
        validCount: 6,
      }),
    ).toEqual({
      key: "settings.wallpaperSource.route.responses",
      seconds: "12.3",
    });

    expect(
      wallpaperXSearchRouteSummary({
        requestedMode: "responses_preview",
        routeUsed: "cli",
        fallbackReason: "oauth_expired",
        durationMs: 987,
        cacheHit: false,
        candidateCount: 4,
        validCount: 3,
      }),
    ).toEqual({
      key: "settings.wallpaperSource.route.fallback",
      seconds: "1.0",
      reasonKey: "settings.wallpaperSource.route.fallback.auth",
    });
  });

  it("groups stable fallback codes without exposing host details", () => {
    expect(wallpaperXSearchFallbackReasonKey("responses_timeout")).toBe(
      "settings.wallpaperSource.route.fallback.network",
    );
    expect(wallpaperXSearchFallbackReasonKey("responses_protocol")).toBe(
      "settings.wallpaperSource.route.fallback.compatibility",
    );
    expect(wallpaperXSearchFallbackReasonKey("responses_circuit_open")).toBe(
      "settings.wallpaperSource.route.fallback.circuit",
    );
    expect(wallpaperXSearchFallbackReasonKey("future_reason")).toBe(
      "settings.wallpaperSource.route.fallback.other",
    );
  });

  it("ignores absent or invalid legacy metadata", () => {
    expect(wallpaperXSearchRouteSummary(undefined)).toBeNull();
    expect(
      wallpaperXSearchRouteSummary({
        requestedMode: "cli",
        routeUsed: "cli",
        durationMs: Number.NaN,
        cacheHit: false,
        candidateCount: 0,
        validCount: 0,
      }),
    ).toBeNull();
  });
});
