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
  it("keeps licensed attribution compact and opens every exact link", () => {
    const onOpen = vi.fn();
    const { container } = render(
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

    const source = screen.getByRole("button", { name: "Openverse" });
    expect(container.querySelector(".wallpaper-attribution--licensed")).not.toBeNull();
    expect(source.querySelector("svg")).not.toBeNull();
    expect(source.textContent).toBe("");
    expect(screen.queryByText("·")).toBeNull();

    fireEvent.click(source);
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

  it("keeps the Web source label visible when author metadata exists", () => {
    const onOpen = vi.fn();
    const { container } = render(
      <WallpaperSourceAttribution
        item={item({
          source: "web",
          sourceName: "photos.example.test",
          sourceUrl: "https://photos.example.test/story",
          authorName: "A. Reporter",
          authorUrl: "https://photos.example.test/authors/reporter",
        })}
        t={(key: MessageKey) => key}
        disabled={false}
        onOpen={onOpen}
      />,
    );

    const source = screen.getByRole("button", {
      name: "photos.example.test",
    });
    expect(source.textContent).toBe("photos.example.test");
    expect(source.querySelector("svg")).toBeNull();
    expect(screen.getByRole("button", { name: "A. Reporter" })).toBeTruthy();
    expect(container.querySelector(".wallpaper-attribution--licensed")).toBeNull();

    fireEvent.click(source);
    fireEvent.click(screen.getByRole("button", { name: "A. Reporter" }));
    expect(onOpen.mock.calls).toEqual([
      ["https://photos.example.test/story"],
      ["https://photos.example.test/authors/reporter"],
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
