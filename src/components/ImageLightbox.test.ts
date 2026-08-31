import { describe, expect, it } from "vitest";
import { toLightboxSlides } from "./ImageLightbox";

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
