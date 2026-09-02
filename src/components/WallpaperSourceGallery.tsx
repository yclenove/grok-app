import { GrokAlbumThumbnail } from "@/components/GrokAlbumThumbnail";
import { WallpaperSourceAttribution } from "@/components/WallpaperSourceAttribution";
import type { MessageKey } from "@/i18n";
import { resolveImageSrcSync } from "@/lib/imageSrc";
import type {
  WallpaperGalleryItem,
  WallpaperSourceKind,
} from "@/lib/wallpaperSource";
import {
  wallpaperGalleryKindFilterLabelKey,
  WALLPAPER_GALLERY_KIND_FILTERS,
  type WallpaperGalleryEmptyPresentation,
  type WallpaperGalleryKindCounts,
  type WallpaperGalleryKindFilter,
} from "@/lib/wallpaperGalleryPro";
import { resolveWallpaperXCitation } from "@/lib/xEvidenceCitation";

type Translate = (
  key: MessageKey,
  vars?: Record<string, string | number | undefined | null>,
) => string;

export type WallpaperSourceGalleryProps = {
  t: Translate;
  tab: WallpaperSourceKind;
  visibleItems: WallpaperGalleryItem[];
  selectedId: string | null;
  previewingId: string | null;
  locked: boolean;
  busy: boolean;
  kindCounts: WallpaperGalleryKindCounts;
  kindFilter: WallpaperGalleryKindFilter;
  galleryFilter: string;
  filtersActive: boolean;
  showFilters: boolean;
  showTextFilter: boolean;
  emptyState: WallpaperGalleryEmptyPresentation | null;
  showEmptyBlock: boolean;
  canLoadMore: boolean;
  loadingMore: boolean;
  onKindFilterChange: (value: WallpaperGalleryKindFilter) => void;
  onGalleryFilterChange: (value: string) => void;
  onClearFilters: () => void;
  onPreview: (item: WallpaperGalleryItem) => void;
  onDropItem: (id: string) => void;
  onOpenXStatus: (url: string) => void;
  onOpenSource: (url: string) => void;
  onDeleteLibraryItem: (
    item: WallpaperGalleryItem,
    event: { preventDefault(): void; stopPropagation(): void },
  ) => void;
  onLoadMore: () => void;
};

