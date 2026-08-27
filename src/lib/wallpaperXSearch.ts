export const WALLPAPER_X_SEARCH_MODES = [
  "cli",
  "responses_preview",
  "auto",
] as const;

export type WallpaperXSearchMode = (typeof WALLPAPER_X_SEARCH_MODES)[number];

export const DEFAULT_WALLPAPER_X_SEARCH_MODE: WallpaperXSearchMode = "cli";

export type WallpaperXSearchRoute = "cli" | "responses";

export type WallpaperXSearchMeta = {
  requestId?: string | null;
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

export const WALLPAPER_X_SEARCH_STAGES = [
  "preparing",
  "searching_x",
  "validating",
  "supplementing",
  "falling_back",
  "done",
] as const;

export type WallpaperXSearchStage =
  (typeof WALLPAPER_X_SEARCH_STAGES)[number];

export type WallpaperXSearchProgress = {
  requestId: string;
  stage: WallpaperXSearchStage;
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
  cacheHit: boolean;
};

export type WallpaperXSearchProgressMessageKey =
  | "settings.wallpaperSource.progress.preparing"
  | "settings.wallpaperSource.searching"
  | "settings.wallpaperSource.progress.validating"
  | "settings.wallpaperSource.progress.supplementing"
  | "settings.wallpaperSource.progress.fallingBack";

export function createWallpaperXSearchRequestId(): string {
  const bytes = new Uint8Array(16);
  if (globalThis.crypto?.getRandomValues) {
    globalThis.crypto.getRandomValues(bytes);
  } else {
    for (let i = 0; i < bytes.length; i += 1) {
      bytes[i] = Math.floor(Math.random() * 256);
    }
  }
  bytes[6] = (bytes[6] & 0x0f) | 0x40;
  bytes[8] = (bytes[8] & 0x3f) | 0x80;
  const hex = Array.from(bytes, (value) =>
    value.toString(16).padStart(2, "0"),
  ).join("");
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`;
}

export function isWallpaperXSearchProgress(
  value: unknown,
): value is WallpaperXSearchProgress {
  if (!value || typeof value !== "object") return false;
  const progress = value as Partial<WallpaperXSearchProgress>;
  return (
    typeof progress.requestId === "string" &&
    progress.requestId.length > 0 &&
    WALLPAPER_X_SEARCH_STAGES.includes(progress.stage as WallpaperXSearchStage)
  );
}

export function wallpaperXSearchProgressMessageKey(
  stage: WallpaperXSearchStage | null | undefined,
): WallpaperXSearchProgressMessageKey | null {
  switch (stage) {
    case "preparing":
      return "settings.wallpaperSource.progress.preparing";
    case "searching_x":
      return "settings.wallpaperSource.searching";
    case "validating":
      return "settings.wallpaperSource.progress.validating";
    case "supplementing":
      return "settings.wallpaperSource.progress.supplementing";
    case "falling_back":
      return "settings.wallpaperSource.progress.fallingBack";
    default:
      return null;
  }
}

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
      cacheHit: meta.cacheHit,
    };
  }
  if (meta.routeUsed === "responses") {
    return {
      key: "settings.wallpaperSource.route.responses",
      seconds,
      cacheHit: meta.cacheHit,
    };
  }
  if (meta.routeUsed === "cli") {
    return {
      key: "settings.wallpaperSource.route.cli",
      seconds,
      cacheHit: meta.cacheHit,
    };
  }
  return null;
}
