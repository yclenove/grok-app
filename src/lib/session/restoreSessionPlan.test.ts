/** @vitest-environment jsdom */
import "@/test/jsdomStubs";
import { useState } from "react";
import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { registerGateAndMetaSubscriptions } from "@/hooks/sessionHostGateSubscriptions";
import { closedSessionPlan, emptySessionPlan, invalidatePlanGate, mergePlanFromEvent, type SessionPlanState } from "@/lib/planSession";
import { SessionGateCache } from "@/lib/sessionGateCache";
import { createSessionImportHostFixture } from "@/test/sessionImportHostFixture";
import { restoreSessionPlan } from "./restoreSessionPlan";

const api = vi.hoisted(() => ({
  sessionPlanChromeGet: vi.fn(), sessionAgentPlanSnapshot: vi.fn(), sessionPendingPlan: vi.fn(), sessionPlanChromeSet: vi.fn(),
}));
vi.mock("@/lib/api", () => api);
const sessionId = "native-viewed";
const live = { sessionId, rpcId: 0, requestGeneration: "current", toolCallId: "plan-tool", body: "当前计划" };
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => { resolve = done; });
  return { promise, resolve };
}
function mount(cached?: SessionPlanState) {
  const { context: c } = createSessionImportHostFixture();
  c.notifyPrefsRef.current = { notifyOnPermission: false, notifyOnTurnDone: false };
  if (cached) c.planBySessionRef.current.set(sessionId, cached);
  const hook = renderHook(() => {
    const [plan, setPlan] = useState(emptySessionPlan("计划")); c.setPlan = setPlan;
    return plan;
  });
  const listeners = new Map<string, (payload: unknown) => void>();
  registerGateAndMetaSubscriptions(c, {
    isCancelled: () => false, track: () => {},
    listenWithRetry: async <T,>(event: string, handler: (payload: T) => void) => {
      listeners.set(event, (payload) => handler(payload as T)); return () => { listeners.delete(event); };
    },
  });
  const restore = () => {
    let completion!: Promise<void>;
    act(() => { completion = restoreSessionPlan(sessionId, () => c.viewingSessionIdRef.current === sessionId, {
      parked: c.planBySessionRef, apply: c.setPlan, markPending: c.markPlanPendingBadge, readyTitle: "计划",
    }); });
    return completion;
  };
  return { c, hook, restore, emit: (payload: typeof live) => act(() => listeners.get("session://plan")!(payload)) };
}
beforeEach(() => {
  vi.resetAllMocks(); vi.useFakeTimers();
  api.sessionPlanChromeGet.mockResolvedValue(null); api.sessionAgentPlanSnapshot.mockResolvedValue({ found: false });
  api.sessionPendingPlan.mockResolvedValue(null); api.sessionPlanChromeSet.mockResolvedValue({});
});
afterEach(() => { cleanup(); vi.clearAllTimers(); vi.useRealTimers(); });

test("a cached old approval cannot suppress the current Host generation", async () => {
  const cached = mergePlanFromEvent(emptySessionPlan(), { ...live, requestGeneration: "old" }, "计划");
  const view = mount(cached); api.sessionPendingPlan.mockResolvedValue(live);
  const completion = view.restore(); expect(view.hook.result.current).toBe(cached);
  await act(async () => { await completion; });
  expect(view.hook.result.current).toMatchObject({ rpcId: 0, requestGeneration: "current", body: "当前计划" });
  expect(api.sessionPendingPlan).toHaveBeenCalledExactlyOnceWith(sessionId);
});

test.each(["null", "unsupported"] as const)("a %s live query retires an old approval while preserving its in-memory body", async (reply) => {
  const cached = mergePlanFromEvent(emptySessionPlan(), { ...live, requestGeneration: "old" }, "计划");
  const view = mount(cached);
  if (reply === "unsupported") api.sessionPendingPlan.mockRejectedValue(new Error("unsupported"));
  await act(async () => { await view.restore(); });
  expect(view.hook.result.current).toMatchObject({ rpcId: null, requestGeneration: null, gateStale: true, body: cached.body });
  expect(view.c.planBySessionRef.current.get(sessionId)).toBe(view.hook.result.current);
});

