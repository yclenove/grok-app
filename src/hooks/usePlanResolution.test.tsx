/** @vitest-environment jsdom */
import "@/test/jsdomStubs";
import { useLayoutEffect, useState } from "react";
import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { createT } from "@/i18n";
import type { AppDialog } from "@/lib/app/appDialogTypes";
import { emptySessionPlan, mergePlanFromEvent, type SessionPlanState } from "@/lib/planSession";
import { getPlanReplyClaims } from "@/lib/planReplyClaims";
import { SessionGateCache } from "@/lib/sessionGateCache";
import type { SideWorkbenchState } from "@/lib/sideWorkbench";
import { createSessionImportHostFixture } from "@/test/sessionImportHostFixture";
import { registerGateAndMetaSubscriptions } from "./sessionHostGateSubscriptions";
import { usePlanResolution } from "./usePlanResolution";

const api = vi.hoisted(() => ({ sessionResolvePlan: vi.fn(), sessionPlanChromeSet: vi.fn(), composerPrefsSet: vi.fn() }));
const history = vi.hoisted(() => ({ recordPlanHistory: vi.fn() }));
vi.mock("@/lib/api", () => api);
vi.mock("@/lib/planHistory", () => history);
const sessionId = "native-viewed";
const event: { sessionId: string; rpcId: number | null; requestGeneration: string | null; chromeRevision: string; toolCallId: string; body: string } =
  { sessionId, rpcId: 0, requestGeneration: "original", chromeRevision: "original-revision", toolCallId: "tool", body: "原始计划" };
const card = mergePlanFromEvent(emptySessionPlan("计划"), event, "计划");
function deferred() {
  let resolve!: (value?: unknown) => void; let reject!: (error: unknown) => void;
  const promise = new Promise((done, fail) => { resolve = done; reject = fail; });
  return { promise, resolve, reject };
}
function mount(initial = card) {
  const { context: c } = createSessionImportHostFixture();
  c.planBySessionRef.current.set(sessionId, initial);
  c.notifyPrefsRef.current = { notifyOnPermission: false, notifyOnTurnDone: false };
  const focus = { sessionId, epoch: 1 };
  const environment = { projectId: "project-a", mode: "plan", goalMode: true, asideCollapsed: false,
    sideWorkbench: { tabs: [{ id: "plan-tab", kind: "plan", name: "计划" }], activeId: "plan-tab", expanded: false, treeVisible: true } as SideWorkbenchState };
  c.planOpenedAsideRef.current = true;
  let dialog: AppDialog = null;
  const setMode = vi.fn(); const setGoalMode = vi.fn(); const toast = vi.fn(); const collapse = vi.fn();
  const createHook = (beforeEffects?: () => void) => renderHook(() => {
    useLayoutEffect(() => { beforeEffects?.(); }, []);
    const [plan, setPlan] = useState(c.planBySessionRef.current.get(c.viewingSessionIdRef.current!) ?? initial);
    const planRef = { current: plan }; c.setPlan = setPlan;
    return usePlanResolution({ ...environment, ...c, plan, planRef, currentViewFocus: () => ({ ...focus }),
      setMode, setGoalMode, collapseAsidePersisted: collapse, showToast: toast, tr: createT("zh"),
      setAppDialog: (next) => { dialog = typeof next === "function" ? next(dialog) : next; },
    });
  });
  const hook = createHook();
  const listeners = new Map<string, (payload: unknown) => void>();
  registerGateAndMetaSubscriptions(c, {
    isCancelled: () => false, track: () => {},
    listenWithRetry: async <T,>(name: string, handler: (payload: T) => void) => {
      listeners.set(name, (payload) => handler(payload as T)); return () => { listeners.delete(name); };
    },
  });
  return { c, hook, focus, environment, setMode, setGoalMode, toast, collapse, createHook,
    emit: (payload: typeof event) => act(() => listeners.get("session://plan")!(payload)),
    confirm: () => { if (dialog?.kind !== "confirm") throw new Error("Expected captured confirmation"); return dialog.onConfirm; },
  };
}
beforeEach(() => {
  vi.resetAllMocks(); vi.useFakeTimers();
  api.sessionResolvePlan.mockResolvedValue({ sessionId }); api.sessionPlanChromeSet.mockResolvedValue({}); api.composerPrefsSet.mockResolvedValue({});
});
afterEach(() => { cleanup(); vi.clearAllTimers(); vi.useRealTimers(); });

