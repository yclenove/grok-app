/** @vitest-environment jsdom */
import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import type { PrepareBatchStatus } from "@/lib/api/sessionMigration";
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
  const promise = new Promise<T>((done) => { resolve = done; });
  return { promise, resolve };
}

function status(
  batchId = "batch-a",
  state: PrepareBatchStatus["state"] = "preparing",
  stateVersion = 1,
): PrepareBatchStatus {
  return {
    version: 1, batchId, scanId: `scan-${batchId}`, epoch: 1,
    selectionId: `selection-${batchId}`, selectionStateVersion: 1, state, stateVersion,
    total: 1, prepared: state === "completed" ? 1 : 0,
    failed: state === "failed" ? 1 : 0, skipped: 0,
    active: state === "preparing" ? 1 : 0,
    cancelRequested: state === "cancelling" || state === "cancelled",
    createdAt: "2026-09-26T00:00:00Z", updatedAt: "2026-09-26T00:00:00Z",
  };
}

beforeEach(() => {
  vi.useFakeTimers();
  vi.resetAllMocks();
  mocks.items.mockResolvedValue({ items: [], nextCursor: null });
  mocks.cancel.mockImplementation(async (batchId: string) => status(batchId, "cancelled", 3));
});
afterEach(() => { cleanup(); vi.useRealTimers(); });

test("stop before the first start reply cancels the accepted batch without releasing busy early", async () => {
  const started = deferred<PrepareBatchStatus>();
  mocks.start.mockReturnValueOnce(started.promise);
  const { result } = renderHook(() => useSessionImportPrepare(vi.fn()));
  let pending!: ReturnType<typeof result.current.start>;
  act(() => { pending = result.current.start("scan-a", 1, "selection-a", []); });
  await act(async () => { await result.current.stop(); });
  expect(result.current.busy).toBe(true);
  expect(result.current.batch).toBeNull();
  expect(mocks.cancel).not.toHaveBeenCalled();
  await act(async () => { started.resolve(status()); await pending; });
  expect(await pending).toBeNull();
  expect(mocks.cancel).toHaveBeenCalledExactlyOnceWith("batch-a");
  expect(result.current.batch?.state).toBe("cancelled");
  expect(result.current.busy).toBe(false);
});

test("stop while replacement awaits prior cancellation prevents launching another batch", async () => {
  const cancelled = deferred<PrepareBatchStatus>();
  mocks.cancel.mockReturnValueOnce(cancelled.promise);
  mocks.start.mockResolvedValueOnce(status("batch-b"));
  const { result } = renderHook(() => useSessionImportPrepare(vi.fn()));
  act(() => result.current.adopt(status()));
  let pending!: ReturnType<typeof result.current.start>;
  act(() => { pending = result.current.start("scan-b", 1, "selection-b", []); });
  await act(async () => { await result.current.stop(); });
  expect(result.current.busy).toBe(true);
  await act(async () => { cancelled.resolve(status("batch-a", "cancelling", 2)); await pending; });
  expect(await pending).toBeNull();
  expect(mocks.start).not.toHaveBeenCalled();
  expect(mocks.cancel).toHaveBeenCalledExactlyOnceWith("batch-a");
  expect(result.current.batch?.batchId).toBe("batch-a");
  expect(result.current.batch?.state).toBe("cancelling");
  expect(result.current.busy).toBe(false);
});

test("stop during a retry reply waits for acceptance then cancels that active batch", async () => {
  const retried = deferred<PrepareBatchStatus>();
  mocks.retry.mockReturnValueOnce(retried.promise);
  const { result } = renderHook(() => useSessionImportPrepare(vi.fn()));
  await act(async () => result.current.adopt(status("batch-a", "failed")));
  let pending!: ReturnType<typeof result.current.retry>;
  act(() => { pending = result.current.retry(); });
  await act(async () => { await result.current.stop(); });
  expect(result.current.busy).toBe(true);
  expect(mocks.cancel).not.toHaveBeenCalled();
  await act(async () => { retried.resolve(status("batch-a", "preparing", 2)); await pending; });
  expect(await pending).toBeNull();
  expect(mocks.cancel).toHaveBeenCalledExactlyOnceWith("batch-a");
  expect(result.current.batch?.state).toBe("cancelled");
  expect(result.current.busy).toBe(false);
});

test.each(["cancel", "discard"] as const)("stop during an existing %s does not submit another cancellation", async (command) => {
  const cancelled = deferred<PrepareBatchStatus>();
  mocks.cancel.mockReturnValueOnce(cancelled.promise);
  const { result } = renderHook(() => useSessionImportPrepare(vi.fn()));
  act(() => result.current.adopt(status()));
  let pending!: Promise<PrepareBatchStatus | boolean | null>;
  act(() => { pending = result.current[command](); });
  await act(async () => { await result.current.stop(); await result.current.stop(); });
  expect(result.current.busy).toBe(true);
  expect(mocks.cancel).toHaveBeenCalledExactlyOnceWith("batch-a");
  await act(async () => { cancelled.resolve(status("batch-a", "cancelling", 2)); await pending; });
  expect(result.current.batch?.state ?? null).toBe(command === "cancel" ? "cancelling" : null);
  expect(result.current.busy).toBe(false);
});

