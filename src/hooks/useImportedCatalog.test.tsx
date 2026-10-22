/** @vitest-environment jsdom */
import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { StrictMode } from "react";
import { useImportedCatalog } from "./useImportedCatalog";

const mocks = vi.hoisted(() => ({ entries: vi.fn(), listen: vi.fn() }));
vi.mock("@/lib/api/sessionMigration", () => ({
  sessionImportEntries: mocks.entries,
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: (...args: unknown[]) => mocks.listen(...args),
}));

beforeEach(() => {
  sessionStorage.clear();
  vi.resetAllMocks();
  mocks.entries.mockResolvedValue({
    items: [
      { importId: "imp:1", title: "Alpha chat", alias: null, hidden: false, provider: "codex", nativeSessionId: "n1", createdAt: "2026-06-01T00:00:00Z" },
    ],
    nextCursor: null,
    total: 1,
    providers: ["codex", "cursor"],
  });
  mocks.listen.mockResolvedValue(() => {});
});
afterEach(() => { cleanup(); vi.useRealTimers(); });

test("sends provider/query/sort to the backend instead of filtering a loaded page", async () => {
  const { result } = renderHook(() => useImportedCatalog());
  await waitFor(() => {
    expect(result.current.visible.map((e) => e.importId)).toEqual(["imp:1"]);
  });
  mocks.entries.mockClear();
  mocks.entries.mockResolvedValue({
    items: [
      { importId: "imp:2", title: "Beta", alias: "nick", hidden: false, provider: "cursor", nativeSessionId: "n2", createdAt: "2026-01-01T00:00:00Z" },
    ],
    nextCursor: null,
    total: 1,
    providers: ["codex", "cursor"],
  });
  act(() => result.current.setQuery("nick"));
  await waitFor(() => {
    expect(mocks.entries).toHaveBeenCalled();
  });
  const args = mocks.entries.mock.calls.at(-1);
  expect(args?.[0]).toBeNull();
  expect(args?.[2]).toMatchObject({ query: "nick", sort: "newest" });
  await waitFor(() => {
    expect(result.current.visible.map((e) => e.importId)).toEqual(["imp:2"]);
  });
});

test("filter change drops the old cursor and does not append the previous page", async () => {
  mocks.entries.mockResolvedValueOnce({
    items: [
      { importId: "imp:1", title: "Alpha chat", alias: null, hidden: false, provider: "codex", nativeSessionId: "n1", createdAt: "2026-06-01T00:00:00Z" },
    ],
    nextCursor: "cursor-codex",
    total: 4,
    providers: ["codex"],
  });
  const { result } = renderHook(() => useImportedCatalog());
  await waitFor(() => {
    expect(result.current.nextCursor).toBe("cursor-codex");
  });
  mocks.entries.mockResolvedValueOnce({
    items: [
      { importId: "imp:9", title: "Cursor row", alias: null, hidden: false, provider: "cursor", nativeSessionId: "n9", createdAt: "2026-02-01T00:00:00Z" },
    ],
    nextCursor: null,
    total: 1,
    providers: ["codex", "cursor"],
  });
  act(() => result.current.setProvider("cursor"));
  await waitFor(() => {
    expect(result.current.visible.map((e) => e.importId)).toEqual(["imp:9"]);
  });
  const last = mocks.entries.mock.calls.at(-1);
  expect(last?.[0]).toBeNull();
  expect(last?.[2]).toMatchObject({ provider: "cursor" });
});

test("loadMore appends the next catalog page with the same filter", async () => {
  mocks.entries
    .mockResolvedValueOnce({
      items: [
        { importId: "imp:1", title: "Alpha chat", alias: null, hidden: false, provider: "codex", nativeSessionId: "n1", createdAt: "2026-06-01T00:00:00Z" },
      ],
      nextCursor: "cursor-2",
      total: 2,
      providers: ["codex"],
    })
    .mockResolvedValueOnce({
      items: [
        { importId: "imp:2", title: "Beta", alias: "nick", hidden: false, provider: "codex", nativeSessionId: "n2", createdAt: "2026-01-01T00:00:00Z" },
      ],
      nextCursor: null,
      total: 2,
      providers: ["codex"],
    });
  const { result } = renderHook(() => useImportedCatalog());
  await waitFor(() => {
    expect(result.current.visible.map((e) => e.importId)).toEqual(["imp:1"]);
  });
  await act(async () => {
    await result.current.loadMore();
  });
  expect(result.current.visible.map((e) => e.importId)).toEqual(["imp:1", "imp:2"]);
  expect(mocks.entries.mock.calls[1][0]).toBe("cursor-2");
});