function itemThumbSrc(item: WallpaperGalleryItem): string {
  if (item.localPath) {
    return resolveImageSrcSync(item.localPath) || item.thumbUrl || item.fullUrl;
  }
  if (item.fullUrl.startsWith("file://")) {
    const path = decodeURIComponent(item.fullUrl.replace(/^file:\/\//, ""));
    return resolveImageSrcSync(path) || item.thumbUrl || item.fullUrl;
  }
  return item.thumbUrl || item.fullUrl;
}

function sourceLabelKey(
  item: WallpaperGalleryItem,
  library: boolean,
): MessageKey | null {
  if (!library) return null;
  if (item.source === "imagine") return "settings.wallpaperImagine";
  if (item.source === "grok_album") return "settings.wallpaperGrokAlbum";
  if (item.source === "x") return "settings.wallpaperFromX";
  if (item.source === "web") return "settings.wallpaperWeb";
  if (item.source === "openverse") return "settings.wallpaperOpenverse";
  if (item.source === "pexels") return "settings.wallpaperPexels";
  return "settings.wallpaperLibrary";
}

export function WallpaperSourceGallery({
  t,
  tab,
  visibleItems,
  selectedId,
  previewingId,
  locked,
  busy,
  kindCounts,
  kindFilter,
  galleryFilter,
  filtersActive,
  showFilters,
  showTextFilter,
  emptyState,
  showEmptyBlock,
  canLoadMore,
  loadingMore,
  onKindFilterChange,
  onGalleryFilterChange,
  onClearFilters,
  onPreview,
  onDropItem,
  onOpenXStatus,
  onOpenSource,
  onDeleteLibraryItem,
  onLoadMore,
}: WallpaperSourceGalleryProps) {
  const library = tab === "library";
  const imagineLayout = tab === "imagine";
  const stableAppendLayout =
    tab === "grok_album" ||
    tab === "x" ||
    tab === "web" ||
    tab === "openverse" ||
    tab === "pexels";
  const showKindFilters =
    kindFilter !== "all" || (kindCounts.image > 0 && kindCounts.video > 0);

  return (
    <>
      {showFilters ? (
        <div
          className={
            "wallpaper-source-filters" +
            (!showKindFilters ? " wallpaper-source-filters--query-only" : "")
          }
        >
          {showKindFilters ? (
            <div
              className="wallpaper-source-chips"
              role="toolbar"
              aria-label={t("settings.wallpaperSource.kindLabel")}
            >
              {WALLPAPER_GALLERY_KIND_FILTERS.map((id) => {
                const count = kindCounts[id];
                if (id !== "all" && count === 0 && kindFilter !== id) return null;
                return (
                  <button
                    key={id}
                    type="button"
                    className={
                      "wallpaper-source-chip" +
                      (kindFilter === id ? " is-active" : "")
                    }
                    aria-pressed={kindFilter === id}
                    disabled={locked && id !== kindFilter}
                    onClick={() => onKindFilterChange(id)}
                  >
                    <span>{t(wallpaperGalleryKindFilterLabelKey(id) as MessageKey)}</span>
                    <span className="wallpaper-source-chip-count">{count}</span>
                  </button>
                );
              })}
            </div>
          ) : null}
          {showTextFilter ? (
            <input
              type="search"
              className="wallpaper-source-form__input wallpaper-source-filters__query"
              value={galleryFilter}
              placeholder={t("settings.wallpaperSource.filterPlaceholder")}
              disabled={locked}
              onChange={(event) => onGalleryFilterChange(event.target.value)}
              autoComplete="off"
              spellCheck={false}
              aria-label={t("settings.wallpaperSource.filterPlaceholder")}
            />
          ) : null}
          {filtersActive ? (
            <button
              type="button"
              className="btn btn--ghost btn--sm"
              onClick={onClearFilters}
              disabled={locked}
            >
              {t("settings.wallpaperSource.clearFilters")}
            </button>
          ) : null}
        </div>
      ) : null}

      <div
        className="wallpaper-masonry-scroll"
        role="list"
        aria-label={t("settings.wallpaperSource.gallery")}
        aria-busy={busy || previewingId !== null}
        tabIndex={visibleItems.length > 0 ? 0 : undefined}
      >
        <div
          className={
            "wallpaper-masonry" +
            (imagineLayout ? " wallpaper-masonry--full" : "") +
            (stableAppendLayout ? " wallpaper-masonry--stable" : "")
          }
        >
          {showEmptyBlock && emptyState ? (
            <div
              className={
                "wallpaper-masonry__empty" +
                (emptyState.kind === "filter_empty"
                  ? " wallpaper-masonry__empty--filter"
                  : "") +
                (emptyState.kind === "error"
                  ? " wallpaper-masonry__empty--error"
                  : "")
              }
              data-kind={emptyState.kind}
              data-soft-fail={emptyState.softFail ? "1" : "0"}
            >
              <p className="wallpaper-masonry__empty-title">
                {t(emptyState.titleKey as MessageKey)}
              </p>
              {emptyState.showClearFilters && !showFilters ? (
                <button
                  type="button"
                  className="btn btn--ghost btn--sm"
                  onClick={onClearFilters}
                >
                  {t("settings.wallpaperSource.clearFilters")}
                </button>
              ) : null}
            </div>
          ) : null}

          {visibleItems.map((item) => {
            const active = item.id === selectedId;
            const loading = previewingId === item.id;
            const localVideo =
              item.kind === "video" &&
              (!!item.localPath || item.fullUrl.startsWith("file://"));
            const citation =
              item.source === "x" || (!item.source && tab === "x")
                ? resolveWallpaperXCitation(item)
                : null;
            const labelKey = sourceLabelKey(item, library);
            const meta = loading
              ? t("settings.wallpaperSource.loadingOriginal")
              : [
                  item.username ? `@${item.username}` : null,
                  item.likes != null ? `♥ ${item.likes}` : null,
                  labelKey ? t(labelKey) : null,
                ]
                  .filter(Boolean)
                  .join(" · ");
            return (
              <div
                key={item.id}
                className={
                  "wallpaper-masonry__card-wrap" +
                  (library ? " wallpaper-masonry__card-wrap--library" : "")
                }
                role="listitem"
              >
                <button
                  type="button"
                  className={
                    "wallpaper-masonry__card" +
                    (active ? " wallpaper-masonry__card--selected" : "") +
                    (loading ? " wallpaper-masonry__card--loading" : "")
                  }
                  disabled={locked && !loading}
                  onClick={() => onPreview(item)}
                  aria-label={t("settings.wallpaperSource.openPreview")}
                >
                  <span className="wallpaper-masonry__media">
                    {item.source === "grok_album" && !library ? (
                      <GrokAlbumThumbnail
                        url={item.thumbUrl || item.fullUrl}
                        alt={item.textPreview || item.prompt || item.username || ""}
                        width={item.width}
                        height={item.height}
                      />
                    ) : localVideo ? (
                      <video
                        src={itemThumbSrc(item)}
                        className="wallpaper-masonry__img"
                        muted
                        playsInline
                        preload="metadata"
                        aria-hidden="true"
                        onError={() => onDropItem(item.id)}
                      />
                    ) : (
                      <img
                        src={itemThumbSrc(item)}
                        alt={item.textPreview || item.prompt || item.username || ""}
                        className="wallpaper-masonry__img"
                        loading="lazy"
                        referrerPolicy="no-referrer"
                        onError={() => onDropItem(item.id)}
                      />
                    )}
                  </span>
                  {meta ? (
                    <span className="wallpaper-masonry__meta">{meta}</span>
                  ) : null}
                </button>

                {citation && !loading ? (
                  <div
                    className={
                      "wallpaper-masonry__cite" +
                      (citation.state === "verified"
                        ? " wallpaper-masonry__cite--verified"
                        : " wallpaper-masonry__cite--unverified")
                    }
                    title={t(citation.hintKey as MessageKey)}
                  >
                    {citation.state === "verified" && citation.statusUrl ? (
                      <button
                        type="button"
                        className="wallpaper-masonry__cite-btn"
                        disabled={locked}
                        onClick={(event) => {
                          event.preventDefault();
                          event.stopPropagation();
                          onOpenXStatus(citation.statusUrl!);
                        }}
                        title={t("settings.wallpaperSource.cite.openPost")}
                        aria-label={t("settings.wallpaperSource.cite.openPost")}
                      >
                        {t(citation.labelKey as MessageKey)}
                      </button>
                    ) : (
                      <span className="wallpaper-masonry__cite-badge">
                        {t(citation.labelKey as MessageKey)}
                      </span>
                    )}
                  </div>
                ) : null}

                {!loading ? (
                  <WallpaperSourceAttribution
                    item={item}
                    t={t}
                    disabled={locked}
                    onOpen={onOpenSource}
                  />
                ) : null}

                {library ? (
                  <button
                    type="button"
                    className="wallpaper-masonry__delete"
                    disabled={locked}
                    onClick={(event) => onDeleteLibraryItem(item, event)}
                    aria-label={t("settings.wallpaperSource.delete")}
                    title={t("settings.wallpaperSource.delete")}
                  >
                    ×
                  </button>
                ) : null}
              </div>
            );
          })}
        </div>

        {canLoadMore ? (
          <div className="wallpaper-source-load-more">
            <button
              type="button"
              className="btn btn--ghost"
              disabled={locked || loadingMore}
              aria-busy={loadingMore}
              onClick={onLoadMore}
            >
              {t(
                loadingMore
                  ? "settings.wallpaperSource.loadingMore"
                  : "settings.wallpaperSource.loadMore",
              )}
            </button>
          </div>
        ) : null}
      </div>
    </>
  );
}
