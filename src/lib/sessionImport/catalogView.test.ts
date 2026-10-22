/** @vitest-environment jsdom */
import { afterEach, expect, test } from "vitest";
import { appendImportedWindow, readCatalogView, writeCatalogView } from "./catalogView";
afterEach(() => sessionStorage.clear());
test("damaged and oversized preferences recover to all histories", () => {
  sessionStorage.setItem("grok:imported-catalog-view:v1:sidebar", "broken");
  expect(readCatalogView("sidebar").workspace).toBeNull();
  writeCatalogView("sidebar", { query: "x".repeat(2049), provider: null, sort: "newest", workspace: null });
  expect(readCatalogView("sidebar").query).toBe("");
});
test("normalized preferences stay separate for settings and sidebar", () => {
  const view = { query: "needle", provider: "codex", sort: "oldest" as const, workspace: { kind: "workspace" as const, sourceWorkspaceId: "ws:one" } };
  writeCatalogView("sidebar", view);
  expect(readCatalogView("sidebar")).toEqual(view);
  expect(readCatalogView("settings").workspace).toBeNull();
  expect(readCatalogView().workspace).toBeNull();
  sessionStorage.setItem("grok:imported-catalog-view:v1:sidebar", JSON.stringify({ ...view, workspace: { ...view.workspace, unexpected: "ignored" } }));
  expect(readCatalogView("sidebar").workspace).toEqual(view.workspace);
});
test("overlapping pages update existing rows without allowing the window to grow", () => {
  const old = Array.from({ length: 200 }, (_, id) => ({ id, name: "old" }));
  const next = appendImportedWindow(old, [{ id: 199, name: "new" }, { id: 200, name: "new" }], (row) => String(row.id));
  expect(next.items).toHaveLength(200);
  expect(next.items[0].id).toBe(1);
  expect(next.items[198].name).toBe("new");
  expect(next.trimmed).toBe(true);
});
