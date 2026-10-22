/** @vitest-environment jsdom */
import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { useImportedReader, type ReaderHit } from "./useImportedReader";

type HitPage = { hits: ReaderHit[]; previousCursor: string | null; nextCursor: string | null };
function page(id: string): HitPage {
  return { hits: [{ eventId: id, snippet: id, kind: "message" }], previousCursor: `before-${id}`, nextCursor: `after-${id}` };
}
function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (cause: unknown) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}
const mocks = vi.hoisted(() => ({ events: vi.fn(), search: vi.fn(), context: vi.fn(), exportMd: vi.fn() }));
vi.mock("@/lib/api/sessionMigration", () => ({
  sessionImportEvents: (...args: [string, string | null, string | null, number, string | null, ("forward" | "backward")?]) => {
    const pending = mocks.events(...args);
    return Promise.resolve(pending).then(async (page) => {
      const { importedReaderPageFixture } = await import("@/test/importedReaderPageFixture");
      return importedReaderPageFixture(page, args[1], args[4]);
    });
  }, sessionImportSearch: mocks.search,
  sessionImportEventContext: mocks.context, sessionImportExportMarkdown: mocks.exportMd,
}));
beforeEach(() => {
  vi.resetAllMocks();
  mocks.events.mockResolvedValue({ events: [{ id: "transcript" }], previousCursor: "previous-event", nextCursor: "next-event" });
  mocks.search.mockResolvedValue(page("middle"));
});
afterEach(cleanup);

async function setup() {
  const onError = vi.fn();
  const hook = renderHook(({ id, revision, leaf }) => useImportedReader(id, onError, { revision, leaf }), {
    initialProps: { id: "imp:paging", revision: "revision-a", leaf: "leaf-a" },
  });
  await waitFor(() => expect(hook.result.current.busy).toBe(false));
  act(() => hook.result.current.setQuery("中文 alpha 🧪"));
  await act(async () => hook.result.current.search());
  mocks.search.mockClear();
  return { ...hook, onError };
}
const directions = [
  { older: "loadMoreHits", newer: "loadPreviousHits", cursor: "before-middle", direction: "backward" },
  { older: "loadPreviousHits", newer: "loadMoreHits", cursor: "after-middle", direction: "forward" },
] as const;
const overlaps = directions.flatMap((direction) => ["success", "failure"].flatMap((settlement) =>
  ["older-first", "newer-first"].map((order) => ({ ...direction, settlement, order })),
));

test.each(overlaps)("$older then $newer ignores superseded $settlement ($order)", async ({ older, newer, cursor, direction, settlement, order }) => {
  const { result, onError } = await setup();
  const first = deferred<HitPage>();
  const second = deferred<HitPage>();
  mocks.search.mockReturnValueOnce(first.promise).mockReturnValueOnce(second.promise);
  let oldTask!: Promise<void>;
  let newTask!: Promise<void>;
  act(() => { oldTask = result.current[older](); });
  act(() => { newTask = result.current[newer](); });
  expect(mocks.search).toHaveBeenLastCalledWith("imp:paging", "中文 alpha 🧪", "revision-a", cursor, "leaf-a", direction);
  const finishOld = async () => act(async () => {
    if (settlement === "success") first.resolve(page("superseded"));
    else first.reject(new Error("superseded page failure"));
    await oldTask;
  });
  const finishNew = async () => act(async () => { second.resolve(page("selected")); await newTask; });
  if (order === "older-first") {
    await finishOld();
    expect(result.current.hits).toEqual(page("middle").hits);
    expect(result.current.busy).toBe(true);
    expect(result.current.error).toBeNull();
    expect(onError).not.toHaveBeenCalled();
    await finishNew();
  } else {
    await finishNew();
    expect(result.current.busy).toBe(true);
    await finishOld();
  }
  expect(result.current.hits).toEqual(page("selected").hits);
  expect(result.current.hitsPrevious).toBe("before-selected");
  expect(result.current.hitsNext).toBe("after-selected");
  expect(result.current.error).toBeNull();
  expect(result.current.busy).toBe(false);
  expect(onError).not.toHaveBeenCalled();
});

