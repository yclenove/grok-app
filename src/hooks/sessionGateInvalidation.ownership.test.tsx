/** @vitest-environment jsdom */
import "@/test/jsdomStubs";
import { useState } from "react";
import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import * as api from "@/lib/api";
import { IDLE_SNAPSHOT, type AskUserPayload, type PermissionPayload } from "@/lib/session";
import { emptySessionPlan, type SessionPlanState } from "@/lib/planSession";
import { createSessionImportHostFixture } from "@/test/sessionImportHostFixture";
import { useSessionHostEvents } from "./useSessionHostEvents";

const sid = "native-viewed";
const plan: SessionPlanState = {
  ...emptySessionPlan(), body: "保留这份中文计划", visible: true, waiting: false,
  rpcId: 0, requestGeneration: "current-plan", chromeRevision: "current-revision",
};
const ask: AskUserPayload = {
  sessionId: sid, rpcId: 0, requestGeneration: "current-ask",
  questions: [{ id: "q", question: "保留哪个工作区？", options: [] }],
};
const perm: PermissionPayload = {
  sessionId: sid, rpcId: 0, requestGeneration: "current-permission", toolCallId: "shell",
  toolName: "shell", title: "命令", preview: "echo synthetic", scopeKey: "shell:echo", options: [],
};
function row() {
  return {
    sessionId: sid, identityVersion: 1,
    planRpcId: 0, planRequestGeneration: "current-plan",
    previousPlanChromeRevision: "current-revision", planChromeRevision: "stale-revision",
    askUserRpcId: 0, askUserRequestGeneration: "current-ask",
    permissionRpcId: 0, permissionRequestGeneration: "current-permission",
  };
}
const handlers = new Map<string, (payload: unknown) => void>();
async function mount(background = false) {
  const { context: c } = createSessionImportHostFixture();
  if (background) c.viewingSessionIdRef.current = "other-focus";
  c.planBySessionRef!.current.set(sid, plan);
  c.pendingAskUserBySessionRef.current.set(sid, ask);
  c.pendingPermBySessionRef.current.set(sid, perm);
  const hook = renderHook(() => {
    const [currentPlan, setPlan] = useState(plan);
    const [currentAsk, setAsk] = useState<AskUserPayload | null>(ask);
    const [currentPerm, setPerm] = useState<PermissionPayload | null>(perm);
    c.setPlan = setPlan; c.setAskUser = setAsk; c.setPerm = setPerm;
    useSessionHostEvents(c);
    return { plan: currentPlan, ask: currentAsk, perm: currentPerm };
  });
  await act(async () => { await vi.advanceTimersByTimeAsync(1); });
  return { c, hook, emit: (event: string, payload: unknown) => act(() => {
    const handler = handlers.get(event);
    expect(handler, "production subscription must be registered").toBeDefined();
    handler!(payload);
  }) };
}
beforeEach(() => {
  vi.useFakeTimers(); handlers.clear();
  vi.spyOn(api, "isTauri").mockReturnValue(true);
  vi.spyOn(api, "sessionGetState").mockResolvedValue({ ...IDLE_SNAPSHOT, sessionId: sid, state: "ready", title: "中文会话", backend: "acp" });
  vi.spyOn(api, "listen").mockImplementation(async <T,>(event: string, handler: (payload: T) => void) => {
    handlers.set(event, (payload) => handler(payload as T));
    return () => { handlers.delete(event); };
  });
});
afterEach(() => { cleanup(); vi.clearAllTimers(); vi.useRealTimers(); vi.restoreAllMocks(); handlers.clear(); });

test.each([false, true])("late invalidation cannot clear newer requests that reused RPC 0 (background=%s)", async (background) => {
  const view = await mount(background);
  view.emit("session://permissions_invalidated", { sessions: [{ ...row(),
    planRequestGeneration: "old-plan", previousPlanChromeRevision: "old-revision",
    askUserRequestGeneration: "old-ask", permissionRequestGeneration: "old-permission",
  }] });
  expect(view.c.planBySessionRef!.current.get(sid)).toBe(plan);
  expect(view.c.pendingAskUserBySessionRef.current.get(sid)).toBe(ask);
  expect(view.c.pendingPermBySessionRef.current.get(sid)).toBe(perm);
  expect(view.hook.result.current).toEqual({ plan, ask, perm });
  expect(view.c.clearPendingGatesRef.current).not.toHaveBeenCalled();
});

