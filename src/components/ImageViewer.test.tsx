/**
 * @vitest-environment jsdom
 */
import {
  act,
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
    onRetryOriginal: () => void;
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
        <button type="button" data-testid="lightbox-retry" onClick={props.onRetryOriginal}>retry</button>
        <button type="button" data-testid="lightbox-view-current" onClick={() => props.onView(props.index)}>view</button>
      </div>
    );
  },
}));

import { ImageViewerProvider } from "./ImageViewer";
import { useImageViewer, type ImageSlideInput } from "./ImageViewerContext";

function ProvidedGalleryTrigger({ slides }: { slides: ImageSlideInput[] }) {
  const viewer = useImageViewer();
  return <button onClick={() => viewer.open(slides)}>open provided</button>;
}

function deferredOriginals(count: number) {
  return Array.from({ length: count }, (_, index) => {
    let resolve!: (value: { src: string }) => void;
    let reject!: (error: Error) => void;
    const promise = new Promise<{ src: string }>((done, fail) => {
      resolve = done;
      reject = fail;
    });
    const load = vi.fn(() => promise);
    return {
      slide: { src: LAZY_PLACEHOLDER, loadOriginal: load },
      load,
      finish: () => resolve({ src: `H:/wallpapers/original-${index}.jpg` }),
      fail: () => reject(new Error("download_failed")),
    };
  });
}

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
  it("bounds rapid navigation and loads only the latest waiting original", async () => {
    const originals = deferredOriginals(5);
    render(<ImageViewerProvider locale="en"><ProvidedGalleryTrigger slides={originals.map((entry) => entry.slide)} /></ImageViewerProvider>);
    fireEvent.click(screen.getByRole("button", { name: "open provided" }));
    await waitFor(() => expect(originals[0].load).toHaveBeenCalledTimes(1));
    for (let i = 1; i < originals.length; i += 1) {
      fireEvent.click(screen.getByTestId("lightbox-next"));
    }
    expect(originals.map((entry) => entry.load.mock.calls.length)).toEqual([1, 1, 0, 0, 0]);
    await act(async () => { originals[0].finish(); });
    await waitFor(() => expect(originals[4].load).toHaveBeenCalledTimes(1));
    expect(originals[2].load).not.toHaveBeenCalled();
    expect(originals[3].load).not.toHaveBeenCalled();
    fireEvent.click(screen.getByTestId("lightbox-retry"));
    expect(originals[4].load).toHaveBeenCalledTimes(1);
    await act(async () => { originals[1].finish(); originals[4].finish(); });
    await waitFor(() => expect(capturedSlides.mock.calls.at(-1)?.[0][4].src).toBe("H:/wallpapers/original-4.jpg"));
  });

  it("releases a failed request's slot without automatically retrying it", async () => {
    const originals = deferredOriginals(3);
    render(<ImageViewerProvider locale="en"><ProvidedGalleryTrigger slides={originals.map((entry) => entry.slide)} /></ImageViewerProvider>);
    fireEvent.click(screen.getByRole("button", { name: "open provided" }));
    await waitFor(() => expect(originals[0].load).toHaveBeenCalledTimes(1));
    fireEvent.click(screen.getByTestId("lightbox-next"));
    fireEvent.click(screen.getByTestId("lightbox-next"));
    await act(async () => { originals[0].fail(); });
    await waitFor(() => expect(originals[2].load).toHaveBeenCalledTimes(1));
    expect(capturedSlides.mock.calls.at(-1)?.[0][0].originalStatus).toBe("error");
    expect(originals[0].load).toHaveBeenCalledTimes(1);
    await act(async () => { originals[1].finish(); originals[2].finish(); });
  });

  it("discards queued downloads on close and retains the bound when reopened", async () => {
    const originals = deferredOriginals(3);
    render(<ImageViewerProvider locale="en"><ProvidedGalleryTrigger slides={originals.map((entry) => entry.slide)} /><InitiallyLazyGalleryTrigger /></ImageViewerProvider>);
    fireEvent.click(screen.getByRole("button", { name: "open provided" }));
    await waitFor(() => expect(originals[0].load).toHaveBeenCalledTimes(1));
    fireEvent.click(screen.getByTestId("lightbox-next"));
    fireEvent.click(screen.getByTestId("lightbox-next"));
    fireEvent.click(screen.getByTestId("lightbox-close"));
    fireEvent.click(screen.getByRole("button", { name: "open selected lazy" }));
    await waitFor(() => expect(capturedSlides.mock.calls.at(-1)?.[0]).toHaveLength(2));
    expect(loadOriginal).not.toHaveBeenCalled();
    await act(async () => { originals[0].finish(); });
    await waitFor(() => expect(loadOriginal).toHaveBeenCalledTimes(1));
    expect(originals[2].load).not.toHaveBeenCalled();
    await act(async () => { originals[1].finish(); });
    await waitFor(() => expect(capturedSlides.mock.calls.at(-1)?.[0][1].src).toBe("H:\\wallpapers\\lazy-original.jpg"));
  });

  it("does not start a queued original after unmount", async () => {
    const originals = deferredOriginals(3);
    const view = render(<ImageViewerProvider locale="en"><ProvidedGalleryTrigger slides={originals.map((entry) => entry.slide)} /></ImageViewerProvider>);
    fireEvent.click(screen.getByRole("button", { name: "open provided" }));
    await waitFor(() => expect(originals[0].load).toHaveBeenCalledTimes(1));
    fireEvent.click(screen.getByTestId("lightbox-next"));
    fireEvent.click(screen.getByTestId("lightbox-next"));
    view.unmount();
    await act(async () => { originals[0].finish(); originals[1].finish(); });
    expect(originals[2].load).not.toHaveBeenCalled();
  });

  it("retains a failed original's placeholder and retries only on request", async () => {
    loadOriginal.mockRejectedValueOnce(new Error("download_failed: private upstream detail"));
    render(<ImageViewerProvider locale="en"><InitiallyLazyGalleryTrigger /></ImageViewerProvider>);
    fireEvent.click(screen.getByRole("button", { name: "open selected lazy" }));
    await screen.findByTestId("lightbox");
    await waitFor(() => expect(capturedSlides.mock.calls.at(-1)?.[0][1].originalStatus).toBe("error"));
    expect(capturedSlides.mock.calls.at(-1)?.[0][1].src).toBe(LAZY_PLACEHOLDER);
    expect(capturedSlides.mock.calls.at(-1)?.[0][1].originalError).toBeUndefined();
    fireEvent.click(screen.getByTestId("lightbox-view-current"));
    fireEvent.resize(window);
    await act(async () => { await new Promise((resolve) => setTimeout(resolve, 150)); });
    expect(loadOriginal).toHaveBeenCalledTimes(1);

    let complete: ((value: Awaited<ReturnType<typeof loadOriginal>>) => void) | undefined;
    loadOriginal.mockImplementationOnce(() => new Promise((resolve) => { complete = resolve; }));
    fireEvent.click(screen.getByTestId("lightbox-retry"));
    fireEvent.click(screen.getByTestId("lightbox-retry"));
    expect(loadOriginal).toHaveBeenCalledTimes(2);
    expect(capturedSlides.mock.calls.at(-1)?.[0][1].originalStatus).toBe("loading");
    await act(async () => { complete?.({src: "H:\\wallpapers\\recovered.jpg", kind: "image", mime: "image/jpeg"}); });
    await waitFor(() => expect(capturedSlides.mock.calls.at(-1)?.[0][1].src).toBe("H:\\wallpapers\\recovered.jpg"));
    expect(capturedSlides.mock.calls.at(-1)?.[0][1].originalStatus).toBeUndefined();
    expect(capturedSlides.mock.calls.at(-1)?.[0][0].src).toBe("H:\\wallpapers\\first.jpg");
  });

  it("retains the thumbnail when the downloaded original cannot decode", async () => {
    loadImageNaturalSize.mockResolvedValueOnce({ width: 0, height: 0 });
    render(<ImageViewerProvider locale="en"><InitiallyLazyGalleryTrigger /></ImageViewerProvider>);
    fireEvent.click(screen.getByRole("button", { name: "open selected lazy" }));
    await waitFor(() => expect(capturedSlides.mock.calls.at(-1)?.[0][1].originalStatus).toBe("error"));
    expect(capturedSlides.mock.calls.at(-1)?.[0][1].src).toBe(LAZY_PLACEHOLDER);
    fireEvent.click(screen.getByTestId("lightbox-retry"));
    await waitFor(() => expect(capturedSlides.mock.calls.at(-1)?.[0][1].src).toBe("H:\\wallpapers\\lazy-original.jpg"));
  });

  it("ignores a failed original after closing and opening another gallery", async () => {
    let reject: ((error: Error) => void) | undefined;
    loadOriginal.mockImplementationOnce(() => new Promise((_resolve, fail) => { reject = fail; }));
    render(<ImageViewerProvider locale="en"><InitiallyLazyGalleryTrigger /><GalleryTrigger /></ImageViewerProvider>);
    fireEvent.click(screen.getByRole("button", { name: "open selected lazy" }));
    await waitFor(() => expect(loadOriginal).toHaveBeenCalledTimes(1));
    fireEvent.click(screen.getByTestId("lightbox-close"));
    fireEvent.click(screen.getByRole("button", { name: "open" }));
    await waitFor(() => expect(capturedSlides.mock.calls.at(-1)?.[0][1].src).toBe("https://example.test/selected.jpg"));
    await act(async () => { reject?.(new Error("download_failed")); });
    expect(capturedSlides.mock.calls.at(-1)?.[0][1].originalStatus).toBeUndefined();
    expect(capturedSlides.mock.calls.at(-1)?.[0][1].src).toBe("https://example.test/selected.jpg");
  });

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
