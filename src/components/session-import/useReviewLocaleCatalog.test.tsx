/** @vitest-environment jsdom */
import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import type { Locale } from "@/i18n";

const catalog = vi.hoisted(() => ({
  ready: new Set<string>(),
  load: vi.fn<(locale: string) => Promise<void>>(),
}));

vi.mock("@/i18n", async (importOriginal) => ({
  ...await importOriginal<typeof import("@/i18n")>(),
  isLocaleCatalogReady: (locale: string) => locale === "en" || catalog.ready.has(locale),
  loadLocaleCatalog: (locale: string) => catalog.load(locale),
}));

import { useReviewLocaleCatalog } from "./useReviewLocaleCatalog";

afterEach(() => {
  cleanup();
  catalog.ready.clear();
  catalog.load.mockReset();
});

test("cold entry waits for the requested catalog despite global test setup", async () => {
  let resolve!: () => void;
  catalog.load.mockImplementation(() => new Promise<void>((done) => { resolve = done; }));
  const { result } = renderHook(() => useReviewLocaleCatalog("zh"));
  expect(result.current.ready).toBe(false);
  expect(catalog.load).toHaveBeenCalledWith("zh");
  await act(async () => { catalog.ready.add("zh"); resolve(); });
  expect(result.current.ready).toBe(true);
  expect(result.current.error).toBe(false);
});

test("a late prior request cannot mark the newly selected locale ready", async () => {
  const pending = new Map<string, () => void>();
  catalog.load.mockImplementation((locale) => new Promise<void>((done) => { pending.set(locale, done); }));
  const { result, rerender } = renderHook(({ locale }) => useReviewLocaleCatalog(locale), {
    initialProps: { locale: "zh" as Locale },
  });
  rerender({ locale: "de" });
  await act(async () => { catalog.ready.add("zh"); pending.get("zh")!(); });
  expect(result.current.ready).toBe(false);
  await act(async () => { catalog.ready.add("de"); pending.get("de")!(); });
  expect(result.current.ready).toBe(true);
});

test("a failed load is visible and a retry can recover", async () => {
  catalog.load.mockRejectedValueOnce(new Error("catalog unavailable"));
  const { result } = renderHook(() => useReviewLocaleCatalog("de"));
  await waitFor(() => expect(result.current.error).toBe(true));
  expect(result.current.ready).toBe(false);
  catalog.load.mockImplementation(async () => { catalog.ready.add("de"); });
  act(() => result.current.retry());
  await waitFor(() => expect(result.current.ready).toBe(true));
  expect(result.current.error).toBe(false);
  expect(catalog.load).toHaveBeenCalledTimes(2);
});