test.each(directions)("superseded $older cannot change the current $newer cursor or bypass its deduplication gate", async ({ older, newer }) => {
  const { result } = await setup();
  const first = deferred<HitPage>();
  const second = deferred<HitPage>();
  mocks.search.mockReturnValueOnce(first.promise).mockReturnValueOnce(second.promise);
  let oldTask!: Promise<void>;
  let newTask!: Promise<void>;
  act(() => { oldTask = result.current[older](); newTask = result.current[newer](); });
  await act(async () => { first.resolve(page("superseded")); await oldTask; });
  await act(async () => { await result.current[newer](); });
  expect(mocks.search).toHaveBeenCalledTimes(2);
  expect(result.current.busy).toBe(true);
  await act(async () => { second.resolve(page("selected")); await newTask; });
  expect(result.current.busy).toBe(false);
});

test.each(["loadMoreHits", "loadPreviousHits"] as const)("%s error remains retryable and successful retry restores the reader", async (method) => {
  const { result, onError } = await setup();
  mocks.search.mockRejectedValueOnce(new Error("page temporarily unavailable"));
  await act(async () => { await result.current[method](); });
  expect(result.current.error).toBeNull();
  expect(result.current.searchError).toContain("temporarily unavailable");
  expect(result.current.events).toEqual([{ id: "transcript" }]);
  expect(result.current.previousCursor).toBe("previous-event");
  expect(result.current.nextCursor).toBe("next-event");
  expect(result.current.hits).toEqual(page("middle").hits);
  expect(result.current.hitsPrevious).toBe("before-middle");
  expect(result.current.hitsNext).toBe("after-middle");
  expect(result.current.busy).toBe(false);
  expect(onError).toHaveBeenCalledTimes(1);
  mocks.search.mockResolvedValueOnce(page("retry"));
  await act(async () => { await result.current[method](); });
  expect(result.current.hits).toEqual(page("retry").hits);
  expect(result.current.error).toBeNull();
  expect(result.current.searchError).toBeNull();
  expect(result.current.busy).toBe(false);
  expect(onError).toHaveBeenCalledTimes(1);
});

test("failed initial search stays separate from the transcript and clear removes only search state", async () => {
  const { result } = await setup();
  mocks.search.mockRejectedValueOnce(new Error("search temporarily unavailable"));
  await act(async () => result.current.search());
  expect(result.current.error).toBeNull();
  expect(result.current.searchError).toContain("temporarily unavailable");
  expect(result.current.events).toEqual([{ id: "transcript" }]);
  expect(result.current.previousCursor).toBe("previous-event");
  expect(result.current.nextCursor).toBe("next-event");
  act(() => result.current.clearSearch());
  expect(result.current.searchError).toBeNull();
  expect(result.current.hits).toEqual([]);
  expect(result.current.hitsPrevious).toBeNull();
  expect(result.current.hitsNext).toBeNull();
  expect(result.current.events).toEqual([{ id: "transcript" }]);
  expect(result.current.nextCursor).toBe("next-event");
});

test("successful search and clearing search cannot dismiss a read or scope error", async () => {
  const { result } = await setup();
  mocks.events.mockRejectedValueOnce(new Error("HISTORY_ACCESS_UNAVAILABLE: hidden"));
  await act(async () => result.current.loadMore());
  const error = result.current.error;
  await act(async () => result.current.search());
  expect(result.current.error).toBe(error);
  expect(result.current.searchError).toBeNull();
  act(() => result.current.clearSearch());
  expect(result.current.error).toBe(error);
});

