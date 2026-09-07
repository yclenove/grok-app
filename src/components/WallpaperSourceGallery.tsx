import { GrokAlbumThumbnail } from "@/components/GrokAlbumThumbnail";
import { IconEdit, IconHeart, IconInfo, IconPlay, IconRefresh } from "@/components/icons";
import { WallpaperMediaDetails } from "@/components/WallpaperMediaDetails";
import { Select } from "@/components/Select";
import { forgetGrokAlbumThumbnail } from "@/lib/grokAlbumThumbnail";
import { forgetRemoteWallpaperThumbnail } from "@/lib/remoteWallpaperThumbnail";
import { RemoteWallpaperThumbnail } from "@/components/RemoteWallpaperThumbnail";
import { WallpaperSourceAttribution } from "@/components/WallpaperSourceAttribution";
import type { MessageKey } from "@/i18n";
import {
  ensureMediaEndpoint,
  resolveImageSrcSync,
} from "@/lib/imageSrc";
import { useCallback, useEffect, useState, type RefObject } from "react";
import type {
  WallpaperGalleryItem,
  WallpaperSourceKind,
  WallpaperLibraryPurpose,
} from "@/lib/wallpaperSource";
import {
  wallpaperGalleryKindFilterLabelKey,
  WALLPAPER_GALLERY_KIND_FILTERS,
  type WallpaperGalleryEmptyPresentation,
  type WallpaperGalleryKindCounts,
  type WallpaperGalleryKindFilter,
} from "@/lib/wallpaperGalleryPro";
import { resolveWallpaperXCitation } from "@/lib/xEvidenceCitation";
import { isWallpaperRemoteSource } from "@/lib/wallpaperRemoteSearch";
import { isWallpaperImageItem } from "@/lib/wallpaperImagine";

type Translate = (
  key: MessageKey,
  vars?: Record<string, string | number | undefined | null>,
) => string;

