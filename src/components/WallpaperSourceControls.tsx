import { useState } from "react";
import { GrokAlbumSourcePanel } from "@/components/GrokAlbumSourcePanel";
import { Select } from "@/components/Select";
import {
  WallpaperImagineControls,
  type WallpaperImagineControlsModel,
} from "@/components/WallpaperImagineControls";
import { WallpaperPexelsKeyControl } from "@/components/WallpaperPexelsKeyControl";
import type { MessageKey } from "@/i18n";
import type {
  GrokAlbumErrorCode,
  GrokAlbumStatus,
} from "@/lib/grokAlbum";
import type { WallpaperSourceKind } from "@/lib/wallpaperSource";
import { isWallpaperRemoteSource } from "@/lib/wallpaperRemoteSearch";
import {
  normalizeWallpaperXSearchMode,
  type WallpaperXSearchMode,
} from "@/lib/wallpaperXSearch";

type Translate = (
  key: MessageKey,
  vars?: Record<string, string | number | undefined | null>,
) => string;

type SelectOption = { value: string; label: string };

export type WallpaperSourceControlsProps = {
  t: Translate;
  tab: WallpaperSourceKind;
  locked: boolean;
  busy: boolean;
  xSearchBusy: boolean;
  remoteSearchBusy: boolean;
  remoteSearchDisabled: boolean;
  hasPexelsKey: boolean | null;
  pexelsKeyInvalid: boolean;
  query: string;
  sort: "top" | "latest";
  sortOptions: SelectOption[];
  xSearchMode: WallpaperXSearchMode;
  imagine: WallpaperImagineControlsModel;
  albumStatus: GrokAlbumStatus;
  albumCachedCount: number;
  albumVisibleCount: number;
  albumErrorCode: GrokAlbumErrorCode | null;
  albumSyncing: boolean;
  onQueryChange: (value: string) => void;
  onSortChange: (value: "top" | "latest") => void;
  onXSearchModeChange?: (
    value: WallpaperXSearchMode,
  ) => void | Promise<void>;
  onXSearchModeSaveError: () => void;
  onSearchX: () => void;
  onCancelX: () => void;
  onSearchRemote: () => void;
  onCancelRemote: () => void;
  onSavePexelsKey: (key: string) => Promise<boolean>;
  onRequestDeletePexelsKey: () => void;
  onOpenAlbum: () => void;
  onSyncAlbum: () => void;
  onRefreshAlbum: () => void;
};

