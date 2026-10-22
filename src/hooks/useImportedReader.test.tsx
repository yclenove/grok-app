/** @vitest-environment jsdom */
import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { useImportedReader } from "./useImportedReader";

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

const mocks = vi.hoisted(() => ({
  context: vi.fn(),
  events: vi.fn(),
  search: vi.fn(),
  exportMd: vi.fn(),
}));
vi.mock("@/lib/api/sessionMigration", () => ({
  sessionImportEventContext: (...args: [string, string, string | null, string | null, unknown]) => {
    const pending = mocks.context(...args);
    return Promise.resolve(pending).then(async (page) => {
      const { importedReaderPageFixture } = await import("@/test/importedReaderPageFixture");
      return importedReaderPageFixture(page, args[2], args[3]);
    });
  },
  sessionImportEvents: (...args: [string, string | null, string | null, number, string | null, ("forward" | "backward")?]) => {
    const pending = mocks.events(...args);
    return Promise.resolve(pending).then(async (page) => {
      const { importedReaderPageFixture } = await import("@/test/importedReaderPageFixture");
      return importedReaderPageFixture(page, args[1], args[4]);
    });
  },
  sessionImportSearch: mocks.search,
  sessionImportExportMarkdown: mocks.exportMd,
}));

beforeEach(() => {
  vi.resetAllMocks();
  mocks.events.mockResolvedValue({
    events: [{ id: "e1", role: "user", blocks: [{ text: "hello" }] }],
    nextCursor: "c1",
  });
  mocks.search.mockResolvedValue({
    hits: [{ eventId: "e1", ordinal: 0, snippet: "hello", kind: "message" }],
    nextCursor: null,
  });
  mocks.context.mockImplementation(
    (_importId: string, eventId: string, revision: string | null, leaf: string | null) =>
      Promise.resolve({
        events: [{ id: eventId, role: "assistant", blocks: [{ text: "context" }] }],
        targetIndex: 0,
        ordinal: 0,
        position: 0,
        total: 1,
        revision: revision ?? "rev-current",
        leaf: leaf ?? "leaf-current",
        anchorCursor: "anchor",
        previousCursor: null,
        nextCursor: null,
      }),
  );
  mocks.exportMd.mockResolvedValue("# leaf\n");
  Object.assign(navigator, {
    clipboard: { writeText: vi.fn().mockResolvedValue(undefined) },
  });
});
afterEach(() => cleanup());

test("clipboard denial leaves the loaded reader intact and allows export retry", async () => {
  const onError = vi.fn();
  const { result } = renderHook(() => useImportedReader("imp:clipboard", onError));
  await waitFor(() => expect(result.current.busy).toBe(false));
  const events = result.current.events;
  vi.mocked(navigator.clipboard.writeText).mockRejectedValueOnce(new Error("clipboard denied"));
  await act(async () => result.current.exportMarkdown());
  expect(result.current.error).toBeNull();
  expect(result.current.events).toEqual(events);
  expect(result.current.notice).toBe("copyFailed");
  await act(async () => result.current.exportMarkdown());
  expect(result.current.notice).toBe("copied");
});

test("coalesces duplicate reader export commands before React rerenders", async () => {
  const pending = deferred<string>();
  mocks.exportMd.mockReturnValueOnce(pending.promise);
  const { result } = renderHook(() => useImportedReader("imp:clipboard", vi.fn()));
  await waitFor(() => expect(result.current.busy).toBe(false));
  let first!: Promise<void>;
  let second!: Promise<void>;
  act(() => { first = result.current.exportMarkdown(); second = result.current.exportMarkdown(); });
  try { expect(mocks.exportMd).toHaveBeenCalledTimes(1); }
  finally { await act(async () => { pending.resolve("# copied"); await Promise.all([first, second]); }); }
});

test("revision health comes from the archive rather than the current message page", async () => {
  const health = { outcome: "partial", issueCount: 1, issues: [{ code: "unknown-role", message: "skipped" }], diagnosticsLimited: false };
  mocks.events.mockResolvedValueOnce({ events: [{ id: "e1", role: "user", text: "complete event" }], nextCursor: "c1", health });
  const { result } = renderHook(() => useImportedReader("imp:health", vi.fn()));
  await waitFor(() => expect(result.current.health).toEqual(health));
  expect(result.current.events).toHaveLength(1);
  mocks.events.mockResolvedValueOnce({ events: [{ id: "e2" }], nextCursor: null, previousCursor: "back", health });
  await act(async () => { await result.current.loadMore(); });
  expect(result.current.health).toEqual(health);
  mocks.events.mockResolvedValueOnce({ events: [{ id: "e1" }], nextCursor: "c1", health });
  await act(async () => { await result.current.loadPrevious(); });
  expect(result.current.health).toEqual(health);
});

