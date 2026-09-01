import { GrokAlbumSourcePanel } from "@/components/GrokAlbumSourcePanel";
import { Select } from "@/components/Select";
import { WallpaperPexelsKeyControl } from "@/components/WallpaperPexelsKeyControl";
import type { MessageKey } from "@/i18n";
import type {
  GrokAlbumErrorCode,
  GrokAlbumStatus,
} from "@/lib/grokAlbum";
import type { WallpaperSourceKind } from "@/lib/wallpaperSource";
import { isWallpaperRemoteSource } from "@/lib/wallpaperRemoteSearch";

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
  onSearchRemote: () => void;
  onCancelRemote: () => void;
  onSavePexelsKey: (key: string) => Promise<boolean>;
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
  remoteSearchBusy,
  remoteSearchDisabled,
  hasPexelsKey,
  pexelsKeyInvalid,
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
  onSearchRemote,
  onCancelRemote,
  onSavePexelsKey,
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
        <div className="wallpaper-source-form__row wallpaper-source-form__row--search wallpaper-source-form__row--x">
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
        <textarea
          className="wallpaper-source-form__textarea"
          value={prompt}
          placeholder={t("settings.wallpaperSource.imaginePlaceholder")}
          disabled={locked}
          rows={2}
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

  return (
    <div className="wallpaper-source-form">
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