export function WallpaperSourceControls({
  t,
  tab,
  locked,
  xSearchBusy,
  remoteSearchBusy,
  remoteSearchDisabled,
  hasPexelsKey,
  pexelsKeyInvalid,
  query,
  sort,
  sortOptions,
  xSearchMode,
  imagine,
  albumStatus,
  albumCachedCount,
  albumVisibleCount,
  albumErrorCode,
  albumSyncing,
  onQueryChange,
  onSortChange,
  onXSearchModeChange,
  onXSearchModeSaveError,
  onSearchX,
  onCancelX,
  onSearchRemote,
  onCancelRemote,
  onSavePexelsKey,
  onRequestDeletePexelsKey,
  onOpenAlbum,
  onSyncAlbum,
  onRefreshAlbum,
}: WallpaperSourceControlsProps) {
  const [xSearchModeSaving, setXSearchModeSaving] = useState(false);

  const changeXSearchMode = async (value: string) => {
    if (!onXSearchModeChange || xSearchModeSaving) return;
    const next = normalizeWallpaperXSearchMode(value);
    if (next === xSearchMode) return;
    setXSearchModeSaving(true);
    try {
      await onXSearchModeChange(next);
    } catch {
      onXSearchModeSaveError();
    } finally {
      setXSearchModeSaving(false);
    }
  };

  if (tab === "x") {
    const xControlsLocked = locked || xSearchModeSaving;
    return (
      <div className="wallpaper-source-form">
        <div className="wallpaper-source-form__row wallpaper-source-form__row--search wallpaper-source-form__row--x">
          <input
            type="search"
            className="wallpaper-source-form__input"
            value={query}
            aria-label={t("settings.wallpaperSource.xPlaceholder")}
            placeholder={t("settings.wallpaperSource.xPlaceholder")}
            disabled={xControlsLocked}
            onChange={(event) => onQueryChange(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter") {
                event.preventDefault();
                onSearchX();
              }
            }}
          />
          {onXSearchModeChange ? (
            <Select
              className="wallpaper-source-form__select wallpaper-source-form__select--route"
              value={xSearchMode}
              options={[
                {
                  value: "cli",
                  label: t("settings.wallpaperXSearchMode.cli"),
                },
                {
                  value: "responses_preview",
                  label: t("settings.wallpaperXSearchMode.responsesPreview"),
                },
              ]}
              disabled={xControlsLocked}
              aria-label={t("settings.wallpaperXSearchMode")}
              title={t("settings.wallpaperXSearchModeDesc")}
              onChange={(value) => void changeXSearchMode(value)}
              placement="down"
            />
          ) : null}
          <Select
            className="wallpaper-source-form__select"
            value={sort}
            options={sortOptions}
            disabled={xControlsLocked}
            aria-label={t("settings.wallpaperSource.sort")}
            onChange={(value) => onSortChange(value === "latest" ? "latest" : "top")}
            placement="down"
          />
          <button
            type="button"
            className={xSearchBusy ? "btn btn--ghost" : "btn btn--solid"}
            disabled={!xSearchBusy && (xControlsLocked || !query.trim())}
            onClick={xSearchBusy ? onCancelX : onSearchX}
          >
            {xSearchBusy
              ? t("settings.wallpaperSource.cancelSearch")
              : t("settings.wallpaperSource.search")}
          </button>
        </div>
      </div>
    );
  }

  if (tab === "imagine") {
    return <WallpaperImagineControls t={t} locked={locked} model={imagine} />;
  }

  if (isWallpaperRemoteSource(tab)) {
    const placeholderKey =
      `settings.wallpaperSource.${tab}.placeholder` as MessageKey;
    return (
      <div className="wallpaper-source-form">
        <div className="wallpaper-source-form__row wallpaper-source-form__row--search wallpaper-source-form__row--remote">
          <input
            type="search"
            className="wallpaper-source-form__input"
            value={query}
            aria-label={t(placeholderKey)}
            placeholder={t(placeholderKey)}
            disabled={locked}
            onChange={(event) => onQueryChange(event.target.value)}
            onKeyDown={(event) => {
              if (
                event.key === "Enter" &&
                !locked &&
                !remoteSearchDisabled
              ) {
                event.preventDefault();
                onSearchRemote();
              }
            }}
          />
          <button
            type="button"
            className={remoteSearchBusy ? "btn btn--ghost" : "btn btn--solid"}
            disabled={
              !remoteSearchBusy &&
              (locked || remoteSearchDisabled || !query.trim())
            }
            onClick={remoteSearchBusy ? onCancelRemote : onSearchRemote}
          >
            {remoteSearchBusy
              ? t("settings.wallpaperSource.cancelSearch")
              : t("settings.wallpaperSource.search")}
          </button>
        </div>
        {tab === "pexels" ? (
          <WallpaperPexelsKeyControl
            t={t}
            hasKey={hasPexelsKey}
            invalid={pexelsKeyInvalid}
            disabled={locked}
            onSave={onSavePexelsKey}
            onRequestDelete={onRequestDeletePexelsKey}
          />
        ) : null}
      </div>
    );
  }

  if (tab === "grok_album") {
    return (
      <GrokAlbumSourcePanel
        t={t}
        status={albumStatus}
        busy={locked}
        syncing={albumSyncing}
        cachedCount={albumCachedCount}
        visibleCount={albumVisibleCount}
        errorCode={albumErrorCode}
        onOpen={onOpenAlbum}
        onSync={onSyncAlbum}
        onRefresh={onRefreshAlbum}
      />
    );
  }

  return null;
}
