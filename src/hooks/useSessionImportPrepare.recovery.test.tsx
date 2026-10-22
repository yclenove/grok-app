/** @vitest-environment jsdom */
import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import type { PrepareBatchItemPage, PrepareBatchStatus } from "@/lib/api/sessionMigration";
import { useSessionImportPrepare } from "./useSessionImportPrepare";

const mocks = vi.hoisted(() => ({
  start: vi.fn(), get: vi.fn(), items: vi.fn(), cancel: vi.fn(), retry: vi.fn(),
}));
vi.mock("@/lib/api/sessionMigration", () => ({
  sessionImportPrepareStart: mocks.start,
  sessionImportPrepareGet: mocks.get,
  sessionImportPrepareItems: mocks.items,
  sessionImportPrepareCancel: mocks.cancel,
  sessionImportPrepareRetry: mocks.retry,
}));

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((ok) => { resolve = ok; });
  return { promise, resolve };
}

function status(state: PrepareBatchStatus["state"] = "preparing", stateVersion = 1): PrepareBatchStatus {
  return {
    version: 1, batchId: "batch-a", scanId: "scan-a", epoch: 1,
    selectionId: "selection-a", selectionStateVersion: 1, state, stateVersion,
    total: 1, prepared: state === "completed" ? 1 : 0, failed: 0, skipped: 0,
    active: state === "preparing" ? 1 : 0, cancelRequested: false,
    createdAt: "2026-09-20T00:00:00Z", updatedAt: "2026-09-20T00:00:00Z",
  };
}

function page(id: string, nextCursor: string | null = null): PrepareBatchItemPage {
  return { items: [{
    candidateId: id, title: id, provider: "codex", sourceRevision: id,
    state: "prepared", attempt: 1, previewId: id, error: null,
  }], nextCursor };
}

beforeEach(() => {
  vi.useFakeTimers();
  vi.resetAllMocks();
  mocks.start.mockResolvedValue(status());
  mocks.items.mockResolvedValue(page("fresh"));
});
afterEach(() => { cleanup(); vi.useRealTimers(); });

async function advance(ms: number) {
  await act(async () => { await vi.advanceTimersByTimeAsync(ms); });
}

test.each(["cancel", "discard", "replace"] as const)(
  "%s failure resumes observation of the original active batch",
  async (operation) => {
    const onError = vi.fn();
    mocks.cancel.mockRejectedValueOnce(new Error("cancel unavailable"));
    mocks.get.mockResolvedValue(status("completed", 3));
    const { result } = renderHook(() => useSessionImportPrepare(onError));
    await act(async () => { await result.current.start("scan-a", 1, "selection-a", []); });
    await act(async () => {
      if (operation === "replace") await result.current.start("scan-b", 1, "selection-b", []);
      else await result.current[operation]();
    });
    expect(onError).toHaveBeenCalledWith("Error: cancel unavailable");
    await advance(1000);
    expect(result.current.batch?.state).toBe("completed");
    expect(mocks.get).toHaveBeenCalledTimes(1);
    expect(result.current.busy).toBe(false);
  },
);

test("a stale same-batch poll does not stop future polls", async () => {
  const stale = deferred<PrepareBatchStatus>();
  mocks.get.mockReturnValueOnce(stale.promise).mockResolvedValue(status("completed", 4));
  const onError = vi.fn();
  const { result } = renderHook(() => useSessionImportPrepare(onError));
  act(() => result.current.adopt(status()));
  await advance(100);
  act(() => result.current.adopt(status("preparing", 3)));
  await act(async () => stale.resolve(status("preparing", 2)));
  await advance(500);
  expect(result.current.batch?.state).toBe("completed");
  expect(mocks.get).toHaveBeenCalledTimes(2);
});

test("retry failure restores the terminal result list without resubmitting preparation", async () => {
  mocks.retry.mockRejectedValueOnce(new Error("retry unavailable"));
  const onError = vi.fn();
  const { result } = renderHook(() => useSessionImportPrepare(onError));
  await act(async () => result.current.adopt(status("failed")));
  expect(result.current.items).toHaveLength(1);
  await act(async () => { await result.current.retry(); });
  expect(result.current.batch?.state).toBe("failed");
  expect(result.current.items).toHaveLength(1);
  expect(result.current.busy).toBe(false);
  expect(mocks.retry).toHaveBeenCalledTimes(1);
});

test("cancellation after preparation resumes invalidates a pending page without locking future pages", async () => {
  const stalePage = deferred<PrepareBatchItemPage>();
  mocks.items.mockReturnValueOnce(stalePage.promise).mockResolvedValue(page("fresh", "next"));
  mocks.cancel.mockResolvedValue(status("cancelled", 3));
  const onError = vi.fn();
  const { result } = renderHook(() => useSessionImportPrepare(onError));
  await act(async () => result.current.adopt(status("failed")));
  // A retry observed for the same batch retains the in-flight results owner.
  act(() => result.current.adopt(status("preparing", 2)));
  await act(async () => { await result.current.cancel(); });
  await act(async () => stalePage.resolve(page("stale")));
  expect(result.current.items.map((entry) => entry.candidateId)).toEqual(["fresh"]);
  expect(result.current.busy).toBe(false);
  mocks.items.mockResolvedValueOnce(page("second"));
  await act(async () => { await result.current.loadMore(); });
  expect(result.current.items.map((entry) => entry.candidateId)).toEqual(["fresh", "second"]);
});

