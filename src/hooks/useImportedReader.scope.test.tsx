/** @vitest-environment jsdom */
import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { useImportedReader } from "./useImportedReader";

const mocks = vi.hoisted(() => ({ events: vi.fn(), search: vi.fn(), context: vi.fn(), exportMd: vi.fn() }));
vi.mock("@/lib/api/sessionMigration", () => ({
  sessionImportEvents: mocks.events, sessionImportSearch: mocks.search,
  sessionImportEventContext: mocks.context, sessionImportExportMarkdown: mocks.exportMd,
}));
function page(revision = "revision-a", leaf = "leaf-a", id = "first") {
  return { events: [{ id, role: "user", blocks: [{ text: id }] }], revision, leaf,
    previousCursor: "before", nextCursor: "after", total: 3, start: 0, end: 1 };
}
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((yes) => { resolve = yes; });
  return { promise, resolve };
}
beforeEach(() => {
  vi.resetAllMocks();
  mocks.events.mockResolvedValue(page());
  mocks.context.mockResolvedValue({ ...page("revision-a", "leaf-a", "target"), targetIndex: 0, ordinal: 0, position: 0, anchorCursor: "anchor" });
  mocks.search.mockResolvedValue({ hits: [{ eventId: "target", snippet: "match", kind: "message" }], previousCursor: "hit-before", nextCursor: "hit-after" });
  mocks.exportMd.mockResolvedValue("# snapshot");
  Object.assign(navigator, { clipboard: { writeText: vi.fn().mockResolvedValue(undefined) } });
});
afterEach(cleanup);

test("an unversioned reader pins the accepted archive for search, paging, jump, reload and export", async () => {
  const { result } = renderHook(() => useImportedReader("imp:snapshot", vi.fn()));
  await waitFor(() => expect(result.current.busy).toBe(false));
  expect(mocks.events).toHaveBeenNthCalledWith(1, "imp:snapshot", null, null, 100, null);
  act(() => result.current.setQuery("中文 draft"));
  await act(async () => result.current.search());
  expect(mocks.search).toHaveBeenLastCalledWith("imp:snapshot", "中文 draft", "revision-a", null, "leaf-a");
  await act(async () => result.current.loadMoreHits());
  expect(mocks.search).toHaveBeenLastCalledWith("imp:snapshot", "中文 draft", "revision-a", "hit-after", "leaf-a", "forward");
  await act(async () => result.current.loadPreviousHits());
  expect(mocks.search).toHaveBeenLastCalledWith("imp:snapshot", "中文 draft", "revision-a", "hit-before", "leaf-a", "backward");
  await act(async () => result.current.ensureEvent("target"));
  expect(mocks.context).toHaveBeenLastCalledWith("imp:snapshot", "target", "revision-a", "leaf-a", { before: 8, after: 8 });
  await act(async () => result.current.loadMore());
  expect(mocks.events).toHaveBeenLastCalledWith("imp:snapshot", "revision-a", "after", 100, "leaf-a");
  await act(async () => result.current.loadPrevious());
  expect(mocks.events).toHaveBeenLastCalledWith("imp:snapshot", "revision-a", "before", 100, "leaf-a", "backward");
  await act(async () => result.current.reload());
  expect(mocks.events).toHaveBeenLastCalledWith("imp:snapshot", "revision-a", null, 100, "leaf-a");
  await act(async () => result.current.exportMarkdown());
  expect(mocks.exportMd).toHaveBeenCalledWith("imp:snapshot", "revision-a", "leaf-a");
  expect(result.current).toMatchObject({ scope: { revision: "revision-a", leaf: "leaf-a" }, query: "中文 draft", error: null });
});

test("a winning anchor context pins the snapshot; superseded initial data cannot rebind it", async () => {
  const initial = deferred<ReturnType<typeof page>>();
  mocks.events.mockReturnValueOnce(initial.promise);
  mocks.context.mockResolvedValueOnce({ ...page("revision-b", "leaf-b", "target"), targetIndex: 0, ordinal: 0, position: 0, anchorCursor: "anchor-b" });
  const { result } = renderHook(() => useImportedReader("imp:anchor", vi.fn()));
  await act(async () => result.current.ensureEvent("target"));
  await act(async () => initial.resolve(page()));
  act(() => result.current.setQuery("kept"));
  await act(async () => result.current.search());
  expect(mocks.search).toHaveBeenLastCalledWith("imp:anchor", "kept", "revision-b", null, "leaf-b");
  expect(result.current).toMatchObject({ scope: { revision: "revision-b", leaf: "leaf-b" }, events: page("revision-b", "leaf-b", "target").events });
});

