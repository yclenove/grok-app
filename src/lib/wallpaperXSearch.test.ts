import { describe, expect, it } from "vitest";
import {
  createWallpaperXSearchRequestId,
  DEFAULT_WALLPAPER_X_SEARCH_MODE,
  isWallpaperXSearchBatch,
  isWallpaperXSearchProgress,
  normalizeWallpaperXSearchMode,
  wallpaperXSearchFallbackReasonKey,
  wallpaperXSearchProgressMessageKey,
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
      cacheHit: false,
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
      cacheHit: false,
    });
  });

  it("groups stable fallback codes without exposing host details", () => {
    expect(wallpaperXSearchFallbackReasonKey("responses_timeout")).toBe(
      "settings.wallpaperSource.route.fallback.network",
    );
    expect(wallpaperXSearchFallbackReasonKey("responses_protocol")).toBe(
      "settings.wallpaperSource.route.fallback.compatibility",
    );
    expect(wallpaperXSearchFallbackReasonKey("responses_tool_not_called")).toBe(
      "settings.wallpaperSource.route.fallback.compatibility",
    );
    expect(wallpaperXSearchFallbackReasonKey("responses_circuit_open")).toBe(
      "settings.wallpaperSource.route.fallback.circuit",
    );
    expect(wallpaperXSearchFallbackReasonKey("future_reason")).toBe(
      "settings.wallpaperSource.route.fallback.other",
    );
  });

  it("separates Responses, CLI, and total time for a real fallback", () => {
    expect(
      wallpaperXSearchRouteSummary({
        requestedMode: "responses_preview",
        routeUsed: "cli",
        fallbackReason: "responses_network",
        durationMs: 84_900,
        responsesDurationMs: 18_400,
        cliDurationMs: 66_500,
        cacheHit: false,
        candidateCount: 8,
        validCount: 6,
      }),
    ).toEqual({
      key: "settings.wallpaperSource.route.fallbackTimed",
      seconds: "84.9",
      responsesSeconds: "18.4",
      cliSeconds: "66.5",
      reasonKey: "settings.wallpaperSource.route.fallback.network",
      cacheHit: false,
    });
  });

  it("keeps valid zero-millisecond split timings", () => {
    expect(
      wallpaperXSearchRouteSummary({
        requestedMode: "responses_preview",
        routeUsed: "cli",
        fallbackReason: "oauth_expired",
        durationMs: 0,
        responsesDurationMs: 0,
        cliDurationMs: 0,
        cacheHit: false,
        candidateCount: 0,
        validCount: 0,
      }),
    ).toMatchObject({
      key: "settings.wallpaperSource.route.fallbackTimed",
      seconds: "0.0",
      responsesSeconds: "0.0",
      cliSeconds: "0.0",
    });
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

  it("creates canonical UUID request ids and validates progress payloads", () => {
    const requestId = createWallpaperXSearchRequestId();
    expect(requestId).toMatch(
      /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/,
    );
    expect(
      isWallpaperXSearchProgress({ requestId, stage: "validating" }),
    ).toBe(true);
    expect(
      isWallpaperXSearchProgress({ requestId, stage: "invented" }),
    ).toBe(false);
    expect(isWallpaperXSearchProgress({ stage: "validating" })).toBe(false);
  });

  it("validates only structurally safe batch payloads", () => {
    const requestId = createWallpaperXSearchRequestId();
    const item = {
      id: "image-1",
      thumbUrl: "https://example.test/thumb.jpg",
      fullUrl: "https://example.test/full.jpg",
      kind: "image",
      source: "x",
    };
    expect(
      isWallpaperXSearchBatch({
        requestId,
        batchIndex: 1,
        items: [item],
        accumulatedCount: 1,
        done: false,
      }),
    ).toBe(true);
    expect(
      isWallpaperXSearchBatch({
        requestId,
        batchIndex: 0,
        items: [item],
        accumulatedCount: 1,
        done: false,
      }),
    ).toBe(false);
    expect(
      isWallpaperXSearchBatch({
        requestId,
        batchIndex: 1,
        items: [item],
        accumulatedCount: 0,
        done: false,
      }),
    ).toBe(false);
    expect(
      isWallpaperXSearchBatch({
        requestId,
        batchIndex: 1,
        items: [{ ...item, fullUrl: "" }],
        accumulatedCount: 1,
        done: true,
      }),
    ).toBe(false);
  });

  it("maps only visible progress stages to localized copy", () => {
    expect(wallpaperXSearchProgressMessageKey("preparing")).toBe(
      "settings.wallpaperSource.progress.preparing",
    );
    expect(wallpaperXSearchProgressMessageKey("searching_x")).toBe(
      "settings.wallpaperSource.searching",
    );
    expect(wallpaperXSearchProgressMessageKey("done")).toBeNull();
  });

  it("keeps cache honesty alongside the actual route", () => {
    expect(
      wallpaperXSearchRouteSummary({
        requestId: "request-2",
        requestedMode: "responses_preview",
        routeUsed: "cli",
        fallbackReason: "oauth_expired",
        durationMs: 1,
        responsesDurationMs: 18_400,
        cliDurationMs: 66_500,
        cacheHit: true,
        candidateCount: 8,
        validCount: 6,
      }),
    ).toMatchObject({
      key: "settings.wallpaperSource.route.fallback",
      cacheHit: true,
      reasonKey: "settings.wallpaperSource.route.fallback.auth",
    });
  });

  it("falls back to total-only copy for invalid or legacy split timings", () => {
    expect(
      wallpaperXSearchRouteSummary({
        requestedMode: "responses_preview",
        routeUsed: "cli",
        fallbackReason: "responses_network",
        durationMs: 900,
        responsesDurationMs: Number.NaN,
        cliDurationMs: -1,
        cacheHit: false,
        candidateCount: 1,
        validCount: 1,
      }),
    ).toMatchObject({
      key: "settings.wallpaperSource.route.fallback",
      seconds: "0.9",
    });
  });
});
