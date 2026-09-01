import type { MessageKey } from "@/i18n";
import type { WallpaperGalleryItem } from "@/lib/wallpaperSource";

type Translate = (key: MessageKey) => string;

export type WallpaperSourceAttributionProps = {
  item: WallpaperGalleryItem;
  t: Translate;
  disabled: boolean;
  onOpen: (url: string) => void;
};

function linkButton(
  label: string,
  url: string | null | undefined,
  disabled: boolean,
  onOpen: (url: string) => void,
) {
  return url ? (
    <button
      type="button"
      className="wallpaper-attribution__link"
      disabled={disabled}
      onClick={() => onOpen(url)}
      title={label}
    >
      {label}
    </button>
  ) : (
    <span className="wallpaper-attribution__text">{label}</span>
  );
}

export function WallpaperSourceAttribution({
  item,
  t,
  disabled,
  onOpen,
}: WallpaperSourceAttributionProps) {
  const sourceLabel = item.sourceName?.trim();
  const sourceUrl = item.sourceUrl?.trim();
  const author = item.authorName?.trim();
  const license = item.license?.trim();
  if (!sourceUrl || !sourceLabel) return null;

  return (
    <div
      className="wallpaper-attribution"
      aria-label={t("settings.wallpaperSource.attribution")}
    >
      {linkButton(sourceLabel, sourceUrl, disabled, onOpen)}
      {author ? (
        <>
          <span aria-hidden="true">·</span>
          {linkButton(author, item.authorUrl, disabled, onOpen)}
        </>
      ) : null}
      {license ? (
        <>
          <span aria-hidden="true">·</span>
          {linkButton(license, item.licenseUrl, disabled, onOpen)}
        </>
      ) : null}
    </div>
  );
}