test("context jump supplies health even when it supersedes the initial event request", async () => {
  const pending = deferred<{ events: unknown[]; nextCursor: null; health: { outcome: string; issueCount: number; issues: never[]; diagnosticsLimited: boolean } }>();
  const health = { outcome: "partial", issueCount: 2, issues: [], diagnosticsLimited: false };
  mocks.events.mockReturnValueOnce(pending.promise);
  mocks.context.mockResolvedValueOnce({ events: [{ id: "target" }], previousCursor: null, nextCursor: null, health });
  const { result } = renderHook(() => useImportedReader("imp:health", vi.fn()));
  await act(async () => { await result.current.ensureEvent("target"); });
  expect(result.current.health).toEqual(health);
  await act(async () => { pending.resolve({ events: [{ id: "old" }], nextCursor: null, health: { ...health, outcome: "complete", issueCount: 0 } }); });
  expect(result.current.health).toEqual(health);
  expect(result.current.events).toEqual([{ id: "target" }]);
});

test("switching revision clears health and ignores old revision diagnostics", async () => {
  const pending = deferred<{ events: unknown[]; nextCursor: null; health: { outcome: string; issueCount: number; issues: never[]; diagnosticsLimited: boolean } }>();
  const health = { outcome: "partial", issueCount: 1, issues: [], diagnosticsLimited: false };
  mocks.events.mockResolvedValueOnce({ events: [{ id: "e1" }], nextCursor: "next", health });
  const { result, rerender } = renderHook(({ revision }) => useImportedReader("imp:health", vi.fn(), { revision }), { initialProps: { revision: "rev-a" } });
  await waitFor(() => expect(result.current.health).toEqual(health));
  mocks.events.mockReturnValueOnce(pending.promise);
  act(() => { void result.current.loadMore(); });
  const newer = deferred<{ events: unknown[]; nextCursor: null }>();
  mocks.events.mockReturnValueOnce(newer.promise);
  rerender({ revision: "rev-b" });
  expect(result.current.health).toBeNull();
  await act(async () => { pending.resolve({ events: [{ id: "old" }], nextCursor: null, health }); });
  expect(result.current.health).toBeNull();
  await act(async () => { newer.resolve({ events: [{ id: "new" }], nextCursor: null }); });
  expect(result.current.health).toEqual({ outcome: "unknown", issueCount: null, issues: [], diagnosticsLimited: false });
});

test("loads events from the shipped page, then search and export", async () => {
  const onError = vi.fn();
  const { result } = renderHook(() => useImportedReader("imp:abc", onError));
  await waitFor(() => {
    expect(result.current.events).toHaveLength(1);
  });
  expect(mocks.events).toHaveBeenCalledWith("imp:abc", null, null, 100, null);
  expect(result.current.nextCursor).toBe("c1");

  act(() => result.current.setQuery("hello"));
  await act(async () => {
    await result.current.search();
  });
  expect(mocks.search).toHaveBeenCalledWith("imp:abc", "hello", "rev-current", null, "leaf-current");
  expect(result.current.hits[0].eventId).toBe("e1");
  expect(result.current.hitsNext).toBeNull();

  mocks.events.mockResolvedValueOnce({
    events: [{ id: "e2" }],
    nextCursor: null,
  });
  let forwardAnchor: string | null | undefined;
  await act(async () => {
    forwardAnchor = await result.current.loadMore();
  });
  expect(forwardAnchor).toBe("e2");
  expect(mocks.events).toHaveBeenLastCalledWith("imp:abc", "rev-current", "c1", 100, "leaf-current");
  expect(result.current.events).toHaveLength(2);

  await act(async () => {
    await result.current.exportMarkdown();
  });
  expect(mocks.exportMd).toHaveBeenCalledWith("imp:abc", "rev-current", "leaf-current");
  expect(navigator.clipboard.writeText).toHaveBeenCalledWith("# leaf\n");
  expect(result.current.notice).toBe("copied");
  expect(onError).not.toHaveBeenCalled();
});