test.each([false, true])("current invalidation preserves the draft and installs its close receipt (background=%s)", async (background) => {
  const view = await mount(background);
  view.emit("session://permissions_invalidated", { sessions: [row()] });
  expect(view.c.planBySessionRef!.current.get(sid)).toMatchObject({
    rpcId: null, requestGeneration: null, body: plan.body, gateStale: true,
    waiting: false, chromeRevision: "stale-revision",
  });
  expect(view.c.pendingAskUserBySessionRef.current.has(sid)).toBe(false);
  expect(view.c.pendingPermBySessionRef.current.has(sid)).toBe(false);
  if (!background) {
    expect(view.hook.result.current.ask).toBeNull();
    expect(view.hook.result.current.perm).toBeNull();
  }
  expect(view.c.clearPendingGatesRef.current).not.toHaveBeenCalled();
});

test("an unscoped legacy invalidation cannot erase modern gates", async () => {
  const view = await mount();
  view.emit("session://permissions_invalidated", {});
  expect(view.hook.result.current).toEqual({ plan, ask, perm });
  expect(view.c.planBySessionRef!.current.get(sid)).toBe(plan);
  expect(view.c.pendingAskUserBySessionRef.current.get(sid)).toBe(ask);
});

test("a late ask-user clear never clears a different questionnaire or plan", async () => {
  const view = await mount();
  view.emit("session://ask_user_cleared", { sessionId: sid, rpcId: 0, requestGeneration: "old-ask" });
  expect(view.hook.result.current).toEqual({ plan, ask, perm });
  expect(view.c.clearPendingGatesRef.current).not.toHaveBeenCalled();
});

test("an error with explicit gate invalidation does not clear the whole session again", async () => {
  const view = await mount();
  view.emit("session://turn_error", { sessionId: sid, humanGatesInvalidated: true, code: "NETWORK_PROVIDER" });
  expect(view.c.clearPendingGatesRef.current).not.toHaveBeenCalled();
});

test("stopping progress also retires the last displayed revision before an unpublished update", async () => {
  const view = await mount();
  const progress = { ...plan, rpcId: null, requestGeneration: null, waiting: true };
  view.c.planBySessionRef!.current.set(sid, progress);
  act(() => view.c.setPlan(progress));
  view.emit("session://permissions_invalidated", { sessions: [{ ...row(),
    planRpcId: null, planRequestGeneration: null, previousPlanChromeRevision: "unpublished-revision",
    publishedPlanChromeRevision: progress.chromeRevision,
  }] });
  expect(view.hook.result.current.plan).toMatchObject({ body: progress.body, waiting: false,
    gateStale: true, chromeRevision: "stale-revision" });
});

test("current ask-user clear retires only that questionnaire", async () => {
  const view = await mount();
  view.emit("session://ask_user_cleared", { sessionId: sid, rpcId: ask.rpcId, requestGeneration: ask.requestGeneration });
  expect(view.hook.result.current).toEqual({ plan, ask: null, perm });
  expect(view.c.pendingPermBySessionRef.current.get(sid)).toBe(perm);
  expect(view.c.planBySessionRef!.current.get(sid)).toBe(plan);
});

test("legacy identity-free cards remain invalidatable by a legacy Host", async () => {
  const view = await mount();
  const legacyPlan = { ...plan, requestGeneration: null, chromeRevision: null };
  const legacyAsk = { ...ask, requestGeneration: undefined };
  const legacyPerm = { ...perm, requestGeneration: null };
  view.c.planBySessionRef!.current.set(sid, legacyPlan);
  view.c.pendingAskUserBySessionRef.current.set(sid, legacyAsk);
  view.c.pendingPermBySessionRef.current.set(sid, legacyPerm);
  act(() => { view.c.setPlan(legacyPlan); view.c.setAskUser(legacyAsk); view.c.setPerm(legacyPerm); });
  view.emit("session://permissions_invalidated", { sessions: [{ sessionId: sid }] });
  expect(view.hook.result.current.plan.rpcId).toBeNull();
  expect(view.hook.result.current.ask).toBeNull();
  expect(view.hook.result.current.perm).toBeNull();
});

test("an old queued updater cannot overwrite a newer cache entry", async () => {
  const view = await mount();
  const setPlan = vi.spyOn(view.c, "setPlan").mockImplementation(() => {});
  view.emit("session://permissions_invalidated", { sessions: [row()] });
  const updater = setPlan.mock.calls[0][0];
  expect(typeof updater).toBe("function");
  const replacement = { ...plan, requestGeneration: "replacement-plan", chromeRevision: "replacement-revision" };
  view.c.planBySessionRef!.current.set(sid, replacement);
  if (typeof updater === "function") expect(updater(plan)).toBe(plan);
  expect(view.c.planBySessionRef!.current.get(sid)).toBe(replacement);
});