test("search and export wait for the first accepted snapshot instead of resolving latest independently", async () => {
  const initial = deferred<ReturnType<typeof page>>();
  mocks.events.mockReturnValueOnce(initial.promise);
  const { result } = renderHook(() => useImportedReader("imp:pending", vi.fn()));
  act(() => result.current.setQuery("正在输入"));
  await act(async () => { await result.current.search(); await result.current.exportMarkdown(); });
  expect(mocks.search).not.toHaveBeenCalled();
  expect(mocks.exportMd).not.toHaveBeenCalled();
  await act(async () => initial.resolve(page()));
  expect(result.current.query).toBe("正在输入");
});

test.each([
  ["revision-b", "leaf-a"], ["revision-a", "leaf-b"], ["", "leaf-a"], ["revision-a", ""],
])("rejects a response outside the explicit snapshot (%s / %s)", async (revision, leaf) => {
  mocks.events.mockResolvedValueOnce(page(revision, leaf));
  const onError = vi.fn();
  const { result } = renderHook(() => useImportedReader("imp:fixed", onError, { revision: "revision-a", leaf: "leaf-a" }));
  await waitFor(() => expect(result.current.busy).toBe(false));
  expect(result.current.events).toEqual([]);
  expect(result.current.error).not.toBeNull();
  expect(onError).toHaveBeenCalledTimes(1);
});

test("rejects a later revision mismatch without mixing events or cursors", async () => {
  const { result } = renderHook(() => useImportedReader("imp:fixed", vi.fn()));
  await waitFor(() => expect(result.current.busy).toBe(false));
  mocks.events.mockResolvedValueOnce({ ...page("revision-b", "leaf-a", "wrong"), nextCursor: "wrong-cursor" });
  await act(async () => result.current.loadMore());
  expect(result.current.events).toEqual(page().events);
  expect(result.current.nextCursor).toBe("after");
  expect(result.current.error).not.toBeNull();
});

test("explicit revision navigation clears state and old results cannot pin the new reader", async () => {
  const old = deferred<ReturnType<typeof page>>();
  mocks.events.mockReturnValueOnce(old.promise).mockResolvedValueOnce(page("revision-b", "leaf-b", "new"));
  const { result, rerender } = renderHook(({ revision }) => useImportedReader("imp:route", vi.fn(), { revision }), { initialProps: { revision: "revision-a" } });
  act(() => result.current.setQuery("old draft"));
  rerender({ revision: "revision-b" });
  await waitFor(() => expect(result.current.busy).toBe(false));
  await act(async () => old.resolve(page()));
  expect(result.current).toMatchObject({ scope: { revision: "revision-b", leaf: "leaf-b" }, query: "", events: page("revision-b", "leaf-b", "new").events });
  await act(async () => result.current.exportMarkdown());
  expect(mocks.exportMd).toHaveBeenLastCalledWith("imp:route", "revision-b", "leaf-b");
});


test.each([undefined, null, 7])("rejects missing or malformed scope fields (%s)", async (revision) => {
  mocks.events.mockResolvedValueOnce({ ...page(), revision });
  const { result } = renderHook(() => useImportedReader("imp:invalid", vi.fn()));
  await waitFor(() => expect(result.current.busy).toBe(false));
  expect(result.current.scope).toBeNull();
  expect(result.current.events).toEqual([]);
  expect(result.current.error).toContain("PLAN_STALE:");
});
test("a conflicting context cannot rebind a previously accepted snapshot", async () => {
  const { result } = renderHook(() => useImportedReader("imp:context-conflict", vi.fn()));
  await waitFor(() => expect(result.current.busy).toBe(false));
  mocks.context.mockResolvedValueOnce({ ...page("revision-b", "leaf-b", "target"), targetIndex: 0, ordinal: 0, position: 0, anchorCursor: "foreign" });
  await act(async () => expect(await result.current.ensureEvent("target")).toBe(false));
  expect(result.current.getScope()).toEqual({ revision: "revision-a", leaf: "leaf-a" });
  expect(result.current.events).toEqual(page().events);
});
test("a captured scope accessor cannot authorize continuation after the reader closes", async () => {
  const { result, rerender } = renderHook(({ id }: { id: string | null }) => useImportedReader(id, vi.fn()), { initialProps: { id: "imp:closing" as string | null } });
  await waitFor(() => expect(result.current.busy).toBe(false));
  const oldScope = result.current.getScope;
  rerender({ id: null });
  expect(oldScope()).toBeNull(); expect(result.current.scope).toBeNull();
});
