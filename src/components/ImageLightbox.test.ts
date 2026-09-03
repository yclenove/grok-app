import { describe, expect, it } from "vitest";
import { ImageLightbox, toLightboxSlides } from "./ImageLightbox";

describe("toLightboxSlides", () => {
  it("keeps images on the image slide path", () => {
    expect(
      toLightboxSlides([
        {
          kind: "image",
          src: "https://example.test/wallpaper.jpg",
          alt: "wallpaper",
          width: 1920,
          height: 1080,
        },
      ]),
    ).toEqual([
      expect.objectContaining({
        type: "image",
        src: "https://example.test/wallpaper.jpg",
        alt: "wallpaper",
        width: 1920,
        height: 1080,
      }),
    ]);
  });

  it("builds video-plugin sources from explicit or inferred MIME", () => {
    expect(
      toLightboxSlides([
        {
          kind: "video",
          src: "http://127.0.0.1/media/clip",
          mime: "video/mp4; charset=binary",
        },
        {
          kind: "video",
          src: "https://example.test/clip.webm?cache=1",
        },
      ]),
    ).toEqual([
      {
        type: "video",
        poster: undefined,
        sources: [
          { src: "http://127.0.0.1/media/clip", type: "video/mp4" },
        ],
      },
      {
        type: "video",
        poster: undefined,
        sources: [
          { src: "https://example.test/clip.webm?cache=1", type: "video/webm" },
        ],
      },
    ]);
  });
});

describe("ImageLightbox", () => {
  it("does not leak the app origin when rendering remote images", () => {
    const rendered = ImageLightbox({
      open: true,
      close: () => undefined,
      index: 0,
      slides: [{ kind: "image", src: "https://cdn.example.test/photo.jpg" }],
      onView: () => undefined,
      labels: {
        next: "Next",
        prev: "Previous",
        close: "Close",
        zoomIn: "Zoom in",
        zoomOut: "Zoom out",
      },
    });
    const props = rendered.props as {
      carousel: { imageProps: { referrerPolicy?: string } };
    };

    expect(props.carousel.imageProps.referrerPolicy).toBe("no-referrer");
  });
});
