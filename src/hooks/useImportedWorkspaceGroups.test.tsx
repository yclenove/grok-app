/** @vitest-environment jsdom */
import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { useImportedWorkspaceGroups } from "./useImportedWorkspaceGroups";
const mocks = vi.hoisted(() => ({ groups: vi.fn(), listen: vi.fn() }));
vi.mock("@/lib/api/sessionWorkspaceAssociation", () => ({ importedWorkspaceGroups: mocks.groups }));
vi.mock("@tauri-apps/api/event", () => ({ listen: mocks.listen }));
afterEach(() => { cleanup(); vi.resetAllMocks(); vi.useRealTimers(); });
beforeEach(() => { mocks.listen.mockResolvedValue(() => {}); });
const item = (id: string) => ({ sourceWorkspaceId: id, displayName: id, sourceId: "source", provider: "codex", sourceLocator: `/repo/${id}`, missing: false });

test("group window stays bounded while an off-page selection survives", async () => {
  mocks.groups.mockImplementation(async (cursor, selectedId) => {
    const start = cursor ? Number(cursor) : 0;
    return { items: Array.from({ length: 50 }, (_, i) => item(String(start + i))), nextCursor: String(start + 50), selected: item(selectedId) };
  });
  const { result } = renderHook(() => useImportedWorkspaceGroups("selected"));
  await waitFor(() => expect(result.current.items).toHaveLength(50));
  for (let n = 0; n < 5; n++) await act(() => result.current.loadMore());
  expect(result.current.items).toHaveLength(200);
  expect(result.current.items[0].sourceWorkspaceId).toBe("100");
  expect(result.current.selected?.sourceWorkspaceId).toBe("selected");
  expect(result.current.windowed).toBe(true);
  await act(() => result.current.refresh());
  expect(result.current.items[0].sourceWorkspaceId).toBe("0");
});

test("failed pages retry the same cursor and duplicate load clicks do not race", async () => {
  mocks.groups.mockResolvedValueOnce({ items: [item("one")], nextCursor: "next", selected: null }).mockRejectedValueOnce("SOURCE_BUSY");
  const { result } = renderHook(() => useImportedWorkspaceGroups(null));
  await waitFor(() => expect(result.current.cursor).toBe("next"));
  await act(() => result.current.loadMore());
  expect(result.current.cursor).toBe("next");
  expect(result.current.items).toHaveLength(1);
  let resolve!: (value: unknown) => void;
  mocks.groups.mockReturnValueOnce(new Promise((done) => { resolve = done; }));
  act(() => { void result.current.loadMore(); void result.current.loadMore(); });
  expect(mocks.groups).toHaveBeenCalledTimes(3);
  await act(async () => { resolve({ items: [item("two")], nextCursor: null, selected: null }); });
  expect(result.current.items).toHaveLength(2);
  expect(result.current.error).toBeNull();
});

test("late selected-group reply cannot replace a new selection", async () => {
  let resolve!: (value: unknown) => void;
  mocks.groups.mockReturnValueOnce(new Promise((done) => { resolve = done; }))
    .mockResolvedValueOnce({ items: [item("two")], nextCursor: null, selected: item("two") });
  const { result, rerender } = renderHook(({ id }) => useImportedWorkspaceGroups(id), { initialProps: { id: "one" } });
  rerender({ id: "two" });
  await waitFor(() => expect(result.current.selected?.sourceWorkspaceId).toBe("two"));
  await act(async () => { resolve({ items: [item("one")], nextCursor: "old", selected: item("one") }); });
  expect(result.current.selected?.sourceWorkspaceId).toBe("two");
  expect(result.current.cursor).toBeNull();
});

test("group progress bursts do not race an existing read or lose the selected group", async () => {
  vi.useFakeTimers();
  let resolve!: (value: unknown) => void;
  mocks.groups.mockReturnValueOnce(new Promise((done) => { resolve = done; }))
    .mockResolvedValue({ items: [item("updated")], nextCursor: null, selected: item("selected") });
  const { result, unmount } = renderHook(() => useImportedWorkspaceGroups("selected"));
  await act(async () => {});
  const changed = mocks.listen.mock.calls.find(([event]) => event === "session-import://changed")![1];
  await act(async () => { for (let i = 0; i < 10_000; i++) changed(); await vi.advanceTimersByTimeAsync(1000); });
  expect(mocks.groups).toHaveBeenCalledOnce();
  await act(async () => { resolve({ items: [item("initial")], nextCursor: null, selected: item("selected") }); });
  await act(async () => { await vi.advanceTimersByTimeAsync(1000); });
  expect(mocks.groups).toHaveBeenCalledTimes(2);
  expect(result.current.items[0].sourceWorkspaceId).toBe("updated");
  expect(result.current.selected?.sourceWorkspaceId).toBe("selected");
  act(() => changed());
  unmount();
  await act(async () => { await vi.advanceTimersByTimeAsync(1000); });
  expect(mocks.groups).toHaveBeenCalledTimes(2);
});