test("approval echoes RPC zero and its generation, settles only its plan and persists the captured composer target", async () => {
  const view = mount(); view.c.pendingAskUserBySessionRef.current.set(sessionId, { sessionId, rpcId: 9, questions: [] });
  await act(async () => { await view.hook.result.current.approvePlan(); });
  expect(api.sessionResolvePlan).toHaveBeenCalledExactlyOnceWith({ decision: "approved", feedback: undefined,
    sessionId, rpcId: 0, requestGeneration: "original" });
  expect(view.hook.result.current.plan).toMatchObject({ rpcId: null, requestGeneration: null, visible: false, busy: false });
  expect(view.c.pendingAskUserBySessionRef.current.has(sessionId)).toBe(true);
  expect(view.setMode).toHaveBeenCalledWith("agent"); expect(view.setGoalMode).toHaveBeenCalledWith(false);
  expect(api.composerPrefsSet).toHaveBeenCalledExactlyOnceWith({ projectId: "project-a", sessionId, mode: "agent" });
  expect(history.recordPlanHistory).toHaveBeenCalledWith(expect.objectContaining({ sessionId, decision: "approved", bodyPreview: "原始计划" }));
  expect(api.sessionPlanChromeSet).not.toHaveBeenCalled(); expect(view.c.clearPendingGatesRef.current).not.toHaveBeenCalled();
});

test("a successful old reply cannot settle a new generation or change its composer", async () => {
  const view = mount(); const pending = deferred(); api.sessionResolvePlan.mockReturnValue(pending.promise);
  let reply!: Promise<void>; act(() => { reply = view.hook.result.current.approvePlan(); });
  expect(view.hook.result.current.plan.busy).toBe(true);
  view.emit({ ...event, requestGeneration: "new", body: "新计划" });
  await act(async () => { pending.resolve(); await reply; });
  expect(view.hook.result.current.plan).toMatchObject({ rpcId: 0, requestGeneration: "new", busy: false, body: "新计划" });
  expect(view.setMode).not.toHaveBeenCalled(); expect(api.composerPrefsSet).not.toHaveBeenCalled();
  expect(history.recordPlanHistory).toHaveBeenCalledWith(expect.objectContaining({ bodyPreview: "原始计划" }));
});

test("an old chat's ACK settles its cache without touching the current chat's plan or modes", async () => {
  const view = mount(); const pending = deferred(); api.sessionResolvePlan.mockReturnValue(pending.promise);
  let reply!: Promise<void>; act(() => { reply = view.hook.result.current.approvePlan(); });
  view.c.viewingSessionIdRef.current = "other-chat"; view.focus.sessionId = "other-chat"; view.focus.epoch++;
  view.emit({ ...event, sessionId: "other-chat", requestGeneration: "other", body: "另一聊天" });
  await act(async () => { pending.resolve(); await reply; });
  expect(view.c.planBySessionRef.current.get(sessionId)?.rpcId).toBeNull();
  expect(view.hook.result.current.plan).toMatchObject({ requestGeneration: "other", body: "另一聊天" });
  expect(view.setMode).not.toHaveBeenCalled(); expect(api.composerPrefsSet).not.toHaveBeenCalled();
});

test.each(["mode", "goal", "project", "navigation"])("approval cannot override a later %s choice, including ABA", async (choice) => {
  const view = mount(); const pending = deferred(); api.sessionResolvePlan.mockReturnValue(pending.promise);
  let reply!: Promise<void>; act(() => { reply = view.hook.result.current.approvePlan(); });
  if (choice === "mode") { view.environment.mode = "chat"; view.hook.rerender(); view.environment.mode = "plan"; }
  if (choice === "goal") { view.environment.goalMode = false; view.hook.rerender(); view.environment.goalMode = true; }
  if (choice === "project") { view.environment.projectId = "project-b"; view.hook.rerender(); view.environment.projectId = "project-a"; }
  if (choice === "navigation") view.focus.epoch += 2;
  view.hook.rerender();
  await act(async () => { pending.resolve(); await reply; });
  expect(view.hook.result.current.plan.rpcId).toBeNull(); expect(view.setMode).not.toHaveBeenCalled();
  expect(view.setGoalMode).not.toHaveBeenCalled(); expect(api.composerPrefsSet).not.toHaveBeenCalled();
});

