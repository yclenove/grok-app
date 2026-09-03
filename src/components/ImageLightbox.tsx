/**
 * yet-another-react-lightbox — loaded only when a gallery opens.
 */

import Lightbox, { type Slide } from "yet-another-react-lightbox";
import Zoom from "yet-another-react-lightbox/plugins/zoom";
import Counter from "yet-another-react-lightbox/plugins/counter";
import Video from "yet-another-react-lightbox/plugins/video";
import "yet-another-react-lightbox/styles.css";
import "yet-another-react-lightbox/plugins/counter.css";

export type ImageLightboxSlide = {
  src: string;
  kind: "image" | "video";
  mime?: string;
  poster?: string;
  alt?: string;
  title?: string;
  width?: number;
  height?: number;
  srcSet?: Array<{ src: string; width: number; height: number }>;
};

function videoMime(slide: ImageLightboxSlide): string {
  const explicit = slide.mime?.split(";", 1)[0]?.trim().toLowerCase();
  if (explicit?.startsWith("video/")) return explicit;
  const path = slide.src.split(/[?#]/, 1)[0]?.toLowerCase() ?? "";
  return path.endsWith(".webm") ? "video/webm" : "video/mp4";
}

export function toLightboxSlides(slides: ImageLightboxSlide[]): Slide[] {
  return slides.map((slide) =>
    slide.kind === "video"
      ? {
          type: "video",
          poster: slide.poster,
          sources: [{ src: slide.src, type: videoMime(slide) }],
        }
      : {
          type: "image",
          src: slide.src,
          alt: slide.alt ?? slide.title,
          title: slide.title,
          width: slide.width,
          height: slide.height,
          ...(slide.srcSet?.length ? { srcSet: slide.srcSet } : {}),
        },
  );
}

export function ImageLightbox({
  open,
  close,
  index,
  slides,
  onView,
  labels,
}: {
  open: boolean;
  close: () => void;
  index: number;
  slides: ImageLightboxSlide[];
  onView: (index: number) => void;
  labels: {
    next: string;
    prev: string;
    close: string;
    zoomIn: string;
    zoomOut: string;
  };
}) {
  return (
    <Lightbox
      open={open}
      close={close}
      index={index}
      slides={toLightboxSlides(slides)}
      on={{
        view: ({ index: i }) => onView(i),
      }}
      plugins={[Video, Zoom, Counter]}
      video={{
        autoPlay: true,
        controls: true,
        loop: true,
        muted: true,
        playsInline: true,
        preload: "metadata",
      }}
      zoom={{
        maxZoomPixelRatio: 4,
        scrollToZoom: true,
      }}
      carousel={{
        finite: slides.length <= 1,
        preload: 2,
        imageFit: "contain",
        imageProps: {
          style: {
            maxWidth: "100%",
            maxHeight: "100%",
            width: "100%",
            height: "100%",
            objectFit: "contain",
          },
          draggable: false,
          referrerPolicy: "no-referrer",
        },
      }}
      controller={{
        closeOnBackdropClick: true,
      }}
      styles={{
        container: { backgroundColor: "rgba(0, 0, 0, 0.92)" },
      }}
      labels={{
        Next: labels.next,
        Previous: labels.prev,
        Close: labels.close,
        "Zoom in": labels.zoomIn,
        "Zoom out": labels.zoomOut,
      }}
    />
  );
}