test("a failed first result page has an explicit read-only retry without a request storm", async () => {
  mocks.items.mockRejectedValueOnce(new Error("items unavailable")).mockResolvedValue(page("fresh"));
  const onError = vi.fn();
  const { result } = renderHook(() => useSessionImportPrepare(onError));
  await act(async () => result.current.adopt(status("completed")));
  expect(result.current.itemsError).toBe(true);
  expect(result.current.busy).toBe(false);
  await advance(10_000);
  expect(mocks.items).toHaveBeenCalledTimes(1);
  await act(async () => { await result.current.retryItems(); });
  expect(result.current.itemsError).toBe(false);
  expect(result.current.items.map((entry) => entry.candidateId)).toEqual(["fresh"]);
  expect(mocks.items.mock.calls.map((call) => call[1])).toEqual([null, null]);
  expect(mocks.retry).not.toHaveBeenCalled();
  expect(mocks.start).not.toHaveBeenCalled();
});

test("retrying a failed next page preserves the previous pages and cursor", async () => {
  mocks.items.mockResolvedValueOnce(page("first", "next"))
    .mockRejectedValueOnce(new Error("items unavailable"))
    .mockResolvedValueOnce(page("second"));
  const onError = vi.fn();
  const { result } = renderHook(() => useSessionImportPrepare(onError));
  await act(async () => result.current.adopt(status("completed")));
  await act(async () => { await result.current.loadMore(); });
  expect(result.current.itemsError).toBe(true);
  expect(result.current.items.map((entry) => entry.candidateId)).toEqual(["first"]);
  await act(async () => { await result.current.retryItems(); });
  expect(result.current.items.map((entry) => entry.candidateId)).toEqual(["first", "second"]);
  expect(mocks.items.mock.calls.map((call) => call[1])).toEqual([null, "next", "next"]);
  expect(result.current.itemsError).toBe(false);
});

test("late pages and polls from before cancellation cannot overwrite the refreshed owner", async () => {
  const stalePoll = deferred<PrepareBatchStatus>();
  mocks.get.mockReturnValueOnce(stalePoll.promise).mockResolvedValue(status("completed", 4));
  mocks.cancel.mockRejectedValueOnce(new Error("cancel unavailable"));
  const onError = vi.fn();
  const { result } = renderHook(() => useSessionImportPrepare(onError));
  act(() => result.current.adopt(status()));
  await advance(100);
  await act(async () => { await result.current.cancel(); });
  await advance(100);
  await act(async () => stalePoll.resolve(status("failed", 9)));
  expect(result.current.batch?.state).toBe("completed");
  expect(result.current.batch?.stateVersion).toBe(4);
});

test("same-batch adoption does not unlock a pending command or page", async () => {
  const cancellation = deferred<PrepareBatchStatus>();
  mocks.cancel.mockReturnValueOnce(cancellation.promise);
  const onError = vi.fn();
  const { result } = renderHook(() => useSessionImportPrepare(onError));
  act(() => result.current.adopt(status()));
  let pending!: ReturnType<typeof result.current.cancel>;
  act(() => { pending = result.current.cancel(); });
  act(() => result.current.adopt(status("preparing", 2)));
  expect(result.current.busy).toBe(true);
  await advance(1000);
  expect(mocks.get).not.toHaveBeenCalled();
  await act(async () => { cancellation.resolve(status("cancelled", 3)); await pending; });
  expect(result.current.busy).toBe(false);
});

test("retry remembers a failed first-page refresh rather than using the retained next cursor", async () => {
  const interrupted = deferred<PrepareBatchItemPage>();
  mocks.items.mockResolvedValueOnce(page("first", "next"))
    .mockReturnValueOnce(interrupted.promise)
    .mockRejectedValueOnce(new Error("refresh unavailable"))
    .mockResolvedValueOnce(page("refreshed"));
  mocks.cancel.mockResolvedValue(status("cancelled", 3));
  const onError = vi.fn();
  const { result } = renderHook(() => useSessionImportPrepare(onError));
  await act(async () => result.current.adopt(status("failed")));
  act(() => { void result.current.loadMore(); });
  // Cancel only after a same-batch retry is active, keeping the old page/cursor
  // in flight so this still exercises invalidation and first-page retry routing.
  act(() => result.current.adopt(status("preparing", 2)));
  await act(async () => { await result.current.cancel(); });
  expect(result.current.itemsError).toBe(true);
  await act(async () => { await result.current.retryItems(); });
  expect(mocks.items.mock.calls.map((call) => call[1])).toEqual([null, "next", null, null]);
  expect(result.current.items.map((entry) => entry.candidateId)).toEqual(["refreshed"]);
  await act(async () => interrupted.resolve(page("stale")));
  expect(result.current.items.map((entry) => entry.candidateId)).toEqual(["refreshed"]);
});
