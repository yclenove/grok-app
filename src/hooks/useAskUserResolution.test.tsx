/** @vitest-environment jsdom */
import "@/test/jsdomStubs";
import { useState } from "react";
import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { createT } from "@/i18n";
import { askUserClockKey } from "@/lib/askUser/askUserIdentity";
import { dropAskUserClocks, getAskUserClocks, isAskUserClockPaused } from "@/lib/askUser/askUserClocks";
import type { AskUserActionOutcome } from "@/lib/askUser/askUserSettle";
import { emptySessionPlan } from "@/lib/planSession";
import type { AskUserPayload } from "@/lib/session";
import { createSessionImportHostFixture } from "@/test/sessionImportHostFixture";
import { registerGateAndMetaSubscriptions } from "./sessionHostGateSubscriptions";
import { useAskUserResolution } from "./useAskUserResolution";

const api = vi.hoisted(() => ({ sessionResolveAskUser: vi.fn(), sessionPlanChromeSet: vi.fn() }));
vi.mock("@/lib/api", () => api);
const card: AskUserPayload = {
  sessionId: "native-viewed", rpcId: 0, requestGeneration: "original",
  questions: [{ id: "q", question: "请选择", options: [{ id: "a", label: "甲" }] }],
};
function deferred() {
  let resolve!: (value?: unknown) => void;
  let reject!: (error: unknown) => void;
  const promise = new Promise((done, fail) => { resolve = done; reject = fail; });
  return { promise, resolve, reject };
}
function mount(locale: "zh" | "en" = "zh") {
  const { context: c } = createSessionImportHostFixture();
  c.notifyPrefsRef.current = { notifyOnPermission: false, notifyOnTurnDone: false };
  const toast = vi.fn();
  const hook = renderHook(() => {
    const [question, setQuestion] = useState<AskUserPayload | null>(null);
    c.setAskUser = setQuestion;
    const reply = useAskUserResolution({
      pendingAskUserBySessionRef: c.pendingAskUserBySessionRef, viewingSessionIdRef: c.viewingSessionIdRef,
      setAskUser: setQuestion, showToast: toast, tr: createT(locale),
    });
    return { question, reply };
  });
  const listeners = new Map<string, (payload: unknown) => void>();
  registerGateAndMetaSubscriptions(c, {
    isCancelled: () => false, track: () => {},
    listenWithRetry: async <T,>(event: string, handler: (payload: T) => void) => {
      listeners.set(event, (payload) => handler(payload as T));
      return () => { listeners.delete(event); };
    },
  });
  const emit = (payload: AskUserPayload) => act(() => {
    listeners.get("session://ask_user")!(payload);
    getAskUserClocks().set(askUserClockKey(payload), 100);
  });
  emit(card);
  return { hook, c, toast, emit };
}
beforeEach(() => { vi.resetAllMocks(); vi.useFakeTimers(); api.sessionResolveAskUser.mockResolvedValue({}); });
afterEach(() => {
  cleanup(); vi.clearAllTimers(); vi.useRealTimers();
  dropAskUserClocks(card.sessionId); dropAskUserClocks("other-chat");
});

test("the actual event/answer path echoes RPC zero and settles only the questionnaire", async () => {
  const view = mount();
  const permission = { sessionId: card.sessionId, rpcId: 6, toolCallId: "tool", title: "shell", preview: "shell", toolName: "shell", options: [], scopeKey: "shell" };
  const plan = { ...emptySessionPlan("plan"), rpcId: 7, requestGeneration: "plan" };
  view.c.pendingPermBySessionRef.current.set(card.sessionId, permission);
  view.c.planBySessionRef.current.set(card.sessionId, plan);
  await act(async () => { await view.hook.result.current.reply(card, "accepted", { q: "甲" }); });
  expect(api.sessionResolveAskUser).toHaveBeenCalledExactlyOnceWith({
    sessionId: card.sessionId, rpcId: 0, requestGeneration: "original", decision: "accepted", answers: { q: "甲" },
  });
  expect(view.hook.result.current.question).toBeNull();
  expect(view.c.pendingAskUserBySessionRef.current.size).toBe(0);
  expect(getAskUserClocks().has(askUserClockKey(card))).toBe(false);
  expect(view.c.pendingPermBySessionRef.current.get(card.sessionId)).toBe(permission);
  expect(view.c.planBySessionRef.current.get(card.sessionId)).toBe(plan);
  expect(view.c.clearPendingGatesRef.current).not.toHaveBeenCalled();
  expect(view.c.setPlan).not.toHaveBeenCalled(); expect(view.c.setPerm).not.toHaveBeenCalled();
});

