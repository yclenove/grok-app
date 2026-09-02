/**
 * @vitest-environment jsdom
 */
import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import "@/test/jsdomStubs";

const loadImageNaturalSize = vi.hoisted(() =>
  vi.fn(async () => ({ width: 640, height: 480 })),
);
const capturedSlides = vi.hoisted(() => vi.fn());
const capturedOpen = vi.hoisted(() => vi.fn());
const viewedOriginal = vi.hoisted(() => vi.fn());
const loadOriginal = vi.hoisted(() =>
  vi.fn(async () => ({
    src: "H:\\wallpapers\\lazy-original.jpg",
    kind: "image" as const,
    mime: "image/jpeg",
  })),
);
const loadFirstDuplicateOriginal = vi.hoisted(() =>
  vi.fn(async () => ({
    src: "H:\\wallpapers\\duplicate-first.jpg",
    kind: "image" as const,
    mime: "image/jpeg",
  })),
);
const loadSecondDuplicateOriginal = vi.hoisted(() =>
  vi.fn(async () => ({
    src: "H:\\wallpapers\\duplicate-second.jpg",
    kind: "image" as const,
    mime: "image/jpeg",
  })),
);
const resolveImageSrc = vi.hoisted(() =>
  vi.fn(async (src: string) => src),
);
const LAZY_PLACEHOLDER =
  "data:image/gif;base64,R0lGODlhAQABAIAAAAAAAP///ywAAAAAAQABAAACAUwAOw==";

vi.mock("@/lib/imageSrc", () => ({
  resolveImageSrc,
  resolveImageSrcs: vi.fn(async (paths: string[]) =>
    paths.map((path) => ({ path, src: path })),
  ),
}));

vi.mock("@/lib/imageLightboxFit", async (importOriginal) => {
  const actual =
    await importOriginal<typeof import("@/lib/imageLightboxFit")>();
  return { ...actual, loadImageNaturalSize };
});

vi.mock("./ImageLightbox", () => ({
  ImageLightbox: (props: {
    open: boolean;
    close: () => void;
    index: number;
    slides: unknown[];
    onView: (index: number) => void;
  }) => {
    capturedSlides(props.slides);
    capturedOpen(props.open);
    return (
      <div data-testid="lightbox" data-open={String(props.open)}>
        <button
          type="button"
          data-testid="lightbox-next"
          onClick={() => props.onView(props.index + 1)}
        >
          next
        </button>
        <button type="button" data-testid="lightbox-close" onClick={props.close}>
          close
        </button>
      </div>
    );
  },
}));

import { ImageViewerProvider } from "./ImageViewer";
import { useImageViewer } from "./ImageViewerContext";

function GalleryTrigger() {
  const viewer = useImageViewer();
  return (
    <button
      type="button"
      onClick={() =>
        viewer.open(
          [
            { src: "https://example.test/first.jpg" },
            { src: "https://example.test/selected.jpg" },
            { src: "https://example.test/third.jpg" },
          ],
          1,
        )
      }
    >
      open
    </button>
  );
}

function LazyGalleryTrigger() {
  const viewer = useImageViewer();
  return (
    <button
      type="button"
      onClick={() =>
        viewer.open([
          { src: "H:\\wallpapers\\first.jpg" },
          {
            src: LAZY_PLACEHOLDER,
            onView: viewedOriginal,
            loadOriginal,
          },
        ])
      }
    >
      open lazy
    </button>
  );
}

function InitiallyLazyGalleryTrigger() {
  const viewer = useImageViewer();
  return (
    <button
      type="button"
      onClick={() =>
        viewer.open(
          [
            { src: "H:\\wallpapers\\first.jpg" },
            {
              src: LAZY_PLACEHOLDER,
              onView: viewedOriginal,
              loadOriginal,
            },
          ],
          1,
        )
      }
    >
      open selected lazy
    </button>
  );
}

function DuplicatePlaceholderGalleryTrigger() {
  const viewer = useImageViewer();
  return (
    <button
      type="button"
      onClick={() =>
        viewer.open(
          [
            {
              src: LAZY_PLACEHOLDER,
              loadOriginal: loadFirstDuplicateOriginal,
            },
            {
              src: LAZY_PLACEHOLDER,
              loadOriginal: loadSecondDuplicateOriginal,
            },
          ],
          1,
        )
      }
    >
      open duplicate placeholders
    </button>
  );
}

afterEach(() => {
  cleanup();
  loadImageNaturalSize.mockClear();
  capturedSlides.mockClear();
  capturedOpen.mockClear();
  viewedOriginal.mockClear();
  loadOriginal.mockClear();
  loadFirstDuplicateOriginal.mockClear();
  loadSecondDuplicateOriginal.mockClear();
  resolveImageSrc.mockClear();
});

