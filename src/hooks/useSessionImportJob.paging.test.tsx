/** @vitest-environment jsdom */
import { StrictMode } from "react";
import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import type { JobRecord } from "@/lib/api/sessionMigration";
import { useSessionImportJob } from "./useSessionImportJob";

const mocks = vi.hoisted(() => ({ listen: vi.fn(), get: vi.fn(), items: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: mocks.listen }));
vi.mock("@/lib/api/sessionMigration", () => ({ sessionImportJobGet: mocks.get, sessionImportJobItems: mocks.items }));

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: Error) => void;
  const promise = new Promise<T>((ok, fail) => { resolve = ok; reject = fail; });
  return { promise, resolve, reject };
}
function page(offset = 0, version = 1, state = "running", jobId = "a"): JobRecord {
  return { jobId, requestId: jobId, planHash: `plan-${jobId}`, state, stateVersion: version,
    createdAt: "2026-09-20T00:00:00Z", itemCount: 10_000, progress: { done: version, retryable: 0 },
    nextItemCursor: offset + 50 < 10_000 ? `cursor:${offset + 50}` : null,
    items: Array.from({ length: 50 }, (_, i) => ({ candidateId: `candidate-${offset + i}`, phase: state })) };
}
let notify: (event: { payload: { jobId: string } }) => void;
beforeEach(() => {
  vi.useFakeTimers(); vi.resetAllMocks();
  mocks.listen.mockImplementation((_event, handler) => { notify = handler; return Promise.resolve(vi.fn()); });
  mocks.get.mockImplementation((id: string) => Promise.resolve(page(0, 1, "running", id)));
  mocks.items.mockImplementation((id: string, cursor: string) =>
    Promise.resolve(page(Number(cursor.split(":")[1]), 1, "running", id)));
});
afterEach(() => { cleanup(); vi.clearAllTimers(); vi.useRealTimers(); });
async function tick() { await act(async () => { await vi.advanceTimersByTimeAsync(300); }); }

test("progress refreshes the selected page through terminal completion and preserves back navigation", async () => {
  const { result } = renderHook(() => useSessionImportJob(vi.fn()));
  await act(async () => { result.current.setJob(page()); });
  await act(async () => { await result.current.loadMoreJobItems(); });
  expect(result.current.job?.items[0].candidateId).toBe("candidate-50");
  mocks.items.mockResolvedValue(page(50, 10000, "completed"));
  act(() => { for (let i = 0; i < 10_000; i++) notify({ payload: { jobId: "a" } }); });
  await tick();
  expect(mocks.get).toHaveBeenCalledTimes(1);
  expect(mocks.items).toHaveBeenCalledTimes(2);
  expect(mocks.items).toHaveBeenLastCalledWith("a", "cursor:50", 50);
  expect(result.current.job).toMatchObject({ state: "completed", progress: { done: 10000 } });
  expect(result.current.job?.items[0].candidateId).toBe("candidate-50");
  expect(result.current.jobPageIndex).toBe(1);
  mocks.get.mockResolvedValue(page(0, 10000, "completed"));
  await act(async () => { await result.current.previousJobItems(); });
  expect(result.current.jobPageIndex).toBe(0);
  expect(result.current.jobHasPrevious).toBe(false);
  expect(result.current.job?.items[0].candidateId).toBe("candidate-0");
});

test("all 200 pages are reachable without accumulating item records", async () => {
  mocks.items.mockImplementation((id: string, cursor: string) =>
    Promise.resolve(page(Number(cursor.split(":")[1]), 10000, "completed", id)));
  const { result } = renderHook(() => useSessionImportJob(vi.fn()));
  act(() => result.current.setJob(page(0, 10000, "completed")));
  for (let index = 1; index < 200; index++) {
    await act(async () => { await result.current.loadMoreJobItems(); });
    expect(result.current.job?.items).toHaveLength(50);
    expect(result.current.job?.items[0].candidateId).toBe(`candidate-${index * 50}`);
  }
  expect(result.current.job?.nextItemCursor).toBeNull();
  expect(result.current.jobPageIndex).toBe(199);
  await act(async () => { await result.current.loadMoreJobItems(); });
  expect(mocks.items).toHaveBeenCalledTimes(199);
  await act(async () => { await result.current.previousJobItems(); });
  expect(result.current.job?.items[0].candidateId).toBe("candidate-9900");
});

