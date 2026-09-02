import { IconExternalLink } from "@/components/icons";
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
  className: string,
  iconOnly = false,
) {
  return url ? (
    <button
      type="button"
      className={`wallpaper-attribution__link ${className}`}
      disabled={disabled}
      onClick={() => onOpen(url)}
      title={label}
      aria-label={iconOnly ? label : undefined}
    >
      {iconOnly ? <IconExternalLink size={13} aria-hidden /> : label}
    </button>
  ) : (
    <span className={`wallpaper-attribution__text ${className}`}>{label}</span>
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
  const licensedProvider =
    item.source === "openverse" || item.source === "pexels";

  return (
    <div
      className={
        "wallpaper-attribution" +
        (licensedProvider ? " wallpaper-attribution--licensed" : "")
      }
      aria-label={t("settings.wallpaperSource.attribution")}
    >
      {linkButton(
        sourceLabel,
        sourceUrl,
        disabled,
        onOpen,
        "wallpaper-attribution__source",
        licensedProvider,
      )}
      {author
        ? linkButton(
            author,
            item.authorUrl,
            disabled,
            onOpen,
            "wallpaper-attribution__author",
          )
        : null}
      {license
        ? linkButton(
            license,
            item.licenseUrl,
            disabled,
            onOpen,
            "wallpaper-attribution__license",
          )
        : null}
    </div>
  );
}
