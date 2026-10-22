/** @vitest-environment jsdom */
import "@/test/jsdomStubs";
import { useState } from "react";
import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { createT } from "@/i18n";
import type { AskUserPayload, PermissionPayload } from "@/lib/session";
import { restoreSessionGate } from "@/lib/sessionGateRestore";
import { SessionGateCache } from "@/lib/sessionGateCache";
import { createSessionImportHostFixture } from "@/test/sessionImportHostFixture";
import { registerGateAndMetaSubscriptions } from "./sessionHostGateSubscriptions";
import { useAskUserResolution } from "./useAskUserResolution";
import { usePermissionResolution } from "./usePermissionResolution";

const api = vi.hoisted(() => ({ sessionResolveAskUser: vi.fn(), sessionResolvePermission: vi.fn() }));
vi.mock("@/lib/api", () => api);
const question: AskUserPayload = {
  sessionId: "native-viewed", rpcId: 0, requestGeneration: "old-query",
  questions: [{ id: "q", question: "请选择", options: [{ id: "a", label: "甲" }] }],
};
const permission: PermissionPayload = {
  sessionId: question.sessionId, rpcId: 0, requestGeneration: "old-query",
  toolCallId: "shell", toolName: "shell", title: "synthetic command", preview: "echo synthetic",
  scopeKey: "shell:echo", options: [{ optionId: "reject-once", kind: "reject_once" }],
};
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => { resolve = done; });
  return { promise, resolve };
}
function mount() {
  const { context: c } = createSessionImportHostFixture();
  c.notifyPrefsRef.current = { notifyOnPermission: false, notifyOnTurnDone: false };
  const hook = renderHook(() => {
    const [askUser, setAskUser] = useState<AskUserPayload | null>(null);
    const [perm, setPerm] = useState<PermissionPayload | null>(null);
    c.setAskUser = setAskUser; c.setPerm = setPerm;
    return {
      askUser, perm,
      replyAsk: useAskUserResolution({ ...c, showToast: vi.fn(), tr: createT("zh") }),
      replyPermission: usePermissionResolution({ ...c, permRaisedAtRef: { current: new Map() }, showToast: vi.fn(), tr: createT("zh") }),
    };
  });
  const listeners = new Map<string, (payload: unknown) => void>();
  registerGateAndMetaSubscriptions(c, {
    isCancelled: () => false, track: () => {},
    listenWithRetry: async <T,>(event: string, handler: (payload: T) => void) => {
      listeners.set(event, (payload) => handler(payload as T));
      return () => { listeners.delete(event); };
    },
  });
  const restoreAsk = (pull: () => Promise<AskUserPayload | null>, enabled = true) => act(() => {
    restoreSessionGate(question.sessionId, () => c.viewingSessionIdRef.current === question.sessionId, {
      parked: c.pendingAskUserBySessionRef, apply: c.setAskUser, pull, enabled,
    });
  });
  return { c, hook, restoreAsk, emit: (event: string, payload: unknown) => act(() => listeners.get(event)!(payload)) };
}
beforeEach(() => {
  vi.resetAllMocks(); vi.useFakeTimers();
  api.sessionResolveAskUser.mockResolvedValue({}); api.sessionResolvePermission.mockResolvedValue({});
});
afterEach(() => { cleanup(); vi.clearAllTimers(); vi.useRealTimers(); });

test("a slow host query cannot resurrect a questionnaire answered through the live event path", async () => {
  const view = mount(); const pull = deferred<AskUserPayload | null>(); view.restoreAsk(() => pull.promise);
  const current = { ...question, requestGeneration: "live-question" };
  view.emit("session://ask_user", current);
  await act(async () => { expect(await view.hook.result.current.replyAsk(current, "accepted")).toEqual({ kind: "delivered" }); });
  await act(async () => { pull.resolve(question); await pull.promise; });
  expect(view.hook.result.current.askUser).toBeNull();
  expect(view.c.pendingAskUserBySessionRef.current.has(question.sessionId)).toBe(false);
  expect(api.sessionResolveAskUser).toHaveBeenCalledTimes(1);
});

test("a slow host query cannot resurrect a permission settled through its actual reply hook", async () => {
  const view = mount(); const pull = deferred<PermissionPayload | null>();
  act(() => restoreSessionGate(permission.sessionId, () => true, {
    parked: view.c.pendingPermBySessionRef, apply: view.c.setPerm, pull: () => pull.promise, enabled: true,
  }));
  const current = { ...permission, requestGeneration: "live-permission" };
  view.emit("session://permission", current);
  await act(async () => { await view.hook.result.current.replyPermission(current, "deny", "reject-once"); });
  await act(async () => { pull.resolve(permission); await pull.promise; });
  expect(view.hook.result.current.perm).toBeNull();
  expect(view.c.pendingPermBySessionRef.current.has(permission.sessionId)).toBe(false);
});

test("the newer empty host reply supersedes an earlier positive query", async () => {
  const view = mount(); const old = deferred<AskUserPayload | null>(); const latest = deferred<AskUserPayload | null>();
  view.restoreAsk(() => old.promise); view.restoreAsk(() => latest.promise);
  await act(async () => { latest.resolve(null); await latest.promise; });
  await act(async () => { old.resolve(question); await old.promise; });
  expect(view.hook.result.current.askUser).toBeNull();
  expect(view.c.pendingAskUserBySessionRef.current.has(question.sessionId)).toBe(false);
});

test("starting a disabled restore retires a previous host pull", async () => {
  const view = mount(); const old = deferred<AskUserPayload | null>(); view.restoreAsk(() => old.promise);
  const disabledPull = vi.fn(); view.restoreAsk(disabledPull, false);
  await act(async () => { old.resolve(question); await old.promise; });
  expect(disabledPull).not.toHaveBeenCalled(); expect(view.hook.result.current.askUser).toBeNull();
});

test("replacing a host cache cannot commit a pull issued against its old owner", async () => {
  const view = mount(); const old = deferred<AskUserPayload | null>(); view.restoreAsk(() => old.promise);
  view.c.pendingAskUserBySessionRef.current = new SessionGateCache();
  await act(async () => { old.resolve(question); await old.promise; });
  expect(view.hook.result.current.askUser).toBeNull();
  expect(view.c.pendingAskUserBySessionRef.current.size).toBe(0);
});

test.each(["delete", "clear"] as const)("an explicit %s while a pull is pending invalidates even an empty cache", async (action) => {
  const view = mount(); const old = deferred<AskUserPayload | null>(); view.restoreAsk(() => old.promise);
  if (action === "delete") view.c.pendingAskUserBySessionRef.current.delete(question.sessionId);
  else view.c.pendingAskUserBySessionRef.current.clear();
  await act(async () => { old.resolve(question); await old.promise; });
  expect(view.hook.result.current.askUser).toBeNull();
});

test("another chat's live question does not discard a valid restore for the viewed chat", async () => {
  const view = mount(); const pull = deferred<AskUserPayload | null>(); view.restoreAsk(() => pull.promise);
  const background = { ...question, sessionId: "native-background", requestGeneration: "background" };
  view.emit("session://ask_user", background);
  await act(async () => { pull.resolve(question); await pull.promise; });
  expect(view.hook.result.current.askUser).toBe(question);
  expect(view.c.pendingAskUserBySessionRef.current.get(background.sessionId)).toBe(background);
});