export type WallpaperSourceGalleryProps = {
  favoriteBusyIds?: ReadonlySet<string>;
  onToggleFavorite?: (item: WallpaperGalleryItem) => void;
  onReusePrompt?: (item: WallpaperGalleryItem) => void;
  libraryPurpose?: WallpaperLibraryPurpose;
  onLibraryPurposeChange?: (purpose: WallpaperLibraryPurpose) => void;
  onRefreshLibrary?: () => void;
  scrollRef?: RefObject<HTMLDivElement | null>;
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
  onGenerateVideo: (item: WallpaperGalleryItem) => void;
  onEditImage: (item: WallpaperGalleryItem) => void;
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

function itemActionAccessibleName(
  t: Translate,
  actionKey: MessageKey,
  item: WallpaperGalleryItem,
  index: number,
): string {
  const context =
    item.textPreview?.trim() ||
    item.prompt?.trim() ||
    item.username?.trim() ||
    item.sourceName?.trim() ||
    item.id.trim() ||
    String(index + 1);
  return `${t(actionKey)}: ${context}`;
}

export function WallpaperSourceGallery({
  scrollRef,
  favoriteBusyIds,
  onToggleFavorite,
  onReusePrompt,
  libraryPurpose = "all",
  onLibraryPurposeChange,
  onRefreshLibrary,
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
  onGenerateVideo,
  onEditImage,
  onOpenXStatus,
  onOpenSource,
  onDeleteLibraryItem,
  onLoadMore,
}: WallpaperSourceGalleryProps) {
  const [, refreshMediaSources] = useState(0);
  const [details, setDetails] = useState<{ tab: string; id: string } | null>(null);
  const detailsItem = details?.tab === tab
    ? visibleItems.find((item) => item.id === details.id) ?? null : null;
  useEffect(() => {
    setDetails((current) => current?.tab === tab && visibleItems.some((item) => item.id === current.id) ? current : null);
  }, [tab, visibleItems]);
  const [failedMedia, setFailedMedia] = useState<Set<string>>(new Set());
  const [mediaAttempts, setMediaAttempts] = useState<Record<string, number>>({});
  const markUnavailable = useCallback((id: string) => {
    setFailedMedia((previous) => previous.has(id) ? previous : new Set(previous).add(id));
  }, []);
  const retryMedia = (item: WallpaperGalleryItem) => {
    if (item.source === "grok_album") forgetGrokAlbumThumbnail(item.thumbUrl || item.fullUrl);
    if (isWallpaperRemoteSource(item.source)) forgetRemoteWallpaperThumbnail(item);
    setMediaAttempts((previous) => ({ ...previous, [item.id]: (previous[item.id] ?? 0) + 1 }));
    setFailedMedia((previous) => { const next = new Set(previous); next.delete(item.id); return next; });
  };

  useEffect(() => {
    const ids = new Set(visibleItems.map((item) => item.id));
    setFailedMedia((previous) => [...previous].every((id) => ids.has(id))
      ? previous : new Set([...previous].filter((id) => ids.has(id))));
    setMediaAttempts((previous) => Object.keys(previous).every((id) => ids.has(id))
      ? previous : Object.fromEntries(Object.entries(previous).filter(([id]) => ids.has(id))));
  }, [visibleItems]);

  useEffect(() => {
    let mounted = true;
    void ensureMediaEndpoint().then(() => {
      if (mounted) refreshMediaSources((value) => value + 1);
    });
    return () => {
      mounted = false;
    };
  }, []);

  const library = tab === "library";
  const imagineLayout = tab === "imagine";
  const stableAppendLayout =
    tab === "library" ||
    tab === "grok_album" ||
    tab === "x" ||
    tab === "web" ||
    tab === "openverse" ||
    tab === "pexels";
  const showKindFilters =
    kindFilter !== "all" || (kindCounts.image > 0 && kindCounts.video > 0);

  return (
    <>
      {showFilters || library ? (
        <div
          className={
            "wallpaper-source-filters" +
            (library ? " wallpaper-library-toolbar" : "") +
            (!showKindFilters ? " wallpaper-source-filters--query-only" : "")
          }
        >
          {library && onLibraryPurposeChange ? (
            <Select className="wallpaper-library-toolbar__collection" value={libraryPurpose}
              options={(["all", "favorites", "generated", "cache"] as const).map((value) => ({ value, label: t(`settings.wallpaperSource.library.${value}`) }))}
              onChange={(value) => onLibraryPurposeChange(value as WallpaperLibraryPurpose)}
              aria-label={t("settings.wallpaperSource.library.collection")} disabled={locked} />
          ) : null}
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
          {library && onRefreshLibrary ? (
            <button type="button" className="btn btn--ghost wallpaper-library-toolbar__refresh"
              onClick={onRefreshLibrary} disabled={locked || busy} aria-busy={busy}
              title={t("settings.wallpaperSource.libraryRefresh")}
              aria-label={t("settings.wallpaperSource.libraryRefresh")}>
              <IconRefresh size={16} />
            </button>
          ) : null}
        </div>
      ) : null}

      <div
        ref={scrollRef}
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

          {visibleItems.map((item, index) => {
            const active = item.id === selectedId;
            const loading = previewingId === item.id;
            const canGenerateVideo = isWallpaperImageItem(item);
            const localVideo =
              item.kind === "video" &&
              (!!item.localPath || item.fullUrl.startsWith("file://"));
            const citation =
              item.source === "x" || (!item.source && tab === "x")
                ? resolveWallpaperXCitation(item)
                : null;
            const labelKey = sourceLabelKey(item, library);
            const mediaAspectRatio =
              Number(item.width) > 0 && Number(item.height) > 0
                ? `${Number(item.width)} / ${Number(item.height)}`
                : undefined;
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
                <div
                  className={
                    "wallpaper-masonry__card" +
                    (active ? " wallpaper-masonry__card--selected" : "") +
                    (loading ? " wallpaper-masonry__card--loading" : "") +
                    (locked && !loading ? " wallpaper-masonry__card--locked" : "")
                  }
                >
                  <div
                    className="wallpaper-masonry__media-shell"
                    style={mediaAspectRatio ? { aspectRatio: mediaAspectRatio } : undefined}
                  >
                    <button
                      type="button"
                      className="wallpaper-masonry__preview"
                      disabled={locked && !loading}
                      onClick={() => onPreview(item)}
                      aria-pressed={active}
                      aria-label={itemActionAccessibleName(
                        t,
                        "settings.wallpaperSource.openPreview",
                        item,
                        index,
                      )}
                    >
                      <span className="wallpaper-masonry__media" key={mediaAttempts[item.id] ?? 0}>
                        {failedMedia.has(item.id) ? (
                          <span className="wallpaper-masonry__thumb-placeholder" data-state="failed" style={{ aspectRatio: mediaAspectRatio ?? "16 / 9" }} />
                        ) : item.source === "grok_album" && !library ? (
                          <GrokAlbumThumbnail
                            url={item.thumbUrl || item.fullUrl}
                            alt={item.textPreview || item.prompt || item.username || ""}
                            width={item.width}
                            height={item.height}
                            itemId={item.id}
                            onUnavailable={markUnavailable}
                          />
                        ) : !library &&
                          !item.localPath &&
                          isWallpaperRemoteSource(item.source) ? (
                          <RemoteWallpaperThumbnail
                            item={item}
                            alt={
                              item.textPreview ||
                              item.prompt ||
                              item.username ||
                              ""
                            }
                            onUnavailable={markUnavailable}
                          />
                        ) : localVideo ? (
                          <video
                            src={itemThumbSrc(item)}
                            className="wallpaper-masonry__img"
                            muted
                            playsInline
                            preload="metadata"
                            aria-hidden="true"
                            onError={() => markUnavailable(item.id)}
                          />
                        ) : (
                          <img
                            src={itemThumbSrc(item)}
                            alt={item.textPreview || item.prompt || item.username || ""}
                            className="wallpaper-masonry__img"
                            loading="lazy"
                            referrerPolicy="no-referrer"
                            onError={() => markUnavailable(item.id)}
                          />
                        )}
                      </span>
                    </button>
                    {failedMedia.has(item.id) ? (
                      <button type="button" className="wallpaper-masonry__video-action wallpaper-masonry__retry" disabled={locked} title={t("ui.errorBoundary.retry")} aria-label={itemActionAccessibleName(t, "ui.errorBoundary.retry", item, index)} onClick={(event) => { event.stopPropagation(); retryMedia(item); }}>
                        <IconRefresh size={18} />
                      </button>
                    ) : null}
                    {canGenerateVideo ? (
                      <>
                      <button
                        type="button"
                        className="wallpaper-masonry__video-action wallpaper-masonry__edit-action"
                        disabled={locked || loading}
                        aria-label={itemActionAccessibleName(t, "settings.wallpaperSource.editImage", item, index)}
                        title={t("settings.wallpaperSource.editImage")}
                        onClick={(event) => {
                          event.preventDefault();
                          event.stopPropagation();
                          onEditImage(item);
                        }}
                      >
                        <IconEdit size={16} />
                      </button>
                      <button
                        type="button"
                        className="wallpaper-masonry__video-action"
                        disabled={locked || loading}
                        aria-label={itemActionAccessibleName(
                          t,
                          "settings.wallpaperSource.generateVideoFromImage",
                          item,
                          index,
                        )}
                        title={t(
                          "settings.wallpaperSource.generateVideoFromImage",
                        )}
                        onClick={(event) => {
                          event.preventDefault();
                          event.stopPropagation();
                          onGenerateVideo(item);
                        }}
                      >
                        <IconPlay size={16} />
                      </button>
                      </>
                    ) : null}
                    {onToggleFavorite ? (
                      <button type="button" className="wallpaper-masonry__video-action wallpaper-masonry__favorite-action"
                        disabled={locked || favoriteBusyIds?.has(item.id)} aria-busy={favoriteBusyIds?.has(item.id)} aria-pressed={!!item.metadata?.favorite}
                        title={t(item.metadata?.favorite ? "settings.wallpaperSource.library.unfavorite" : "settings.wallpaperSource.library.favorite")}
                        aria-label={itemActionAccessibleName(t, item.metadata?.favorite ? "settings.wallpaperSource.library.unfavorite" : "settings.wallpaperSource.library.favorite", item, index)}
                        onClick={(event) => { event.stopPropagation(); onToggleFavorite(item); }}>
                        <IconHeart size={16} />
                      </button>
                    ) : null}
                  </div>
                  <div className="wallpaper-masonry__caption">
                    {meta ? <span className="wallpaper-masonry__meta">{meta}</span> : null}
                    <button type="button" className="btn btn--ghost btn--icon wallpaper-masonry__details"
                      disabled={locked} title={t("settings.wallpaperSource.details.title")}
                      aria-label={itemActionAccessibleName(t, "settings.wallpaperSource.details.title", item, index)}
                      onClick={() => setDetails({ tab, id: item.id })}>
                      <IconInfo size={15} />
                    </button>
                  </div>
                </div>

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
      <WallpaperMediaDetails item={detailsItem} t={t} locked={locked}
        onClose={() => setDetails(null)} onOpenSource={onOpenSource}
        onReusePrompt={onReusePrompt ? (item) => { setDetails(null); onReusePrompt(item); } : undefined} />
    </>
  );
}
