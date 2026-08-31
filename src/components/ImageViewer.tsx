/**
 * Global media lightbox (yet-another-react-lightbox) + open/copy helpers.
 * Images keep zoom/copy support; video slides use the lightbox video plugin.
 *
 * Initial fit: always contain within the stage (upscale small images to fill,
 * downscale large ones). Logical slide width/height are inflated when the
 * natural bitmap is smaller than the stage so YARL's max-width cap and zoom
 * math do not leave a tiny thumbnail in the middle of the window.
 * Shared context and hooks live in the non-component ImageViewerContext module;
 * keep them outside this Fast Refresh boundary so mounted providers and
 * refreshed consumers retain the same context identity.
 */

import {
  lazy,
  Suspense,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react";
import { resolveImageSrc } from "@/lib/imageSrc";
import { copyImageFromPath, copyImageFromSrc } from "@/lib/copyImage";
import {
  lightboxSlideDimensions,
  lightboxSlideRect,
  lightboxYarlSlideSize,
  loadImageNaturalSize,
} from "@/lib/imageLightboxFit";
import { createT, type Locale } from "@/i18n";
import {
  ImageViewerContext,
  type ImageSlideInput,
  type ImageViewerApi,
} from "@/components/ImageViewerContext";

const ImageLightbox = lazy(async () => {
  const m = await import("./ImageLightbox");
  return { default: m.ImageLightbox };
});

interface ResolvedSlide {
  src: string;
  kind: "image" | "video";
  mime?: string;
  poster?: string;
  alt?: string;
  title?: string;
  onView?: ImageSlideInput["onView"];
  loadOriginal?: ImageSlideInput["loadOriginal"];
  /** Original path/url for copy. */
  origin: string;
  /** Logical size for YARL fit + zoom (may exceed natural for small images). */
  width?: number;
  height?: number;
  /**
   * Same logical size as width/height — keeps Zoom imageRect ≥ stage fit so
   * drag-pan works after zoom-in (see lightboxYarlSlideSize).
   */
  srcSet?: Array<{ src: string; width: number; height: number }>;
}

interface ImageViewerProviderProps {
  children: ReactNode;
  locale: Locale;
}

/** Stage size from the current window (SSR-safe fallback). */
function currentStageRect() {
  if (typeof window === "undefined") {
    return lightboxSlideRect(1920, 1080);
  }
  return lightboxSlideRect(window.innerWidth, window.innerHeight);
}

async function withLogicalImageSize(
  slide: ResolvedSlide,
  stage: ReturnType<typeof currentStageRect>,
): Promise<ResolvedSlide> {
  if (slide.kind === "video") return slide;
  const natural = await loadImageNaturalSize(slide.src);
  if (!(natural.width > 0 && natural.height > 0)) return slide;
  const logical = lightboxSlideDimensions(natural, stage);
  const sizeFields = lightboxYarlSlideSize(slide.src, logical);
  return sizeFields ? { ...slide, ...sizeFields } : slide;
}

export function ImageViewerProvider({
  children,
  locale,
}: ImageViewerProviderProps) {
  const tr = useMemo(() => createT(locale), [locale]);
  const [isOpen, setIsOpen] = useState(false);
  const [index, setIndex] = useState(0);
  const [slides, setSlides] = useState<ResolvedSlide[]>([]);
  const slidesRef = useRef(slides);
  const generationRef = useRef(0);
  const originalLoadsRef = useRef(new Set<string>());
  slidesRef.current = slides;

  const close = useCallback(() => {
    generationRef.current += 1;
    originalLoadsRef.current.clear();
    setIsOpen(false);
  }, []);

  const openViewer = useCallback(
    (input: ImageSlideInput[] | string[], startIndex = 0) => {
      const normalized: ImageSlideInput[] = input.map((item) =>
        typeof item === "string" ? { src: item } : item,
      );
      if (!normalized.length) return;

      void (async () => {
        const generation = generationRef.current + 1;
        generationRef.current = generation;
        originalLoadsRef.current.clear();
        const resolved = (
          await Promise.all(
            normalized.map(async (input, inputIndex) => {
              const src = await resolveImageSrc(input.src);
              return src
                ? { path: input.src, src, input, inputIndex }
                : null;
            }),
          )
        ).filter((entry): entry is NonNullable<typeof entry> => entry !== null);
        if (!resolved.length) return;

        const next: ResolvedSlide[] = resolved.map(
          ({ path, src, input }) => {
            const kind = input.kind === "video" ? "video" : "image";
            if (kind === "video") {
              return {
                src,
                origin: path,
                kind,
                mime: input.mime,
                poster: input.poster,
                alt: input.alt ?? input.title,
                title: input.title,
                onView: input.onView,
                loadOriginal: input.loadOriginal,
              };
            }
            return {
              src,
              origin: path,
              kind,
              alt: input.alt ?? input.title,
              title: input.title,
              onView: input.onView,
              loadOriginal: input.loadOriginal,
            };
          },
        );

        const requestedInputIndex = Math.min(
          Math.max(Number.isFinite(startIndex) ? Math.trunc(startIndex) : 0, 0),
          normalized.length - 1,
        );
        let idx = resolved.findIndex(
          (entry) => entry.inputIndex === requestedInputIndex,
        );
        if (idx < 0) idx = 0;

        // Opening a gallery must not wait for every remote sibling. Resolve
        // only an eager selected image now; a lazy placeholder opens
        // immediately and the effect below upgrades it to the local original.
        // Navigation hydrates later slides on demand while preserving the
        // small-image fit and zoom behavior.
        const selected = next[idx];
        if (selected && !selected.loadOriginal) {
          next[idx] = await withLogicalImageSize(
            selected,
            currentStageRect(),
          );
        }

        if (generationRef.current !== generation) return;
        setSlides(next);
        setIndex(idx);
        setIsOpen(true);
      })();
    },
    [],
  );

  const copyImage = useCallback(async (pathOrUrl: string) => {
    // Prefer Host path write for absolute files; then URL/fetch path.
    const r = await copyImageFromPath(pathOrUrl);
    if (r.ok) return true;
    const src = await resolveImageSrc(pathOrUrl);
    if (!src) return false;
    return (await copyImageFromSrc(src)).ok;
  }, []);

  const hydrateSlideAt = useCallback(
    (targetIndex: number, force = false) => {
      const slide = slidesRef.current[targetIndex];
      if (!slide) {
        return;
      }
      const generation = generationRef.current;
      const expectedOrigin = slide.origin;
      const expectedSrc = slide.src;

      if (slide.loadOriginal) {
        const loadKey = `${generation}:${targetIndex}:${expectedOrigin}`;
        if (originalLoadsRef.current.has(loadKey)) return;
        originalLoadsRef.current.add(loadKey);
        void (async () => {
          try {
            const loaded = await slide.loadOriginal?.();
            if (!loaded || generationRef.current !== generation) return;
            const loadedSrc = await resolveImageSrc(loaded.src);
            if (!loadedSrc || generationRef.current !== generation) return;
            const upgraded: ResolvedSlide = {
              ...slide,
              src: loadedSrc,
              origin: loaded.src,
              kind: loaded.kind === "video" ? "video" : "image",
              mime: loaded.mime,
              poster: loaded.poster ?? slide.poster,
              loadOriginal: undefined,
              width: undefined,
              height: undefined,
              srcSet: undefined,
            };
            const updated = await withLogicalImageSize(
              upgraded,
              currentStageRect(),
            );
            if (generationRef.current !== generation) return;
            setSlides((current) => {
              const previous = current[targetIndex];
              if (
                previous?.origin !== expectedOrigin ||
                previous.src !== expectedSrc
              ) {
                return current;
              }
              const next = current.slice();
              next[targetIndex] = updated;
              return next;
            });
          } catch {
            // Keep the already-viewable placeholder; revisiting may retry.
          } finally {
            originalLoadsRef.current.delete(loadKey);
          }
        })();
        return;
      }

      if (
        slide.kind === "video" ||
        (!force && slide.width && slide.height)
      ) {
        return;
      }
      void withLogicalImageSize(slide, currentStageRect()).then((updated) => {
        if (generationRef.current !== generation) return;
        setSlides((current) => {
          if (
            current[targetIndex]?.origin !== expectedOrigin ||
            current[targetIndex]?.src !== expectedSrc
          ) {
            return current;
          }
          const previous = current[targetIndex];
          if (
            previous?.width === updated.width &&
            previous?.height === updated.height
          ) {
            return current;
          }
          const next = current.slice();
          next[targetIndex] = updated;
          return next;
        });
      });
    },
    [],
  );

  const handleView = useCallback(
    (nextIndex: number) => {
      setIndex(nextIndex);
      slidesRef.current[nextIndex]?.onView?.();
      hydrateSlideAt(nextIndex);
    },
    [hydrateSlideAt],
  );

  // A lazy slide may be the item that opened the viewer, not only a sibling
  // reached with Previous/Next. Open immediately with its bounded thumbnail,
  // then upgrade that first visible slide without blocking the lightbox.
  useEffect(() => {
    if (!isOpen) return;
    hydrateSlideAt(index);
  }, [hydrateSlideAt, index, isOpen]);

  const api = useMemo<ImageViewerApi>(
    () => ({
      open: openViewer,
      close,
      copyImage,
    }),
    [openViewer, close, copyImage],
  );

  // Right-click inside lightbox → copy current image (keeps Zoom plugin intact).
  useEffect(() => {
    if (!isOpen) return;
    const onCtx = (e: MouseEvent) => {
      const target = e.target as HTMLElement | null;
      if (!target?.closest?.(".yarl__root")) return;
      const img = target.closest("img") as HTMLImageElement | null;
      if (!img) return;
      const src = img.currentSrc || img.src;
      if (!src) return;
      e.preventDefault();
      e.stopPropagation();
      void copyImageFromSrc(src);
    };
    document.addEventListener("contextmenu", onCtx, true);
    return () => document.removeEventListener("contextmenu", onCtx, true);
  }, [isOpen]);

  // Recompute logical dims on resize so small images still fill a larger stage.
  useEffect(() => {
    if (!isOpen) return;
    let cancelled = false;
    let timer: ReturnType<typeof setTimeout> | null = null;
    const onResize = () => {
      if (timer) clearTimeout(timer);
      timer = setTimeout(() => {
        if (!cancelled) hydrateSlideAt(index, true);
      }, 120);
    };
    window.addEventListener("resize", onResize);
    return () => {
      cancelled = true;
      if (timer) clearTimeout(timer);
      window.removeEventListener("resize", onResize);
    };
  }, [hydrateSlideAt, index, isOpen]);

  return (
    <ImageViewerContext.Provider value={api}>
      {children}
      {isOpen ? (
        <Suspense fallback={null}>
          <ImageLightbox
            open={isOpen}
            close={close}
            index={index}
            slides={slides}
            onView={handleView}
            labels={{
              next: tr("image.next"),
              prev: tr("image.prev"),
              close: tr("image.close"),
              zoomIn: tr("image.zoomIn"),
              zoomOut: tr("image.zoomOut"),
            }}
          />
        </Suspense>
      ) : null}
    </ImageViewerContext.Provider>
  );
}