test("double loadMore while busy invokes the IPC once", async () => {
  let resolvePage!: (value: unknown) => void;
  mocks.entries.mockResolvedValueOnce({
    items: [
      { importId: "imp:1", title: "Alpha chat", alias: null, hidden: false, provider: "codex", nativeSessionId: "n1", createdAt: "2026-06-01T00:00:00Z" },
    ],
    nextCursor: "cursor-2",
    total: 2,
    providers: ["codex"],
  });
  const { result } = renderHook(() => useImportedCatalog());
  await waitFor(() => {
    expect(result.current.nextCursor).toBe("cursor-2");
  });
  mocks.entries.mockClear();
  mocks.entries.mockImplementationOnce(
    () =>
      new Promise((ok) => {
        resolvePage = ok;
      }),
  );
  act(() => {
    void result.current.loadMore();
  });
  act(() => {
    void result.current.loadMore();
  });
  expect(mocks.entries).toHaveBeenCalledTimes(1);
  await act(async () => {
    resolvePage({
      items: [
        { importId: "imp:2", title: "Beta", alias: null, hidden: false, provider: "codex", nativeSessionId: "n2", createdAt: "2026-01-01T00:00:00Z" },
      ],
      nextCursor: null,
      total: 2,
    });
  });
});

test("catalog load error is visible and not shown as an empty store", async () => {
  mocks.entries.mockRejectedValueOnce(new Error("catalog-down"));
  const { result } = renderHook(() => useImportedCatalog());
  await waitFor(() => {
    expect(result.current.error).toContain("catalog-down");
  });
  expect(result.current.visible).toEqual([]);
  expect(result.current.total).toBe(0);
});

test("workspace filter persists across remounts and keeps the reader scope unfiltered", async () => {
  const first = renderHook(() => useImportedCatalog("sidebar"));
  await waitFor(() => expect(first.result.current.busy).toBe(false));
  act(() => first.result.current.setWorkspace({ kind: "workspace", sourceWorkspaceId: "ws:one" }));
  await waitFor(() => expect(mocks.entries.mock.calls.at(-1)?.[2]).toMatchObject({ workspace: { kind: "workspace", sourceWorkspaceId: "ws:one" } }));
  first.unmount();
  const reopened = renderHook(() => useImportedCatalog("sidebar"));
  expect(reopened.result.current.workspace).toEqual({ kind: "workspace", sourceWorkspaceId: "ws:one" });
  const reader = renderHook(() => useImportedCatalog());
  expect(reader.result.current.workspace).toBeNull();
  await waitFor(() => expect(reader.result.current.busy).toBe(false));
});

test("old workspace page replies cannot append into a newly selected workspace", async () => {
  mocks.entries.mockResolvedValueOnce({ items: [{ importId: "old" }], nextCursor: "old-page", total: 8 });
  const { result } = renderHook(() => useImportedCatalog());
  await waitFor(() => expect(result.current.nextCursor).toBe("old-page"));
  let resolve!: (value: unknown) => void;
  mocks.entries.mockReturnValueOnce(new Promise((done) => { resolve = done; }));
  act(() => { void result.current.loadMore(); });
  mocks.entries.mockResolvedValueOnce({ items: [{ importId: "new" }], nextCursor: null, total: 1 });
  act(() => result.current.setWorkspace({ kind: "unassigned" }));
  await waitFor(() => expect(result.current.entries.map((entry) => entry.importId)).toEqual(["new"]));
  await act(async () => { resolve({ items: [{ importId: "late" }], nextCursor: "late-page", total: 8 }); });
  expect(result.current.entries.map((entry) => entry.importId)).toEqual(["new"]);
  expect(result.current.nextCursor).toBeNull();
  expect(result.current.total).toBe(1);
});

test("workspace notifications refresh the active filter; error retains selection and last readable rows", async () => {
  const { result } = renderHook(() => useImportedCatalog());
  await waitFor(() => expect(result.current.busy).toBe(false));
  act(() => result.current.setWorkspace({ kind: "unassigned" }));
  await waitFor(() => expect(result.current.busy).toBe(false));
  mocks.entries.mockRejectedValueOnce("SOURCE_BUSY");
  const callback = mocks.listen.mock.calls.find(([event]) => event === "session-import://workspace-changed")?.[1];
  act(() => { callback(); });
  await waitFor(() => expect(result.current.error).toContain("SOURCE_BUSY"));
  expect(result.current.workspace).toEqual({ kind: "unassigned" });
  expect(result.current.entries[0].importId).toBe("imp:1");
  await act(() => result.current.load());
  expect(result.current.error).toBeNull();
  expect(mocks.entries.mock.calls.at(-1)?.[2].workspace).toEqual({ kind: "unassigned" });
});

test("long catalog traversal stays bounded and can return to the beginning", async () => {
  mocks.entries.mockImplementation(async (cursor) => {
    const start = cursor ? Number(cursor) : 0;
    return { items: Array.from({ length: 50 }, (_, i) => ({ importId: String(start + i) })), nextCursor: String(start + 50), total: 1000 };
  });
  const { result } = renderHook(() => useImportedCatalog());
  await waitFor(() => expect(result.current.entries).toHaveLength(50));
  for (let n = 0; n < 5; n++) await act(() => result.current.loadMore());
  expect(result.current.entries).toHaveLength(200);
  expect(result.current.entries[0].importId).toBe("100");
  expect(result.current.total).toBe(1000);
  expect(result.current.windowed).toBe(true);
  await act(() => result.current.load());
  expect(result.current.entries[0].importId).toBe("0");
  expect(result.current.windowed).toBe(false);
});

