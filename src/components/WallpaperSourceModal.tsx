/** Wallpaper sources with progressive galleries, media preview, and apply. */

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { GlassModal } from "@/components/GlassModal";
import { WallpaperLibraryDeleteDialog } from "@/components/WallpaperLibraryDeleteDialog";
import { WallpaperSourceControls } from "@/components/WallpaperSourceControls";
import { WallpaperSourceFeedback } from "@/components/WallpaperSourceFeedback";
import { WallpaperSourceFooter } from "@/components/WallpaperSourceFooter";
import { WallpaperSourceGallery } from "@/components/WallpaperSourceGallery";
import { WallpaperSourceTabs } from "@/components/WallpaperSourceTabs";
import {
  useImageViewerOptional,
  type ImageSlideInput,
} from "@/components/ImageViewerContext";
import { useWallpaperGrokAlbum } from "@/hooks/useWallpaperGrokAlbum";
import { useWallpaperXSearch } from "@/hooks/useWallpaperXSearch";
import * as api from "@/lib/api";
import { isDesktopHost } from "@/lib/api";
import {
  dedupeGalleryItems,
  errorCodeFromSearchResult,
  fileFromAbsolutePath,
  libraryEntriesToGalleryItems,
  parseWallpaperSourceError,
  type WallpaperGalleryItem,
  type WallpaperSourceKind,
  type WallpaperSourceErrorCode,
} from "@/lib/wallpaperSource";
import {
  classifyWallpaperGalleryError,
  countGalleryByKind,
  filterGalleryItems,
  isWallpaperGallerySoftFail,
  resolveWallpaperGalleryEmptyState,
  wallpaperGalleryHasActiveFilters,
  type WallpaperGalleryKindFilter,
} from "@/lib/wallpaperGalleryPro";
import {
  countWallpaperXCitations,
  recordWallpaperXEvidencePick,
  wallpaperXEvidenceFromGalleryItem,
  wallpaperXSearchCitationSummaryKey,
} from "@/lib/xEvidenceCitation";
import { resolveGrokAlbumEmptyPresentation } from "@/lib/grokAlbum";
import { peekGrokAlbumThumbnail } from "@/lib/grokAlbumThumbnail";
import {
  cancelGrokAlbumMediaRequests,
  EMPTY_WALLPAPER_IMAGE_PLACEHOLDER,
  ensureLocalWallpaperMedia,
} from "@/lib/wallpaperSourceMedia";
import {
  wallpaperXSearchProgressMessageKey,
  wallpaperXSearchRouteSummary,
  type WallpaperXSearchMeta,
} from "@/lib/wallpaperXSearch";
import { WallpaperPrepareError } from "@/lib/themeSkin";
import type { MessageKey } from "@/i18n";

export type WallpaperSourceTab = WallpaperSourceKind;

export type WallpaperSourceModalProps = {
  open: boolean;
  onClose: () => void;
  initialTab?: WallpaperSourceTab;
  t: (
    key: MessageKey,
    vars?: Record<string, string | number | undefined | null>,
  ) => string;
  /** Apply prepared File via parent (prepareWallpaperFromFile + onWallpaper). */
  onPickFile: (file: File) => void | Promise<void>;
  /** Jump to Account settings when login is required. */
  onRequestLogin?: () => void;
};

function errorMessage(
  t: WallpaperSourceModalProps["t"],
  code: WallpaperSourceErrorCode,
): string {
  const key = `settings.wallpaperSource.err.${code}` as MessageKey;
  const msg = t(key);
  return msg === key ? t("settings.wallpaperSource.err.generic") : msg;
}

