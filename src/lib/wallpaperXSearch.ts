export const WALLPAPER_X_SEARCH_MODES = [
  "cli",
  "responses_preview",
  "auto",
] as const;

export type WallpaperXSearchMode = (typeof WALLPAPER_X_SEARCH_MODES)[number];

export const DEFAULT_WALLPAPER_X_SEARCH_MODE: WallpaperXSearchMode = "cli";

export type WallpaperXSearchRoute = "cli" | "responses";

export type WallpaperXSearchMeta = {
  requestedMode: WallpaperXSearchMode;
  routeUsed: WallpaperXSearchRoute;
  fallbackReason?: string | null;
  durationMs: number;
  cacheHit: boolean;
  searchCalls?: number | null;
  candidateCount: number;
  validCount: number;
  model?: string | null;
  effort?: "low" | "medium" | string | null;
};

export type WallpaperXSearchFallbackReasonKey =
  | "settings.wallpaperSource.route.fallback.auth"
  | "settings.wallpaperSource.route.fallback.network"
  | "settings.wallpaperSource.route.fallback.compatibility"
  | "settings.wallpaperSource.route.fallback.empty"
  | "settings.wallpaperSource.route.fallback.circuit"
  | "settings.wallpaperSource.route.fallback.other";

export type WallpaperXSearchRouteSummary = {
  key:
    | "settings.wallpaperSource.route.cli"
    | "settings.wallpaperSource.route.responses"
    | "settings.wallpaperSource.route.fallback";
  seconds: string;
  reasonKey?: WallpaperXSearchFallbackReasonKey;
};

/** Unknown or missing persisted values must never opt users into preview mode. */
export function normalizeWallpaperXSearchMode(
  raw: unknown,
): WallpaperXSearchMode {
  return raw === "responses_preview" || raw === "auto"
    ? raw
    : DEFAULT_WALLPAPER_X_SEARCH_MODE;
}

export function wallpaperXSearchFallbackReasonKey(
  reason: string | null | undefined,
): WallpaperXSearchFallbackReasonKey {
  switch ((reason ?? "").trim().toLowerCase()) {
    case "oauth_unavailable":
    case "oauth_expired":
    case "responses_unauthorized":
      return "settings.wallpaperSource.route.fallback.auth";
    case "responses_server_error":
    case "responses_timeout":
    case "responses_tls":
    case "responses_network":
      return "settings.wallpaperSource.route.fallback.network";
    case "responses_bad_request":
    case "responses_invalid_json":
    case "responses_protocol":
    case "responses_search_budget_exceeded":
      return "settings.wallpaperSource.route.fallback.compatibility";
    case "responses_empty":
      return "settings.wallpaperSource.route.fallback.empty";
    case "responses_circuit_open":
      return "settings.wallpaperSource.route.fallback.circuit";
    default:
      return "settings.wallpaperSource.route.fallback.other";
  }
}

export function wallpaperXSearchRouteSummary(
  meta: WallpaperXSearchMeta | null | undefined,
): WallpaperXSearchRouteSummary | null {
  if (!meta || !Number.isFinite(meta.durationMs) || meta.durationMs < 0) {
    return null;
  }
  const seconds = (meta.durationMs / 1000).toFixed(1);
  if (meta.routeUsed === "cli" && meta.fallbackReason) {
    return {
      key: "settings.wallpaperSource.route.fallback",
      seconds,
      reasonKey: wallpaperXSearchFallbackReasonKey(meta.fallbackReason),
    };
  }
  if (meta.routeUsed === "responses") {
    return {
      key: "settings.wallpaperSource.route.responses",
      seconds,
    };
  }
  if (meta.routeUsed === "cli") {
    return {
      key: "settings.wallpaperSource.route.cli",
      seconds,
    };
  }
  return null;
}