test("duplicate page clicks share one flight and a failed page preserves its cursor and readable items", async () => {
  const pending = deferred<JobRecord>();
  mocks.items.mockReturnValueOnce(pending.promise);
  const error = vi.fn();
  const { result } = renderHook(() => useSessionImportJob(error));
  act(() => result.current.setJob(page(0, 1, "completed")));
  act(() => { void result.current.loadMoreJobItems(); void result.current.loadMoreJobItems(); });
  expect(mocks.items).toHaveBeenCalledTimes(1);
  expect(result.current.jobBusy).toBe(true);
  await act(async () => { pending.reject(new Error("SOURCE_BUSY")); });
  expect(error).toHaveBeenCalledWith("Error: SOURCE_BUSY");
  expect(result.current.jobPageIndex).toBe(0);
  expect(result.current.job?.nextItemCursor).toBe("cursor:50");
  expect(result.current.jobBusy).toBe(false);
  await act(async () => { await result.current.loadMoreJobItems(); });
  expect(result.current.jobPageIndex).toBe(1);
});

test("an old background read cannot reset a newly selected page or release its pending click", async () => {
  const old = deferred<JobRecord>();
  const next = deferred<JobRecord>();
  mocks.get.mockReturnValueOnce(old.promise);
  mocks.items.mockReturnValueOnce(next.promise);
  const { result } = renderHook(() => useSessionImportJob(vi.fn()));
  await act(async () => { result.current.setJob(page()); });
  act(() => { void result.current.loadMoreJobItems(); });
  await act(async () => { old.resolve(page(0, 2)); });
  expect(result.current.jobBusy).toBe(true);
  act(() => { void result.current.loadMoreJobItems(); });
  expect(mocks.items).toHaveBeenCalledTimes(1);
  await act(async () => { next.resolve(page(50, 3)); });
  expect(result.current.jobPageIndex).toBe(1);
  expect(result.current.job?.stateVersion).toBe(3);
});

test("late mutation replies cannot roll back a newer version and duplicate actions are ignored", async () => {
  const pending = deferred<JobRecord>();
  const operation = vi.fn().mockReturnValue(pending.promise);
  const { result } = renderHook(() => useSessionImportJob(vi.fn()));
  await act(async () => { result.current.setJob(page()); });
  act(() => { void result.current.mutateJob(operation); void result.current.mutateJob(operation); });
  expect(operation).toHaveBeenCalledTimes(1);
  // An already-in-flight background read may finish during the mutation.
  const background = deferred<JobRecord>();
  mocks.get.mockReturnValueOnce(background.promise);
  // Explicitly start the read before a second mutation for an actual overlap.
  await act(async () => { pending.resolve(page(0, 2)); });
  act(() => notify({ payload: { jobId: "a" } }));
  await tick();
  const late = deferred<JobRecord>();
  act(() => { void result.current.mutateJob(() => late.promise); });
  await act(async () => { background.resolve(page(0, 9, "completed")); });
  await act(async () => { late.resolve(page(0, 3, "running")); });
  expect(result.current.job?.stateVersion).toBe(9);
  expect(result.current.job?.state).toBe("completed");
  expect(result.current.jobBusy).toBe(false);
});

test("switching A -> B -> A rejects old mutation errors and finally handlers", async () => {
  const old = deferred<JobRecord>();
  const currentAction = deferred<JobRecord>();
  const error = vi.fn();
  const { result } = renderHook(() => useSessionImportJob(error));
  act(() => result.current.setJob(page(0, 1, "completed")));
  act(() => { void result.current.mutateJob(() => old.promise); });
  act(() => result.current.setJob(page(0, 1, "completed", "b")));
  act(() => result.current.setJob(page(0, 20, "completed")));
  act(() => { void result.current.mutateJob(() => currentAction.promise); });
  await act(async () => { old.reject(new Error("obsolete")); });
  expect(error).not.toHaveBeenCalled();
  expect(result.current.jobBusy).toBe(true);
  expect(result.current.job?.stateVersion).toBe(20);
  await act(async () => { currentAction.resolve(page(0, 21, "completed")); });
  expect(result.current.jobBusy).toBe(false);
  expect(result.current.job?.stateVersion).toBe(21);
});