test("a new live event wins over the pending disk and Host queries", async () => {
  const view = mount(); const old = deferred<typeof live | null>(); api.sessionPendingPlan.mockReturnValue(old.promise);
  const completion = view.restore(); const current = { ...live, requestGeneration: "event-wins", body: "直播计划" };
  view.emit(current);
  await act(async () => { old.resolve(live); await completion; });
  expect(view.hook.result.current).toMatchObject({ rpcId: current.rpcId, requestGeneration: current.requestGeneration, body: current.body });
  expect(view.c.planBySessionRef.current.get(sessionId)).toBe(view.hook.result.current);
});

test("an event followed by invalidation cannot be undone by a stale pull", async () => {
  const view = mount(); const old = deferred<typeof live | null>(); api.sessionPendingPlan.mockReturnValue(old.promise);
  const completion = view.restore(); view.emit(live);
  const invalidated = invalidatePlanGate(view.c.planBySessionRef.current.get(sessionId)!);
  act(() => { view.c.planBySessionRef.current.set(sessionId, invalidated); view.c.setPlan(invalidated); });
  await act(async () => { old.resolve(live); await completion; });
  expect(view.hook.result.current).toBe(invalidated); expect(view.hook.result.current.rpcId).toBeNull();
});

test("an event followed by removal cannot resurrect from an empty cache", async () => {
  const view = mount(); const old = deferred<typeof live | null>(); api.sessionPendingPlan.mockReturnValue(old.promise);
  const completion = view.restore(); view.emit(live);
  const empty = emptySessionPlan("计划");
  act(() => { view.c.planBySessionRef.current.delete(sessionId); view.c.setPlan(empty); });
  await act(async () => { old.resolve(live); await completion; });
  expect(view.hook.result.current).toBe(empty); expect(view.c.planBySessionRef.current.has(sessionId)).toBe(false);
});

test("a newer empty restore prevents an older positive query from committing", async () => {
  const view = mount(); const old = deferred<typeof live | null>(); const latest = deferred<typeof live | null>();
  api.sessionPendingPlan.mockReturnValueOnce(old.promise).mockReturnValueOnce(latest.promise);
  const first = view.restore(); const second = view.restore();
  await act(async () => { latest.resolve(null); await second; });
  const final = view.hook.result.current;
  await act(async () => { old.resolve(live); await first; });
  expect(view.hook.result.current).toBe(final); expect(final.rpcId).toBeNull();
  expect(view.c.markPlanPendingBadge).toHaveBeenCalledTimes(1);
});

test.each(["navigation", "cache replacement"] as const)("a restore cannot commit after %s", async (change) => {
  const view = mount(); const old = deferred<typeof live | null>(); api.sessionPendingPlan.mockReturnValue(old.promise);
  const completion = view.restore();
  if (change === "navigation") view.c.viewingSessionIdRef.current = "another-chat";
  else view.c.planBySessionRef.current = new SessionGateCache();
  await act(async () => { old.resolve(live); await completion; });
  expect(view.c.planBySessionRef.current.has(sessionId)).toBe(false);
  expect(view.c.markPlanPendingBadge).not.toHaveBeenCalled(); expect(view.hook.result.current.rpcId).toBeNull();
});

test("a stale disk response cannot reopen a local hard dismissal", async () => {
  const closed = closedSessionPlan("计划", live.toolCallId, live.rpcId, live.requestGeneration);
  const view = mount(closed); api.sessionPlanChromeGet.mockResolvedValue({ body: "旧计划", visible: true });
  await act(async () => { await view.restore(); });
  expect(view.hook.result.current).toBe(closed);
});

test("a distinct persisted plan tool reopens progress without fabricating an approval", async () => {
  const closed = closedSessionPlan("计划", live.toolCallId, live.rpcId, live.requestGeneration);
  const view = mount(closed);
  api.sessionPlanChromeGet.mockResolvedValue({ body: "新一轮计划", visible: true, toolCallId: "new-tool" });
  await act(async () => { await view.restore(); });
  expect(view.hook.result.current).toMatchObject({ userClosed: false, visible: true, rpcId: null, toolCallId: "new-tool" });
});

test.each(["current", "new-generation"])("a hard dismissal reopens only for a new Host generation: %s", async (generation) => {
  const closed = closedSessionPlan("计划", live.toolCallId, live.rpcId, live.requestGeneration);
  const view = mount(closed); api.sessionPendingPlan.mockResolvedValue({ ...live, requestGeneration: generation });
  await act(async () => { await view.restore(); });
  if (generation === "current") expect(view.hook.result.current).toBe(closed);
  else expect(view.hook.result.current).toMatchObject({ userClosed: false, rpcId: 0, requestGeneration: generation });
});