export function WallpaperSourceModal({
  open,
  onClose,
  initialTab = "x",
  t,
  onPickFile,
  onRequestLogin,
}: WallpaperSourceModalProps) {
  const viewer = useImageViewerOptional();
  const {
    busy: xSearchBusy,
    requestId: xSearchRequestId,
    stage: xSearchStage,
    progressiveItems: xProgressiveItems,
    search: searchX,
    loadMore: loadMoreX,
    cancel: cancelXSearch,
  } = useWallpaperXSearch();
  const [tab, setTab] = useState<WallpaperSourceTab>(initialTab);
  const grokAlbum = useWallpaperGrokAlbum(open && tab === "grok_album");
  const [query, setQuery] = useState("");
  const [sort, setSort] = useState<"top" | "latest">("top");
  const [prompt, setPrompt] = useState("");
  const [aspect, setAspect] = useState("16:9");
  const [items, setItems] = useState<WallpaperGalleryItem[]>([]);
  /** Client-side gallery filter (not the X search box). */
  const [galleryFilter, setGalleryFilter] = useState("");
  const [kindFilter, setKindFilter] =
    useState<WallpaperGalleryKindFilter>("all");
  /** True after at least one search/generate finished this open. */
  const [hasSearched, setHasSearched] = useState(false);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [sourceBusy, setSourceBusy] = useState(false);
  const [applying, setApplying] = useState(false);
  const [previewingId, setPreviewingId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [errorCode, setErrorCode] = useState<WallpaperSourceErrorCode | null>(
    null,
  );
  const [statusHint, setStatusHint] = useState<string | null>(null);
  /** Soft citation honesty after an X search (verified / unverified counts). */
  const [citeSummary, setCiteSummary] = useState<string | null>(null);
  const [routeMeta, setRouteMeta] = useState<WallpaperXSearchMeta | null>(null);
  const [loadMoreAttempted, setLoadMoreAttempted] = useState(false);
  const responseContinuationRef = useRef<{
    query: string;
    sort: "top" | "latest";
  } | null>(null);
  const progressiveRequestRef = useRef<string | null>(null);
  const appliedProgressiveKeysRef = useRef<Set<string>>(new Set());
  const sourceGenerationRef = useRef(0);
  const busy =
    sourceBusy || xSearchBusy || (tab === "grok_album" && grokAlbum.busy);

  useEffect(() => {
    sourceGenerationRef.current += 1;
    if (!open) return;
    setTab(initialTab);
    setError(null);
    setErrorCode(null);
    setStatusHint(null);
    setCiteSummary(null);
    setRouteMeta(null);
    setLoadMoreAttempted(false);
    responseContinuationRef.current = null;
    setSelectedId(null);
    setPreviewingId(null);
    setGalleryFilter("");
    setKindFilter(initialTab === "library" ? "image" : "all");
    setHasSearched(false);
    setItems([]);
  }, [open, initialTab]);

  useEffect(() => {
    if (!open || tab !== "x") {
      void cancelXSearch();
    }
  }, [open, tab, cancelXSearch]);

  useEffect(() => {
    if (!open || tab !== "grok_album") return;
    setItems((current) => {
      const previous = new Map(current.map((item) => [item.id, item]));
      return grokAlbum.items.map((item) => {
        const saved = previous.get(item.id);
        return saved?.localPath
          ? { ...item, localPath: saved.localPath }
          : item;
      });
    });
    setHasSearched(grokAlbum.hasSynced);
    setSelectedId((current) =>
      current && grokAlbum.items.some((item) => item.id === current)
        ? current
        : null,
    );
  }, [open, tab, grokAlbum.items, grokAlbum.hasSynced]);

  useEffect(() => {
    if (!open || tab !== "x" || !xSearchBusy || !xSearchRequestId) return;
    if (progressiveRequestRef.current !== xSearchRequestId) {
      progressiveRequestRef.current = xSearchRequestId;
      appliedProgressiveKeysRef.current.clear();
    }

    const fresh = xProgressiveItems.filter((item) => {
      const key = (item.localPath || item.fullUrl || item.id).trim();
      if (!key || appliedProgressiveKeysRef.current.has(key)) return false;
      appliedProgressiveKeysRef.current.add(key);
      return true;
    });
    if (fresh.length === 0) return;

    setItems((current) => dedupeGalleryItems([...current, ...fresh]));
    setHasSearched(true);
  }, [
    open,
    tab,
    xSearchBusy,
    xSearchRequestId,
    xProgressiveItems,
  ]);

  const kindCounts = useMemo(() => countGalleryByKind(items), [items]);

  const visibleItems = useMemo(
    () =>
      filterGalleryItems(items, {
        query: galleryFilter,
        kind: kindFilter,
      }),
    [items, galleryFilter, kindFilter],
  );

  const filtersActive = wallpaperGalleryHasActiveFilters({
    query: galleryFilter,
    kind: kindFilter,
  });

  const emptyState = useMemo(() => {
    const base = resolveWallpaperGalleryEmptyState({
      loading: busy,
      query: galleryFilter,
      itemCount: visibleItems.length,
      error: errorCode
        ? { code: errorCode, message: error ?? errorCode }
        : error,
      totalCount: items.length,
      kindFilter,
      hasSearched,
    });
    if (!base) return null;
    // Library tab: honest idle / empty copy (disk cache, not X/Imagine search).
    if (tab === "library" && (base.kind === "idle" || base.kind === "empty")) {
      return {
        ...base,
        titleKey:
          base.kind === "empty"
            ? "settings.wallpaperSource.empty.noResults"
            : "settings.wallpaperSource.emptyGallery",
        hintKey: "settings.wallpaperSource.empty.libraryIdleHint",
      };
    }
    if (
      tab === "grok_album" &&
      (base.kind === "idle" || base.kind === "empty")
    ) {
      return resolveGrokAlbumEmptyPresentation(base, grokAlbum.status);
    }
    return base;
  }, [
    busy,
    galleryFilter,
    visibleItems.length,
    errorCode,
    error,
    items.length,
    kindFilter,
    hasSearched,
    tab,
    grokAlbum.status,
  ]);

  const galleryErrorKind = useMemo(() => {
    if (!errorCode && !error) return null;
    // Prefer structured code + detail so desktop-only / free-text still classify
    // (e.g. generic + "desktop app" → host soft-fail).
    return classifyWallpaperGalleryError(
      errorCode
        ? { code: errorCode, message: error ?? errorCode }
        : error,
    );
  }, [errorCode, error]);

  const clearGalleryFilters = useCallback(() => {
    setGalleryFilter("");
    setKindFilter("all");
  }, []);

  const switchTab = useCallback((next: WallpaperSourceTab) => {
    sourceGenerationRef.current += 1;
    cancelGrokAlbumMediaRequests();
    setTab(next);
    setItems([]);
    setHasSearched(false);
    setSelectedId(null);
    setError(null);
    setErrorCode(null);
    setStatusHint(null);
    setCiteSummary(null);
    setRouteMeta(null);
    setLoadMoreAttempted(false);
    setGalleryFilter("");
    setKindFilter(next === "library" ? "image" : "all");
    responseContinuationRef.current = null;
    progressiveRequestRef.current = null;
    appliedProgressiveKeysRef.current.clear();
  }, []);

  const selected = useMemo(
    () => visibleItems.find((i) => i.id === selectedId) ?? null,
    [visibleItems, selectedId],
  );

  const sortOptions = useMemo(
    () => [
      { value: "top", label: t("settings.wallpaperSource.sortTop") },
      { value: "latest", label: t("settings.wallpaperSource.sortLatest") },
    ],
    [t],
  );

  const routeStatus = useMemo(() => {
    const summary = wallpaperXSearchRouteSummary(routeMeta);
    if (!summary) return null;
    const route = t(summary.key as MessageKey, {
      seconds: summary.seconds,
      responsesSeconds: summary.responsesSeconds,
      cliSeconds: summary.cliSeconds,
      reason: summary.reasonKey
        ? t(summary.reasonKey as MessageKey)
        : undefined,
    });
    return summary.cacheHit
      ? t("settings.wallpaperSource.route.cached", { route })
      : route;
  }, [routeMeta, t]);

  const xProgressStatus = useMemo(() => {
    if (!xSearchBusy) return null;
    const key = wallpaperXSearchProgressMessageKey(xSearchStage);
    return key ? t(key as MessageKey) : null;
  }, [xSearchBusy, xSearchStage, t]);

  const updateXCitationSummary = useCallback(
    (
      list: WallpaperGalleryItem[],
      resultErrorCode?: WallpaperSourceErrorCode | null,
    ) => {
      const counts = countWallpaperXCitations(list);
      const key = wallpaperXSearchCitationSummaryKey({
        itemCount: counts.total,
        verified: counts.verified,
        unverified: counts.unverified,
        errorCode: resultErrorCode ?? undefined,
      });
      setCiteSummary(
        key
          ? t(key as MessageKey, {
              verified: counts.verified,
              unverified: counts.unverified,
            })
          : null,
      );
    },
    [t],
  );

  const aspectOptions = useMemo(
    () => [
      { value: "16:9", label: "16:9" },
      { value: "9:16", label: "9:16" },
      { value: "1:1", label: "1:1" },
      { value: "4:3", label: "4:3" },
      { value: "auto", label: "auto" },
    ],
    [],
  );

  const runXSearch = useCallback(async () => {
    const q = query.trim();
    if (!q) {
      setErrorCode("empty");
      setError(errorMessage(t, "empty"));
      setCiteSummary(null);
      setRouteMeta(null);
      return;
    }
    if (!isDesktopHost()) {
      setErrorCode("generic");
      setError(t("settings.wallpaperSource.err.desktopOnly"));
      setCiteSummary(null);
      setRouteMeta(null);
      return;
    }
    setError(null);
    setErrorCode(null);
    setCiteSummary(null);
    setRouteMeta(null);
    setStatusHint(null);
    setLoadMoreAttempted(false);
    responseContinuationRef.current = null;
    setSelectedId(null);
    setGalleryFilter("");
    setKindFilter("all");
    setHasSearched(false);
    setItems([]);
    progressiveRequestRef.current = null;
    appliedProgressiveKeysRef.current.clear();
    try {
      const res = await searchX(q, sort);
      if (!res) return;
      setRouteMeta(res.meta ?? null);
      const list = dedupeGalleryItems(res.items || []);
      const code = errorCodeFromSearchResult({ ...res, items: list });
      setHasSearched(true);
      if (code) {
        responseContinuationRef.current = null;
        // Honest empty/error — never invent CDN gallery cards
        setItems([]);
        setErrorCode(code);
        setError(errorMessage(t, code));
        updateXCitationSummary([], code);
      } else {
        responseContinuationRef.current =
          res.meta?.routeUsed === "responses" ? { query: q, sort } : null;
        setItems(list);
        setError(null);
        setErrorCode(null);
        updateXCitationSummary(list);
      }
    } catch (e) {
      setHasSearched(true);
      // Keep already validated batches visible if the final invoke transport
      // fails. A normal lane failure is represented by a successful partial
      // Host result and is reconciled by the authoritative list above.
      setCiteSummary(null);
      setRouteMeta(null);
      responseContinuationRef.current = null;
      const code = parseWallpaperSourceError(e);
      setErrorCode(code);
      setError(errorMessage(t, code));
    }
  }, [query, sort, t, searchX, updateXCitationSummary]);

  const runXLoadMore = useCallback(async () => {
    const continuation = responseContinuationRef.current;
    if (
      !continuation ||
      loadMoreAttempted ||
      routeMeta?.routeUsed !== "responses"
    ) {
      return;
    }

    const initialItems = items;
    setLoadMoreAttempted(true);
    setError(null);
    setErrorCode(null);
    setStatusHint(null);
    progressiveRequestRef.current = null;
    appliedProgressiveKeysRef.current.clear();
    try {
      const res = await loadMoreX(continuation.query, continuation.sort);
      if (!res) {
        setLoadMoreAttempted(false);
        return;
      }
      const extra = dedupeGalleryItems(res.items || []);
      const code = errorCodeFromSearchResult({ ...res, items: extra });
      if (code) {
        if (code === "empty") {
          setStatusHint(t("settings.wallpaperSource.noMore"));
        } else {
          setErrorCode(code);
          setError(errorMessage(t, code));
        }
        return;
      }

      const merged = dedupeGalleryItems([...initialItems, ...extra]);
      setItems(merged);
      setError(null);
      setErrorCode(null);
      updateXCitationSummary(merged);
    } catch (e) {
      const code = parseWallpaperSourceError(e);
      setErrorCode(code);
      setError(errorMessage(t, code));
    }
  }, [
    items,
    routeMeta,
    loadMoreAttempted,
    loadMoreX,
    t,
    updateXCitationSummary,
  ]);

  const runImagine = useCallback(async () => {
    const p = prompt.trim();
    if (!p) {
      setErrorCode("empty");
      setError(errorMessage(t, "empty"));
      return;
    }
    if (!isDesktopHost()) {
      setErrorCode("generic");
      setError(t("settings.wallpaperSource.err.desktopOnly"));
      return;
    }
    setSourceBusy(true);
    setError(null);
    setErrorCode(null);
    setCiteSummary(null);
    setStatusHint(t("settings.wallpaperSource.generating"));
    setSelectedId(null);
    setGalleryFilter("");
    setKindFilter("all");
    try {
      const res = await api.wallpaperImagine(p, aspect);
      const list = dedupeGalleryItems(res.items || []);
      const code = errorCodeFromSearchResult({ ...res, items: list });
      setHasSearched(true);
      if (code) {
        setItems([]);
        setErrorCode(code);
        setError(errorMessage(t, code));
      } else {
        setItems(list);
        setError(null);
        setErrorCode(null);
        if (list[0]) setSelectedId(list[0].id);
      }
    } catch (e) {
      setHasSearched(true);
      setItems([]);
      const code = parseWallpaperSourceError(e);
      setErrorCode(code);
      setError(errorMessage(t, code));
    } finally {
      setSourceBusy(false);
      setStatusHint(null);
    }
  }, [prompt, aspect, t]);

  const dropItem = useCallback((id: string) => {
    setItems((prev) => prev.filter((it) => it.id !== id));
    setSelectedId((cur) => (cur === id ? null : cur));
  }, []);

  const loadLibrary = useCallback(async () => {
    if (!isDesktopHost()) {
      setErrorCode("generic");
      setError(t("settings.wallpaperSource.err.desktopOnly"));
      return;
    }
    setSourceBusy(true);
    setError(null);
    setErrorCode(null);
    setStatusHint(t("settings.wallpaperSource.libraryLoading"));
    setSelectedId(null);
    setGalleryFilter("");
    // Static first: default kind chip to images (video still available via chip).
    setKindFilter("image");
    try {
      const entries = await api.wallpaperLibraryList(96);
      const list = libraryEntriesToGalleryItems(entries, { staticFirst: true });
      setHasSearched(true);
      setItems(list);
      setError(null);
      setErrorCode(null);
    } catch (e) {
      setHasSearched(true);
      setItems([]);
      const code = parseWallpaperSourceError(e);
      setErrorCode(code);
      setError(errorMessage(t, code));
    } finally {
      setSourceBusy(false);
      setStatusHint(null);
    }
  }, [t]);

  useEffect(() => {
    if (!open || tab !== "library") return;
    void loadLibrary();
    // loadLibrary is stable on `t`; re-run when opening library tab.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open, tab]);

  /**
   * Soft-delete a library file. Failures keep the card and show a soft warn —
   * never invent a successful delete.
   */
  const [deleteConfirm, setDeleteConfirm] = useState<WallpaperGalleryItem | null>(
    null,
  );

  const requestDeleteLibraryItem = useCallback(
    (item: WallpaperGalleryItem, ev?: { preventDefault(): void; stopPropagation(): void }) => {
      ev?.preventDefault();
      ev?.stopPropagation();
      if (busy || applying || previewingId) return;
      setDeleteConfirm(item);
    },
    [busy, applying, previewingId],
  );

  const deleteLibraryItem = useCallback(
    async (item: WallpaperGalleryItem) => {
      const path = item.localPath?.trim();
      if (!path) return;
      if (!isDesktopHost()) {
        setErrorCode("generic");
        setError(t("settings.wallpaperSource.err.desktopOnly"));
        return;
      }
      setStatusHint(t("settings.wallpaperSource.deleting"));
      try {
        await api.wallpaperLibraryDelete(path);
        dropItem(item.id);
        setError(null);
        setErrorCode(null);
      } catch (e) {
        const code = parseWallpaperSourceError(e);
        setErrorCode(code);
        setError(
          code === "url_blocked"
            ? t("settings.wallpaperSource.err.deleteDenied")
            : t("settings.wallpaperSource.err.deleteFailed"),
        );
      } finally {
        setStatusHint(null);
      }
    },
    [busy, applying, previewingId, t, dropItem],
  );

  /**
   * Click card: load original into library if needed, open media viewer, mark selected.
   * User then confirms with footer "Set as background".
   */
  const openItemPreview = useCallback(
    async (item: WallpaperGalleryItem) => {
      if (busy || applying || previewingId) return;
      if (!isDesktopHost()) {
        setErrorCode("generic");
        setError(t("settings.wallpaperSource.err.desktopOnly"));
        return;
      }
      const sourceGeneration = sourceGenerationRef.current;
      setSelectedId(item.id);
      setPreviewingId(item.id);
      setError(null);
      setErrorCode(null);
      const lazyAlbumPreview = item.source === "grok_album";
      if (!lazyAlbumPreview) {
        setStatusHint(t("settings.wallpaperSource.loadingOriginal"));
      }
      try {
        // X/Imagine keep their established eager local download. Grok album
        // opens from its bounded in-memory thumbnail and upgrades the selected
        // slide in the viewer, so the first visual response is immediate.
        const local = lazyAlbumPreview
          ? null
          : await ensureLocalWallpaperMedia(item);
        if (sourceGeneration !== sourceGenerationRef.current) return;
        if (local) {
          setItems((prev) =>
            prev.map((it) =>
              it.id === item.id
                ? {
                    ...it,
                    localPath: local.path,
                    fullUrl: it.fullUrl.startsWith("http")
                      ? it.fullUrl
                      : `file://${local.path}`,
                  }
                : it,
            ),
          );
        }

        // Only open downloadable / already-local siblings in the lightbox
        // (visible set only — never invent off-filter CDN cards).
        const viable = visibleItems.filter(
          (it) => it.id === item.id || it.localPath || it.fullUrl.startsWith("http"),
        );
        const slides: ImageSlideInput[] = viable.map((it) => {
          const lazyAlbumOriginal =
            it.source === "grok_album" &&
            !it.localPath &&
            it.fullUrl.startsWith("http");
          if (lazyAlbumOriginal) {
            const thumbnail =
              peekGrokAlbumThumbnail(it.thumbUrl || it.fullUrl) ||
              EMPTY_WALLPAPER_IMAGE_PLACEHOLDER;
            return {
              src: thumbnail,
              kind: "image",
              title:
                it.textPreview ||
                it.prompt ||
                (it.username ? `@${it.username}` : undefined),
              alt: it.prompt || it.textPreview || undefined,
              onView: () => setSelectedId(it.id),
              loadOriginal: async () => {
                try {
                  const loaded = await ensureLocalWallpaperMedia(it);
                  if (sourceGeneration !== sourceGenerationRef.current) {
                    return null;
                  }
                  setItems((previous) =>
                    previous.map((candidate) =>
                      candidate.id === it.id
                        ? {
                            ...candidate,
                            localPath: loaded.path,
                            fullUrl: candidate.fullUrl.startsWith("http")
                              ? candidate.fullUrl
                              : `file://${loaded.path}`,
                          }
                        : candidate,
                    ),
                  );
                  return {
                    src: loaded.path,
                    kind: it.kind === "video" ? "video" : "image",
                    mime: loaded.mime,
                    poster: it.kind === "video" ? thumbnail : undefined,
                  };
                } catch (e) {
                  if (sourceGeneration !== sourceGenerationRef.current) {
                    return null;
                  }
                  const code = parseWallpaperSourceError(e);
                  setErrorCode(code);
                  setError(errorMessage(t, code));
                  throw e;
                }
              },
            };
          }
          const path =
            it.id === item.id && local
              ? local.path
              : it.localPath ||
                (it.fullUrl.startsWith("file://")
                  ? decodeURIComponent(it.fullUrl.replace(/^file:\/\//, ""))
                  : it.fullUrl);
          return {
            src: path,
            kind: it.kind === "video" ? "video" : "image",
            mime: it.id === item.id ? local?.mime : undefined,
            title:
              it.textPreview ||
              it.prompt ||
              (it.username ? `@${it.username}` : undefined),
            alt: it.prompt || it.textPreview || undefined,
            onView: () => setSelectedId(it.id),
          };
        });
        const idx = Math.max(
          0,
          viable.findIndex((it) => it.id === item.id),
        );
        viewer.open(slides, idx);
      } catch (e) {
        if (sourceGeneration !== sourceGenerationRef.current) return;
        // Eager X/Imagine failures represent broken cards. Album thumbnails
        // remain useful even when an original route is temporarily unavailable.
        if (!lazyAlbumPreview) dropItem(item.id);
        const code = parseWallpaperSourceError(e);
        setErrorCode(code);
        setError(errorMessage(t, code));
      } finally {
        if (sourceGeneration === sourceGenerationRef.current) {
          setPreviewingId(null);
          setStatusHint(null);
        }
      }
    },
    [busy, applying, previewingId, visibleItems, t, viewer, dropItem],
  );

  const openXStatus = useCallback((url: string) => {
    void api.openExternalUrl(url).catch(() => {
      /* soft-fail: citation open is best-effort */
    });
  }, []);

  const applySelected = useCallback(async () => {
    if (!selected) return;
    if (!isDesktopHost()) {
      setErrorCode("generic");
      setError(t("settings.wallpaperSource.err.desktopOnly"));
      return;
    }
    setApplying(true);
    setError(null);
    setErrorCode(null);
    setStatusHint(t("settings.wallpaperSource.applying"));
    try {
      const local = await ensureLocalWallpaperMedia(selected);
      // Local evidence ring for X picks only (path + status url meta; no cloud).
      if ((selected.source || "x") === "x") {
        const pick = wallpaperXEvidenceFromGalleryItem(selected, local.path);
        if (pick) recordWallpaperXEvidencePick(pick);
      }
      const file = await fileFromAbsolutePath(local.path, {
        name: local.name,
        mime: local.mime,
      });
      await onPickFile(file);
      onClose();
    } catch (e) {
      // prepareWallpaperFromFile errors use settings.wallpaper.err.* keys
      if (e instanceof WallpaperPrepareError) {
        const key = `settings.wallpaper.err.${e.code}` as MessageKey;
        const msg = t(key);
        setErrorCode("generic");
        setError(msg === key ? t("settings.wallpaper.err.generic") : msg);
        return;
      }
      const code = parseWallpaperSourceError(e);
      setErrorCode(code);
      setError(errorMessage(t, code));
    } finally {
      setApplying(false);
      setStatusHint(null);
    }
  }, [selected, t, onPickFile, onClose]);

  const closeModal = useCallback(() => {
    sourceGenerationRef.current += 1;
    cancelGrokAlbumMediaRequests();
    if (xSearchBusy) void cancelXSearch();
    onClose();
  }, [xSearchBusy, cancelXSearch, onClose]);

  const authNeeded = errorCode === "auth_required";
  const locked = busy || applying || previewingId !== null;
  const tabSwitchLocked =
    applying ||
    previewingId !== null ||
    (sourceBusy && !xSearchBusy) ||
    (tab === "grok_album" && grokAlbum.busy);
  const canLoadMore =
    (tab === "x" &&
      !busy &&
      !loadMoreAttempted &&
      routeMeta?.routeUsed === "responses" &&
      responseContinuationRef.current !== null &&
      items.length > 0) ||
    (tab === "grok_album" &&
      (!busy || grokAlbum.loadingMore) &&
      grokAlbum.canLoadMore &&
      items.length > 0);
  const showGalleryFilters = items.length > 0 || filtersActive;
  const softFailError =
    galleryErrorKind != null && isWallpaperGallerySoftFail(galleryErrorKind);
  // Error banner already carries detail for empty/error — avoid stacking the
  // same honesty block; still show loading / idle / filter-empty surfaces.
  const showEmptyBlock =
    emptyState != null &&
    (emptyState.kind === "loading" ||
      emptyState.kind === "idle" ||
      emptyState.kind === "filter_empty" ||
      !error);

  return (
    <>
    <GlassModal
      open={open}
      onClose={closeModal}
      title={t("settings.wallpaperSource.title")}
      size="lg"
      className="wallpaper-source-modal"
      wrapBody
      bodyClassName="wallpaper-source-modal__body"
      closeLabel={t("common.close")}
      footer={
        <WallpaperSourceFooter
          t={t}
          hasSelection={selected !== null}
          applying={applying}
          applyDisabled={!selected || locked}
          onClose={closeModal}
          onApply={() => void applySelected()}
        />
      }
    >
      <WallpaperSourceTabs
        t={t}
        value={tab}
        disabled={tabSwitchLocked}
        onChange={switchTab}
      />

      <WallpaperSourceControls
        t={t}
        tab={tab}
        locked={locked}
        busy={busy}
        xSearchBusy={xSearchBusy}
        query={query}
        sort={sort}
        sortOptions={sortOptions}
        prompt={prompt}
        aspect={aspect}
        aspectOptions={aspectOptions}
        albumStatus={grokAlbum.status}
        albumCachedCount={grokAlbum.cachedCount}
        albumVisibleCount={grokAlbum.visibleCount}
        albumErrorCode={grokAlbum.errorCode}
        albumSyncing={grokAlbum.syncing}
        onQueryChange={setQuery}
        onSortChange={setSort}
        onSearchX={() => void runXSearch()}
        onCancelX={() => void cancelXSearch()}
        onPromptChange={setPrompt}
        onAspectChange={setAspect}
        onGenerate={() => void runImagine()}
        onOpenAlbum={() =>
          void grokAlbum.open(t("settings.wallpaperGrokAlbum"))
        }
        onSyncAlbum={() => void grokAlbum.sync()}
        onRefreshAlbum={() => void grokAlbum.refresh()}
        onRefreshLibrary={() => void loadLibrary()}
      />

      <WallpaperSourceFeedback
        t={t}
        tab={tab}
        progress={xProgressStatus || statusHint}
        routeStatus={routeStatus}
        citationSummary={citeSummary}
        error={error}
        errorKind={galleryErrorKind}
        softFail={softFailError}
        authNeeded={authNeeded}
        onRequestLogin={onRequestLogin}
      />

      <WallpaperSourceGallery
        t={t}
        tab={tab}
        visibleItems={visibleItems}
        selectedId={selectedId}
        previewingId={previewingId}
        locked={locked}
        busy={busy}
        kindCounts={kindCounts}
        kindFilter={kindFilter}
        galleryFilter={galleryFilter}
        filtersActive={filtersActive}
        showFilters={showGalleryFilters}
        emptyState={emptyState}
        showEmptyBlock={showEmptyBlock}
        canLoadMore={canLoadMore}
        loadingMore={tab === "grok_album" && grokAlbum.loadingMore}
        onKindFilterChange={setKindFilter}
        onGalleryFilterChange={setGalleryFilter}
        onClearFilters={clearGalleryFilters}
        onPreview={(item) => void openItemPreview(item)}
        onDropItem={dropItem}
        onOpenXStatus={openXStatus}
        onDeleteLibraryItem={requestDeleteLibraryItem}
        onLoadMore={() =>
          void (tab === "grok_album" ? grokAlbum.loadMore() : runXLoadMore())
        }
      />
    </GlassModal>
    <WallpaperLibraryDeleteDialog
      t={t}
      item={deleteConfirm}
      onClose={() => setDeleteConfirm(null)}
      onConfirm={(item) => {
        setDeleteConfirm(null);
        void deleteLibraryItem(item);
      }}
    />
    </>
  );
}