test("double loadMore while a page is in flight invokes events IPC once", async () => {
  const pending = deferred<{ events: unknown[]; nextCursor: string | null }>();
  const onError = vi.fn();
  const { result } = renderHook(() => useImportedReader("imp:abc", onError));
  await waitFor(() => {
    expect(result.current.nextCursor).toBe("c1");
  });
  mocks.events.mockClear();
  mocks.events.mockReturnValueOnce(pending.promise);
  act(() => {
    void result.current.loadMore();
  });
  act(() => {
    void result.current.loadMore();
  });
  expect(mocks.events).toHaveBeenCalledTimes(1);
  await act(async () => {
    pending.resolve({ events: [{ id: "e2" }], nextCursor: null });
  });
});

test("search paging replaces the bounded hit window and can navigate back", async () => {
  mocks.search
    .mockResolvedValueOnce({
      hits: [{ eventId: "e1", snippet: "a", kind: "message" }],
      previousCursor: null,
      nextCursor: "c1",
    })
    .mockResolvedValueOnce({
      hits: [{ eventId: "e2", snippet: "b", kind: "message" }],
      previousCursor: "back-c1",
      nextCursor: null,
    })
    .mockResolvedValueOnce({
      hits: [{ eventId: "e1", snippet: "a", kind: "message" }],
      previousCursor: null,
      nextCursor: "c1-again",
    });
  const onError = vi.fn();
  const { result } = renderHook(() => useImportedReader("imp:abc", onError));
  await waitFor(() => {
    expect(result.current.events).toHaveLength(1);
  });
  act(() => result.current.setQuery("a"));
  await act(async () => {
    await result.current.search();
  });
  expect(result.current.hitsNext).toBe("c1");
  await act(async () => {
    await result.current.loadMoreHits();
  });
  expect(mocks.search).toHaveBeenLastCalledWith(
    "imp:abc",
    "a",
    "rev-current",
    "c1",
    "leaf-current",
    "forward",
  );
  expect(result.current.hits.map((h) => h.eventId)).toEqual(["e2"]);
  expect(result.current.hitsPrevious).toBe("back-c1");
  expect(result.current.hitsNext).toBeNull();
  await act(async () => {
    await result.current.loadPreviousHits();
  });
  expect(mocks.search).toHaveBeenLastCalledWith(
    "imp:abc",
    "a",
    "rev-current",
    "back-c1",
    "leaf-current",
    "backward",
  );
  expect(result.current.hits.map((h) => h.eventId)).toEqual(["e1"]);
  expect(result.current.hitsPrevious).toBeNull();
  expect(result.current.hitsNext).toBe("c1-again");
  expect(onError).not.toHaveBeenCalled();
});

test("ensureEvent uses one bounded context lookup for an unloaded hit", async () => {
  const onError = vi.fn();
  const { result } = renderHook(() => useImportedReader("imp:abc", onError));
  await waitFor(() => {
    expect(result.current.events).toHaveLength(1);
  });
  await act(async () => {
    const found = await result.current.ensureEvent("e9");
    expect(found).toBe(true);
  });
  expect(mocks.context).toHaveBeenCalledWith(
    "imp:abc",
    "e9",
    "rev-current",
    "leaf-current",
    { before: 8, after: 8 },
  );
  expect(mocks.events).toHaveBeenCalledTimes(1);
  expect(result.current.events).toHaveLength(1);
  expect(result.current.events).toEqual([
    { id: "e9", role: "assistant", blocks: [{ text: "context" }] },
  ]);
  expect(onError).not.toHaveBeenCalled();
});

