/** @vitest-environment jsdom */
import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { useImportedEntry } from "./useImportedEntry";
import type { CatalogEntry } from "@/lib/api/sessionMigration";
const { lookup } = vi.hoisted(() => ({ lookup: vi.fn() }));
vi.mock("@/lib/api/sessionMigration", () => ({ sessionImportEntry: lookup }));
const entry: CatalogEntry = { importId: "imp:a", installationId: "synthetic", provider: "generic-file",
  nativeSessionId: "a.json", title: "Archive A", alias: "First alias", hidden: false, currentRevision: "latest", createdAt: "2026-09-30T00:00:00Z" };
beforeEach(() => { vi.resetAllMocks(); lookup.mockResolvedValue(entry); });
afterEach(cleanup);

test("off-page metadata follows catalog refreshes without owning reader scope", async () => {
  const view = renderHook(({ version }) => useImportedEntry("imp:a", undefined, version), { initialProps: { version: 0 } });
  await waitFor(() => expect(view.result.current.entry?.alias).toBe("First alias"));
  lookup.mockResolvedValue({ ...entry, alias: "Changed elsewhere" }); view.rerender({ version: 1 });
  await waitFor(() => expect(view.result.current.entry?.alias).toBe("Changed elsewhere"));
  expect(lookup).toHaveBeenCalledTimes(2);
});

test("a catalog hit supersedes an in-flight exact lookup", async () => {
  let resolve!: (entry: CatalogEntry) => void;
  lookup.mockReturnValue(new Promise<CatalogEntry>((done) => { resolve = done; }));
  const view = renderHook(({ known }: { known?: CatalogEntry }) => useImportedEntry("imp:a", known), { initialProps: {} });
  view.rerender({ known: { ...entry, alias: "Authoritative catalog" } });
  await act(async () => resolve(entry));
  expect(view.result.current.entry?.alias).toBe("Authoritative catalog"); expect(view.result.current.error).toBe(false);
});

test("mismatched metadata is rejected and retry recovers", async () => {
  lookup.mockResolvedValueOnce({ ...entry, importId: "imp:wrong" });
  const view = renderHook(() => useImportedEntry("imp:a"));
  await waitFor(() => expect(view.result.current.error).toBe(true));
  expect(view.result.current.entry).toBeNull(); act(() => view.result.current.reload());
  await waitFor(() => expect(view.result.current.entry?.importId).toBe("imp:a"));
});

test("unknown metadata settles without falling back to another archive", async () => {
  lookup.mockResolvedValue(null);
  const view = renderHook(() => useImportedEntry("imp:missing", entry));
  await waitFor(() => expect(view.result.current.busy).toBe(false));
  expect(view.result.current.entry).toBeNull(); expect(view.result.current.error).toBe(false);
});

test("closed readers perform no metadata lookup", () => {
  const view = renderHook(() => useImportedEntry(null, entry));
  expect(lookup).not.toHaveBeenCalled(); expect(view.result.current.entry).toBeNull(); expect(view.result.current.busy).toBe(false);
});
