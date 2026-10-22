/** @vitest-environment jsdom */
import "@/test/jsdomStubs";
import { useState } from "react";
import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { createT } from "@/i18n";
import type { PermissionPayload } from "@/lib/session";
import { emptySessionPlan } from "@/lib/planSession";
import { permissionClockKey } from "@/lib/session/permissionIdentity";
import { createSessionImportHostFixture } from "@/test/sessionImportHostFixture";
import { registerGateAndMetaSubscriptions } from "./sessionHostGateSubscriptions";
import { usePermissionResolution } from "./usePermissionResolution";

const api = vi.hoisted(() => ({ sessionResolvePermission: vi.fn(), sessionPlanChromeSet: vi.fn() }));
vi.mock("@/lib/api", () => api);
const card: PermissionPayload = {
  rpcId: 81, requestGeneration: "original-generation", sessionId: "native-viewed",
  toolCallId: "工具", toolName: "shell", title: "python 示例.py", preview: "python 示例.py",
  scopeKey: "shell:python", options: [{ optionId: "allow-once", kind: "allow_once" }],
};
function deferred() {
  let resolve!: (value?: unknown) => void;
  let reject!: (error: unknown) => void;
  const promise = new Promise((done, fail) => { resolve = done; reject = fail; });
  return { promise, resolve, reject };
}
function mount(locale: "zh" | "en" = "zh") {
  const fixture = createSessionImportHostFixture();
  const c = fixture.context;
  c.localeRef.current = locale;
  c.trRef.current = createT(locale);
  c.notifyPrefsRef.current = { notifyOnPermission: false, notifyOnTurnDone: false };
  const clocks = { current: new Map<string, number>() };
  const toast = vi.fn();
  const hook = renderHook(() => {
    const [permission, setPermission] = useState<PermissionPayload | null>(null);
    c.setPerm = setPermission;
    const resolve = usePermissionResolution({
      pendingPermBySessionRef: c.pendingPermBySessionRef, permRaisedAtRef: clocks,
      viewingSessionIdRef: c.viewingSessionIdRef, setPerm: setPermission,
      showToast: toast, tr: createT(locale),
    });
    return { permission, resolve };
  });
  const listeners = new Map<string, (payload: unknown) => void>();
  registerGateAndMetaSubscriptions(c, {
    isCancelled: () => false, track: () => {},
    listenWithRetry: async <T,>(event: string, handler: (payload: T) => void) => {
      listeners.set(event, (payload) => handler(payload as T));
      return () => { listeners.delete(event); };
    },
  });
  const emit = (p: PermissionPayload) => act(() => {
    listeners.get("session://permission")!(p);
    clocks.current.set(permissionClockKey(p), 100);
  });
  emit(card);
  return { hook, c, clocks, toast, emit };
}
beforeEach(() => { vi.resetAllMocks(); vi.useFakeTimers(); });
afterEach(() => { cleanup(); vi.clearAllTimers(); vi.useRealTimers(); });

test("reply echoes the event identity and clears only its permission and clock", async () => {
  const view = mount();
  const ask = { sessionId: card.sessionId, rpcId: 19, questions: [], toolCallId: "question" };
  const plan = { ...emptySessionPlan("plan"), rpcId: 20 };
  view.c.pendingAskUserBySessionRef.current.set(card.sessionId, ask);
  view.c.planBySessionRef.current.set(card.sessionId, plan);
  await act(async () => { await view.hook.result.current.resolve(card, "allow_once", "allow-once"); });
  expect(api.sessionResolvePermission).toHaveBeenCalledExactlyOnceWith({
    rpcId: card.rpcId, requestGeneration: card.requestGeneration, decision: "allow_once",
    optionId: "allow-once", scopeKey: card.scopeKey, sessionId: card.sessionId,
    options: card.options, toolName: card.toolName,
  });
  expect(view.hook.result.current.permission).toBeNull();
  expect(view.c.pendingPermBySessionRef.current.size).toBe(0);
  expect(view.clocks.current.size).toBe(0);
  expect(view.c.pendingAskUserBySessionRef.current.get(card.sessionId)).toBe(ask);
  expect(view.c.planBySessionRef.current.get(card.sessionId)).toBe(plan);
  expect(view.c.clearPendingGatesRef.current).not.toHaveBeenCalled();
  expect(view.c.setPlan).not.toHaveBeenCalled();
  expect(view.c.setAskUser).not.toHaveBeenCalled();
});