test("late success preserves a new identical same-RPC question and its deadline", async () => {
  const view = mount(); const pending = deferred(); api.sessionResolveAskUser.mockReturnValue(pending.promise);
  let reply!: Promise<AskUserActionOutcome>;
  act(() => { reply = view.hook.result.current.reply(card, "cancelled"); });
  const next = { ...card, requestGeneration: "next" }; view.emit(next);
  await act(async () => { pending.resolve(); await reply; });
  expect(view.hook.result.current.question).toBe(next);
  expect(view.c.pendingAskUserBySessionRef.current.get(card.sessionId)).toBe(next);
  expect(getAskUserClocks().get(askUserClockKey(next))).toBe(100);
  expect(getAskUserClocks().has(askUserClockKey(card))).toBe(false);
});

test.each(["accepted", "cancelled"] as const)("a failed %s restores the original card and permits one explicit retry", async (decision) => {
  const view = mount(); const error = new Error("synthetic write timeout");
  api.sessionResolveAskUser.mockRejectedValueOnce(error);
  await act(async () => { expect(await view.hook.result.current.reply(card, decision)).toEqual({ kind: "failed", error }); });
  expect(view.hook.result.current.question).toBe(card);
  expect(view.c.pendingAskUserBySessionRef.current.get(card.sessionId)).toBe(card);
  expect(isAskUserClockPaused(askUserClockKey(card))).toBe(true);
  expect(view.toast).toHaveBeenCalledWith(String(error), 4500);
  await act(async () => { expect(await view.hook.result.current.reply(card, decision)).toEqual({ kind: "delivered" }); });
  expect(api.sessionResolveAskUser).toHaveBeenCalledTimes(2);
  expect(view.hook.result.current.question).toBeNull();
  expect(isAskUserClockPaused(askUserClockKey(card))).toBe(false);
});

test.each(["replacement", "navigation"])("a late failure cannot restore into a newer view: %s", async (change) => {
  const view = mount(); const pending = deferred(); api.sessionResolveAskUser.mockReturnValue(pending.promise);
  let reply!: Promise<AskUserActionOutcome>;
  act(() => { reply = view.hook.result.current.reply(card, "accepted"); });
  const next = { ...card, requestGeneration: "next", sessionId: change === "navigation" ? "other-chat" : card.sessionId };
  view.c.viewingSessionIdRef.current = next.sessionId; view.emit(next);
  await act(async () => { pending.reject(new Error("old reply failure")); await reply; });
  expect(view.hook.result.current.question).toBe(next);
  expect(view.c.pendingAskUserBySessionRef.current.get(next.sessionId)).toBe(next);
  expect(getAskUserClocks().get(askUserClockKey(next))).toBe(100);
  expect(isAskUserClockPaused(askUserClockKey(next))).toBe(false);
  expect(view.toast).not.toHaveBeenCalled();
  if (change === "navigation") expect(view.c.pendingAskUserBySessionRef.current.get(card.sessionId)).toBe(card);
});

test("an old chat's successful reply clears its cache without hiding the current chat's question", async () => {
  const view = mount(); const pending = deferred(); api.sessionResolveAskUser.mockReturnValue(pending.promise);
  let reply!: Promise<AskUserActionOutcome>;
  act(() => { reply = view.hook.result.current.reply(card, "cancelled"); });
  const other = { ...card, sessionId: "other-chat", requestGeneration: "other" };
  view.c.viewingSessionIdRef.current = other.sessionId; view.emit(other);
  await act(async () => { pending.resolve(); await reply; });
  expect(view.hook.result.current.question).toBe(other);
  expect(view.c.pendingAskUserBySessionRef.current.has(card.sessionId)).toBe(false);
  expect(getAskUserClocks().has(askUserClockKey(other))).toBe(true);
});

test("duplicate accept/cancel claims, including restored clones, dispatch only once", async () => {
  const view = mount(); const pending = deferred(); api.sessionResolveAskUser.mockReturnValue(pending.promise);
  let first!: Promise<AskUserActionOutcome>; let second!: Promise<AskUserActionOutcome>;
  act(() => {
    first = view.hook.result.current.reply(card, "accepted");
    second = view.hook.result.current.reply({ ...card }, "cancelled");
  });
  expect(await second).toEqual({ kind: "skipped" });
  expect(api.sessionResolveAskUser).toHaveBeenCalledTimes(1);
  await act(async () => { pending.resolve(); await first; });
  await act(async () => { expect(await view.hook.result.current.reply(card, "cancelled")).toEqual({ kind: "skipped" }); });
  expect(api.sessionResolveAskUser).toHaveBeenCalledTimes(1);
});

