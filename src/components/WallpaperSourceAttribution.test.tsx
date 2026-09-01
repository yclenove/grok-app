/**
 * @vitest-environment jsdom
 */
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { MessageKey } from "@/i18n";
import type { WallpaperGalleryItem } from "@/lib/wallpaperSource";
import { WallpaperSourceAttribution } from "./WallpaperSourceAttribution";

afterEach(() => cleanup());

function item(
  provenance: Partial<WallpaperGalleryItem>,
): WallpaperGalleryItem {
  return {
    id: "remote-image",
    thumbUrl: "https://images.example.test/thumb.jpg",
    fullUrl: "https://images.example.test/full.jpg",
    kind: "image",
    source: "openverse",
    ...provenance,
  };
}

describe("WallpaperSourceAttribution", () => {
  it("opens the exact source, author, and license links", () => {
    const onOpen = vi.fn();
    render(
      <WallpaperSourceAttribution
        item={item({
          sourceName: "Openverse",
          sourceUrl: "https://openverse.org/image/source",
          authorName: "A. Photographer",
          authorUrl: "https://example.test/author",
          license: "CC BY 4.0",
          licenseUrl: "https://creativecommons.org/licenses/by/4.0/",
        })}
        t={(key: MessageKey) => key}
        disabled={false}
        onOpen={onOpen}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "Openverse" }));
    fireEvent.click(
      screen.getByRole("button", { name: "A. Photographer" }),
    );
    fireEvent.click(screen.getByRole("button", { name: "CC BY 4.0" }));

    expect(onOpen.mock.calls).toEqual([
      ["https://openverse.org/image/source"],
      ["https://example.test/author"],
      ["https://creativecommons.org/licenses/by/4.0/"],
    ]);
  });

  it("renders non-linked metadata as text and requires a source link", () => {
    const { container, rerender } = render(
      <WallpaperSourceAttribution
        item={item({
          sourceName: "example.test",
          sourceUrl: "https://example.test/photo",
          authorName: "Unknown profile",
          license: "Publisher terms",
        })}
        t={(key: MessageKey) => key}
        disabled={false}
        onOpen={vi.fn()}
      />,
    );

    expect(screen.getByText("Unknown profile")).toBeTruthy();
    expect(
      screen.queryByRole("button", { name: "Unknown profile" }),
    ).toBeNull();

    rerender(
      <WallpaperSourceAttribution
        item={item({ sourceName: "example.test" })}
        t={(key: MessageKey) => key}
        disabled={false}
        onOpen={vi.fn()}
      />,
    );
    expect(container.firstChild).toBeNull();
  });
});
