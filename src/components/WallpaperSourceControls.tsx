import { GrokAlbumSourcePanel } from "@/components/GrokAlbumSourcePanel";
import { Select } from "@/components/Select";
import type { MessageKey } from "@/i18n";
import type {
  GrokAlbumErrorCode,
  GrokAlbumStatus,
} from "@/lib/grokAlbum";
import type { WallpaperSourceKind } from "@/lib/wallpaperSource";

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
  query: string;
  sort: "top" | "latest";
  sortOptions: SelectOption[];
  prompt: string;
  aspect: string;
  aspectOptions: SelectOption[];
  albumStatus: GrokAlbumStatus;
  albumCachedCount: number;
  albumVisibleCount: number;
  albumErrorCode: GrokAlbumErrorCode | null;
  albumSyncing: boolean;
  onQueryChange: (value: string) => void;
  onSortChange: (value: "top" | "latest") => void;
  onSearchX: () => void;
  onCancelX: () => void;
  onPromptChange: (value: string) => void;
  onAspectChange: (value: string) => void;
  onGenerate: () => void;
  onOpenAlbum: () => void;
  onSyncAlbum: () => void;
  onRefreshAlbum: () => void;
  onRefreshLibrary: () => void;
};

export function WallpaperSourceControls({
  t,
  tab,
  locked,
  busy,
  xSearchBusy,
  query,
  sort,
  sortOptions,
  prompt,
  aspect,
  aspectOptions,
  albumStatus,
  albumCachedCount,
  albumVisibleCount,
  albumErrorCode,
  albumSyncing,
  onQueryChange,
  onSortChange,
  onSearchX,
  onCancelX,
  onPromptChange,
  onAspectChange,
  onGenerate,
  onOpenAlbum,
  onSyncAlbum,
  onRefreshAlbum,
  onRefreshLibrary,
}: WallpaperSourceControlsProps) {
  if (tab === "x") {
    return (
      <div className="wallpaper-source-form">
        <p className="wallpaper-source-form__hint">
          {t("settings.wallpaperSource.xHint")}
        </p>
        <div className="wallpaper-source-form__row">
          <input
            type="search"
            className="wallpaper-source-form__input"
            value={query}
            placeholder={t("settings.wallpaperSource.xPlaceholder")}
            disabled={locked}
            onChange={(event) => onQueryChange(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter") {
                event.preventDefault();
                onSearchX();
              }
            }}
          />
          <Select
            className="wallpaper-source-form__select"
            value={sort}
            options={sortOptions}
            disabled={locked}
            aria-label={t("settings.wallpaperSource.sort")}
            onChange={(value) => onSortChange(value === "latest" ? "latest" : "top")}
            placement="down"
          />
          <button
            type="button"
            className={xSearchBusy ? "btn btn--ghost" : "btn btn--solid"}
            disabled={!xSearchBusy && (locked || !query.trim())}
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
    return (
      <div className="wallpaper-source-form">
        <p className="wallpaper-source-form__hint">
          {t("settings.wallpaperSource.imagineHint")}
        </p>
        <textarea
          className="wallpaper-source-form__textarea"
          value={prompt}
          placeholder={t("settings.wallpaperSource.imaginePlaceholder")}
          disabled={locked}
          rows={3}
          onChange={(event) => onPromptChange(event.target.value)}
        />
        <div className="wallpaper-source-form__row">
          <Select
            className="wallpaper-source-form__select"
            value={aspect}
            options={aspectOptions}
            disabled={locked}
            aria-label={t("settings.wallpaperSource.aspect")}
            onChange={onAspectChange}
            placement="down"
          />
          <button
            type="button"
            className="btn btn--solid"
            disabled={locked || !prompt.trim()}
            onClick={onGenerate}
          >
            {busy
              ? t("settings.wallpaperSource.generating")
              : t("settings.wallpaperSource.generate")}
          </button>
        </div>
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

  return (
    <div className="wallpaper-source-form">
      <p className="wallpaper-source-form__hint">
        {t("settings.wallpaperSource.libraryHint")}
      </p>
      <div className="wallpaper-source-form__row">
        <button
          type="button"
          className="btn btn--solid"
          disabled={locked}
          onClick={onRefreshLibrary}
        >
          {busy
            ? t("settings.wallpaperSource.libraryLoading")
            : t("settings.wallpaperSource.libraryRefresh")}
        </button>
      </div>
    </div>
  );
}