test("request claims survive a composer domain remount sharing the same host cache", async () => {
  const view = mount(); const pending = deferred(); api.sessionResolveAskUser.mockReturnValue(pending.promise);
  let reply!: Promise<AskUserActionOutcome>;
  act(() => { reply = view.hook.result.current.reply(card, "accepted"); });
  view.hook.unmount();
  const remount = renderHook(() => useAskUserResolution({
    pendingAskUserBySessionRef: view.c.pendingAskUserBySessionRef, viewingSessionIdRef: view.c.viewingSessionIdRef,
    setAskUser: view.c.setAskUser, showToast: view.toast, tr: createT("zh"),
  }));
  await act(async () => { expect(await remount.result.current({ ...card }, "cancelled")).toEqual({ kind: "skipped" }); });
  expect(api.sessionResolveAskUser).toHaveBeenCalledTimes(1);
  await act(async () => { pending.resolve(); await reply; });
});

test("two generations may reply independently, without an old ACK releasing the newer claim", async () => {
  const view = mount(); const old = deferred(); const next = deferred();
  api.sessionResolveAskUser.mockReturnValueOnce(old.promise).mockReturnValueOnce(next.promise);
  let oldReply!: Promise<AskUserActionOutcome>; let nextReply!: Promise<AskUserActionOutcome>;
  act(() => { oldReply = view.hook.result.current.reply(card, "cancelled"); });
  const nextCard = { ...card, requestGeneration: "next" }; view.emit(nextCard);
  act(() => { nextReply = view.hook.result.current.reply(nextCard, "accepted"); });
  await act(async () => { old.resolve(); await oldReply; });
  await act(async () => { expect(await view.hook.result.current.reply(nextCard, "cancelled")).toEqual({ kind: "skipped" }); });
  expect(api.sessionResolveAskUser).toHaveBeenCalledTimes(2);
  await act(async () => { next.resolve(); await nextReply; });
  expect(view.c.pendingAskUserBySessionRef.current.size).toBe(0);
});

test("an invalidated request cannot be resurrected by a failed reply", async () => {
  const view = mount(); const pending = deferred(); api.sessionResolveAskUser.mockReturnValue(pending.promise);
  let reply!: Promise<AskUserActionOutcome>;
  act(() => { reply = view.hook.result.current.reply(card, "cancelled"); });
  view.c.pendingAskUserBySessionRef.current.delete(card.sessionId);
  await act(async () => { pending.reject(new Error("transport retired")); expect(await reply).toEqual({ kind: "skipped" }); });
  expect(view.hook.result.current.question).toBeNull(); expect(view.toast).not.toHaveBeenCalled();
  expect(getAskUserClocks().has(askUserClockKey(card))).toBe(false);
});

test("a legacy same-RPC replacement has an independent clock and survives the old ACK", async () => {
  const view = mount(); const legacy = { ...card, requestGeneration: undefined }; view.emit(legacy);
  const pending = deferred(); api.sessionResolveAskUser.mockReturnValue(pending.promise);
  let reply!: Promise<AskUserActionOutcome>;
  act(() => { reply = view.hook.result.current.reply(legacy, "accepted"); });
  const replacement = { ...legacy }; view.emit(replacement);
  expect(askUserClockKey(legacy)).not.toBe(askUserClockKey(replacement));
  await act(async () => { pending.resolve(); await reply; });
  expect(view.hook.result.current.question).toBe(replacement);
  expect(view.c.pendingAskUserBySessionRef.current.get(card.sessionId)).toBe(replacement);
  expect(getAskUserClocks().has(askUserClockKey(replacement))).toBe(true);
});

test.each(["zh", "en"] as const)("the retry failure message uses the current locale: %s", async (locale) => {
  const view = mount(locale); api.sessionResolveAskUser.mockRejectedValue({ code: "UNSUPPORTED" });
  await act(async () => { await view.hook.result.current.reply(card, "cancelled"); });
  expect(view.toast).toHaveBeenCalledWith(createT(locale)("mirror.unsupported"), 4500);
});
