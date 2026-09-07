/** @vitest-environment jsdom */
import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { useWallpaperMediaActions } from "./useWallpaperMediaActions";
import type { WallpaperGalleryItem } from "@/lib/wallpaperSource";
const mocks = vi.hoisted(() => ({ local: vi.fn(), remember: vi.fn() }));
vi.mock("@/lib/api", () => ({ wallpaperLibraryRemember: mocks.remember }));
vi.mock("@/lib/wallpaperSourceMedia", () => ({ ensureLocalWallpaperMedia: mocks.local }));
afterEach(() => { cleanup(); vi.resetAllMocks(); });
const item: WallpaperGalleryItem = { id: "one", source: "web", kind: "image", fullUrl: "https://example.org/photo.jpg", thumbUrl: "" };

it("persists a favorite after materializing the original and reports only confirmed state", async () => {
  mocks.local.mockResolvedValue({ path: "/library/photo.jpg" });
  mocks.remember.mockResolvedValue({ id: "media-id", favorite: true });
  const onChanged = vi.fn();
  const onError = vi.fn();
  const hook = renderHook(() => useWallpaperMediaActions({ open: true, source: "web", onChanged, onError }));
  await act(async () => hook.result.current.toggleFavorite(item));
  expect(mocks.remember).toHaveBeenCalledWith("/library/photo.jpg", item, true);
  expect(onChanged).toHaveBeenCalledWith({ ...item, localPath: "/library/photo.jpg", metadata: { id: "media-id", favorite: true } });
  expect(hook.result.current.busyIds.size).toBe(0);
  expect(onError).not.toHaveBeenCalled();
});

it("fences a late download on source switch before it can save a favorite", async () => {
  let resolve!: (value: { path: string }) => void;
  mocks.local.mockReturnValue(new Promise((done) => { resolve = done; }));
  const onChanged = vi.fn();
  const onError = vi.fn();
  const hook = renderHook(({ source }) => useWallpaperMediaActions({ open: true, source, onChanged, onError }), { initialProps: { source: "web" } });
  let pending!: Promise<void>;
  act(() => { pending = hook.result.current.toggleFavorite(item); });
  act(() => { void hook.result.current.toggleFavorite(item); });
  expect(mocks.local).toHaveBeenCalledTimes(1);
  hook.rerender({ source: "library" });
  await act(async () => { resolve({ path: "/library/photo.jpg" }); await pending; });
  expect(mocks.remember).not.toHaveBeenCalled();
  expect(onChanged).not.toHaveBeenCalled();
});

it("leaves the original state on write failure and allows an explicit retry", async () => {
  mocks.local.mockResolvedValue({ path: "/library/photo.jpg" });
  mocks.remember.mockRejectedValueOnce(new Error("catalog_write_failed")).mockResolvedValueOnce({ favorite: true });
  const onChanged = vi.fn();
  const onError = vi.fn();
  const hook = renderHook(() => useWallpaperMediaActions({ open: true, source: "web", onChanged, onError }));
  await act(async () => hook.result.current.toggleFavorite(item));
  expect(onChanged).not.toHaveBeenCalled();
  expect(onError).toHaveBeenCalledTimes(1);
  expect(mocks.remember).toHaveBeenCalledTimes(1);
  await act(async () => hook.result.current.toggleFavorite(item));
  expect(onChanged).toHaveBeenCalledTimes(1);
});
