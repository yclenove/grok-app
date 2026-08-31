import type { MessageKey } from "@/i18n";
import type { WallpaperSourceKind } from "@/lib/wallpaperSource";

type Translate = (key: MessageKey) => string;

const SOURCE_TABS: ReadonlyArray<{
  id: WallpaperSourceKind;
  labelKey: MessageKey;
}> = [
  { id: "x", labelKey: "settings.wallpaperFromX" },
  { id: "imagine", labelKey: "settings.wallpaperImagine" },
  { id: "grok_album", labelKey: "settings.wallpaperGrokAlbum" },
  { id: "library", labelKey: "settings.wallpaperLibrary" },
];

export type WallpaperSourceTabsProps = {
  t: Translate;
  value: WallpaperSourceKind;
  disabled: boolean;
  onChange: (value: WallpaperSourceKind) => void;
};

export function WallpaperSourceTabs({
  t,
  value,
  disabled,
  onChange,
}: WallpaperSourceTabsProps) {
  return (
    <div className="wallpaper-source-tabs" role="tablist">
      {SOURCE_TABS.map((tab) => {
        const active = value === tab.id;
        return (
          <button
            key={tab.id}
            type="button"
            role="tab"
            aria-selected={active}
            className={
              "wallpaper-source-tabs__btn" +
              (active ? " wallpaper-source-tabs__btn--active" : "")
            }
            onClick={() => onChange(tab.id)}
            disabled={disabled}
          >
            {t(tab.labelKey)}
          </button>
        );
      })}
    </div>
  );
}
