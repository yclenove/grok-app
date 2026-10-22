/** @vitest-environment jsdom */
import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { useImportedReader } from "./useImportedReader";

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (cause: unknown) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}
const health = { outcome: "complete" as const, issueCount: 0, issues: [], diagnosticsLimited: false };
const page = { events: [{ id: "recovered", role: "user", text: "recovered history" }], previousCursor: null, nextCursor: "next", health };
const mocks = vi.hoisted(() => ({ events: vi.fn(), search: vi.fn(), context: vi.fn(), exportMd: vi.fn() }));
vi.mock("@/lib/api/sessionMigration", () => ({
  sessionImportEvents: (...args: [string, string | null, string | null, number, string | null, ("forward" | "backward")?]) => {
    const pending = mocks.events(...args);
    return Promise.resolve(pending).then(async (page) => {
      const { importedReaderPageFixture } = await import("@/test/importedReaderPageFixture");
      return importedReaderPageFixture(page, args[1], args[4]);
    });
  }, sessionImportSearch: mocks.search,
  sessionImportEventContext: (...args: [string, string, string | null, string | null, unknown]) => {
    const pending = mocks.context(...args);
    return Promise.resolve(pending).then(async (page) => {
      const { importedReaderPageFixture } = await import("@/test/importedReaderPageFixture");
      return importedReaderPageFixture(page, args[2], args[3]);
    });
  }, sessionImportExportMarkdown: mocks.exportMd,
}));
beforeEach(() => { vi.resetAllMocks(); mocks.events.mockResolvedValue(page); });
afterEach(cleanup);
function setup() {
  const onError = vi.fn();
  const hook = renderHook(({ id, revision, leaf }) => useImportedReader(id, onError, { revision, leaf }), {
    initialProps: { id: "imp:recovery" as string | null, revision: "fixed-revision", leaf: "fixed-leaf" },
  });
  return { ...hook, onError };
}

test("failed first page has an explicit refresh that restores the selected revision and leaf", async () => {
  mocks.events.mockRejectedValueOnce(new Error("archive temporarily unavailable"));
  const { result, onError } = setup();
  await waitFor(() => expect(result.current.error).toContain("temporarily unavailable"));
  expect(result.current.events).toEqual([]);
  expect(result.current.nextCursor).toBeNull();
  expect(result.current.busy).toBe(false);
  await act(async () => result.current.reload());
  expect(mocks.events).toHaveBeenLastCalledWith("imp:recovery", "fixed-revision", null, 100, "fixed-leaf");
  expect(result.current.events).toEqual(page.events);
  expect(result.current.health).toEqual(health);
  expect(result.current.error).toBeNull();
  expect(result.current.nextCursor).toBe("next");
  expect(result.current.busy).toBe(false);
  expect(onError).toHaveBeenCalledTimes(1);
});

test("refresh failures stay visible and retryable without losing previously loaded history or the query", async () => {
  const { result, onError } = setup();
  await waitFor(() => expect(result.current.busy).toBe(false));
  act(() => result.current.setQuery("draft query 中文 🧪"));
  mocks.events.mockRejectedValueOnce(new Error("still offline"));
  await act(async () => result.current.reload());
  expect(result.current.error).toContain("still offline");
  expect(result.current.events).toEqual(page.events);
  expect(result.current.query).toBe("draft query 中文 🧪");
  expect(result.current.busy).toBe(false);
  await act(async () => result.current.reload());
  expect(result.current.error).toBeNull();
  expect(result.current.query).toBe("draft query 中文 🧪");
  expect(onError).toHaveBeenCalledTimes(1);
});

test.each(["initial", "refresh"] as const)("duplicate refresh does not fork an in-flight %s read", async (phase) => {
  const pending = deferred<typeof page>();
  if (phase === "initial") mocks.events.mockReturnValueOnce(pending.promise);
  const { result } = setup();
  if (phase === "refresh") {
    await waitFor(() => expect(result.current.busy).toBe(false));
    mocks.events.mockReturnValueOnce(pending.promise);
  }
  let first!: Promise<void>;
  act(() => { first = result.current.reload(); });
  await act(async () => result.current.reload());
  expect(mocks.events).toHaveBeenCalledTimes(phase === "initial" ? 1 : 2);
  expect(result.current.busy).toBe(true);
  await act(async () => { pending.resolve(page); await first; });
  expect(result.current.busy).toBe(false);
});