test("old page and mutation successes cannot enter a newly adopted instance of the same job", async () => {
  const oldPage = deferred<JobRecord>();
  const oldMutation = deferred<JobRecord>();
  const { result } = renderHook(() => useSessionImportJob(vi.fn()));
  mocks.get.mockReturnValueOnce(oldPage.promise);
  await act(async () => { result.current.setJob(page()); });
  act(() => { void result.current.mutateJob(() => oldMutation.promise); });
  await act(async () => { result.current.setJob(page(0, 50, "completed")); });
  await act(async () => { oldPage.resolve(page(0, 100)); oldMutation.resolve(page(0, 101)); });
  expect(result.current.job?.stateVersion).toBe(50);
  expect(result.current.jobActive).toBe(false);
  expect(result.current.jobBusy).toBe(false);
  mocks.get.mockResolvedValue(page(0, 52));
  await act(async () => { result.current.setJob(page(0, 51)); });
  expect(result.current.job?.stateVersion).toBe(52);
});

test("mutation replies preserve the current page and retry a failed terminal page refresh", async () => {
  const error = vi.fn();
  const { result } = renderHook(() => useSessionImportJob(error));
  await act(async () => { result.current.setJob(page()); });
  await act(async () => { await result.current.loadMoreJobItems(); });
  mocks.items.mockRejectedValueOnce(new Error("SOURCE_BUSY"));
  mocks.items.mockRejectedValueOnce(new Error("SOURCE_BUSY_AGAIN"));
  mocks.items.mockResolvedValue(page(50, 3, "cancelled"));
  await act(async () => { await result.current.mutateJob(async () => page(0, 3, "cancelled")); });
  expect(result.current.job?.items[0].candidateId).toBe("candidate-50");
  expect(result.current.jobItemsStale).toBe(true);
  expect(result.current.job?.state).toBe("cancelled");
  await act(async () => { await vi.advanceTimersByTimeAsync(1500); });
  expect(result.current.jobItemsStale).toBe(false);
  expect(result.current.job?.items.every((item) => item.phase === "cancelled")).toBe(true);
  expect(result.current.jobPageIndex).toBe(1);
  const calls = mocks.items.mock.calls.length;
  await act(async () => { await vi.advanceTimersByTimeAsync(3000); });
  expect(mocks.items).toHaveBeenCalledTimes(calls);
});

test("a lost retry reply is recovered from a formerly terminal job without resetting its page", async () => {
  const { result } = renderHook(() => useSessionImportJob(vi.fn()));
  act(() => result.current.setJob(page(0, 4, "cancelled")));
  mocks.items.mockResolvedValueOnce(page(50, 4, "cancelled"));
  await act(async () => { await result.current.loadMoreJobItems(); });
  mocks.items.mockRejectedValueOnce(new Error("read also lost"));
  mocks.items.mockRejectedValueOnce(new Error("initial recovery read also lost"));
  mocks.items.mockResolvedValue(page(50, 5, "queued"));
  await act(async () => { await result.current.mutateJob(async () => { throw new Error("reply lost"); }); });
  expect(result.current.jobItemsStale).toBe(true);
  await act(async () => { await vi.advanceTimersByTimeAsync(1500); });
  expect(result.current.jobActive).toBe(true);
  expect(result.current.job?.stateVersion).toBe(5);
  expect(result.current.jobPageIndex).toBe(1);
});

test("read errors after unmount stay silent, and a StrictMode mount can still adopt and poll", async () => {
  const pending = deferred<JobRecord>();
  mocks.get.mockReturnValueOnce(pending.promise);
  const error = vi.fn();
  const { result, unmount } = renderHook(() => useSessionImportJob(error), { wrapper: StrictMode });
  await act(async () => { result.current.setJob(page()); });
  expect(mocks.get).toHaveBeenCalledOnce();
  unmount();
  await act(async () => { pending.reject(new Error("late read")); });
  expect(error).not.toHaveBeenCalled();
});