test("late success preserves a new identical same-RPC card and its own timeout", async () => {
  const view = mount();
  const pending = deferred(); api.sessionResolvePermission.mockReturnValue(pending.promise);
  let reply!: Promise<void>;
  act(() => { reply = view.hook.result.current.resolve(card, "allow_session", "allow-always-command"); });
  const next = { ...card, requestGeneration: "next-generation" };
  view.emit(next);
  await act(async () => { pending.resolve(); await reply; });
  expect(view.hook.result.current.permission).toBe(next);
  expect(view.c.pendingPermBySessionRef.current.get(card.sessionId)).toBe(next);
  expect(view.clocks.current.get(permissionClockKey(next))).toBe(100);
  expect(view.clocks.current.has(permissionClockKey(card))).toBe(false);
  expect(view.toast).not.toHaveBeenCalled();
});

test("reply to an old chat clears its cache without hiding another chat's prompt", async () => {
  const view = mount();
  const pending = deferred(); api.sessionResolvePermission.mockReturnValue(pending.promise);
  const reply = view.hook.result.current.resolve(card, "deny", "reject-once");
  const next = { ...card, sessionId: "other-chat", requestGeneration: "other-generation" };
  view.c.viewingSessionIdRef.current = next.sessionId;
  view.emit(next);
  await act(async () => { pending.resolve(); await reply; });
  expect(view.hook.result.current.permission).toBe(next);
  expect(view.c.pendingPermBySessionRef.current.has(card.sessionId)).toBe(false);
  expect(view.c.pendingPermBySessionRef.current.get(next.sessionId)).toBe(next);
  expect(view.clocks.current.has(permissionClockKey(next))).toBe(true);
});

test.each(["replacement", "navigation"])("late failure does not affect a newer view: %s", async (change) => {
  const view = mount();
  const pending = deferred(); api.sessionResolvePermission.mockReturnValue(pending.promise);
  const reply = view.hook.result.current.resolve(card, "deny", "reject-once");
  const observed = reply.catch((error: unknown) => error);
  const next = { ...card, requestGeneration: "new", sessionId: change === "navigation" ? "other-chat" : card.sessionId };
  view.c.viewingSessionIdRef.current = next.sessionId;
  view.emit(next);
  const error = new Error("synthetic late failure");
  await act(async () => { pending.reject(error); expect(await observed).toBe(error); });
  expect(view.hook.result.current.permission).toBe(next);
  expect(view.c.pendingPermBySessionRef.current.get(next.sessionId)).toBe(next);
  expect(view.clocks.current.has(permissionClockKey(next))).toBe(true);
  expect(view.toast).not.toHaveBeenCalled();
});

test.each(["zh", "en"] as const)("current failure preserves the card and shows localized mirror error: %s", async (locale) => {
  const view = mount(locale);
  api.sessionResolvePermission.mockRejectedValue({ code: "UNSUPPORTED" });
  await act(async () => {
    await expect(view.hook.result.current.resolve(card, "deny", "reject-once"))
      .rejects.toThrow(createT(locale)("mirror.unsupported"));
  });
  expect(view.toast).toHaveBeenCalledWith(createT(locale)("mirror.unsupported"), 4000);
  expect(view.hook.result.current.permission).toBe(card);
  expect(view.clocks.current.has(permissionClockKey(card))).toBe(true);
});

test("a restored clone of the same modern request is cleared on success", async () => {
  const view = mount();
  const pending = deferred(); api.sessionResolvePermission.mockReturnValue(pending.promise);
  const reply = view.hook.result.current.resolve(card, "deny", "reject-once");
  view.emit({ ...card, responseError: "retry" });
  await act(async () => { pending.resolve(); await reply; });
  expect(view.hook.result.current.permission).toBeNull();
  expect(view.c.pendingPermBySessionRef.current.size).toBe(0);
});

test("legacy same-RPC replacement keeps its prompt and countdown", async () => {
  const view = mount();
  const old = { ...card, requestGeneration: undefined }; view.emit(old);
  const pending = deferred(); api.sessionResolvePermission.mockReturnValue(pending.promise);
  const reply = view.hook.result.current.resolve(old, "deny", "reject-once");
  const next = { ...old }; view.emit(next);
  await act(async () => { pending.resolve(); await reply; });
  expect(view.hook.result.current.permission).toBe(next);
  expect(view.clocks.current.has(permissionClockKey(next))).toBe(true);
});