test.each(["id", "revision", "leaf"] as const)("refresh success and finally cannot overwrite a new reader %s", async (field) => {
  const { result, rerender } = setup();
  await waitFor(() => expect(result.current.busy).toBe(false));
  const old = deferred<typeof page>(); const current = deferred<typeof page>();
  mocks.events.mockReturnValueOnce(old.promise).mockReturnValueOnce(current.promise);
  let task!: Promise<void>; const oldReload = result.current.reload;
  act(() => { task = result.current.reload(); });
  rerender({ id: "imp:recovery", revision: "fixed-revision", leaf: "fixed-leaf", [field]: field === "id" ? "imp:new" : "new" });
  await act(async () => { old.resolve({ ...page, events: [{ id: "old", role: "user", text: "old" }] }); await task; await oldReload(); });
  expect(result.current.events).toEqual([]);
  expect(result.current.busy).toBe(true);
  expect(mocks.events).toHaveBeenCalledTimes(3);
  await act(async () => current.resolve(page));
  expect(result.current.events).toEqual(page.events);
  expect(result.current.busy).toBe(false);
});

test("closing the reader ignores late refresh failure and old callbacks", async () => {
  const { result, rerender, onError } = setup();
  await waitFor(() => expect(result.current.busy).toBe(false));
  const pending = deferred<typeof page>(); mocks.events.mockReturnValueOnce(pending.promise);
  let task!: Promise<void>; const oldReload = result.current.reload;
  act(() => { task = result.current.reload(); });
  rerender({ id: null, revision: "fixed-revision", leaf: "fixed-leaf" });
  await act(async () => { pending.reject(new Error("late failure")); await task; await oldReload(); await result.current.reload(); });
  expect(result.current.error).toBeNull(); expect(result.current.events).toEqual([]);
  expect(result.current.busy).toBe(false); expect(onError).not.toHaveBeenCalled();
  expect(mocks.events).toHaveBeenCalledTimes(2);
});

test.each(["success", "failure"] as const)("a newer context jump supersedes a refresh %s without releasing its busy lane early", async (settlement) => {
  const { result, onError } = setup(); await waitFor(() => expect(result.current.busy).toBe(false));
  const pending = deferred<typeof page>(); mocks.events.mockReturnValueOnce(pending.promise);
  let task!: Promise<void>; act(() => { task = result.current.reload(); });
  mocks.context.mockResolvedValueOnce({ ...page, events: [{ id: "target" }], nextCursor: null });
  await act(async () => expect(await result.current.ensureEvent("target")).toBe(true));
  expect(result.current.busy).toBe(true);
  await act(async () => { if (settlement === "success") pending.resolve(page); else pending.reject(new Error("superseded")); await task; });
  expect(result.current.events).toEqual([{ id: "target" }]);
  expect(result.current.error).toBeNull(); expect(onError).not.toHaveBeenCalled(); expect(result.current.busy).toBe(false);
});

test("a newer refresh owns the event window over a pending context jump", async () => {
  const { result } = setup(); await waitFor(() => expect(result.current.busy).toBe(false));
  const pending = deferred<typeof page>(); mocks.context.mockReturnValueOnce(pending.promise);
  let jump!: Promise<boolean>; act(() => { jump = result.current.ensureEvent("target"); });
  await act(async () => result.current.reload());
  expect(result.current.busy).toBe(true);
  await act(async () => { pending.resolve({ ...page, events: [{ id: "target", role: "user", text: "late target" }] }); expect(await jump).toBe(false); });
  expect(result.current.events).toEqual(page.events); expect(result.current.busy).toBe(false);
});

test("a synchronously throwing transport releases refresh ownership for the next attempt", async () => {
  const { result, onError } = setup(); await waitFor(() => expect(result.current.busy).toBe(false));
  mocks.events.mockImplementationOnce(() => { throw new Error("sync transport failure"); });
  await act(async () => result.current.reload());
  expect(result.current.error).toContain("sync transport failure"); expect(result.current.busy).toBe(false);
  await act(async () => result.current.reload());
  expect(result.current.error).toBeNull(); expect(onError).toHaveBeenCalledTimes(1);
});
