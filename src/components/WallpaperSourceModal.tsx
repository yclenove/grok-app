/** Wallpaper sources with progressive galleries, media preview, and apply. */

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { GlassModal } from "@/components/GlassModal";
import { WallpaperLibraryDeleteDialog } from "@/components/WallpaperLibraryDeleteDialog";
import { WallpaperPexelsKeyDeleteDialog } from "@/components/WallpaperPexelsKeyDeleteDialog";
import { WallpaperSourceControls } from "@/components/WallpaperSourceControls";
import { WallpaperSourceFeedback } from "@/components/WallpaperSourceFeedback";
import { WallpaperSourceFooter } from "@/components/WallpaperSourceFooter";
import { WallpaperSourceGallery } from "@/components/WallpaperSourceGallery";
import { WallpaperSourceTabs } from "@/components/WallpaperSourceTabs";
import type {
  WallpaperSourceModalProps,
  WallpaperSourceTab,
} from "@/components/WallpaperSourceModal.types";
import type { MessageKey } from "@/i18n";
import { useImageViewerOptional } from "@/components/ImageViewerContext";
import { useWallpaperGrokAlbum } from "@/hooks/useWallpaperGrokAlbum";
import { useWallpaperImagineController } from "@/hooks/useWallpaperImagineController";
import { useWallpaperItemPreview } from "@/hooks/useWallpaperItemPreview";
import { useWallpaperRemoteSourceController } from "@/hooks/useWallpaperRemoteSourceController";
import { useWallpaperXSearch } from "@/hooks/useWallpaperXSearch";
import * as api from "@/lib/api";
import { isDesktopHost } from "@/lib/api";
import {
  appendGalleryItems,
  dedupeGalleryItems,
  errorCodeFromSearchResult,
  libraryEntriesToGalleryItems,
  mergeAuthoritativeGalleryItems,
  parseWallpaperSourceError,
  type WallpaperGalleryItem,
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
import { resolveGrokAlbumEmptyPresentation } from "@/lib/grokAlbum";
import {
  cancelGrokAlbumMediaRequests,
  cancelRemoteWallpaperMediaRequests,
} from "@/lib/wallpaperSourceMedia";
import { prepareWallpaperSelection } from "@/lib/wallpaperApply";
import {
  normalizeWallpaperXSearchMode,
  wallpaperXSearchProgressMessageKey,
  type WallpaperXSearchMeta,
  type WallpaperXSearchMode,
} from "@/lib/wallpaperXSearch";
import { isWallpaperRemoteSource } from "@/lib/wallpaperRemoteSearch";
import { isWallpaperImageItem } from "@/lib/wallpaperImagine";
import { WallpaperPrepareError } from "@/lib/themeSkin";
import {
  wallpaperSourceErrorMessage as errorMessage,
  wallpaperSourceRouteStatus,
} from "@/lib/wallpaperSourcePresentation";
const WALLPAPER_SOURCE_PANEL_ID = "wallpaper-source-panel";

export type {
  WallpaperSourceModalProps,
  WallpaperSourceTab,
} from "@/components/WallpaperSourceModal.types";

export function WallpaperSourceModal({
  open,
  onClose,
  initialTab = "x",
  t,
  onPickFile,
  onRequestLogin,
  wallpaperXSearchMode = "cli",
  onWallpaperXSearchMode,
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
  const [routeMeta, setRouteMeta] = useState<WallpaperXSearchMeta | null>(null);
  const [hasPexelsKey, setHasPexelsKey] = useState<boolean | null>(null);
  const [pexelsKeyDeleteOpen, setPexelsKeyDeleteOpen] = useState(false);
  const [pexelsKeyDeleting, setPexelsKeyDeleting] = useState(false);
  const [loadMoreAttempted, setLoadMoreAttempted] = useState(false);
  const [xLoadingMore, setXLoadingMore] = useState(false);
  const responseContinuationRef = useRef<{
    query: string;
    sort: "top" | "latest";
  } | null>(null);
  const progressiveRequestRef = useRef<string | null>(null);
  const appliedProgressiveKeysRef = useRef<Set<string>>(new Set());
  const sourceGenerationRef = useRef(0);
  const imagineController = useWallpaperImagineController({
    open,
    enabled: open && tab === "imagine",
    t,
    setItems,
    setHasSearched,
    setSelectedId,
    setError,
    setErrorCode,
    setStatusHint,
    setGalleryFilter,
    setKindFilter,
  });
  const remoteSource = isWallpaperRemoteSource(tab) ? tab : null;
  const remoteController = useWallpaperRemoteSourceController({
    enabled: open && remoteSource !== null,
    source: remoteSource,
    query,
    items,
    t,
    setItems,
    setHasSearched,
    setSelectedId,
    setError,
    setErrorCode,
    setStatusHint,
    setGalleryFilter,
    setKindFilter,
  });
  const busy =
    sourceBusy ||
    xSearchBusy ||
    remoteController.busy ||
    imagineController.busy ||
    (tab === "grok_album" && grokAlbum.busy);
  // Search and sync requests run in the background once cards exist. Keep the
  // gallery usable while they append results; only local source mutations and
  // an active preview/apply operation serialize card interaction.
  const interactionLocked =
    sourceBusy || imagineController.busy || applying || previewingId !== null;

  useEffect(() => {
    sourceGenerationRef.current += 1;
    // A close invalidates any pending media preparation. Reset the local busy
    // state here as well so reopening cannot inherit a stale apply operation.
    setApplying(false);
    if (!open) return;
    setTab(initialTab);
    setError(null);
    setErrorCode(null);
    setStatusHint(null);
    setRouteMeta(null);
    setHasPexelsKey(null);
    setPexelsKeyDeleteOpen(false);
    setPexelsKeyDeleting(false);
    setSourceBusy(false);
    setLoadMoreAttempted(false);
    setXLoadingMore(false);
    responseContinuationRef.current = null;
    setSelectedId(null);
    setPreviewingId(null);
    setGalleryFilter("");
    setKindFilter("all");
    setHasSearched(false);
    setItems([]);
  }, [open, initialTab]);

  useEffect(() => {
    if (!open || tab !== "pexels" || hasPexelsKey !== null) return;
    let active = true;
    void api
      .secretsGetMasked()
      .then((masked) => {
        if (active) setHasPexelsKey(masked.hasPexelsKey);
      })
      .catch(() => {
        if (!active) return;
        setHasPexelsKey(false);
        setErrorCode("generic");
        setError(t("settings.wallpaperSource.pexels.keyLoadFailed"));
      });
    return () => {
      active = false;
    };
  }, [hasPexelsKey, open, tab, t]);

  useEffect(() => {
    if (!open) void cancelXSearch();
  }, [open, cancelXSearch]);

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
    if (!open || tab !== "grok_album" || grokAlbum.status === "ready") return;
    // Album navigation or verification can temporarily replace the collection
    // with an empty snapshot. A filter from the previous ready snapshot is no
    // longer meaningful and otherwise leaves a misleading zero-count toolbar.
    setGalleryFilter("");
    setKindFilter("all");
  }, [open, tab, grokAlbum.status]);

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
      totalCount: hasSearched ? items.length : null,
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

  const changeXSearchMode = useCallback(
    async (next: WallpaperXSearchMode) => {
      if (!onWallpaperXSearchMode) return;
      await onWallpaperXSearchMode(normalizeWallpaperXSearchMode(next));
      responseContinuationRef.current = null;
      setLoadMoreAttempted(false);
    },
    [onWallpaperXSearchMode],
  );

  const reportXSearchModeSaveError = useCallback(() => {
    setErrorCode("generic");
    setError(t("settings.wallpaperSource.routeSaveFailed"));
  }, [t]);

  const switchTab = useCallback(
    (next: WallpaperSourceTab) => {
      if (next === tab) return;
      sourceGenerationRef.current += 1;
      cancelGrokAlbumMediaRequests();
      void cancelRemoteWallpaperMediaRequests();
      void remoteController.cancel();
      remoteController.clear();
      if (tab === "x") void cancelXSearch();
      if (tab === "imagine") imagineController.cancelAll();
      setTab(next);
      setSourceBusy(false);
      setItems([]);
      setHasSearched(false);
      setSelectedId(null);
      setError(null);
      setErrorCode(null);
      setStatusHint(null);
      setRouteMeta(null);
      setLoadMoreAttempted(false);
      setXLoadingMore(false);
      setGalleryFilter("");
      setKindFilter("all");
      responseContinuationRef.current = null;
      progressiveRequestRef.current = null;
      appliedProgressiveKeysRef.current.clear();
    },
    [
      imagineController.cancelAll,
      cancelXSearch,
      remoteController.cancel,
      remoteController.clear,
      tab,
    ],
  );

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

  const routeStatus = useMemo(
    () => wallpaperSourceRouteStatus(t, routeMeta),
    [routeMeta, t],
  );

  const xProgressStatus = useMemo(() => {
    if (!xSearchBusy) return null;
    const key = wallpaperXSearchProgressMessageKey(xSearchStage);
    return key ? t(key as MessageKey) : null;
  }, [xSearchBusy, xSearchStage, t]);

  const runXSearch = useCallback(async () => {
    const q = query.trim();
    if (!q) {
      setErrorCode("empty");
      setError(errorMessage(t, "empty"));
      setRouteMeta(null);
      return;
    }
    if (!isDesktopHost()) {
      setErrorCode("generic");
      setError(t("settings.wallpaperSource.err.desktopOnly"));
      setRouteMeta(null);
      return;
    }
    const sourceGeneration = sourceGenerationRef.current;
    setError(null);
    setErrorCode(null);
    setRouteMeta(null);
    setStatusHint(null);
    setLoadMoreAttempted(false);
    setXLoadingMore(false);
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
      if (!res || sourceGeneration !== sourceGenerationRef.current) return;
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
      } else {
        responseContinuationRef.current =
          res.meta?.routeUsed === "responses" &&
          res.meta.continuationAvailable !== false
            ? { query: q, sort }
            : null;
        setItems((current) =>
          mergeAuthoritativeGalleryItems(current, list),
        );
        setError(null);
        setErrorCode(null);
      }
    } catch (e) {
      if (sourceGeneration !== sourceGenerationRef.current) return;
      setHasSearched(true);
      // Keep already validated batches visible if the final invoke transport
      // fails. A normal lane failure is represented by a successful partial
      // Host result and is reconciled by the authoritative list above.
      setRouteMeta(null);
      responseContinuationRef.current = null;
      const code = parseWallpaperSourceError(e);
      setErrorCode(code);
      setError(errorMessage(t, code));
    }
  }, [query, sort, t, searchX]);

  const runXLoadMore = useCallback(async () => {
    const continuation = responseContinuationRef.current;
    if (
      !continuation ||
      continuation.query !== query.trim() ||
      continuation.sort !== sort ||
      loadMoreAttempted ||
      routeMeta?.routeUsed !== "responses"
    ) {
      return;
    }

    const sourceGeneration = sourceGenerationRef.current;
    const baselineItems = items;
    setLoadMoreAttempted(true);
    setXLoadingMore(true);
    setError(null);
    setErrorCode(null);
    setStatusHint(null);
    progressiveRequestRef.current = null;
    appliedProgressiveKeysRef.current.clear();
    try {
      const res = await loadMoreX(continuation.query, continuation.sort);
      if (sourceGeneration !== sourceGenerationRef.current) return;
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
          setLoadMoreAttempted(false);
          setErrorCode(code);
          setError(errorMessage(t, code));
        }
        return;
      }

      const authoritativeItems = appendGalleryItems(baselineItems, extra);
      setItems((current) =>
        mergeAuthoritativeGalleryItems(current, authoritativeItems),
      );
      setError(null);
      setErrorCode(null);
    } catch (e) {
      if (sourceGeneration !== sourceGenerationRef.current) return;
      setLoadMoreAttempted(false);
      const code = parseWallpaperSourceError(e);
      setErrorCode(code);
      setError(errorMessage(t, code));
    } finally {
      if (sourceGeneration === sourceGenerationRef.current) {
        setXLoadingMore(false);
      }
    }
  }, [
    items,
    query,
    routeMeta,
    loadMoreAttempted,
    loadMoreX,
    sort,
    t,
  ]);

  const runRemoteSearch = useCallback(() => {
    // A same-tab search is a new source generation too. Invalidate any lazy
    // original still owned by a closed Viewer before its late completion can
    // write error or selection state into the replacement gallery.
    sourceGenerationRef.current += 1;
    void cancelRemoteWallpaperMediaRequests();
    void remoteController.search();
  }, [remoteController.search]);

  const savePexelsKey = useCallback(
    async (key: string): Promise<boolean> => {
      try {
        // The Host keys provider continuation by credential revision. Cancel
        // and discard the old generation before rotating the credential so a
        // prefetched page cannot be consumed under the new key.
        sourceGenerationRef.current += 1;
        await remoteController.cancel();
        await cancelRemoteWallpaperMediaRequests();
        await api.secretsSet({ pexelsApiKey: key });
        remoteController.clear();
        setHasPexelsKey(true);
        if (
          errorCode === "pexels_key_required" ||
          errorCode === "pexels_key_invalid"
        ) {
          setError(null);
          setErrorCode(null);
        }
        return true;
      } catch {
        setErrorCode("generic");
        setError(t("settings.wallpaperSource.pexels.keySaveFailed"));
        return false;
      }
    },
    [errorCode, remoteController, t],
  );

  const deletePexelsKey = useCallback(async () => {
    if (pexelsKeyDeleting) return;
    setPexelsKeyDeleting(true);
    setError(null);
    setErrorCode(null);
    try {
      // Invalidate UI generations before the Host credential changes so an
      // already-running search or original fetch cannot repopulate this tab.
      sourceGenerationRef.current += 1;
      await remoteController.cancel();
      await cancelRemoteWallpaperMediaRequests();
      await api.secretsSet({ pexelsApiKey: "" });
      remoteController.clear();
      setHasPexelsKey(false);
      setPexelsKeyDeleteOpen(false);
    } catch {
      setPexelsKeyDeleteOpen(false);
      setErrorCode("generic");
      setError(t("settings.wallpaperSource.pexels.keyDeleteFailed"));
    } finally {
      setPexelsKeyDeleting(false);
    }
  }, [pexelsKeyDeleting, remoteController, t]);

  const dropItem = useCallback((id: string) => {
    setItems((prev) => prev.filter((it) => it.id !== id));
    setSelectedId((cur) => (cur === id ? null : cur));
    // A browser decode/hotlink failure changes the visible count after the
    // Host result completed, so do not leave a stale "total" status behind.
    setStatusHint(null);
  }, []);

  const loadLibrary = useCallback(async () => {
    if (!isDesktopHost()) {
      setErrorCode("generic");
      setError(t("settings.wallpaperSource.err.desktopOnly"));
      return;
    }
    const sourceGeneration = sourceGenerationRef.current;
    setSourceBusy(true);
    setError(null);
    setErrorCode(null);
    setStatusHint(t("settings.wallpaperSource.libraryLoading"));
    setSelectedId(null);
    setGalleryFilter("");
    // Keep every item visible; the Host list is already ordered static-first.
    setKindFilter("all");
    try {
      const entries = await api.wallpaperLibraryList(96);
      if (sourceGeneration !== sourceGenerationRef.current) return;
      const list = libraryEntriesToGalleryItems(entries, { staticFirst: true });
      setHasSearched(true);
      setItems(list);
      setError(null);
      setErrorCode(null);
    } catch (e) {
      if (sourceGeneration !== sourceGenerationRef.current) return;
      setHasSearched(true);
      setItems([]);
      const code = parseWallpaperSourceError(e);
      setErrorCode(code);
      setError(errorMessage(t, code));
    } finally {
      if (sourceGeneration === sourceGenerationRef.current) {
        setSourceBusy(false);
        setStatusHint(null);
      }
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

  const openItemPreview = useWallpaperItemPreview({
    interactionLocked,
    visibleItems,
    sourceGenerationRef,
    viewer,
    t,
    dropItem,
    setItems,
    setSelectedId,
    setPreviewingId,
    setError,
    setErrorCode,
    setStatusHint,
  });

  const openXStatus = useCallback((url: string) => {
    void api.openExternalUrl(url).catch(() => {
      /* soft-fail: citation open is best-effort */
    });
  }, []);

  const generateVideoFromItem = useCallback(
    (item: WallpaperGalleryItem) => {
      if (interactionLocked || !isWallpaperImageItem(item)) return;
      // Capture any in-memory Host thumbnail before switching sources clears
      // the remote gallery caches. The controller then materializes the
      // validated original inside the wallpaper library.
      imagineController.beginVideoFromItem(item);
      if (tab !== "imagine") switchTab("imagine");
    },
    [
      imagineController.beginVideoFromItem,
      interactionLocked,
      switchTab,
      tab,
    ],
  );

  const applySelected = useCallback(async () => {
    if (!selected) return;
    if (!isDesktopHost()) {
      setErrorCode("generic");
      setError(t("settings.wallpaperSource.err.desktopOnly"));
      return;
    }
    const sourceGeneration = sourceGenerationRef.current;
    setApplying(true);
    setError(null);
    setErrorCode(null);
    setStatusHint(t("settings.wallpaperSource.applying"));
    try {
      const file = await prepareWallpaperSelection(
        selected,
        () => sourceGeneration === sourceGenerationRef.current,
      );
      if (!file) return;
      await onPickFile(file);
      if (sourceGeneration !== sourceGenerationRef.current) return;
      onClose();
    } catch (e) {
      if (sourceGeneration !== sourceGenerationRef.current) return;
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
      if (sourceGeneration === sourceGenerationRef.current) {
        setApplying(false);
        setStatusHint(null);
      }
    }
  }, [selected, t, onPickFile, onClose]);

  const closeModal = useCallback(() => {
    sourceGenerationRef.current += 1;
    cancelGrokAlbumMediaRequests();
    void cancelRemoteWallpaperMediaRequests();
    imagineController.cancelAll();
    void cancelXSearch();
    if (remoteController.busy) void remoteController.cancel();
    onClose();
  }, [
    cancelXSearch,
    imagineController.cancelAll,
    remoteController,
    onClose,
  ]);

  const closeModalOnEscape = useCallback(() => {
    if (viewer.isOpen()) {
      viewer.close();
      return;
    }
    closeModal();
  }, [closeModal, viewer]);

  const authNeeded = errorCode === "auth_required";
  const locked =
    busy || applying || previewingId !== null || pexelsKeyDeleting;
  const tabSwitchLocked =
    applying ||
    previewingId !== null ||
    (sourceBusy && !xSearchBusy) ||
    (tab === "grok_album" && grokAlbum.busy);
  const canLoadMore =
    (tab === "x" &&
      (!busy || xLoadingMore) &&
      (!loadMoreAttempted || xLoadingMore) &&
      routeMeta?.routeUsed === "responses" &&
      responseContinuationRef.current?.query === query.trim() &&
      responseContinuationRef.current?.sort === sort &&
      items.length > 0) ||
    (tab === "grok_album" &&
      (!busy || grokAlbum.loadingMore) &&
      grokAlbum.canLoadMore &&
      items.length > 0) ||
    (remoteSource !== null &&
      (!busy || remoteController.loadingMore) &&
      remoteController.canLoadMore);
  const collectionSupportsTextFilter =
    tab === "grok_album" || tab === "library";
  const showGalleryTextFilter =
    galleryFilter.length > 0 ||
    (collectionSupportsTextFilter && kindCounts.all >= 16);
  const showGalleryFilters =
    filtersActive ||
    showGalleryTextFilter ||
    (kindCounts.image > 0 && kindCounts.video > 0);
  const softFailError =
    galleryErrorKind != null && isWallpaperGallerySoftFail(galleryErrorKind);
  // Controls and feedback already expose idle/loading state. Keep the gallery
  // quiet until it has a result, a real empty outcome, or active filters.
  const showEmptyBlock =
    emptyState != null &&
    (emptyState.kind === "filter_empty" ||
      (emptyState.kind === "idle" && tab === "library") ||
      ((emptyState.kind === "empty" || emptyState.kind === "error") && !error));

  return (
    <>
      <GlassModal
        open={open}
        onClose={closeModal}
        onEscape={closeModalOnEscape}
        title={t("settings.wallpaperSource.title")}
        size="lg"
        className="wallpaper-source-modal"
        wrapBody
        bodyClassName="wallpaper-source-modal__body"
        closeLabel={t("common.close")}
        footer={
          <WallpaperSourceFooter
            t={t}
            applying={applying}
            applyDisabled={!selected || interactionLocked}
            onClose={closeModal}
            onApply={() => void applySelected()}
          />
        }
      >
        <div className="wallpaper-source-layout">
          <WallpaperSourceTabs
            t={t}
            value={tab}
            disabled={tabSwitchLocked}
            panelId={WALLPAPER_SOURCE_PANEL_ID}
            onChange={switchTab}
          />

          <div
            id={WALLPAPER_SOURCE_PANEL_ID}
            className="wallpaper-source-workspace"
            role="tabpanel"
            aria-labelledby={`wallpaper-source-tab-${tab}`}
          >
            <div className="wallpaper-source-toolbar">
              <WallpaperSourceControls
                t={t}
                tab={tab}
                locked={locked}
                busy={busy}
                xSearchBusy={xSearchBusy}
                remoteSearchBusy={remoteController.busy}
                remoteSearchDisabled={
                  tab === "pexels" && hasPexelsKey !== true
                }
                hasPexelsKey={hasPexelsKey}
                pexelsKeyInvalid={errorCode === "pexels_key_invalid"}
                query={query}
                sort={sort}
                sortOptions={sortOptions}
                xSearchMode={wallpaperXSearchMode}
                imagine={imagineController.controls}
                albumStatus={grokAlbum.status}
                albumCachedCount={grokAlbum.cachedCount}
                albumVisibleCount={grokAlbum.visibleCount}
                albumErrorCode={grokAlbum.errorCode}
                albumSyncing={grokAlbum.syncing}
                onQueryChange={setQuery}
                onSortChange={setSort}
                onXSearchModeChange={
                  onWallpaperXSearchMode ? changeXSearchMode : undefined
                }
                onXSearchModeSaveError={reportXSearchModeSaveError}
                onSearchX={() => void runXSearch()}
                onCancelX={() => void cancelXSearch()}
                onSearchRemote={runRemoteSearch}
                onCancelRemote={() => void remoteController.cancel()}
                onSavePexelsKey={savePexelsKey}
                onRequestDeletePexelsKey={() => setPexelsKeyDeleteOpen(true)}
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
                progress={
                  xProgressStatus || remoteController.progress || statusHint
                }
                routeStatus={routeStatus}
                error={error}
                errorKind={galleryErrorKind}
                softFail={softFailError}
                authNeeded={authNeeded}
                onRequestLogin={onRequestLogin}
              />
            </div>

            <WallpaperSourceGallery
              t={t}
              tab={tab}
              visibleItems={visibleItems}
              selectedId={selectedId}
              previewingId={previewingId}
              locked={interactionLocked}
              busy={busy}
              kindCounts={kindCounts}
              kindFilter={kindFilter}
              galleryFilter={galleryFilter}
              filtersActive={filtersActive}
              showFilters={showGalleryFilters}
              showTextFilter={showGalleryTextFilter}
              emptyState={emptyState}
              showEmptyBlock={showEmptyBlock}
              canLoadMore={canLoadMore}
              loadingMore={
                (tab === "grok_album" && grokAlbum.loadingMore) ||
                (tab === "x" && xLoadingMore) ||
                remoteController.loadingMore
              }
              onKindFilterChange={setKindFilter}
              onGalleryFilterChange={setGalleryFilter}
              onClearFilters={clearGalleryFilters}
              onPreview={(item) => void openItemPreview(item)}
              onGenerateVideo={generateVideoFromItem}
              onDropItem={dropItem}
              onOpenXStatus={openXStatus}
              onOpenSource={openXStatus}
              onDeleteLibraryItem={requestDeleteLibraryItem}
              onLoadMore={() =>
                void (tab === "grok_album"
                  ? grokAlbum.loadMore()
                  : remoteSource
                    ? remoteController.loadMore()
                    : runXLoadMore())
              }
            />
          </div>
        </div>
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
      <WallpaperPexelsKeyDeleteDialog
        t={t}
        open={pexelsKeyDeleteOpen}
        deleting={pexelsKeyDeleting}
        onClose={() => {
          if (!pexelsKeyDeleting) setPexelsKeyDeleteOpen(false);
        }}
        onConfirm={() => void deletePexelsKey()}
      />
    </>
  );
}