test("the confirmation keeps its original chat after navigation", async () => {
  const view = mount(); act(() => view.hook.result.current.dismissPlan()); const confirm = view.confirm();
  view.c.viewingSessionIdRef.current = "other-chat"; view.focus.epoch++;
  view.emit({ ...event, sessionId: "other-chat", requestGeneration: "other" });
  await act(async () => { await confirm(); });
  expect(api.sessionResolvePlan).toHaveBeenCalledWith(expect.objectContaining({ sessionId, requestGeneration: "original", decision: "abandoned" }));
  expect(view.c.planBySessionRef.current.get(sessionId)).toMatchObject({ userClosed: true, closedRequestGeneration: "original" });
  expect(view.hook.result.current.plan.requestGeneration).toBe("other"); expect(view.collapse).not.toHaveBeenCalled();
});

test("an expired confirmation cannot abandon a same-RPC replacement", async () => {
  const view = mount(); act(() => view.hook.result.current.dismissPlan()); const confirm = view.confirm();
  view.emit({ ...event, requestGeneration: "new" });
  await act(async () => { await confirm(); });
  expect(api.sessionResolvePlan).not.toHaveBeenCalled(); expect(history.recordPlanHistory).not.toHaveBeenCalled();
  expect(view.hook.result.current.plan.requestGeneration).toBe("new");
});

test.each(["approved", "abandoned"] as const)("failed %s leaves a retryable original plan and never archives", async (decision) => {
  const view = mount(); api.sessionResolvePlan.mockRejectedValueOnce(new Error("synthetic writer failed"));
  const perform = async () => {
    if (decision === "approved") await view.hook.result.current.approvePlan();
    else { view.hook.result.current.dismissPlan(); await view.confirm()(); }
  };
  await act(perform);
  expect(view.hook.result.current.plan).toMatchObject({ rpcId: 0, busy: false, userClosed: false });
  expect(history.recordPlanHistory).not.toHaveBeenCalled(); expect(view.collapse).not.toHaveBeenCalled(); expect(view.toast).toHaveBeenCalled();
  await act(perform); expect(api.sessionResolvePlan).toHaveBeenCalledTimes(2); expect(history.recordPlanHistory).toHaveBeenCalledTimes(1);
});

test("approve, revise and an earlier confirmation share one synchronous claim", async () => {
  const view = mount(); act(() => view.hook.result.current.dismissPlan()); const confirm = view.confirm();
  const pending = deferred(); api.sessionResolvePlan.mockReturnValue(pending.promise);
  let reply!: Promise<void>;
  await act(async () => { reply = view.hook.result.current.approvePlan(); await view.hook.result.current.requestPlanChanges("修改"); await confirm(); });
  expect(api.sessionResolvePlan).toHaveBeenCalledTimes(1);
  await act(async () => { pending.resolve(); await reply; });
});

test("claims and settlement notifications survive a composer remount", async () => {
  const view = mount(); const pending = deferred(); api.sessionResolvePlan.mockReturnValue(pending.promise);
  let reply!: Promise<void>; act(() => { reply = view.hook.result.current.approvePlan(); });
  view.hook.unmount(); const remount = view.createHook(); expect(remount.result.current.plan.busy).toBe(true);
  await act(async () => { await remount.result.current.approvePlan(); }); expect(api.sessionResolvePlan).toHaveBeenCalledTimes(1);
  await act(async () => { pending.resolve(); await reply; });
  expect(remount.result.current.plan).toMatchObject({ rpcId: null, busy: false });
});

test("revision failure retains its note, disables duplicate sends and permits retry", async () => {
  const view = mount(); act(() => view.hook.result.current.openRequestPlanChanges());
  act(() => view.hook.result.current.setPlanReviseNote("保留这段中文意见"));
  const pending = deferred(); api.sessionResolvePlan.mockReturnValueOnce(pending.promise);
  let reply!: Promise<void>; act(() => { reply = view.hook.result.current.submitPlanRevision(); });
  expect(view.hook.result.current.planReviseBusy).toBe(true);
  await act(async () => { await view.hook.result.current.submitPlanRevision(); view.hook.result.current.closePlanRevise(); });
  expect(api.sessionResolvePlan).toHaveBeenCalledTimes(1); expect(view.hook.result.current.planReviseOpen).toBe(true);
  await act(async () => { pending.reject(new Error("synthetic failure")); await reply; });
  expect(view.hook.result.current.planReviseNote).toBe("保留这段中文意见"); expect(view.hook.result.current.planReviseBusy).toBe(false);
  await act(async () => { await view.hook.result.current.submitPlanRevision(); });
  expect(api.sessionResolvePlan).toHaveBeenLastCalledWith(expect.objectContaining({ feedback: "保留这段中文意见", decision: "cancelled" }));
  expect(view.hook.result.current.planReviseOpen).toBe(false); expect(view.hook.result.current.plan.rpcId).toBeNull();
});

