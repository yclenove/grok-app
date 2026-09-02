/**
 * @vitest-environment jsdom
 */
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { WallpaperGalleryItem } from "@/lib/wallpaperSource";

const peekThumbnail = vi.hoisted(() => vi.fn());
const resolveThumbnail = vi.hoisted(() => vi.fn());
const forgetThumbnail = vi.hoisted(() => vi.fn());
const subscribeThumbnail = vi.hoisted(() => vi.fn());

vi.mock("@/lib/remoteWallpaperThumbnail", () => ({
  peekRemoteWallpaperThumbnail: peekThumbnail,
  resolveRemoteWallpaperThumbnail: resolveThumbnail,
  forgetRemoteWallpaperThumbnail: forgetThumbnail,
  subscribeRemoteWallpaperThumbnail: subscribeThumbnail,
}));

import { RemoteWallpaperThumbnail } from "./RemoteWallpaperThumbnail";

const DATA_URL = `data:image/jpeg;base64,${"A".repeat(64)}`;
const ITEM: WallpaperGalleryItem = {
  id: "web-image",
  thumbUrl: "https://cdn.example.test/photo.jpg",
  fullUrl: "https://cdn.example.test/photo.jpg",
  kind: "image",
  width: 1920,
  height: 1080,
  source: "web",
  sourceUrl: "https://photos.example.test/item",
};

beforeEach(() => {
  peekThumbnail.mockReset();
  resolveThumbnail.mockReset();
  forgetThumbnail.mockReset();
  subscribeThumbnail.mockReset();
  peekThumbnail.mockReturnValue(null);
  subscribeThumbnail.mockReturnValue(() => undefined);
});

afterEach(() => cleanup());

describe("RemoteWallpaperThumbnail", () => {
  it("uses the direct URL first and recovers through a Host thumbnail", async () => {
    resolveThumbnail.mockResolvedValue(DATA_URL);
    const onUnavailable = vi.fn();
    const view = render(
      <RemoteWallpaperThumbnail
        item={ITEM}
        alt="remote image"
        onUnavailable={onUnavailable}
      />,
    );

    const direct = screen.getByRole("img", { name: "remote image" });
    expect(direct.getAttribute("src")).toBe(ITEM.thumbUrl);
    fireEvent.error(direct);
    expect(
      view.container.querySelector('[data-state="loading"]'),
    ).toBeTruthy();

    const recovered = await screen.findByRole("img", { name: "remote image" });
    expect(recovered.getAttribute("src")).toBe(DATA_URL);
    expect(recovered.getAttribute("data-host-thumbnail")).toBe("true");
    expect(onUnavailable).not.toHaveBeenCalled();
  });

  it("drops the card only after both the direct and Host paths fail", async () => {
    resolveThumbnail.mockResolvedValue(null);
    const onUnavailable = vi.fn();
    render(
      <RemoteWallpaperThumbnail
        item={ITEM}
        alt="remote image"
        onUnavailable={onUnavailable}
      />,
    );

    fireEvent.error(screen.getByRole("img", { name: "remote image" }));
    await waitFor(() => expect(onUnavailable).toHaveBeenCalledWith(ITEM.id));
  });

  it("drops a corrupted Host thumbnail and cools down the cache key", async () => {
    peekThumbnail.mockReturnValue(DATA_URL);
    const onUnavailable = vi.fn();
    render(
      <RemoteWallpaperThumbnail
        item={ITEM}
        alt="remote image"
        onUnavailable={onUnavailable}
      />,
    );

    const fallback = screen.getByRole("img", { name: "remote image" });
    fireEvent.error(fallback);
    expect(forgetThumbnail).toHaveBeenCalled();
    expect(onUnavailable).toHaveBeenCalledWith(ITEM.id);
  });
});