test("concurrent jumps to the same event share one context IPC", async () => {
  const pending = deferred<{
    events: unknown[];
    targetIndex: number;
    ordinal: number;
    position: number;
    total: number;
    revision: string;
    leaf: string;
    anchorCursor: string;
    previousCursor: string | null;
    nextCursor: string | null;
  }>();
  mocks.context.mockReturnValueOnce(pending.promise);
  const onError = vi.fn();
  const { result } = renderHook(() => useImportedReader("imp:abc", onError));
  await waitFor(() => expect(result.current.events).toHaveLength(1));
  let first!: Promise<boolean>;
  let second!: Promise<boolean>;
  act(() => {
    first = result.current.ensureEvent("e5000");
    second = result.current.ensureEvent("e5000");
  });
  expect(first).toBe(second);
  expect(mocks.context).toHaveBeenCalledTimes(1);
  await act(async () => {
    pending.resolve({
      events: [{ id: "e5000" }],
      targetIndex: 0,
      ordinal: 5000,
      position: 5000,
      total: 10_000,
      revision: "rev-current",
      leaf: "leaf-current",
      anchorCursor: "anchor-5000",
      previousCursor: "before-5000",
      nextCursor: "after-5000",
    });
    await first;
  });
  expect(result.current.events).toEqual([{ id: "e5000" }]);
  expect(result.current.nextCursor).toBe("after-5000");
  expect(result.current.busy).toBe(false);
  expect(onError).not.toHaveBeenCalled();
});

test("a superseded jump cannot land success, error, or finally over the new target", async () => {
  const first = deferred<{
    events: unknown[];
    targetIndex: number;
    ordinal: number;
    position: number;
    total: number;
    revision: string;
    leaf: string;
    anchorCursor: string;
    previousCursor: string | null;
    nextCursor: string | null;
  }>();
  const second = deferred<{
    events: unknown[];
    targetIndex: number;
    ordinal: number;
    position: number;
    total: number;
    revision: string;
    leaf: string;
    anchorCursor: string;
    previousCursor: string | null;
    nextCursor: string | null;
  }>();
  mocks.context.mockReturnValueOnce(first.promise).mockReturnValueOnce(second.promise);
  const onError = vi.fn();
  const { result } = renderHook(() => useImportedReader("imp:abc", onError));
  await waitFor(() => expect(result.current.events).toHaveLength(1));
  act(() => {
    void result.current.ensureEvent("early");
    void result.current.ensureEvent("late");
  });
  await act(async () => {
    second.resolve({
      events: [{ id: "late" }],
      targetIndex: 0,
      ordinal: 9999,
      position: 9999,
      total: 10_000,
      revision: "rev-current",
      leaf: "leaf-current",
      anchorCursor: "anchor-late",
      previousCursor: "before-late",
      nextCursor: null,
    });
    await second.promise;
  });
  expect(result.current.events).toEqual([{ id: "late" }]);
  expect(result.current.busy).toBe(true);
  await act(async () => {
    first.reject(new Error("stale jump failed"));
    await first.promise.catch(() => undefined);
  });
  expect(result.current.events).toEqual([{ id: "late" }]);
  expect(result.current.error).toBeNull();
  expect(result.current.busy).toBe(false);
  expect(onError).not.toHaveBeenCalled();
});

test("a late initial page cannot replace a restored event context", async () => {
  const initial = deferred<{ events: unknown[]; nextCursor: string | null }>();
  const context = deferred<{
    events: unknown[];
    targetIndex: number;
    ordinal: number;
    position: number;
    total: number;
    revision: string;
    leaf: string;
    anchorCursor: string;
    previousCursor: string | null;
    nextCursor: string | null;
  }>();
  mocks.events.mockReturnValueOnce(initial.promise);
  mocks.context.mockReturnValueOnce(context.promise);
  const onError = vi.fn();
  const { result } = renderHook(() =>
    useImportedReader("imp:abc", onError, { revision: "rev-a", leaf: "leaf-a" }),
  );

  let jump!: Promise<boolean>;
  act(() => {
    jump = result.current.ensureEvent("e5000");
  });
  await act(async () => {
    context.resolve({
      events: [{ id: "e5000" }],
      targetIndex: 0,
      ordinal: 5000,
      position: 5000,
      total: 10_000,
      revision: "rev-a",
      leaf: "leaf-a",
      anchorCursor: "anchor-5000",
      previousCursor: "before-5000",
      nextCursor: "after-5000",
    });
    expect(await jump).toBe(true);
  });
  expect(result.current.events).toEqual([{ id: "e5000" }]);
  expect(result.current.nextCursor).toBe("after-5000");

  await act(async () => {
    initial.resolve({ events: [{ id: "e0" }], nextCursor: "after-e0" });
    await initial.promise;
  });
  expect(result.current.events).toEqual([{ id: "e5000" }]);
  expect(result.current.nextCursor).toBe("after-5000");
  expect(result.current.error).toBeNull();
  expect(result.current.busy).toBe(false);
  expect(onError).not.toHaveBeenCalled();
});