test("an old revision ACK cannot close or clear a new generation's revision modal", async () => {
  const view = mount(); act(() => view.hook.result.current.openRequestPlanChanges());
  const oldClose = view.hook.result.current.closePlanRevise; const oldSubmit = view.hook.result.current.submitPlanRevision;
  const pending = deferred(); api.sessionResolvePlan.mockReturnValueOnce(pending.promise);
  let reply!: Promise<void>; act(() => { reply = view.hook.result.current.submitPlanRevision(); });
  view.emit({ ...event, requestGeneration: "new" }); act(() => view.hook.result.current.openRequestPlanChanges());
  act(() => view.hook.result.current.setPlanReviseNote("新的意见"));
  await act(async () => { oldClose(); await oldSubmit(); pending.resolve(); await reply; });
  expect(view.hook.result.current.planReviseOpen).toBe(true); expect(view.hook.result.current.planReviseNote).toBe("新的意见");
  expect(api.sessionResolvePlan).toHaveBeenCalledTimes(1);
});

test("a hidden revision modal cannot hijack another chat's direct edited-plan submission", async () => {
  const view = mount(); act(() => view.hook.result.current.openRequestPlanChanges());
  view.c.viewingSessionIdRef.current = "other-chat"; view.focus.epoch++;
  view.emit({ ...event, sessionId: "other-chat", requestGeneration: "other" });
  await act(async () => { await view.hook.result.current.requestPlanChanges("另一聊天的编辑计划"); });
  expect(api.sessionResolvePlan).toHaveBeenCalledExactlyOnceWith(expect.objectContaining({ sessionId: "other-chat", requestGeneration: "other" }));
});

test.each([false, true])("dismissal only closes its original active plan pane (changed=%s)", async (changed) => {
  const view = mount(); const pending = deferred(); api.sessionResolvePlan.mockReturnValue(pending.promise);
  act(() => view.hook.result.current.dismissPlan()); let reply!: void | Promise<void>;
  act(() => { reply = view.confirm()(); });
  if (changed) {
    view.environment.sideWorkbench = { ...view.environment.sideWorkbench, activeId: "files", tabs: [{ id: "files", kind: "file", name: "文件" }] };
    view.hook.rerender();
  }
  await act(async () => { pending.resolve(); await reply; });
  expect(view.collapse).toHaveBeenCalledTimes(changed ? 0 : 1);
});

test("a failed read-only chrome dismissal cannot pretend it was saved", async () => {
  const view = mount({ ...card, rpcId: null, requestGeneration: null });
  api.sessionPlanChromeSet.mockRejectedValue(new Error("synthetic persistence failure"));
  act(() => view.hook.result.current.dismissPlan()); await act(async () => { await view.confirm()(); });
  expect(api.sessionResolvePlan).not.toHaveBeenCalled(); expect(history.recordPlanHistory).not.toHaveBeenCalled();
  expect(view.hook.result.current.plan.userClosed).toBe(false); expect(view.toast).toHaveBeenCalled();
});

test("a legacy same-RPC replacement is a different reply owner", async () => {
  const legacy: SessionPlanState = { ...card, requestGeneration: null }; const view = mount(legacy);
  const pending = deferred(); api.sessionResolvePlan.mockReturnValue(pending.promise);
  let reply!: Promise<void>; act(() => { reply = view.hook.result.current.approvePlan(); });
  const replacement = { ...legacy }; act(() => { view.c.planBySessionRef.current.set(sessionId, replacement); view.c.setPlan(replacement); });
  await act(async () => { pending.resolve(); await reply; });
  expect(view.hook.result.current.plan.rpcId).toBe(0); expect(view.setMode).not.toHaveBeenCalled();
});