test.each(["a".repeat(4097), "你".repeat(1366), "🧪".repeat(1025), "İ".repeat(1366)])(
  "rejects normalized UTF-8 query byte overflow locally (%#)", async (query) => {
    const { result, onError } = await setup();
    act(() => result.current.setQuery(query));
    await act(async () => result.current.search());
    expect(mocks.search).not.toHaveBeenCalled();
    expect(result.current.query).toBe(query);
    expect(result.current.searchError).toBe("queryTooLong");
    expect(result.current.error).toBeNull();
    expect(result.current.events).toEqual([{ id: "transcript" }]);
    expect(result.current.nextCursor).toBe("next-event");
    expect(result.current.busy).toBe(false);
    expect(onError).not.toHaveBeenCalled();
  },
);

test.each(["a".repeat(4096), "🧪".repeat(1024), `  ${"你".repeat(1365)}a  `])(
  "accepts exactly 4096 normalized UTF-8 query bytes without changing the draft (%#)", async (query) => {
    const { result } = await setup();
    act(() => result.current.setQuery(query));
    await act(async () => result.current.search());
    expect(mocks.search).toHaveBeenCalledExactlyOnceWith("imp:paging", query, "revision-a", null, "leaf-a");
    expect(result.current.searchError).toBeNull();
    expect(result.current.query).toBe(query);
  },
);

test("local validation invalidates an earlier search and keeps its busy ownership until settlement", async () => {
  const { result } = await setup();
  const pending = deferred<HitPage>();
  mocks.search.mockReturnValueOnce(pending.promise);
  let task!: Promise<void>;
  act(() => { task = result.current.search(); });
  act(() => result.current.setQuery("🧪".repeat(1025)));
  await act(async () => result.current.search());
  expect(result.current.searchError).toBe("queryTooLong");
  expect(result.current.busy).toBe(true);
  await act(async () => { pending.resolve(page("obsolete")); await task; });
  expect(result.current.searchError).toBe("queryTooLong");
  expect(result.current.hits.some((hit) => hit.eventId === "obsolete")).toBe(false);
  expect(result.current.busy).toBe(false);
  act(() => result.current.clearSearch());
  expect(result.current.searchError).toBeNull();
});

test("clearing search preserves export feedback and read failures", async () => {
  const { result } = await setup();
  Object.assign(navigator, { clipboard: { writeText: vi.fn().mockResolvedValue(undefined) } });
  mocks.exportMd.mockResolvedValue("# exported");
  await act(async () => result.current.exportMarkdown());
  expect(result.current.notice).toBe("copied");
  act(() => result.current.clearSearch());
  expect(result.current.notice).toBe("copied");
  mocks.events.mockRejectedValueOnce(new Error("event read failed"));
  await act(async () => result.current.loadMore());
  act(() => result.current.clearSearch());
  expect(result.current.error).toContain("event read failed");
});

test("a rejected replacement query cannot reuse the previous query's hit cursors", async () => {
  const { result } = await setup();
  act(() => result.current.setQuery("replacement"));
  mocks.search.mockRejectedValueOnce(new Error("search failed"));
  await act(async () => result.current.search());
  expect(result.current.hits).toEqual([]);
  expect(result.current.hitsPrevious).toBeNull();
  expect(result.current.hitsNext).toBeNull();
  mocks.search.mockClear();
  await act(async () => { await result.current.loadMoreHits(); await result.current.loadPreviousHits(); });
  expect(mocks.search).not.toHaveBeenCalled();
  expect(result.current.events).toEqual([{ id: "transcript" }]);
  expect(result.current.previousCursor).toBe("previous-event");
  expect(result.current.nextCursor).toBe("next-event");
});

test("reading another event page cannot dismiss a search failure", async () => {
  const { result } = await setup();
  mocks.search.mockRejectedValueOnce(new Error("search failed"));
  await act(async () => result.current.search());
  const searchError = result.current.searchError;
  mocks.events.mockResolvedValueOnce({ events: [{ id: "second" }], previousCursor: "back", nextCursor: null });
  await act(async () => result.current.loadMore());
  expect(result.current.searchError).toBe(searchError);
  expect(result.current.error).toBeNull();
  expect(result.current.events).toEqual([{ id: "transcript" }, { id: "second" }]);
});