test("a late forward page cannot replace a newer event context", async () => {
  const page = deferred<{ events: unknown[]; nextCursor: string | null }>();
  const context = deferred<{
    events: unknown[];
    targetIndex: number;
    ordinal: number;
    position: number;
    total: number;
    revision: string;
    leaf: string;
    anchorCursor: string;
    previousCursor: string | null;
    nextCursor: string | null;
  }>();
  const onError = vi.fn();
  const { result } = renderHook(() => useImportedReader("imp:abc", onError));
  await waitFor(() => expect(result.current.nextCursor).toBe("c1"));
  mocks.events.mockReturnValueOnce(page.promise);
  mocks.context.mockReturnValueOnce(context.promise);
  act(() => {
    void result.current.loadMore();
  });
  let jump!: Promise<boolean>;
  act(() => {
    jump = result.current.ensureEvent("e5000");
  });
  await act(async () => {
    context.resolve({
      events: [{ id: "e5000" }],
      targetIndex: 0,
      ordinal: 5000,
      position: 5000,
      total: 10_000,
      revision: "rev-current",
      leaf: "leaf-current",
      anchorCursor: "anchor-5000",
      previousCursor: "before-5000",
      nextCursor: "after-5000",
    });
    expect(await jump).toBe(true);
  });
  await act(async () => {
    page.resolve({ events: [{ id: "e2" }], nextCursor: "after-e2" });
    await page.promise;
  });
  expect(result.current.events).toEqual([{ id: "e5000" }]);
  expect(result.current.nextCursor).toBe("after-5000");
  expect(result.current.busy).toBe(false);
  expect(onError).not.toHaveBeenCalled();
});

test("loads the previous bounded event window from a context cursor", async () => {
  const onError = vi.fn();
  const { result } = renderHook(() =>
    useImportedReader("imp:abc", onError, { revision: "rev-a", leaf: "leaf-a" }),
  );
  await waitFor(() => expect(result.current.events).toHaveLength(1));
  mocks.context.mockResolvedValueOnce({
    events: [{ id: "e5000" }],
    targetIndex: 0,
    ordinal: 5000,
    position: 5000,
    total: 10_000,
    revision: "rev-a",
    leaf: "leaf-a",
    anchorCursor: "anchor-5000",
    previousCursor: "before-5000",
    nextCursor: "after-5000",
  });
  await act(async () => {
    expect(await result.current.ensureEvent("e5000")).toBe(true);
  });
  expect(result.current.previousCursor).toBe("before-5000");
  mocks.events.mockResolvedValueOnce({
    events: [{ id: "e4900" }, { id: "e4999" }],
    previousCursor: "before-4900",
    nextCursor: "after-4999",
    total: 10_000,
    start: 4900,
    end: 5000,
    revision: "rev-a",
    leaf: "leaf-a",
  });
  let previousAnchor: string | null | undefined;
  await act(async () => {
    previousAnchor = await result.current.loadPrevious();
  });
  expect(previousAnchor).toBe("e4999");
  expect(mocks.events).toHaveBeenLastCalledWith(
    "imp:abc",
    "rev-a",
    "before-5000",
    100,
    "leaf-a",
    "backward",
  );
  expect(result.current.events).toEqual([{ id: "e4900" }, { id: "e4999" }]);
  expect(result.current.previousCursor).toBe("before-4900");
  expect(result.current.nextCursor).toBe("after-4999");
  expect(onError).not.toHaveBeenCalled();
});

test("search and export stay bound to the given revision and leaf", async () => {
  const onError = vi.fn();
  const { result } = renderHook(() =>
    useImportedReader("imp:abc", onError, { revision: "rev-fixed", leaf: "a3" }),
  );
  await waitFor(() => {
    expect(result.current.events).toHaveLength(1);
  });
  expect(mocks.events).toHaveBeenCalledWith("imp:abc", "rev-fixed", null, 100, "a3");
  act(() => result.current.setQuery("hello"));
  await act(async () => {
    await result.current.search();
  });
  expect(mocks.search).toHaveBeenCalledWith(
    "imp:abc",
    "hello",
    "rev-fixed",
    null,
    "a3",
  );
  await act(async () => {
    await result.current.exportMarkdown();
  });
  expect(mocks.exportMd).toHaveBeenCalledWith("imp:abc", "rev-fixed", "a3");
  expect(onError).not.toHaveBeenCalled();
});

