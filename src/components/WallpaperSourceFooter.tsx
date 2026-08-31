import type { MessageKey } from "@/i18n";

type Translate = (key: MessageKey) => string;

export type WallpaperSourceFooterProps = {
  t: Translate;
  hasSelection: boolean;
  applying: boolean;
  applyDisabled: boolean;
  onClose: () => void;
  onApply: () => void;
};

export function WallpaperSourceFooter({
  t,
  hasSelection,
  applying,
  applyDisabled,
  onClose,
  onApply,
}: WallpaperSourceFooterProps) {
  return (
    <>
      <span className="wallpaper-source-footer-hint">
        {hasSelection
          ? t("settings.wallpaperSource.previewThenApply")
          : t("settings.wallpaperSource.clickToPreview")}
      </span>
      <button
        type="button"
        className="btn btn--ghost"
        onClick={onClose}
        disabled={applying}
      >
        {t("common.cancel")}
      </button>
      <button
        type="button"
        className="btn btn--solid"
        disabled={applyDisabled}
        onClick={onApply}
      >
        {applying
          ? t("settings.wallpaperSource.applying")
          : t("settings.wallpaperSource.apply")}
      </button>
    </>
  );
}
