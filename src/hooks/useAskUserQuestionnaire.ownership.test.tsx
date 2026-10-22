/** @vitest-environment jsdom */
import "@/test/jsdomStubs";
import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import type { AskUserPayload } from "@/lib/session";
import { dropAskUserClocks, getAskUserClocks, isAskUserClockPaused } from "@/lib/askUser/askUserClocks";
import { askUserClockKey } from "@/lib/askUser/askUserIdentity";
import { useAskUserQuestionnaire } from "./useAskUserQuestionnaire";

const original: AskUserPayload = {
  sessionId: "questionnaire-test", rpcId: 0, requestGeneration: "original",
  questions: [{ id: "q", question: "选择方案", options: [{ id: "a", label: "甲" }] }],
};
function deferred() {
  let resolve!: () => void;
  const promise = new Promise<void>((done) => { resolve = done; });
  return { promise, resolve };
}
beforeEach(() => { vi.useFakeTimers(); dropAskUserClocks(original.sessionId); });
afterEach(() => { cleanup(); dropAskUserClocks(original.sessionId); vi.clearAllTimers(); vi.useRealTimers(); vi.restoreAllMocks(); });

test("same RPC with a new generation resets choices and free text", () => {
  const hook = renderHook(({ payload }) => useAskUserQuestionnaire(payload, 0, vi.fn()), {
    initialProps: { payload: original },
  });
  act(() => {
    hook.result.current.toggleOption(original.questions[0], 0, "a");
    hook.result.current.writeFreeText("q", "保留给原问题", false);
  });
  hook.rerender({ payload: { ...original, requestGeneration: "next" } });
  expect(hook.result.current.selected).toEqual({});
  expect(hook.result.current.freeText).toEqual({});
});

test("two submit calls before React renders dispatch only one answer", async () => {
  const pending = deferred();
  const send = vi.fn(() => pending.promise);
  const hook = renderHook(() => useAskUserQuestionnaire(original, 0, vi.fn()));
  let first!: Promise<unknown>;
  let second!: Promise<unknown>;
  act(() => {
    first = hook.result.current.submit(send, { q: "甲" });
    second = hook.result.current.submit(send, { q: "乙" });
  });
  const count = send.mock.calls.length;
  await act(async () => { pending.resolve(); await Promise.all([first, second]); });
  expect(count).toBe(1);
  expect(send).toHaveBeenCalledWith({ q: "甲" });
});

test("the old answer's finally cannot unlock a new question's active answer", async () => {
  const old = deferred();
  const next = deferred();
  const hook = renderHook(({ payload }) => useAskUserQuestionnaire(payload, 0, vi.fn()), {
    initialProps: { payload: original },
  });
  let oldReply!: Promise<unknown>;
  act(() => { oldReply = hook.result.current.submit(() => old.promise); });
  hook.rerender({ payload: { ...original, rpcId: 1, requestGeneration: "next" } });
  let nextReply!: Promise<unknown>;
  act(() => { nextReply = hook.result.current.submit(() => next.promise); });
  await act(async () => { old.resolve(); await oldReply; });
  const stillBusy = hook.result.current.busy;
  await act(async () => { next.resolve(); await nextReply; });
  expect(stillBusy).toBe(true);
});

test("an already queued old timeout cannot cancel the replacement generation", async () => {
  const timers = vi.spyOn(window, "setTimeout");
  const oldCancel = vi.fn(); const nextCancel = vi.fn();
  const hook = renderHook(({ payload, cancel }) => useAskUserQuestionnaire(payload, 30, cancel), {
    initialProps: { payload: original, cancel: oldCancel },
  });
  const oldTimeout = timers.mock.calls[0][0] as () => void;
  hook.rerender({ payload: { ...original, requestGeneration: "next" }, cancel: nextCancel });
  await act(async () => { oldTimeout(); });
  expect(oldCancel).not.toHaveBeenCalled(); expect(nextCancel).not.toHaveBeenCalled();
  await act(async () => { vi.advanceTimersByTime(30_000); });
  expect(nextCancel).toHaveBeenCalledTimes(1);
});

test("a failed automatic cancel pauses its deadline and remains manually retryable", async () => {
  const error = new Error("synthetic cancel failed");
  const cancel = vi.fn().mockResolvedValueOnce({ kind: "failed", error }).mockResolvedValue(undefined);
  const hook = renderHook(() => useAskUserQuestionnaire(original, 30, cancel));
  await act(async () => { vi.advanceTimersByTime(30_000); });
  expect(cancel).toHaveBeenCalledTimes(1);
  expect(isAskUserClockPaused(askUserClockKey(original))).toBe(true);
  expect(hook.result.current.countdownSec).toBeNull();
  await act(async () => { vi.advanceTimersByTime(60_000); });
  expect(cancel).toHaveBeenCalledTimes(1);
  await act(async () => { await hook.result.current.cancel(); });
  expect(cancel).toHaveBeenCalledTimes(2);
  expect(isAskUserClockPaused(askUserClockKey(original))).toBe(false);
});

test("returning to an unanswered request resumes the deadline, while a new generation gets a fresh one", async () => {
  const cancel = vi.fn();
  const first = renderHook(() => useAskUserQuestionnaire(original, 30, cancel));
  act(() => { vi.advanceTimersByTime(12_000); }); first.unmount();
  const second = renderHook(({ payload }) => useAskUserQuestionnaire(payload, 30, cancel), { initialProps: { payload: original } });
  expect(second.result.current.countdownSec).toBe(18);
  second.rerender({ payload: { ...original, requestGeneration: "next" } });
  expect(second.result.current.countdownSec).toBe(30);
  expect(getAskUserClocks().size).toBe(2);
  await act(async () => { vi.advanceTimersByTime(18_000); });
  expect(cancel).not.toHaveBeenCalled();
});

test("a submit and cancel in the same frame share one synchronous claim", async () => {
  const pending = deferred(); const cancel = vi.fn(); const submit = vi.fn(() => pending.promise);
  const hook = renderHook(() => useAskUserQuestionnaire(original, 30, cancel));
  let reply!: Promise<unknown>;
  act(() => { reply = hook.result.current.submit(submit); void hook.result.current.cancel(); });
  expect(submit).toHaveBeenCalledTimes(1); expect(cancel).not.toHaveBeenCalled();
  await act(async () => { pending.resolve(); await reply; vi.advanceTimersByTime(60_000); });
  expect(cancel).not.toHaveBeenCalled();
});