test("a late page from the previous revision and leaf cannot replace the current owner", async () => {
  const oldPage = deferred<{
    events: unknown[];
    nextCursor: string | null;
  }>();
  mocks.events
    .mockImplementationOnce(() => oldPage.promise)
    .mockResolvedValueOnce({
      events: [{ id: "new-owner" }],
      nextCursor: null,
    });
  const onError = vi.fn();
  const { result, rerender } = renderHook(
    ({ revision, leaf }: { revision: string; leaf: string }) =>
      useImportedReader("imp:abc", onError, { revision, leaf }),
    { initialProps: { revision: "rev-old", leaf: "leaf-old" } },
  );

  rerender({ revision: "rev-new", leaf: "leaf-new" });
  await waitFor(() => {
    expect(result.current.events).toEqual([{ id: "new-owner" }]);
  });

  await act(async () => {
    oldPage.resolve({ events: [{ id: "stale-owner" }], nextCursor: "4" });
    await oldPage.promise;
  });

  expect(result.current.events).toEqual([{ id: "new-owner" }]);
  expect(result.current.nextCursor).toBeNull();
  expect(mocks.events).toHaveBeenNthCalledWith(
    1,
    "imp:abc",
    "rev-old",
    null,
    100,
    "leaf-old",
  );
  expect(mocks.events).toHaveBeenNthCalledWith(
    2,
    "imp:abc",
    "rev-new",
    null,
    100,
    "leaf-new",
  );
  expect(onError).not.toHaveBeenCalled();
});

test("a late failure from a previous import owner is ignored", async () => {
  const oldPage = deferred<{
    events: unknown[];
    nextCursor: string | null;
  }>();
  mocks.events
    .mockImplementationOnce(() => oldPage.promise)
    .mockResolvedValueOnce({ events: [{ id: "current" }], nextCursor: null });
  const onError = vi.fn();
  const { result, rerender } = renderHook(
    ({ importId }: { importId: string }) => useImportedReader(importId, onError),
    { initialProps: { importId: "imp:old" } },
  );

  rerender({ importId: "imp:new" });
  await waitFor(() => {
    expect(result.current.events).toEqual([{ id: "current" }]);
  });
  await act(async () => {
    oldPage.reject(new Error("old owner failed"));
    await oldPage.promise.catch(() => undefined);
  });

  expect(result.current.events).toEqual([{ id: "current" }]);
  expect(result.current.error).toBeNull();
  expect(onError).not.toHaveBeenCalled();
});

test("reader load error is distinct from empty history", async () => {
  mocks.events.mockReset();
  mocks.events.mockRejectedValueOnce(new Error("HISTORY_ACCESS_UNAVAILABLE: hidden"));
  const onError = vi.fn();
  const { result } = renderHook(() => useImportedReader("imp:abc", onError));
  await waitFor(() => {
    expect(result.current.error).toContain("HISTORY_ACCESS_UNAVAILABLE");
  });
  expect(result.current.events).toEqual([]);
  expect(result.current.busy).toBe(false);
  expect(onError).toHaveBeenCalled();
});

test("clearSearch drops query and hits", async () => {
  const onError = vi.fn();
  const { result } = renderHook(() => useImportedReader("imp:abc", onError));
  await waitFor(() => {
    expect(result.current.events).toHaveLength(1);
  });
  act(() => result.current.setQuery("hello"));
  await act(async () => {
    await result.current.search();
  });
  expect(result.current.hits).toHaveLength(1);
  act(() => result.current.clearSearch());
  expect(result.current.query).toBe("");
  expect(result.current.hits).toEqual([]);
  expect(result.current.notice).toBeNull();
});

