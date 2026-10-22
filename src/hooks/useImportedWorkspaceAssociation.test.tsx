/** @vitest-environment jsdom */
import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import type { ImportedWorkspaceAssociation, ImportedWorkspaceChoice } from "@/lib/api/sessionWorkspaceAssociation";
import { useImportedWorkspaceAssociation } from "./useImportedWorkspaceAssociation";

const mocks = vi.hoisted(() => ({ get: vi.fn(), options: vi.fn(), update: vi.fn() }));
vi.mock("@/lib/api/sessionWorkspaceAssociation", () => ({
  importedWorkspaceGet: mocks.get, importedWorkspaceOptions: mocks.options, importedWorkspaceUpdate: mocks.update,
}));
afterEach(() => { cleanup(); vi.resetAllMocks(); });
const one: ImportedWorkspaceChoice = { sourceWorkspaceId: "ws:one", revision: "a".repeat(64), displayName: "One" };
const two: ImportedWorkspaceChoice = { ...one, sourceWorkspaceId: "ws:two", displayName: "Two" };
function record(importId = "imp:one", choice: ImportedWorkspaceChoice | null = null): ImportedWorkspaceAssociation {
  return { importId, sourceId: "source", provider: "codex", sourceReady: true, version: choice ? 1 : 0, choice, status: choice ? "assigned" : "unassigned" };
}
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => { resolve = done; });
  return { resolve, promise };
}
beforeEach(() => {
  mocks.get.mockImplementation(async (id) => record(id));
  mocks.options.mockResolvedValue({ items: [one, two], nextCursor: null });
  mocks.update.mockImplementation(async (input) => ({ ...record(input.importId, input.choice), version: input.expectedVersion + 1 }));
});

test("loading is read only; draft choice is saved explicitly and clear advances the version", async () => {
  const { result } = renderHook(() => useImportedWorkspaceAssociation("imp:one"));
  await waitFor(() => expect(result.current.association?.version).toBe(0));
  expect(mocks.update).not.toHaveBeenCalled();
  act(() => result.current.choose(one.sourceWorkspaceId));
  expect(mocks.update).not.toHaveBeenCalled();
  await act(() => result.current.save());
  expect(mocks.update.mock.calls[0][0]).toMatchObject({ importId: "imp:one", expectedVersion: 0, choice: one });
  expect(result.current.association?.choice).toEqual(one);
  await act(() => result.current.save(true));
  expect(mocks.update.mock.calls[1][0]).toMatchObject({ expectedVersion: 1, choice: null });
  expect(result.current.association?.version).toBe(2);
  expect(result.current.draft).toBeNull();
});

test("lost response retry reuses the exact request while a changed choice gets a new request", async () => {
  mocks.update.mockRejectedValueOnce("SOURCE_BUSY: reply lost").mockRejectedValueOnce("SOURCE_BUSY: reply lost");
  const { result } = renderHook(() => useImportedWorkspaceAssociation("imp:one"));
  await waitFor(() => expect(result.current.busy).toBe(false));
  act(() => result.current.choose(one.sourceWorkspaceId));
  await act(() => result.current.save());
  expect(result.current.draft).toEqual(one);
  expect(result.current.error).toContain("SOURCE_BUSY");
  await act(() => result.current.save());
  expect(mocks.update.mock.calls[1][0]).toEqual(mocks.update.mock.calls[0][0]);
  act(() => result.current.choose(two.sourceWorkspaceId));
  await act(() => result.current.save());
  expect(mocks.update.mock.calls[2][0].requestId).not.toBe(mocks.update.mock.calls[0][0].requestId);
});

test("synchronous duplicate saves and changes during a save cannot race", async () => {
  const response = deferred<ImportedWorkspaceAssociation>();
  mocks.update.mockReturnValue(response.promise);
  const { result } = renderHook(() => useImportedWorkspaceAssociation("imp:one"));
  await waitFor(() => expect(result.current.association).not.toBeNull());
  act(() => result.current.choose(one.sourceWorkspaceId));
  let saving!: Promise<void>;
  act(() => { saving = result.current.save(); void result.current.save(); result.current.choose(two.sourceWorkspaceId); void result.current.refresh(); });
  expect(mocks.update).toHaveBeenCalledTimes(1);
  expect(result.current.draft).toEqual(one);
  await act(async () => { response.resolve(record("imp:one", one)); await saving; });
  expect(result.current.saved).toBe(true);
});

test("late read after switching history cannot load old options or replace the new history", async () => {
  const response = deferred<ImportedWorkspaceAssociation>();
  mocks.get.mockReturnValueOnce(response.promise);
  const { result, rerender } = renderHook(({ id }) => useImportedWorkspaceAssociation(id), { initialProps: { id: "imp:one" } });
  rerender({ id: "imp:two" });
  await waitFor(() => expect(result.current.association?.importId).toBe("imp:two"));
  await act(async () => { response.resolve(record("imp:one", one)); });
  expect(result.current.association?.importId).toBe("imp:two");
  expect(mocks.options.mock.calls.map(([id]) => id)).toEqual(["imp:two"]);
});

