import { afterEach, expect, test, vi } from "vitest";

afterEach(() => {
  vi.doUnmock("./messages/de");
  vi.resetModules();
});

test("a rejected lazy catalog load does not poison later retries", async () => {
  vi.resetModules();
  let unavailable = true;
  vi.doMock("./messages/de", async () => {
    const original = await vi.importActual<typeof import("./messages/de")>("./messages/de");
    return {
      get de() {
        if (unavailable) throw new Error("catalog unavailable");
        return original.de;
      },
    };
  });
  const { loadLocaleCatalog, isLocaleCatalogReady } = await import("./messages");
  await expect(loadLocaleCatalog("de")).rejects.toThrow("catalog unavailable");
  expect(isLocaleCatalogReady("de")).toBe(false);
  unavailable = false;
  await loadLocaleCatalog("de");
  expect(isLocaleCatalogReady("de")).toBe(true);
});
