/** @vitest-environment jsdom */
import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import type { JobRecord } from "@/lib/api/sessionMigration";
import { useSessionImportJob } from "./useSessionImportJob";

const mocks = vi.hoisted(() => ({ listen: vi.fn(), get: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: mocks.listen }));
vi.mock("@/lib/api/sessionMigration", () => ({ sessionImportJobGet: mocks.get }));

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((r) => { resolve = r; });
  return { promise, resolve };
}

function state(jobId: string, stateVersion = 1, status = "queued"): JobRecord {
  return { jobId, stateVersion, state: status, requestId: jobId,
    planHash: "plan", createdAt: "2026-09-08T00:00:00Z", items: [] };
}

beforeEach(() => { vi.useFakeTimers(); vi.resetAllMocks(); });
afterEach(() => { cleanup(); vi.useRealTimers(); });

test("subscribes before reading state and stops polling when the job finishes", async () => {
  const subscribed = deferred<() => void>();
  const stop = vi.fn();
  mocks.listen.mockReturnValue(subscribed.promise);
  mocks.get.mockResolvedValue(state("a", 4, "completed"));
  const error = vi.fn();
  const { result } = renderHook(() => useSessionImportJob(error));
  await act(async () => { result.current.setJob(state("a")); });
  expect(mocks.get).not.toHaveBeenCalled();
  await act(async () => { subscribed.resolve(stop); });
  expect(result.current.job?.state).toBe("completed");
  expect(stop).toHaveBeenCalledOnce();
  await act(async () => { await vi.advanceTimersByTimeAsync(3000); });
  expect(mocks.get).toHaveBeenCalledOnce();
});

test("late results from the previous job cannot replace the selected job", async () => {
  const old = deferred<JobRecord>();
  mocks.listen.mockResolvedValue(vi.fn());
  mocks.get.mockImplementation((id: string) => id === "a" ? old.promise : Promise.resolve(state("b", 2)));
  const error = vi.fn();
  const { result } = renderHook(() => useSessionImportJob(error));
  await act(async () => { result.current.setJob(state("a")); });
  await act(async () => { result.current.setJob(state("b")); });
  await act(async () => { old.resolve(state("a", 9, "completed")); });
  expect(result.current.job?.jobId).toBe("b");
  expect(result.current.job?.stateVersion).toBe(2);
});

test("a change notification during an outstanding read triggers a fresh read", async () => {
  const old = deferred<JobRecord>();
  let notify!: (event: { payload: { jobId: string } }) => void;
  mocks.listen.mockImplementation((_name, callback) => { notify = callback; return Promise.resolve(vi.fn()); });
  mocks.get.mockReturnValueOnce(old.promise).mockResolvedValue(state("a", 3, "completed"));
  const error = vi.fn();
  const { result } = renderHook(() => useSessionImportJob(error));
  await act(async () => { result.current.setJob(state("a")); });
  await act(async () => { notify({ payload: { jobId: "a" } }); });
  await act(async () => { old.resolve(state("a", 2, "running")); });
  await act(async () => { await vi.advanceTimersByTimeAsync(300); });
  expect(mocks.get).toHaveBeenCalledTimes(2);
  expect(result.current.job?.stateVersion).toBe(3);
  expect(result.current.jobActive).toBe(false);
});

test("unmount during listener setup releases the listener without reading", async () => {
  const subscribed = deferred<() => void>();
  const stop = vi.fn();
  mocks.listen.mockReturnValue(subscribed.promise);
  const error = vi.fn();
  const { result, unmount } = renderHook(() => useSessionImportJob(error));
  await act(async () => { result.current.setJob(state("a")); });
  unmount();
  await act(async () => { subscribed.resolve(stop); });
  expect(stop).toHaveBeenCalledOnce();
  expect(mocks.get).not.toHaveBeenCalled();
});