test("unmount ignores late results and unregisters both native event listeners", async () => {
  const stop = vi.fn();
  mocks.listen.mockResolvedValue(stop);
  let resolve!: (value: unknown) => void;
  mocks.entries.mockReturnValueOnce(new Promise((done) => { resolve = done; }));
  const { unmount } = renderHook(() => useImportedCatalog());
  await act(async () => {});
  unmount();
  expect(stop).toHaveBeenCalledTimes(2);
  await act(async () => { resolve({ items: [], nextCursor: null, total: 0 }); });
  for (const [, callback] of mocks.listen.mock.calls) callback();
  expect(mocks.entries).toHaveBeenCalledTimes(1);
});

test("StrictMode remount keeps the current load and disposes only obsolete subscriptions", async () => {
  const stop = vi.fn();
  mocks.listen.mockResolvedValue(stop);
  let resolve!: (value: unknown) => void;
  mocks.entries.mockReturnValueOnce(new Promise((done) => { resolve = done; }));
  const { result, unmount } = renderHook(() => useImportedCatalog("sidebar"), {
    wrapper: StrictMode,
  });
  await waitFor(() => expect(result.current.entries[0]?.importId).toBe("imp:1"));
  await act(async () => { resolve({ items: [{ importId: "obsolete" }], nextCursor: "old", total: 99 }); });
  expect(result.current.entries[0].importId).toBe("imp:1");
  expect(result.current.busy).toBe(false);
  expect(stop).toHaveBeenCalledTimes(2);
  unmount();
  expect(stop).toHaveBeenCalledTimes(4);
});

test("ten thousand progress events wait for the initial read and coalesce into one current read", async () => {
  vi.useFakeTimers();
  let resolve!: (value: unknown) => void;
  mocks.entries.mockReturnValueOnce(new Promise((done) => { resolve = done; }));
  const { result } = renderHook(() => useImportedCatalog());
  await act(async () => {});
  const changed = mocks.listen.mock.calls.find(([event]) => event === "session-import://changed")![1];
  await act(async () => {
    for (let i = 0; i < 10_000; i++) changed();
    await vi.advanceTimersByTimeAsync(1000);
  });
  expect(mocks.entries).toHaveBeenCalledOnce();
  await act(async () => { resolve({ items: [{ importId: "initial" }], nextCursor: null, total: 1 }); });
  expect(result.current.entries[0].importId).toBe("initial");
  await act(async () => { await vi.advanceTimersByTimeAsync(1000); });
  expect(mocks.entries).toHaveBeenCalledTimes(2);
  expect(result.current.entries[0].importId).toBe("imp:1");
});

test("a slow background reply does not block notifications for a new filter", async () => {
  vi.useFakeTimers();
  const { result } = renderHook(() => useImportedCatalog());
  await act(async () => {});
  let resolve!: (value: unknown) => void;
  mocks.entries.mockReturnValueOnce(new Promise((done) => { resolve = done; }));
  const changed = mocks.listen.mock.calls.find(([event]) => event === "session-import://changed")![1];
  await act(async () => { changed(); await vi.advanceTimersByTimeAsync(300); });
  expect(mocks.entries).toHaveBeenCalledTimes(2);
  mocks.entries.mockResolvedValue({ items: [{ importId: "current" }], nextCursor: null, total: 1 });
  await act(async () => { result.current.setProvider("cursor"); });
  expect(mocks.entries).toHaveBeenCalledTimes(3);
  await act(async () => { for (let i = 0; i < 10_000; i++) changed(); await vi.advanceTimersByTimeAsync(1000); });
  expect(mocks.entries).toHaveBeenCalledTimes(4);
  expect(mocks.entries.mock.calls.at(-1)?.[2].provider).toBe("cursor");
  await act(async () => { resolve({ items: [{ importId: "obsolete" }], nextCursor: "old", total: 99 }); });
  expect(result.current.entries[0].importId).toBe("current");
  expect(result.current.nextCursor).toBeNull();
});

test("manual refresh consumes queued hints and unmount drops pending background work", async () => {
  vi.useFakeTimers();
  const { result, unmount } = renderHook(() => useImportedCatalog());
  await act(async () => {});
  const changed = mocks.listen.mock.calls.find(([event]) => event === "session-import://changed")![1];
  act(() => changed());
  await act(() => result.current.load());
  await act(async () => { await vi.advanceTimersByTimeAsync(1000); });
  expect(mocks.entries).toHaveBeenCalledTimes(2);
  act(() => changed());
  unmount();
  await act(async () => { await vi.advanceTimersByTimeAsync(1000); });
  expect(mocks.entries).toHaveBeenCalledTimes(2);
  expect(vi.getTimerCount()).toBe(0);
});