describe("ImageViewerProvider", () => {
  it("opens after sizing only the selected image", async () => {
    render(
      <ImageViewerProvider locale="en">
        <GalleryTrigger />
      </ImageViewerProvider>,
    );

    fireEvent.click(screen.getByRole("button", { name: "open" }));
    await screen.findByTestId("lightbox");

    expect(loadImageNaturalSize).toHaveBeenCalledTimes(1);
    expect(loadImageNaturalSize).toHaveBeenCalledWith(
      "https://example.test/selected.jpg",
    );
    await waitFor(() => expect(capturedSlides).toHaveBeenCalled());
    const slides = capturedSlides.mock.calls.at(-1)?.[0] as Array<{
      src: string;
      width?: number;
    }>;
    expect(slides).toHaveLength(3);
    expect(slides[0]?.width).toBeUndefined();
    expect(slides[1]?.width).toBeGreaterThan(0);
    expect(slides[2]?.width).toBeUndefined();
  });

  it("keeps the lightbox mounted with open=false so its exit lifecycle completes", async () => {
    render(
      <ImageViewerProvider locale="en">
        <GalleryTrigger />
      </ImageViewerProvider>,
    );

    fireEvent.click(screen.getByRole("button", { name: "open" }));
    const lightbox = await screen.findByTestId("lightbox");
    expect(lightbox.getAttribute("data-open")).toBe("true");

    fireEvent.click(screen.getByTestId("lightbox-close"));

    expect(screen.getByTestId("lightbox").getAttribute("data-open")).toBe(
      "false",
    );
    expect(capturedOpen.mock.calls.map(([open]) => open)).toContain(false);
  });

  it("resolves all slide sources without serializing on the first one", async () => {
    let resolveFirst: (() => void) | undefined;
    const firstSource = new Promise<string>((resolve) => {
      resolveFirst = () => resolve("https://example.test/first.jpg");
    });
    resolveImageSrc.mockImplementationOnce(() => firstSource);

    render(
      <ImageViewerProvider locale="en">
        <GalleryTrigger />
      </ImageViewerProvider>,
    );

    fireEvent.click(screen.getByRole("button", { name: "open" }));
    await waitFor(() => expect(resolveImageSrc).toHaveBeenCalledTimes(3));
    expect(screen.queryByTestId("lightbox")).toBeNull();

    resolveFirst?.();
    await screen.findByTestId("lightbox");
  });

  it("upgrades a placeholder to its local original on navigation", async () => {
    render(
      <ImageViewerProvider locale="en">
        <LazyGalleryTrigger />
      </ImageViewerProvider>,
    );

    fireEvent.click(screen.getByRole("button", { name: "open lazy" }));
    await screen.findByTestId("lightbox");
    expect(loadOriginal).not.toHaveBeenCalled();

    fireEvent.click(screen.getByTestId("lightbox-next"));
    expect(viewedOriginal).toHaveBeenCalledTimes(1);
    await waitFor(() => expect(loadOriginal).toHaveBeenCalledTimes(1));
    await waitFor(() => {
      const slides = capturedSlides.mock.calls.at(-1)?.[0] as Array<{
        src: string;
        width?: number;
      }>;
      expect(slides[1]?.src).toBe(
        "H:\\wallpapers\\lazy-original.jpg",
      );
      expect(slides[1]?.width).toBeGreaterThan(0);
    });
    expect(loadImageNaturalSize).toHaveBeenCalledWith(
      "H:\\wallpapers\\lazy-original.jpg",
    );
  });

  it("opens immediately and upgrades an initially selected lazy slide", async () => {
    render(
      <ImageViewerProvider locale="en">
        <InitiallyLazyGalleryTrigger />
      </ImageViewerProvider>,
    );

    fireEvent.click(screen.getByRole("button", { name: "open selected lazy" }));
    await screen.findByTestId("lightbox");
    expect(loadImageNaturalSize).not.toHaveBeenCalledWith(LAZY_PLACEHOLDER);
    await waitFor(() => expect(loadOriginal).toHaveBeenCalledTimes(1));
    await waitFor(() => {
      const slides = capturedSlides.mock.calls.at(-1)?.[0] as Array<{
        src: string;
      }>;
      expect(slides[1]?.src).toBe("H:\\wallpapers\\lazy-original.jpg");
    });
    expect(loadImageNaturalSize).not.toHaveBeenCalledWith(LAZY_PLACEHOLDER);
  });

  it("keeps the selected index when lazy slides share one placeholder", async () => {
    render(
      <ImageViewerProvider locale="en">
        <DuplicatePlaceholderGalleryTrigger />
      </ImageViewerProvider>,
    );

    fireEvent.click(
      screen.getByRole("button", { name: "open duplicate placeholders" }),
    );
    await screen.findByTestId("lightbox");

    await waitFor(() =>
      expect(loadSecondDuplicateOriginal).toHaveBeenCalledTimes(1),
    );
    expect(loadFirstDuplicateOriginal).not.toHaveBeenCalled();
    await waitFor(() => {
      const slides = capturedSlides.mock.calls.at(-1)?.[0] as Array<{
        src: string;
      }>;
      expect(slides[0]?.src).toBe(LAZY_PLACEHOLDER);
      expect(slides[1]?.src).toBe(
        "H:\\wallpapers\\duplicate-second.jpg",
      );
    });
  });
});