test("a later search is not overwritten by an earlier query success", async () => {
  const first = deferred<{
    hits: { eventId: string; snippet: string; kind: string }[];
    nextCursor: string | null;
  }>();
  const second = deferred<{
    hits: { eventId: string; snippet: string; kind: string }[];
    nextCursor: string | null;
  }>();
  mocks.search.mockReturnValueOnce(first.promise).mockReturnValueOnce(second.promise);
  const onError = vi.fn();
  const { result } = renderHook(() => useImportedReader("imp:abc", onError));
  await waitFor(() => {
    expect(result.current.events).toHaveLength(1);
  });
  act(() => result.current.setQuery("alpha"));
  act(() => {
    void result.current.search();
  });
  act(() => result.current.setQuery("beta"));
  act(() => {
    void result.current.search();
  });
  await act(async () => {
    second.resolve({
      hits: [{ eventId: "b1", snippet: "beta-hit", kind: "message" }],
      nextCursor: "c-beta",
    });
    await second.promise;
  });
  expect(result.current.hits.map((h) => h.eventId)).toEqual(["b1"]);
  await act(async () => {
    first.resolve({
      hits: [{ eventId: "a1", snippet: "alpha-hit", kind: "message" }],
      nextCursor: "c-alpha",
    });
    await first.promise;
  });
  expect(result.current.hits.map((h) => h.eventId)).toEqual(["b1"]);
  expect(result.current.hitsNext).toBe("c-beta");
  expect(result.current.notice).toBeNull();
  expect(onError).not.toHaveBeenCalled();
});

test("an earlier search error and finally cannot land on a later query", async () => {
  const first = deferred<{
    hits: { eventId: string; snippet: string; kind: string }[];
    nextCursor: string | null;
  }>();
  const second = deferred<{
    hits: { eventId: string; snippet: string; kind: string }[];
    nextCursor: string | null;
  }>();
  mocks.search.mockReturnValueOnce(first.promise).mockReturnValueOnce(second.promise);
  const onError = vi.fn();
  const { result } = renderHook(() => useImportedReader("imp:abc", onError));
  await waitFor(() => {
    expect(result.current.events).toHaveLength(1);
  });
  act(() => result.current.setQuery("alpha"));
  act(() => {
    void result.current.search();
  });
  act(() => result.current.setQuery("beta"));
  act(() => {
    void result.current.search();
  });
  expect(result.current.busy).toBe(true);
  await act(async () => {
    first.reject(new Error("alpha-failed"));
    await first.promise.catch(() => undefined);
  });
  expect(result.current.error).toBeNull();
  expect(onError).not.toHaveBeenCalled();
  expect(result.current.busy).toBe(true);
  await act(async () => {
    second.resolve({
      hits: [{ eventId: "b1", snippet: "beta", kind: "message" }],
      nextCursor: null,
    });
    await second.promise;
  });
  expect(result.current.hits.map((h) => h.eventId)).toEqual(["b1"]);
  expect(result.current.busy).toBe(false);
  expect(result.current.error).toBeNull();
});

test("double search loadMoreHits with the same cursor invokes search IPC once", async () => {
  mocks.search.mockResolvedValueOnce({
    hits: [{ eventId: "e1", snippet: "a", kind: "message" }],
    nextCursor: "c-hits",
  });
  const pending = deferred<{
    hits: { eventId: string; snippet: string; kind: string }[];
    nextCursor: string | null;
  }>();
  const onError = vi.fn();
  const { result } = renderHook(() => useImportedReader("imp:abc", onError));
  await waitFor(() => {
    expect(result.current.events).toHaveLength(1);
  });
  act(() => result.current.setQuery("a"));
  await act(async () => {
    await result.current.search();
  });
  expect(result.current.hitsNext).toBe("c-hits");
  mocks.search.mockClear();
  mocks.search.mockReturnValueOnce(pending.promise);
  act(() => {
    void result.current.loadMoreHits();
  });
  act(() => {
    void result.current.loadMoreHits();
  });
  expect(mocks.search).toHaveBeenCalledTimes(1);
  await act(async () => {
    pending.resolve({
      hits: [{ eventId: "e2", snippet: "b", kind: "message" }],
      nextCursor: null,
    });
    await pending.promise;
  });
});