test.each(["start", "retry"] as const)("failed cleanup after a stopped %s retains the accepted batch for retry", async (command) => {
  const accepted = deferred<PrepareBatchStatus>();
  mocks[command].mockReturnValueOnce(accepted.promise);
  mocks.cancel.mockRejectedValueOnce(new Error("cleanup cancellation unavailable"));
  const onError = vi.fn();
  const { result } = renderHook(() => useSessionImportPrepare(onError));
  if (command === "retry") await act(async () => result.current.adopt(status("batch-a", "failed")));
  let pending!: ReturnType<typeof result.current.start>;
  act(() => {
    pending = command === "start"
      ? result.current.start("scan-a", 1, "selection-a", []) : result.current.retry();
  });
  await act(async () => { await result.current.stop(); });
  await act(async () => { accepted.resolve(status("batch-a", "preparing", 2)); await pending; });
  expect(await pending).toBeNull();
  expect(result.current.batch?.state).toBe("preparing");
  expect(result.current.batch?.stateVersion).toBe(2);
  expect(result.current.busy).toBe(false);
  expect(onError).toHaveBeenCalledWith("Error: cleanup cancellation unavailable");
  await act(async () => { await result.current.cancel(); });
  expect(mocks.cancel).toHaveBeenCalledTimes(2);
  expect(result.current.batch?.state).toBe("cancelled");
});

test("stopping an idle terminal batch preserves prepared results", async () => {
  const completed = status("batch-a", "completed", 4);
  const { result } = renderHook(() => useSessionImportPrepare(vi.fn()));
  await act(async () => result.current.adopt(completed));
  await act(async () => { await result.current.stop(); });
  expect(mocks.cancel).not.toHaveBeenCalled();
  expect(result.current.batch).toEqual(completed);
  expect(result.current.busy).toBe(false);
});

test("a stopped start that already completed keeps its terminal snapshot", async () => {
  const started = deferred<PrepareBatchStatus>();
  mocks.start.mockReturnValueOnce(started.promise);
  const { result } = renderHook(() => useSessionImportPrepare(vi.fn()));
  let pending!: ReturnType<typeof result.current.start>;
  act(() => { pending = result.current.start("scan-a", 1, "selection-a", []); });
  await act(async () => { await result.current.stop(); });
  const completed = status("batch-a", "completed", 2);
  await act(async () => { started.resolve(completed); await pending; });
  expect(await pending).toBeNull();
  expect(mocks.cancel).not.toHaveBeenCalled();
  expect(result.current.batch).toEqual(completed);
});

test("retry cannot supersede an in-flight retry command", async () => {
  const retried = deferred<PrepareBatchStatus>();
  mocks.retry.mockReturnValueOnce(retried.promise).mockResolvedValue(status("batch-a", "preparing", 3));
  const { result } = renderHook(() => useSessionImportPrepare(vi.fn()));
  await act(async () => result.current.adopt(status("batch-a", "failed")));
  let pending!: ReturnType<typeof result.current.retry>;
  act(() => { pending = result.current.retry(); });
  await act(async () => { expect(await result.current.retry()).toBeNull(); });
  expect(mocks.retry).toHaveBeenCalledTimes(1);
  expect(result.current.busy).toBe(true);
  await act(async () => { retried.resolve(status("batch-a", "preparing", 2)); await pending; });
  expect(result.current.batch?.stateVersion).toBe(2);
  expect(result.current.busy).toBe(false);
});

test("a stop marker cannot cancel a newer start that superseded its request", async () => {
  const first = deferred<PrepareBatchStatus>();
  const second = deferred<PrepareBatchStatus>();
  mocks.start.mockReturnValueOnce(first.promise).mockReturnValueOnce(second.promise);
  const { result } = renderHook(() => useSessionImportPrepare(vi.fn()));
  let oldStart!: ReturnType<typeof result.current.start>;
  let newStart!: ReturnType<typeof result.current.start>;
  act(() => { oldStart = result.current.start("scan-a", 1, "selection-a", []); });
  await act(async () => { await result.current.stop(); });
  act(() => { newStart = result.current.start("scan-b", 1, "selection-b", []); });
  await act(async () => { first.resolve(status("batch-a")); await oldStart; });
  expect(result.current.busy).toBe(true);
  await act(async () => { second.resolve(status("batch-b")); await newStart; });
  expect(mocks.cancel).toHaveBeenCalledExactlyOnceWith("batch-a");
  expect(result.current.batch?.batchId).toBe("batch-b");
  expect(await newStart).toEqual(status("batch-b"));
  expect(result.current.busy).toBe(false);
});