test("navigation cannot borrow a same-tool progress card from the previously rendered chat", () => {
  const view = mount({ ...card, rpcId: null, requestGeneration: null });
  view.c.viewingSessionIdRef.current = "other-chat";
  view.c.planBySessionRef.current.set("other-chat", { ...card, rpcId: null, requestGeneration: null, body: "另一聊天" });
  act(() => view.hook.result.current.dismissPlan());
  expect(view.confirm).toThrow("Expected captured confirmation"); expect(api.sessionPlanChromeSet).not.toHaveBeenCalled();
});

test("an old reply's finally cannot release a newer generation's pending claim", async () => {
  const view = mount(); const old = deferred(); const next = deferred();
  api.sessionResolvePlan.mockReturnValueOnce(old.promise).mockReturnValueOnce(next.promise);
  let oldReply!: Promise<void>; let nextReply!: Promise<void>;
  act(() => { oldReply = view.hook.result.current.approvePlan(); });
  view.emit({ ...event, requestGeneration: "new" });
  act(() => { nextReply = view.hook.result.current.approvePlan(); });
  await act(async () => { old.resolve(); await oldReply; });
  expect(view.hook.result.current.plan).toMatchObject({ requestGeneration: "new", busy: true });
  await act(async () => { await view.hook.result.current.requestPlanChanges("重复修改"); });
  expect(api.sessionResolvePlan).toHaveBeenCalledTimes(2);
  await act(async () => { next.reject(new Error("new failure")); await nextReply; });
  expect(view.hook.result.current.plan).toMatchObject({ requestGeneration: "new", busy: false, rpcId: 0 });
});

test("closing empty non-RPC waiting chrome does not add an empty plan-history entry", async () => {
  const view = mount({ ...emptySessionPlan("计划"), visible: true, chromeRevision: "empty-revision" });
  act(() => view.hook.result.current.dismissPlan()); await act(async () => { await view.confirm()(); });
  expect(api.sessionPlanChromeSet).toHaveBeenCalledTimes(1); expect(history.recordPlanHistory).not.toHaveBeenCalled();
  expect(view.hook.result.current.plan.userClosed).toBe(true);
});

test("read-only dismissal echoes its captured revision and missing identity never reports success", async () => {
  const view = mount({ ...card, rpcId: null, requestGeneration: null });
  act(() => view.hook.result.current.dismissPlan()); await act(async () => { await view.confirm()(); });
  expect(api.sessionPlanChromeSet).toHaveBeenCalledExactlyOnceWith(sessionId,
    expect.objectContaining({ userClosed: true }), "original-revision");
  expect(view.hook.result.current.plan.userClosed).toBe(true);
  cleanup(); vi.clearAllMocks();
  const missing = mount({ ...card, rpcId: null, requestGeneration: null, chromeRevision: null });
  act(() => missing.hook.result.current.dismissPlan()); await act(async () => { await missing.confirm()(); });
  expect(api.sessionPlanChromeSet).not.toHaveBeenCalled(); expect(history.recordPlanHistory).not.toHaveBeenCalled();
  expect(missing.hook.result.current.plan.userClosed).toBe(false);
  expect(missing.toast).toHaveBeenCalledWith(createT("zh")("plan.changedRefresh"), 4500);
});

test.each([false, true])("an old no-RPC close cannot settle same-tool progress from a new revision (late=%s)", async (late) => {
  const view = mount({ ...card, rpcId: null, requestGeneration: null });
  const pending = deferred(); api.sessionPlanChromeSet.mockReturnValue(pending.promise);
  act(() => view.hook.result.current.dismissPlan()); const confirm = view.confirm();
  let reply: void | Promise<void>;
  if (late) act(() => { reply = confirm(); });
  view.emit({ ...event, rpcId: null, requestGeneration: null,
    chromeRevision: "next-revision", body: "同工具的新中文计划" });
  if (!late) act(() => { reply = confirm(); });
  await act(async () => { pending.resolve(); await reply; });
  expect(api.sessionPlanChromeSet).toHaveBeenCalledTimes(late ? 1 : 0);
  expect(view.hook.result.current.plan).toMatchObject({ userClosed: false, chromeRevision: "next-revision", body: "同工具的新中文计划" });
  expect(view.collapse).not.toHaveBeenCalled();
});

test("revise-to-read-only closes using the decision receipt rather than the original event revision", async () => {
  const view = mount(); api.sessionResolvePlan.mockResolvedValue({ sessionId, planChromeRevision: "decision-revision" });
  await act(async () => { await view.hook.result.current.requestPlanChanges("请修改"); });
  expect(view.hook.result.current.plan).toMatchObject({ rpcId: null, chromeRevision: "decision-revision" });
  act(() => view.hook.result.current.dismissPlan()); await act(async () => { await view.confirm()(); });
  expect(api.sessionPlanChromeSet).toHaveBeenCalledExactlyOnceWith(sessionId,
    expect.objectContaining({ userClosed: true }), "decision-revision");
});

