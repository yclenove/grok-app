/** @vitest-environment jsdom */
import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import type {
  PrepareBatchItem,
  PrepareBatchItemPage,
  PrepareBatchState,
  PrepareBatchStatus,
} from "@/lib/api/sessionMigration";
import { useSessionImportPrepare } from "./useSessionImportPrepare";

const mocks = vi.hoisted(() => ({
  start: vi.fn(),
  get: vi.fn(),
  items: vi.fn(),
  cancel: vi.fn(),
  retry: vi.fn(),
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
  let reject!: (cause: unknown) => void;
  const promise = new Promise<T>((ok, fail) => {
    resolve = ok;
    reject = fail;
  });
  return { promise, resolve, reject };
}

function status(
  batchId: string,
  state: PrepareBatchState = "preparing",
  stateVersion = 1,
): PrepareBatchStatus {
  const prepared = state === "completed" || state === "planned" ? 1 : 0;
  return {
    version: 1,
    batchId,
    scanId: `scan-${batchId}`,
    epoch: 1,
    selectionId: `selection-${batchId}`,
    selectionStateVersion: 1,
    state,
    stateVersion,
    total: 1,
    prepared,
    failed: 0,
    skipped: 0,
    active: state === "preparing" ? 1 : 0,
    cancelRequested: state === "cancelling" || state === "cancelled",
    createdAt: "2026-09-13T00:00:00Z",
    updatedAt: "2026-09-13T00:00:00Z",
  };
}

function item(candidateId: string): PrepareBatchItem {
  return {
    candidateId,
    title: candidateId,
    provider: "codex",
    sourceRevision: `revision-${candidateId}`,
    state: "prepared",
    attempt: 1,
    previewId: `preview-${candidateId}`,
    error: null,
  };
}

function page(ids: string[], nextCursor: string | null = null): PrepareBatchItemPage {
  return { items: ids.map(item), nextCursor };
}

beforeEach(() => {
  vi.useFakeTimers();
  vi.resetAllMocks();
  mocks.cancel.mockImplementation(async (batchId: string) => status(batchId, "cancelled", 2));
  mocks.items.mockResolvedValue(page([]));
});

afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

async function advance(ms: number) {
  await act(async () => {
    await vi.advanceTimersByTimeAsync(ms);
  });
}

async function flush() {
  await act(async () => {
    await Promise.resolve();
  });
}

test("duplicate cancel requests cannot supersede the pending cancellation owner", async () => {
  const cancellation = deferred<PrepareBatchStatus>();
  mocks.cancel.mockReturnValueOnce(cancellation.promise);
  const { result } = renderHook(() => useSessionImportPrepare(vi.fn()));
  act(() => result.current.adopt(status("batch-a")));
  let first!: ReturnType<typeof result.current.cancel>;
  let duplicate!: ReturnType<typeof result.current.cancel>;
  act(() => {
    first = result.current.cancel();
    duplicate = result.current.cancel();
  });
  expect(mocks.cancel).toHaveBeenCalledTimes(1);
  expect(await duplicate).toBeNull();
  expect(result.current.busy).toBe(true);
  const acknowledged = status("batch-a", "cancelling", 2);
  await act(async () => { cancellation.resolve(acknowledged); await first; });
  expect(await first).toEqual(acknowledged);
  expect(result.current.batch).toEqual(acknowledged);
  expect(result.current.busy).toBe(false);
});

test.each(["cancelling", "completed", "completed-with-issues", "failed", "cancelled", "planned"] as const)(
  "cancel ignores an already %s batch without replacing its state",
  async (state) => {
    const onError = vi.fn();
    const { result } = renderHook(() => useSessionImportPrepare(onError));
    const current = status("batch-a", state, 3);
    await act(async () => result.current.adopt(current));
    let cancelled: PrepareBatchStatus | null = status("sentinel");
    await act(async () => { cancelled = await result.current.cancel(); });
    expect(cancelled).toBeNull();
    expect(mocks.cancel).not.toHaveBeenCalled();
    expect(result.current.batch).toEqual(current);
    expect(result.current.busy).toBe(false);
    expect(onError).not.toHaveBeenCalled();
  },
);

test.each(["queued", "preparing"] as const)("cancel still accepts an idle %s batch", async (state) => {
  const { result } = renderHook(() => useSessionImportPrepare(vi.fn()));
  act(() => result.current.adopt(status("batch-a", state)));
  await act(async () => { await result.current.cancel(); });
  expect(mocks.cancel).toHaveBeenCalledExactlyOnceWith("batch-a");
  expect(result.current.batch?.state).toBe("cancelled");
});

test("ignoring cancel while cancelling keeps the existing poll owner intact", async () => {
  const polled = deferred<PrepareBatchStatus>();
  mocks.get.mockReturnValueOnce(polled.promise);
  const onError = vi.fn();
  const { result } = renderHook(() => useSessionImportPrepare(onError));
  act(() => result.current.adopt(status("batch-a", "cancelling", 2)));
  await advance(100);
  await act(async () => { expect(await result.current.cancel()).toBeNull(); });
  await act(async () => polled.resolve(status("batch-a", "cancelled", 3)));
  expect(mocks.cancel).not.toHaveBeenCalled();
  expect(mocks.get).toHaveBeenCalledTimes(1);
  expect(result.current.batch?.state).toBe("cancelled");
  expect(result.current.batch?.stateVersion).toBe(3);
  expect(result.current.active).toBe(false);
  expect(onError).not.toHaveBeenCalled();
});

test("cancel does not invalidate a replacement command waiting for its cancellation acknowledgement", async () => {
  const cancellation = deferred<PrepareBatchStatus>();
  mocks.cancel.mockReturnValueOnce(cancellation.promise);
  mocks.start.mockResolvedValueOnce(status("batch-b"));
  const { result } = renderHook(() => useSessionImportPrepare(vi.fn()));
  act(() => result.current.adopt(status("batch-a")));
  let replacement!: ReturnType<typeof result.current.start>;
  act(() => { replacement = result.current.start("scan-b", 1, "selection-b", []); });
  let cancelled: PrepareBatchStatus | null = status("sentinel");
  await act(async () => { cancelled = await result.current.cancel(); });
  expect(cancelled).toBeNull();
  expect(mocks.cancel).toHaveBeenCalledExactlyOnceWith("batch-a");
  expect(result.current.busy).toBe(true);
  await act(async () => { cancellation.resolve(status("batch-a", "cancelling", 2)); await replacement; });
  expect(result.current.batch?.batchId).toBe("batch-b");
  expect(mocks.start).toHaveBeenCalledTimes(1);
  expect(result.current.busy).toBe(false);
});

test("a late poll response cannot replace an adopted batch", async () => {
  const polled = deferred<PrepareBatchStatus>();
  mocks.start.mockResolvedValueOnce(status("batch-a"));
  mocks.get.mockReturnValueOnce(polled.promise);
  const onError = vi.fn();
  const { result } = renderHook(() => useSessionImportPrepare(onError));

  await act(async () => {
    await result.current.start("scan-a", 1, "selection-a", []);
  });
  await advance(100);
  act(() => result.current.adopt(status("batch-b")));
  await act(async () => polled.resolve(status("batch-a", "completed", 2)));

  expect(result.current.batch?.batchId).toBe("batch-b");
  expect(result.current.batch?.state).toBe("preparing");
  expect(onError).not.toHaveBeenCalled();
});

test("late cancel and retry responses cannot revive an old owner", async () => {
  const cancelled = deferred<PrepareBatchStatus>();
  const retried = deferred<PrepareBatchStatus>();
  mocks.start.mockResolvedValueOnce(status("batch-a"));
  mocks.cancel.mockReturnValueOnce(cancelled.promise);
  mocks.retry.mockReturnValueOnce(retried.promise);
  const { result } = renderHook(() => useSessionImportPrepare(vi.fn()));

  await act(async () => {
    await result.current.start("scan-a", 1, "selection-a", []);
  });
  let cancelPromise!: Promise<PrepareBatchStatus | null>;
  act(() => {
    cancelPromise = result.current.cancel();
  });
  act(() => result.current.adopt(status("batch-b", "failed", 3)));
  await act(async () => cancelled.resolve(status("batch-a", "cancelled", 2)));
  await cancelPromise;
  expect(result.current.batch?.batchId).toBe("batch-b");

  let retryPromise!: Promise<PrepareBatchStatus | null>;
  act(() => {
    retryPromise = result.current.retry();
  });
  act(() => result.current.clear());
  await act(async () => retried.resolve(status("batch-b", "preparing", 4)));
  await retryPromise;
  expect(result.current.batch).toBeNull();
});

test("polling recovers after one transient Host error", async () => {
  mocks.start.mockResolvedValueOnce(status("batch-a"));
  mocks.get
    .mockRejectedValueOnce(new Error("temporary sqlite busy"))
    .mockResolvedValueOnce(status("batch-a", "completed", 2));
  const onError = vi.fn();
  const { result } = renderHook(() => useSessionImportPrepare(onError));

  await act(async () => {
    await result.current.start("scan-a", 1, "selection-a", []);
  });
  await advance(100);
  expect(onError).toHaveBeenCalledWith("Error: temporary sqlite busy");
  await advance(500);
  expect(result.current.batch?.state).toBe("completed");
  expect(mocks.get).toHaveBeenCalledTimes(2);
});

test("terminal items load once and load-more pages remain appended", async () => {
  mocks.start.mockResolvedValueOnce(status("batch-a", "completed"));
  mocks.items
    .mockResolvedValueOnce(page(["one"], "cursor-2"))
    .mockResolvedValueOnce(page(["two"], "cursor-3"))
    .mockResolvedValueOnce(page(["three"]));
  const { result } = renderHook(() => useSessionImportPrepare(vi.fn()));

  await act(async () => {
    await result.current.start("scan-a", 1, "selection-a", []);
  });
  await flush();
  expect(result.current.items.map((entry) => entry.candidateId)).toEqual(["one"]);
  await act(async () => {
    await result.current.loadMore();
    await result.current.loadMore();
  });

  expect(result.current.items.map((entry) => entry.candidateId)).toEqual(["one", "two", "three"]);
  expect(mocks.items.mock.calls.map((call) => call[1])).toEqual([null, "cursor-2", "cursor-3"]);
});

test("a superseded start cancels the batch it created late", async () => {
  const first = deferred<PrepareBatchStatus>();
  const second = deferred<PrepareBatchStatus>();
  mocks.start.mockReturnValueOnce(first.promise).mockReturnValueOnce(second.promise);
  const { result } = renderHook(() => useSessionImportPrepare(vi.fn()));

  let firstStart!: Promise<PrepareBatchStatus | null>;
  let secondStart!: Promise<PrepareBatchStatus | null>;
  act(() => {
    firstStart = result.current.start("scan-a", 1, "selection-a", []);
    secondStart = result.current.start("scan-b", 2, "selection-b", []);
  });
  await act(async () => first.resolve(status("orphan-a")));
  await firstStart;
  expect(mocks.cancel).toHaveBeenCalledWith("orphan-a");
  await act(async () => second.resolve(status("batch-b")));
  await secondStart;
  expect(result.current.batch?.batchId).toBe("batch-b");
});

test("adopting a new terminal batch releases an old items request owner", async () => {
  const oldPage = deferred<PrepareBatchItemPage>();
  mocks.start.mockResolvedValueOnce(status("batch-a", "completed"));
  mocks.items.mockReturnValueOnce(oldPage.promise).mockResolvedValueOnce(page(["new"]));
  const { result } = renderHook(() => useSessionImportPrepare(vi.fn()));

  await act(async () => {
    await result.current.start("scan-a", 1, "selection-a", []);
  });
  await flush();
  expect(mocks.items).toHaveBeenCalledWith("batch-a", null, 50);
  act(() => result.current.adopt(status("batch-b", "completed")));
  await flush();
  expect(mocks.items).toHaveBeenCalledWith("batch-b", null, 50);
  expect(result.current.items.map((entry) => entry.candidateId)).toEqual(["new"]);
  await act(async () => oldPage.resolve(page(["old"])));

  expect(result.current.items.map((entry) => entry.candidateId)).toEqual(["new"]);
});

test("re-adopting the same terminal batch preserves its in-flight items owner", async () => {
  const oldPage = deferred<PrepareBatchItemPage>();
  mocks.start.mockResolvedValueOnce(status("batch-a", "completed"));
  mocks.items.mockReturnValueOnce(oldPage.promise);
  const { result } = renderHook(() => useSessionImportPrepare(vi.fn()));

  await act(async () => {
    await result.current.start("scan-a", 1, "selection-a", []);
  });
  await flush();
  act(() => result.current.adopt(status("batch-a", "completed", 2)));
  await flush();

  expect(mocks.items).toHaveBeenCalledTimes(1);
  await act(async () => oldPage.resolve(page(["same-batch"])));
  expect(result.current.items.map((entry) => entry.candidateId)).toEqual(["same-batch"]);
});

test("re-adopting the same active batch preserves polling and rejects version rollback", async () => {
  const oldPoll = deferred<PrepareBatchStatus>();
  mocks.start.mockResolvedValueOnce(status("batch-a"));
  mocks.get.mockReturnValueOnce(oldPoll.promise);
  const onError = vi.fn();
  const { result } = renderHook(() => useSessionImportPrepare(onError));

  await act(async () => {
    await result.current.start("scan-a", 1, "selection-a", []);
  });
  await advance(100);
  act(() => result.current.adopt(status("batch-a", "preparing", 2)));
  await act(async () => oldPoll.resolve(status("batch-a", "completed", 3)));
  expect(mocks.get).toHaveBeenCalledTimes(1);
  expect(result.current.batch?.state).toBe("completed");
  act(() => result.current.adopt(status("batch-a", "failed", 2)));
  expect(result.current.batch?.state).toBe("completed");
});

test("starting a replacement waits for active-batch cancellation acknowledgement", async () => {
  const cancellation = deferred<PrepareBatchStatus>();
  mocks.start
    .mockResolvedValueOnce(status("batch-a"))
    .mockResolvedValueOnce(status("batch-b"));
  mocks.cancel.mockReturnValueOnce(cancellation.promise);
  const { result } = renderHook(() => useSessionImportPrepare(vi.fn()));

  await act(async () => {
    await result.current.start("scan-a", 1, "selection-a", []);
  });
  let replacement!: Promise<PrepareBatchStatus | null>;
  act(() => {
    replacement = result.current.start("scan-b", 2, "selection-b", []);
  });
  expect(mocks.start).toHaveBeenCalledTimes(1);
  await act(async () => cancellation.resolve(status("batch-a", "cancelling", 2)));
  await replacement;

  expect(mocks.start).toHaveBeenCalledTimes(2);
  expect(result.current.batch?.batchId).toBe("batch-b");
});

test("a replacement does not start when active-batch cancellation fails", async () => {
  mocks.start.mockResolvedValueOnce(status("batch-a"));
  mocks.cancel.mockRejectedValueOnce(new Error("cancel unavailable"));
  const onError = vi.fn();
  const { result } = renderHook(() => useSessionImportPrepare(onError));

  await act(async () => {
    await result.current.start("scan-a", 1, "selection-a", []);
  });
  let replacement: PrepareBatchStatus | null = status("sentinel");
  await act(async () => {
    replacement = await result.current.start("scan-b", 2, "selection-b", []);
  });

  expect(replacement).toBeNull();
  expect(mocks.start).toHaveBeenCalledTimes(1);
  expect(result.current.batch?.batchId).toBe("batch-a");
  expect(onError).toHaveBeenCalledWith("Error: cancel unavailable");
});

test("discard acknowledges active cancellation before clearing local ownership", async () => {
  const cancellation = deferred<PrepareBatchStatus>();
  mocks.start.mockResolvedValueOnce(status("batch-a"));
  mocks.cancel.mockReturnValueOnce(cancellation.promise);
  const { result } = renderHook(() => useSessionImportPrepare(vi.fn()));

  await act(async () => {
    await result.current.start("scan-a", 1, "selection-a", []);
  });
  let discarded!: Promise<boolean>;
  act(() => {
    discarded = result.current.discard();
  });
  expect(result.current.batch?.batchId).toBe("batch-a");
  await act(async () => cancellation.resolve(status("batch-a", "cancelling", 2)));

  expect(await discarded).toBe(true);
  expect(result.current.batch).toBeNull();
});

test("discard preserves the active batch when cancellation is not acknowledged", async () => {
  mocks.start.mockResolvedValueOnce(status("batch-a"));
  mocks.cancel.mockRejectedValueOnce(new Error("cancel unavailable"));
  const onError = vi.fn();
  const { result } = renderHook(() => useSessionImportPrepare(onError));

  await act(async () => {
    await result.current.start("scan-a", 1, "selection-a", []);
  });
  let discarded = true;
  await act(async () => {
    discarded = await result.current.discard();
  });

  expect(discarded).toBe(false);
  expect(result.current.batch?.batchId).toBe("batch-a");
  expect(onError).toHaveBeenCalledWith("Error: cancel unavailable");
});
