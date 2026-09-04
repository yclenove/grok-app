import type { MessageKey } from "@/i18n";
import type { WallpaperSourceErrorCode } from "@/lib/wallpaperSource";
import {
  wallpaperXSearchRouteSummary,
  type WallpaperXSearchMeta,
} from "@/lib/wallpaperXSearch";

type Translate = (
  key: MessageKey,
  vars?: Record<string, string | number | undefined | null>,
) => string;

export const WALLPAPER_ASPECT_OPTIONS = [
  { value: "16:9", label: "16:9" },
  { value: "9:16", label: "9:16" },
  { value: "1:1", label: "1:1" },
  { value: "4:3", label: "4:3" },
  { value: "auto", label: "auto" },
];

export function wallpaperSourceErrorMessage(
  t: Translate,
  code: WallpaperSourceErrorCode,
): string {
  const key = `settings.wallpaperSource.err.${code}` as MessageKey;
  const message = t(key);
  return message === key ? t("settings.wallpaperSource.err.generic") : message;
}

export function wallpaperSourceRouteStatus(
  t: Translate,
  meta: WallpaperXSearchMeta | null,
): string | null {
  const summary = wallpaperXSearchRouteSummary(meta);
  if (!summary) return null;
  const route = t(summary.key as MessageKey, {
    seconds: summary.seconds,
    responsesSeconds: summary.responsesSeconds,
    cliSeconds: summary.cliSeconds,
    reason: summary.reasonKey
      ? t(summary.reasonKey as MessageKey)
      : undefined,
  });
  return summary.cacheHit
    ? t("settings.wallpaperSource.route.cached", { route })
    : route;
}
