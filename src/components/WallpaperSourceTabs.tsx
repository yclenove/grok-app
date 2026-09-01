import { useEffect, useRef, type ReactNode } from "react";
import {
  IconCamera,
  IconExportImage,
  IconFolder,
  IconImagine,
  IconPhotoSearch,
  IconSearch,
  IconWorld,
} from "@/components/icons";
import type { MessageKey } from "@/i18n";
import type { WallpaperSourceKind } from "@/lib/wallpaperSource";

type Translate = (key: MessageKey) => string;

const SOURCE_TABS: ReadonlyArray<{
  id: WallpaperSourceKind;
  labelKey: MessageKey;
  icon: ReactNode;
  groupStart?: boolean;
}> = [
  {
    id: "x",
    labelKey: "settings.wallpaperFromX",
    icon: <IconSearch size={15} />,
  },
  {
    id: "web",
    labelKey: "settings.wallpaperWeb",
    icon: <IconWorld size={15} />,
  },
  {
    id: "openverse",
    labelKey: "settings.wallpaperOpenverse",
    icon: <IconPhotoSearch size={15} />,
  },
  {
    id: "pexels",
    labelKey: "settings.wallpaperPexels",
    icon: <IconCamera size={15} />,
  },
  {
    id: "imagine",
    labelKey: "settings.wallpaperImagine",
    icon: <IconImagine size={15} />,
    groupStart: true,
  },
  {
    id: "grok_album",
    labelKey: "settings.wallpaperGrokAlbum",
    icon: <IconExportImage size={15} />,
    groupStart: true,
  },
  {
    id: "library",
    labelKey: "settings.wallpaperLibrary",
    icon: <IconFolder size={15} />,
  },
];

export type WallpaperSourceTabsProps = {
  t: Translate;
  value: WallpaperSourceKind;
  disabled: boolean;
  panelId: string;
  onChange: (value: WallpaperSourceKind) => void;
};

export function WallpaperSourceTabs({
  t,
  value,
  disabled,
  panelId,
  onChange,
}: WallpaperSourceTabsProps) {
  const activeTabRef = useRef<HTMLButtonElement | null>(null);

  useEffect(() => {
    activeTabRef.current?.scrollIntoView?.({
      block: "nearest",
      inline: "nearest",
    });
  }, [value]);

  return (
    <div
      className="wallpaper-source-tabs"
      role="tablist"
      aria-orientation="horizontal"
      aria-label={t("settings.wallpaperSource.title")}
    >
      {SOURCE_TABS.map((tab) => {
        const active = value === tab.id;
        return (
          <button
            key={tab.id}
            ref={active ? activeTabRef : undefined}
            type="button"
            role="tab"
            id={`wallpaper-source-tab-${tab.id}`}
            aria-controls={panelId}
            aria-selected={active}
            aria-label={t(tab.labelKey)}
            title={t(tab.labelKey)}
            data-group-start={tab.groupStart ? "true" : undefined}
            className={
              "wallpaper-source-tabs__btn" +
              (active ? " wallpaper-source-tabs__btn--active" : "")
            }
            onClick={() => onChange(tab.id)}
            disabled={disabled}
          >
            {tab.icon}
            <span className="wallpaper-source-tabs__label">
              {t(tab.labelKey)}
            </span>
          </button>
        );
      })}
    </div>
  );
}