test.each(["id", "revision", "leaf"] as const)("reader %s changes clear a previous local search failure", async (field) => {
  const { result, rerender } = await setup();
  act(() => result.current.setQuery("你".repeat(1366)));
  await act(async () => result.current.search());
  expect(result.current.searchError).toBe("queryTooLong");
  rerender({ id: "imp:paging", revision: "revision-a", leaf: "leaf-a", [field]: field === "id" ? "imp:other" : "other" });
  await waitFor(() => expect(result.current.busy).toBe(false));
  expect(result.current.searchError).toBeNull();
  expect(result.current.error).toBeNull();
  expect(result.current.query).toBe("");
});

test("successful forward event retry clears the error that otherwise hides loaded history", async () => {
  const { result, onError } = await setup();
  mocks.events.mockRejectedValueOnce(new Error("event page temporarily unavailable"));
  await act(async () => { await result.current.loadMore(); });
  expect(result.current.error).toContain("temporarily unavailable");
  expect(result.current.events).toEqual([{ id: "transcript" }]);
  expect(result.current.nextCursor).toBe("next-event");
  mocks.events.mockResolvedValueOnce({ events: [{ id: "recovered" }], previousCursor: "back", nextCursor: null });
  await act(async () => { expect(await result.current.loadMore()).toBe("recovered"); });
  expect(result.current.events).toEqual([{ id: "transcript" }, { id: "recovered" }]);
  expect(result.current.error).toBeNull();
  expect(result.current.busy).toBe(false);
  expect(onError).toHaveBeenCalledTimes(1);
});

test.each(["clear", "search"] as const)("%s invalidates pending hit pages without releasing their busy lane early", async (action) => {
  const { result, onError } = await setup();
  const pending = deferred<HitPage>();
  mocks.search.mockReturnValueOnce(pending.promise);
  let task!: Promise<void>;
  act(() => { task = result.current.loadMoreHits(); });
  if (action === "clear") act(() => result.current.clearSearch());
  else {
    act(() => result.current.setQuery("replacement"));
    mocks.search.mockResolvedValueOnce(page("replacement"));
    await act(async () => result.current.search());
  }
  const expected = action === "clear" ? [] : page("replacement").hits;
  expect(result.current.busy).toBe(true);
  await act(async () => { pending.reject(new Error("stale page")); await task; });
  expect(result.current.hits).toEqual(expected);
  expect(result.current.error).toBeNull();
  expect(result.current.busy).toBe(false);
  expect(onError).not.toHaveBeenCalled();
});

test.each(["id", "revision", "leaf"] as const)("changing reader %s isolates pending hit page success and finally", async (field) => {
  const { result, rerender, onError } = await setup();
  const old = deferred<HitPage>();
  const current = deferred<{ events: unknown[]; nextCursor: null }>();
  mocks.search.mockReturnValueOnce(old.promise);
  let task!: Promise<void>;
  act(() => { task = result.current.loadPreviousHits(); });
  mocks.events.mockReturnValueOnce(current.promise);
  rerender({ id: "imp:paging", revision: "revision-a", leaf: "leaf-a", [field]: field === "id" ? "imp:other" : "other" });
  await act(async () => { old.resolve(page("old-owner")); await task; });
  expect(result.current.hits).toEqual([]);
  expect(result.current.hitsNext).toBeNull();
  expect(result.current.busy).toBe(true);
  expect(result.current.error).toBeNull();
  await act(async () => { current.resolve({ events: [{ id: "new-owner" }], nextCursor: null }); });
  expect(result.current.events).toEqual([{ id: "new-owner" }]);
  expect(result.current.busy).toBe(false);
  expect(onError).not.toHaveBeenCalled();
});