test("late save after close cannot restore a closed history", async () => {
  const response = deferred<ImportedWorkspaceAssociation>();
  mocks.update.mockReturnValueOnce(response.promise);
  const { result, rerender } = renderHook(({ id }: { id: string | null }) => useImportedWorkspaceAssociation(id), { initialProps: { id: "imp:one" as string | null } });
  await waitFor(() => expect(result.current.association).not.toBeNull());
  act(() => result.current.choose(one.sourceWorkspaceId));
  let saving!: Promise<void>;
  act(() => { saving = result.current.save(); });
  rerender({ id: null });
  await act(async () => { response.resolve(record("imp:one", one)); await saving; });
  expect(result.current.association).toBeNull();
  expect(result.current.items).toEqual([]);
  expect(result.current.busy).toBe(false);
  expect(result.current.saved).toBe(false);
});

test("failed refresh and page load retain draft and cursor; retry merges without duplicates", async () => {
  mocks.options.mockResolvedValueOnce({ items: [one], nextCursor: "page2" })
    .mockRejectedValueOnce("SOURCE_BUSY").mockRejectedValueOnce("SOURCE_BUSY")
    .mockResolvedValueOnce({ items: [one, two], nextCursor: null });
  const { result } = renderHook(() => useImportedWorkspaceAssociation("imp:one"));
  await waitFor(() => expect(result.current.items).toEqual([one]));
  act(() => result.current.choose(one.sourceWorkspaceId));
  await act(() => result.current.refresh());
  expect(result.current.draft).toEqual(one);
  expect(result.current.cursor).toBe("page2");
  await act(() => result.current.loadMore());
  expect(result.current.items).toEqual([one]);
  expect(result.current.cursor).toBe("page2");
  await act(() => result.current.loadMore());
  expect(result.current.items).toEqual([one, two]);
  expect(result.current.cursor).toBeNull();
  expect(result.current.draft).toEqual(one);
});

test("unknown source skips options but its previous association can still be cleared", async () => {
  mocks.get.mockResolvedValue({ ...record("imp:one", one), sourceReady: false, status: "stale" });
  const { result } = renderHook(() => useImportedWorkspaceAssociation("imp:one"));
  await waitFor(() => expect(result.current.association).not.toBeNull());
  expect(mocks.options).not.toHaveBeenCalled();
  await act(() => result.current.save(true));
  expect(mocks.update.mock.calls[0][0]).toMatchObject({ expectedVersion: 1, choice: null });
});

test("refreshing stale workspace offers the new revision but never saves it automatically", async () => {
  mocks.get.mockResolvedValue({ ...record("imp:one", one), status: "stale" });
  const updated = { ...one, revision: "b".repeat(64) };
  mocks.options.mockResolvedValue({ items: [updated], nextCursor: null });
  const { result } = renderHook(() => useImportedWorkspaceAssociation("imp:one"));
  await waitFor(() => expect(result.current.draft).toEqual(updated));
  expect(result.current.association?.choice).toEqual(one);
  expect(mocks.update).not.toHaveBeenCalled();
  await act(() => result.current.save());
  expect(mocks.update.mock.calls[0][0].choice).toEqual(updated);
});

test("paging hundreds of candidates preserves an unsaved off-window choice for explicit save", async () => {
  const choices = Array.from({ length: 300 }, (_, n) => ({ ...one, sourceWorkspaceId: `ws:${n}`, displayName: `Workspace ${n}` }));
  mocks.options.mockImplementation(async (_id, cursor) => {
    const start = Number(cursor ?? 0);
    return { items: choices.slice(start, start + 50), nextCursor: start < 250 ? String(start + 50) : null };
  });
  const { result } = renderHook(() => useImportedWorkspaceAssociation("imp:one"));
  await waitFor(() => expect(result.current.items).toHaveLength(50));
  act(() => result.current.choose(choices[0].sourceWorkspaceId));
  for (let page = 0; page < 5; page++) await act(() => result.current.loadMore());
  expect(result.current.items).toHaveLength(200);
  expect(result.current.items[0]).toEqual(choices[100]);
  expect(result.current.windowed).toBe(true);
  expect(result.current.draft).toEqual(choices[0]);
  expect(mocks.update).not.toHaveBeenCalled();
  await act(() => result.current.save());
  expect(mocks.update.mock.calls[0][0].choice).toEqual(choices[0]);
  await act(() => result.current.refresh());
  expect(result.current.items).toHaveLength(50);
  expect(result.current.windowed).toBe(false);
});