test("foreground and background plan events never rewrite Host chrome", () => {
  const view = mount(); view.emit(event); view.emit({ ...event, sessionId: "other-chat" });
  expect(api.sessionPlanChromeSet).not.toHaveBeenCalled();
});

test("a render-to-effect cache replacement cannot subscribe the old store as the new cache", async () => {
  const view = mount(); view.hook.unmount();
  const oldCache = view.c.planBySessionRef.current;
  const oldClaims = getPlanReplyClaims(oldCache);
  const nextCache = new SessionGateCache([[sessionId, card]]);
  const remount = view.createHook(() => { view.c.planBySessionRef.current = nextCache; });
  act(() => oldClaims.settle({ sessionId, plan: card }, { ...card, rpcId: null, requestGeneration: null }));
  expect(remount.result.current.plan).toMatchObject({ rpcId: 0, requestGeneration: "original" });
  remount.rerender();
  await act(async () => { await remount.result.current.approvePlan(); });
  expect(remount.result.current.plan.rpcId).toBeNull();
  expect(oldCache.get(sessionId)?.rpcId).toBe(0);
});

test("revision retry after navigating away and back keeps the failed note and rebinds its view", async () => {
  const view = mount();
  act(() => view.hook.result.current.openRequestPlanChanges());
  act(() => view.hook.result.current.setPlanReviseNote("网络失败后仍需保留的中文意见"));
  api.sessionResolvePlan.mockRejectedValueOnce(new Error("synthetic failure"));
  await act(async () => { await view.hook.result.current.submitPlanRevision(); });
  view.c.viewingSessionIdRef.current = "other-chat"; view.focus.sessionId = "other-chat"; view.focus.epoch++;
  view.emit({ ...event, sessionId: "other-chat", requestGeneration: "other", body: "另一聊天" });
  expect(view.hook.result.current.planReviseOpen).toBe(false);
  view.c.viewingSessionIdRef.current = sessionId; view.focus.sessionId = sessionId; view.focus.epoch++;
  act(() => view.c.setPlan(view.c.planBySessionRef.current.get(sessionId)!));
  expect(view.hook.result.current.planReviseOpen).toBe(false);
  act(() => view.hook.result.current.openRequestPlanChanges());
  expect(view.hook.result.current.planReviseOpen).toBe(true);
  expect(view.hook.result.current.planReviseNote).toBe("网络失败后仍需保留的中文意见");
  await act(async () => { await view.hook.result.current.submitPlanRevision(); });
  expect(api.sessionResolvePlan).toHaveBeenLastCalledWith(expect.objectContaining({ feedback: "网络失败后仍需保留的中文意见" }));
  expect(view.hook.result.current.planReviseOpen).toBe(false);
});

test("revision notes are reset after explicit cancel or a same-RPC new generation", () => {
  const view = mount();
  act(() => view.hook.result.current.openRequestPlanChanges());
  act(() => view.hook.result.current.setPlanReviseNote("主动取消的草稿"));
  act(() => view.hook.result.current.closePlanRevise());
  act(() => view.hook.result.current.openRequestPlanChanges());
  expect(view.hook.result.current.planReviseNote).toBe("");
  act(() => view.hook.result.current.setPlanReviseNote("上一代草稿"));
  view.emit({ ...event, requestGeneration: "new" });
  act(() => view.hook.result.current.openRequestPlanChanges());
  expect(view.hook.result.current.planReviseNote).toBe("");
});

test("captured actions check the target cache's busy claim after replacement before rerender", () => {
  const view = mount();
  const open = view.hook.result.current.openRequestPlanChanges;
  const dismiss = view.hook.result.current.dismissPlan;
  const cache = new SessionGateCache([[sessionId, card]]);
  view.c.planBySessionRef.current = cache;
  const owner = { sessionId, plan: card };
  const claims = getPlanReplyClaims(cache); claims.claim(owner);
  act(() => { open(); dismiss(); });
  expect(view.hook.result.current.planReviseOpen).toBe(false);
  expect(view.confirm).toThrow("Expected captured confirmation");
  claims.release(owner);
});
