import { useCallback, type Dispatch, type RefObject, type SetStateAction } from "react";
import type { ImageSlideInput, ImageViewerApi } from "@/components/ImageViewerContext";
import type { MessageKey } from "@/i18n";
import { isDesktopHost } from "@/lib/api";
import { peekGrokAlbumThumbnail } from "@/lib/grokAlbumThumbnail";
import { peekRemoteWallpaperThumbnail } from "@/lib/remoteWallpaperThumbnail";
import { isWallpaperRemoteSource } from "@/lib/wallpaperRemoteSearch";
import {
  parseWallpaperSourceError,
  type WallpaperGalleryItem,
  type WallpaperSourceErrorCode,
} from "@/lib/wallpaperSource";
import {
  EMPTY_WALLPAPER_IMAGE_PLACEHOLDER,
  ensureLocalWallpaperMedia,
} from "@/lib/wallpaperSourceMedia";
import { wallpaperSourceErrorMessage } from "@/lib/wallpaperSourcePresentation";

type Translate = (
  key: MessageKey,
  vars?: Record<string, string | number | undefined | null>,
) => string;

type PreviewOptions = {
  interactionLocked: boolean;
  visibleItems: WallpaperGalleryItem[];
  sourceGenerationRef: RefObject<number>;
  viewer: ImageViewerApi;
  t: Translate;
  dropItem: (id: string) => void;
  setItems: Dispatch<SetStateAction<WallpaperGalleryItem[]>>;
  setSelectedId: Dispatch<SetStateAction<string | null>>;
  setPreviewingId: Dispatch<SetStateAction<string | null>>;
  setError: Dispatch<SetStateAction<string | null>>;
  setErrorCode: Dispatch<SetStateAction<WallpaperSourceErrorCode | null>>;
  setStatusHint: Dispatch<SetStateAction<string | null>>;
};

function slideTitle(item: WallpaperGalleryItem): string | undefined {
  return (
    item.textPreview ||
    item.prompt ||
    (item.username ? `@${item.username}` : undefined)
  );
}