test("progress bursts stay bounded while polling still recovers the final state", async () => {
  const first = deferred<JobRecord>();
  let notify!: (event: { payload: { jobId: string } }) => void;
  mocks.listen.mockImplementation((_name, callback) => { notify = callback; return Promise.resolve(vi.fn()); });
  mocks.get.mockReturnValueOnce(first.promise).mockResolvedValue(state("a", 10001, "completed"));
  const error = vi.fn();
  const { result } = renderHook(() => useSessionImportJob(error));
  await act(async () => { result.current.setJob(state("a")); });
  await act(async () => {
    for (let i = 0; i < 10_000; i++) notify({ payload: { jobId: "a" } });
    await vi.advanceTimersByTimeAsync(3000);
  });
  expect(mocks.get).toHaveBeenCalledOnce();
  await act(async () => { first.resolve(state("a", 2, "running")); });
  await act(async () => { await vi.advanceTimersByTimeAsync(1000); });
  expect(mocks.get).toHaveBeenCalledTimes(2);
  expect(result.current.job?.stateVersion).toBe(10001);
  expect(result.current.jobActive).toBe(false);
  await act(async () => { await vi.advanceTimersByTimeAsync(5000); });
  expect(mocks.get).toHaveBeenCalledTimes(2);
});

test("polling survives an unavailable listener and a read failure", async () => {
  mocks.listen.mockRejectedValue(new Error("event bridge unavailable"));
  mocks.get.mockResolvedValueOnce(state("a", 2, "running"))
    .mockRejectedValueOnce(new Error("SOURCE_BUSY"))
    .mockResolvedValue(state("a", 3, "completed"));
  const error = vi.fn();
  const { result } = renderHook(() => useSessionImportJob(error));
  await act(async () => { result.current.setJob(state("a")); });
  await act(async () => { await vi.advanceTimersByTimeAsync(1500); });
  expect(error).toHaveBeenCalledOnce();
  expect(result.current.jobActive).toBe(true);
  await act(async () => { await vi.advanceTimersByTimeAsync(1500); });
  expect(result.current.job?.state).toBe("completed");
});

test.each(["recovery_needed", "future_host_state"])("does not poll or restart an inactive %s job", async (status) => {
  const error = vi.fn();
  const { result } = renderHook(() => useSessionImportJob(error));
  await act(async () => { result.current.setJob(state("a", 1, status)); });
  await act(async () => { await vi.advanceTimersByTimeAsync(5000); });
  expect(result.current.jobActive).toBe(false);
  expect(mocks.listen).not.toHaveBeenCalled();
  expect(mocks.get).not.toHaveBeenCalled();
  expect(error).not.toHaveBeenCalled();
  mocks.get.mockResolvedValue(state("a", 2, status));
  await act(async () => { await result.current.refreshJobItems(); });
  expect(mocks.get).toHaveBeenCalledExactlyOnceWith("a");
  expect(result.current.job?.stateVersion).toBe(2);
});

test("explicit recovery retry re-enters normal subscription and polling until completion", async () => {
  const stop = vi.fn(); mocks.listen.mockResolvedValue(stop);
  mocks.get.mockResolvedValueOnce(state("a", 3, "running")).mockResolvedValue(state("a", 4, "completed"));
  const error = vi.fn(); const retry = vi.fn().mockResolvedValue(state("a", 2, "queued"));
  const { result } = renderHook(() => useSessionImportJob(error));
  await act(async () => { result.current.setJob(state("a", 1, "recovery_needed")); });
  expect(mocks.get).not.toHaveBeenCalled(); expect(retry).not.toHaveBeenCalled();
  await act(async () => { await result.current.mutateJob(retry); });
  expect(retry).toHaveBeenCalledExactlyOnceWith("a", {
    expectedStateVersion: 1, requestId: expect.any(String),
  });
  expect(result.current.jobActive).toBe(true);
  await act(async () => { await vi.advanceTimersByTimeAsync(1500); });
  expect(result.current.job?.state).toBe("completed");
  expect(result.current.jobActive).toBe(false); expect(stop).toHaveBeenCalledOnce();
  expect(error).not.toHaveBeenCalled();
});