test("search hit pages stay eventId-stable when a page repeats an id", async () => {
  mocks.search
    .mockResolvedValueOnce({
      hits: [{ eventId: "e1", snippet: "a", kind: "message" }],
      nextCursor: "c1",
    })
    .mockResolvedValueOnce({
      hits: [
        { eventId: "e1", snippet: "a-again", kind: "message" },
        { eventId: "e2", snippet: "b", kind: "message" },
      ],
      nextCursor: null,
    });
  const onError = vi.fn();
  const { result } = renderHook(() => useImportedReader("imp:abc", onError));
  await waitFor(() => {
    expect(result.current.events).toHaveLength(1);
  });
  act(() => result.current.setQuery("a"));
  await act(async () => {
    await result.current.search();
  });
  await act(async () => {
    await result.current.loadMoreHits();
  });
  expect(result.current.hits.map((h) => h.eventId)).toEqual(["e1", "e2"]);
});

test("double search with the same query invokes search IPC once", async () => {
  const pending = deferred<{
    hits: { eventId: string; snippet: string; kind: string }[];
    nextCursor: string | null;
  }>();
  mocks.search.mockReturnValue(pending.promise);
  const onError = vi.fn();
  const { result } = renderHook(() => useImportedReader("imp:abc", onError));
  await waitFor(() => {
    expect(result.current.events).toHaveLength(1);
  });
  act(() => result.current.setQuery("same"));
  act(() => {
    void result.current.search();
  });
  act(() => {
    void result.current.search();
  });
  expect(mocks.search).toHaveBeenCalledTimes(1);
  await act(async () => {
    pending.resolve({
      hits: [{ eventId: "e1", snippet: "same", kind: "message" }],
      nextCursor: null,
    });
    await pending.promise;
  });
});

test("previous identity loadMore finally cannot clear the new identity page flight", async () => {
  const oldMore = deferred<{ events: unknown[]; nextCursor: string | null }>();
  const newMore = deferred<{ events: unknown[]; nextCursor: string | null }>();
  mocks.events.mockImplementation(
    (importId: string, _rev: string | null, cursor: string | null) => {
      if (importId === "imp:old" && cursor == null) {
        return Promise.resolve({ events: [{ id: "o1" }], nextCursor: "old-c" });
      }
      if (importId === "imp:old" && cursor === "old-c") return oldMore.promise;
      if (importId === "imp:new" && cursor == null) {
        return Promise.resolve({ events: [{ id: "n1" }], nextCursor: "new-c" });
      }
      if (importId === "imp:new" && cursor === "new-c") return newMore.promise;
      return Promise.resolve({ events: [], nextCursor: null });
    },
  );
  const onError = vi.fn();
  const { result, rerender } = renderHook(
    ({ importId }: { importId: string }) => useImportedReader(importId, onError),
    { initialProps: { importId: "imp:old" } },
  );
  await waitFor(() => {
    expect(result.current.nextCursor).toBe("old-c");
  });
  act(() => {
    void result.current.loadMore();
  });
  rerender({ importId: "imp:new" });
  await waitFor(() => {
    expect(result.current.nextCursor).toBe("new-c");
  });
  act(() => {
    void result.current.loadMore();
  });
  await act(async () => {
    oldMore.resolve({ events: [{ id: "o2" }], nextCursor: null });
    await oldMore.promise;
  });
  act(() => {
    void result.current.loadMore();
  });
  const newMoreCalls = mocks.events.mock.calls.filter(
    (call) => call[0] === "imp:new" && call[2] === "new-c",
  );
  expect(newMoreCalls).toHaveLength(1);
  await act(async () => {
    newMore.resolve({ events: [{ id: "n2" }], nextCursor: null });
    await newMore.promise;
  });
});

test("clearSearch during an in-flight search drops the late success", async () => {
  const pending = deferred<{
    hits: { eventId: string; snippet: string; kind: string }[];
    nextCursor: string | null;
  }>();
  mocks.search.mockReturnValueOnce(pending.promise);
  const onError = vi.fn();
  const { result } = renderHook(() => useImportedReader("imp:abc", onError));
  await waitFor(() => {
    expect(result.current.events).toHaveLength(1);
  });
  act(() => result.current.setQuery("late"));
  act(() => {
    void result.current.search();
  });
  act(() => result.current.clearSearch());
  expect(result.current.hits).toEqual([]);
  await act(async () => {
    pending.resolve({
      hits: [{ eventId: "late-1", snippet: "should-not-land", kind: "message" }],
      nextCursor: "c-late",
    });
    await pending.promise;
  });
  expect(result.current.hits).toEqual([]);
  expect(result.current.hitsNext).toBeNull();
  expect(result.current.query).toBe("");
  expect(onError).not.toHaveBeenCalled();
});