export function useWallpaperItemPreview({
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
}: PreviewOptions) {
  return useCallback(
    async (item: WallpaperGalleryItem) => {
      if (interactionLocked) return;
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
      const immediateRemotePreview = isWallpaperRemoteSource(item.source);
      const eagerLocalPreview = !lazyAlbumPreview && !immediateRemotePreview;
      if (eagerLocalPreview) {
        setStatusHint(t("settings.wallpaperSource.loadingOriginal"));
      }

      try {
        // X/Imagine keep their established eager local download. Grok album
        // upgrades its in-memory thumbnail on demand. Public remote providers
        // have already passed Host probing, so preview their rendered URL
        // immediately and defer the full validated download until Apply.
        const local = eagerLocalPreview
          ? await ensureLocalWallpaperMedia(item)
          : null;
        if (sourceGeneration !== sourceGenerationRef.current) return;
        if (local) {
          setItems((previous) =>
            previous.map((candidate) =>
              candidate.id === item.id
                ? {
                    ...candidate,
                    localPath: local.path,
                    fullUrl: candidate.fullUrl.startsWith("http")
                      ? candidate.fullUrl
                      : `file://${local.path}`,
                  }
                : candidate,
            ),
          );
        }

        const viable = visibleItems.filter(
          (candidate) =>
            candidate.id === item.id ||
            candidate.localPath ||
            candidate.fullUrl.startsWith("http"),
        );
        const slides: ImageSlideInput[] = viable.map((candidate) => {
          const lazyAlbumOriginal =
            candidate.source === "grok_album" &&
            !candidate.localPath &&
            candidate.fullUrl.startsWith("http");
          if (lazyAlbumOriginal) {
            const thumbnail =
              peekGrokAlbumThumbnail(candidate.thumbUrl || candidate.fullUrl) ||
              EMPTY_WALLPAPER_IMAGE_PLACEHOLDER;
            return {
              src: thumbnail,
              kind: "image",
              title: slideTitle(candidate),
              alt: candidate.prompt || candidate.textPreview || undefined,
              onView: () => setSelectedId(candidate.id),
              loadOriginal: async () => {
                try {
                  const loaded = await ensureLocalWallpaperMedia(candidate);
                  if (sourceGeneration !== sourceGenerationRef.current) {
                    return null;
                  }
                  setItems((previous) =>
                    previous.map((current) =>
                      current.id === candidate.id
                        ? {
                            ...current,
                            localPath: loaded.path,
                            fullUrl: current.fullUrl.startsWith("http")
                              ? current.fullUrl
                              : `file://${loaded.path}`,
                          }
                        : current,
                    ),
                  );
                  return {
                    src: loaded.path,
                    kind: candidate.kind === "video" ? "video" : "image",
                    mime: loaded.mime,
                    poster: candidate.kind === "video" ? thumbnail : undefined,
                  };
                } catch (error) {
                  if (sourceGeneration !== sourceGenerationRef.current) {
                    return null;
                  }
                  const code = parseWallpaperSourceError(error);
                  setErrorCode(code);
                  setError(wallpaperSourceErrorMessage(t, code));
                  throw error;
                }
              },
            };
          }

          const lazyRemoteOriginal =
            isWallpaperRemoteSource(candidate.source) &&
            !candidate.localPath &&
            candidate.fullUrl.startsWith("http");
          if (lazyRemoteOriginal) {
            const remoteThumbnail = peekRemoteWallpaperThumbnail(candidate);
            return {
              // A Host thumbnail normally replaces this URL shortly after the
              // card mounts. A cache miss must not block the lightbox, though:
              // open with the already validated result URL and let the Host
              // original loader immediately upgrade it to a local file.
              src:
                remoteThumbnail || candidate.thumbUrl || candidate.fullUrl,
              kind: "image",
              title: slideTitle(candidate),
              alt: candidate.prompt || candidate.textPreview || undefined,
              onView: () => setSelectedId(candidate.id),
              loadOriginal: async () => {
                try {
                  const loaded = await ensureLocalWallpaperMedia(candidate);
                  if (sourceGeneration !== sourceGenerationRef.current) {
                    return null;
                  }
                  setItems((previous) =>
                    previous.map((current) =>
                      current.id === candidate.id
                        ? { ...current, localPath: loaded.path }
                        : current,
                    ),
                  );
                  return {
                    src: loaded.path,
                    kind: "image" as const,
                    mime: loaded.mime,
                  };
                } catch (error) {
                  if (sourceGeneration !== sourceGenerationRef.current) {
                    return null;
                  }
                  const code = parseWallpaperSourceError(error);
                  setErrorCode(code);
                  setError(wallpaperSourceErrorMessage(t, code));
                  throw error;
                }
              },
            };
          }

          const path =
            candidate.id === item.id && local
              ? local.path
              : candidate.localPath ||
                (candidate.fullUrl.startsWith("file://")
                  ? decodeURIComponent(candidate.fullUrl.replace(/^file:\/\//, ""))
                  : candidate.fullUrl);
          return {
            src: path,
            kind: candidate.kind === "video" ? "video" : "image",
            mime: candidate.id === item.id ? local?.mime : undefined,
            title: slideTitle(candidate),
            alt: candidate.prompt || candidate.textPreview || undefined,
            onView: () => setSelectedId(candidate.id),
          };
        });
        const index = Math.max(
          0,
          viable.findIndex((candidate) => candidate.id === item.id),
        );
        viewer.open(slides, index);
      } catch (error) {
        if (sourceGeneration !== sourceGenerationRef.current) return;
        // Eager X/Imagine failures represent broken cards. Album and remote
        // previews do not perform an original download on this path.
        if (eagerLocalPreview) dropItem(item.id);
        const code = parseWallpaperSourceError(error);
        setErrorCode(code);
        setError(wallpaperSourceErrorMessage(t, code));
      } finally {
        if (sourceGeneration === sourceGenerationRef.current) {
          setPreviewingId(null);
          setStatusHint(null);
        }
      }
    },
    [
      dropItem,
      interactionLocked,
      setError,
      setErrorCode,
      setItems,
      setPreviewingId,
      setSelectedId,
      setStatusHint,
      sourceGenerationRef,
      t,
      viewer,
      visibleItems,
    ],
  );
}
