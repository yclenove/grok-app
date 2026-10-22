import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { createImportRefreshQueue, refreshPauseMs } from "./refreshQueue";

function deferred() {
  let resolve!: () => void;
  const promise = new Promise<void>((done) => { resolve = done; });
  return { promise, resolve };
}
beforeEach(() => vi.useFakeTimers());
afterEach(() => vi.useRealTimers());

test("ten thousand hints coalesce and retain one trailing read during an outstanding request", async () => {
  const first = deferred();
  const load = vi.fn().mockReturnValueOnce(first.promise).mockResolvedValue(undefined);
  const queue = createImportRefreshQueue(load);
  for (let i = 0; i < 10_000; i++) queue.request();
  await vi.advanceTimersByTimeAsync(1000);
  expect(load).toHaveBeenCalledTimes(1);
  for (let i = 0; i < 10_000; i++) queue.request();
  await vi.advanceTimersByTimeAsync(3000);
  expect(load).toHaveBeenCalledTimes(1);
  first.resolve();
  // The four-second read is followed by the longest pause, then one trailing read.
  await vi.advanceTimersByTimeAsync(3900);
  expect(load).toHaveBeenCalledTimes(1);
  await vi.advanceTimersByTimeAsync(200);
  expect(load).toHaveBeenCalledTimes(2);
  await vi.advanceTimersByTimeAsync(5000);
  expect(load).toHaveBeenCalledTimes(2);
  queue.dispose();
});

test("the pause after a read grows with its duration within fixed bounds", () => {
  expect(refreshPauseMs(0)).toBe(250);
  expect(refreshPauseMs(100)).toBe(250);
  expect(refreshPauseMs(700)).toBe(1400);
  expect(refreshPauseMs(60_000)).toBe(4000);
});

test("slow reads keep continuous hints to at most a third of the time", async () => {
  const load = vi.fn(() => new Promise<void>((done) => { setTimeout(done, 700); }));
  const queue = createImportRefreshQueue(load);
  for (let i = 0; i < 200; i++) {
    queue.request();
    await vi.advanceTimersByTimeAsync(50);
  }
  // 10 s of hints: each 700 ms read is followed by a 1400 ms pause.
  expect(load.mock.calls.length).toBeGreaterThanOrEqual(4);
  expect(load.mock.calls.length).toBeLessThanOrEqual(5);
  queue.dispose();
});

test("manual paging blocks background reads without losing the pending hint", async () => {
  let busy = true;
  const load = vi.fn().mockResolvedValue(undefined);
  const queue = createImportRefreshQueue(load, () => busy);
  queue.request();
  await vi.advanceTimersByTimeAsync(3000);
  expect(load).not.toHaveBeenCalled();
  busy = false;
  await vi.advanceTimersByTimeAsync(1000);
  expect(load).toHaveBeenCalledOnce();
  queue.dispose();
});

test("continuous hints remain bounded without postponing all refreshes until the stream ends", async () => {
  const load = vi.fn().mockResolvedValue(undefined);
  const queue = createImportRefreshQueue(load);
  for (let i = 0; i < 100; i++) {
    queue.request();
    await vi.advanceTimersByTimeAsync(50);
  }
  expect(load.mock.calls.length).toBeGreaterThanOrEqual(4);
  expect(load.mock.calls.length).toBeLessThanOrEqual(20);
  queue.dispose();
});

test("an obsolete request cannot block a new view or release its running request", async () => {
  const old = deferred();
  const current = deferred();
  const load = vi.fn().mockReturnValueOnce(old.promise).mockReturnValueOnce(current.promise).mockResolvedValue(undefined);
  const queue = createImportRefreshQueue(load);
  queue.request();
  await vi.advanceTimersByTimeAsync(500);
  queue.reset();
  queue.request();
  await vi.advanceTimersByTimeAsync(500);
  expect(load).toHaveBeenCalledTimes(2);
  queue.request();
  old.resolve();
  await vi.advanceTimersByTimeAsync(1000);
  expect(load).toHaveBeenCalledTimes(2);
  current.resolve();
  // The current read started at 750 ms and ended at 2 s, so its trailing
  // read follows a 2.5 s pause.
  await vi.advanceTimersByTimeAsync(2400);
  expect(load).toHaveBeenCalledTimes(2);
  await vi.advanceTimersByTimeAsync(200);
  expect(load).toHaveBeenCalledTimes(3);
  queue.dispose();
});

test("a new view starts again from the shortest pause", async () => {
  const slow = deferred();
  const load = vi.fn().mockReturnValueOnce(slow.promise).mockResolvedValue(undefined);
  const queue = createImportRefreshQueue(load);
  queue.request();
  await vi.advanceTimersByTimeAsync(250);
  queue.request();
  await vi.advanceTimersByTimeAsync(3000);
  slow.resolve();
  await vi.advanceTimersByTimeAsync(0);
  queue.reset();
  queue.request();
  await vi.advanceTimersByTimeAsync(300);
  expect(load).toHaveBeenCalledTimes(2);
  queue.dispose();
});

test("disposal clears queued work and a late result cannot schedule another read", async () => {
  const pending = deferred();
  const load = vi.fn().mockReturnValue(pending.promise);
  const queue = createImportRefreshQueue(load);
  queue.request();
  await vi.advanceTimersByTimeAsync(500);
  queue.request();
  queue.dispose();
  pending.resolve();
  queue.request();
  await vi.advanceTimersByTimeAsync(5000);
  expect(load).toHaveBeenCalledOnce();
  expect(vi.getTimerCount()).toBe(0);
});

test("a failed read does not spin or prevent a later hint from retrying", async () => {
  const load = vi.fn().mockRejectedValueOnce(new Error("visible loader error")).mockResolvedValue(undefined);
  const queue = createImportRefreshQueue(load);
  queue.request();
  await vi.advanceTimersByTimeAsync(5000);
  expect(load).toHaveBeenCalledOnce();
  queue.request();
  await vi.advanceTimersByTimeAsync(500);
  expect(load).toHaveBeenCalledTimes(2);
  queue.dispose();
});
