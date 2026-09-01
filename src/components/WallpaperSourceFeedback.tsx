import type { MessageKey } from "@/i18n";
import {
  wallpaperGalleryErrorTitleKey,
  type WallpaperGalleryErrorKind,
} from "@/lib/wallpaperGalleryPro";
import type { WallpaperSourceKind } from "@/lib/wallpaperSource";

type Translate = (
  key: MessageKey,
  vars?: Record<string, string | number | undefined | null>,
) => string;

export type WallpaperSourceFeedbackProps = {
  t: Translate;
  tab: WallpaperSourceKind;
  progress: string | null;
  routeStatus: string | null;
  error: string | null;
  errorKind: WallpaperGalleryErrorKind | null;
  softFail: boolean;
  authNeeded: boolean;
  onRequestLogin?: () => void;
};

export function WallpaperSourceFeedback({
  t,
  tab,
  progress,
  routeStatus,
  error,
  errorKind,
  softFail,
  authNeeded,
  onRequestLogin,
}: WallpaperSourceFeedbackProps) {
  const showXFeedback = tab === "x";

  return (
    <>
      {progress || (routeStatus && showXFeedback) ? (
        <div className="wallpaper-source-feedback" role="status">
          {progress ? (
            <span className="wallpaper-source-status">{progress}</span>
          ) : null}
          {routeStatus && showXFeedback ? (
            <span className="wallpaper-source-route-summary">{routeStatus}</span>
          ) : null}
        </div>
      ) : null}

      {error ? (
        <div
          className={
            "wallpaper-source-error" +
            (softFail ? " wallpaper-source-error--soft" : "")
          }
          role="alert"
        >
          {errorKind ? (
            <span
              className={
                "wallpaper-source-err-chip" +
                (softFail ? " wallpaper-source-err-chip--soft" : "")
              }
              data-kind={errorKind}
            >
              {t(wallpaperGalleryErrorTitleKey(errorKind) as MessageKey)}
            </span>
          ) : null}
          <p>{error}</p>
          {authNeeded && onRequestLogin ? (
            <button
              type="button"
              className="btn btn--ghost btn--sm"
              onClick={onRequestLogin}
            >
              {t("settings.wallpaperSource.goLogin")}
            </button>
          ) : null}
        </div>
      ) : null}
    </>
  );
}
