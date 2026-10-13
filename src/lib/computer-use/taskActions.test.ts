import { beforeEach, expect, it, vi } from "vitest";
import type { ComputerTaskExecution, ComputerTaskKey, ComputerTaskView } from "@/lib/api/computerTask";
import type { ComputerTaskPickerStarted, ComputerTaskPickerView } from "@/lib/api/computerTaskPicker";
import type { ComputerTaskSnapshot } from "./taskSource";
import { ComputerTaskActions } from "./taskActions";

const key: ComputerTaskKey = { connectionId: "synthetic-connection", taskId: "synthetic-task", requestId: "synthetic-request", revision: 1 };
const pending = (): ComputerTaskView => ({
  sessionId: "synthetic-app", connectionId: key.connectionId, taskId: key.taskId, featureEnabled: false,
  request: { key, phase: "pending", purpose: "Synthetic owned task", scopeIntent: "Only fixture windows", surfaces: ["desktop"], capabilities: ["observe", "input"], remainingMs: 60_000 },
  grants: [], executions: [], pickerSurfaces: ["desktop"],
});
const execution = (): ComputerTaskExecution => ({ key, runId: "synthetic-run", generation: "18446744073709551614", phase: "idle", target: { targetId: "fixture-window", surface: "desktop" }, targetName: "Fixture editor", cleanupPending: false, resumeAvailable: false, hasFreshObservation: true, lastFailure: null });
function snapshot(view = pending(), state: ComputerTaskSnapshot["state"] = "live"): ComputerTaskSnapshot {
  return { state, view, owner: { sessionId: view.sessionId, connectionId: view.connectionId, subscriptionId: "synthetic-stream", hostInstanceId: "synthetic-host" } };
}
const picker = (): ComputerTaskPickerStarted => ({ sessionId: "synthetic-app", key, pickerId: "synthetic-picker", maxSelection: 2, surface: "desktop" });
const candidates = (): ComputerTaskPickerView => ({ ...picker(), candidates: [
  { candidateId: "fixture-A", title: "Owned editor", appName: "Synthetic app", kind: "window", surface: "desktop" },
  { candidateId: "fixture-B", title: "Owned viewer", appName: "Synthetic app", kind: "window", surface: "desktop" },
], truncated: false });
function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: Error) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  // A red implementation may never consume this fixture promise. Its rejection
  // must not replace the meaningful call/state assertions with runner noise.
  void promise.catch(() => {});
  return { promise, resolve, reject };
}
function fixture(view = pending()) {
  const deps = {
    begin: vi.fn(async () => picker()), load: vi.fn(async () => candidates()), cancel: vi.fn(async () => true),
    approve: vi.fn(async () => ({ requestId: key.requestId, revision: key.revision, runId: "synthetic-run", decisionClass: "synthetic" })),
    managedApprove: vi.fn(async () => ({ requestId: key.requestId, revision: key.revision, runId: "synthetic-run", decisionClass: "synthetic" })),
    decide: vi.fn(async (sessionId: string, decidedKey: ComputerTaskKey, decision: "deny" | "cancel" | "stop") => ({ sessionId, key: decidedKey, decision })),
    pause: vi.fn(async () => {}),
    preparation: vi.fn(), confirm: vi.fn(),
    resume: vi.fn(async () => ({ phase: "in_progress" as const })),
  };
  const actions = new ComputerTaskActions("synthetic-app", deps);
  actions.update(snapshot(view));
  return { actions, deps };
}

beforeEach(() => vi.clearAllMocks());

const managedPending = (): ComputerTaskView => ({
  ...pending(), request: { ...pending().request!, surfaces: ["managed-browser"] }, pickerSurfaces: ["managed-browser"],
});

const appWebviewPending = (): ComputerTaskView => ({
  ...pending(), request: { ...pending().request!, surfaces: ["app-webview"] }, pickerSurfaces: ["app-webview"],
});

it("managed consent starts only after one explicit gesture without a fake window picker", async () => {
  const f = fixture(managedPending());
  f.actions.subscribe(vi.fn()); f.actions.subscribe(vi.fn());
  f.actions.update(snapshot(managedPending()));
  for (const call of Object.values(f.deps)) expect(call).not.toHaveBeenCalled();
  await Promise.all([f.actions.choose("managed-browser", "card"), f.actions.choose("managed-browser", "panel")]);
  expect(f.deps.managedApprove).toHaveBeenCalledExactlyOnceWith("synthetic-app", key);
  for (const call of [f.deps.begin, f.deps.load, f.deps.cancel, f.deps.approve]) expect(call).not.toHaveBeenCalled();
  expect(f.actions.getSnapshot()).toMatchObject({ pending: "approve", choosing: false, picker: null, awaitingHost: true });
});

it("managed approval failure needs a new gesture to retry and never fabricates a grant", async () => {
  const f = fixture(managedPending());
  f.deps.managedApprove.mockRejectedValueOnce(new Error("synthetic runtime failure"));
  await f.actions.choose("managed-browser");
  expect(f.actions.getSnapshot()).toMatchObject({ pending: null, error: "managed", awaitingHost: false });
  expect(f.deps.managedApprove).toHaveBeenCalledTimes(1);
  f.actions.update(snapshot(managedPending()));
  expect(f.deps.managedApprove).toHaveBeenCalledTimes(1);
  await f.actions.choose("managed-browser");
  expect(f.deps.managedApprove).toHaveBeenCalledTimes(2);
  expect(f.actions.getSnapshot()).toMatchObject({ pending: "approve", error: null, awaitingHost: true });
});

it.each([
  ["managed_browser_macos_requires_14: native macOS 13", "managed-macos"],
  ["managed_browser_os_version_unavailable: native query failed", "managed-os-version"],
] as const)("keeps %s actionable without automatic repair, retry or authority", async (message, error) => {
  const f = fixture(managedPending());
  f.deps.managedApprove.mockRejectedValueOnce(message);
  await f.actions.choose("managed-browser");
  expect(f.actions.getSnapshot()).toMatchObject({ pending: null, error, awaitingHost: false });
  f.actions.update(snapshot(managedPending()));
  expect(f.deps.managedApprove).toHaveBeenCalledTimes(1);
  for (const call of [f.deps.begin, f.deps.load, f.deps.approve]) expect(call).not.toHaveBeenCalled();
});

it.each(["resolve", "reject"] as const)("Stop overtakes managed preparation and ignores its late %s", async (late) => {
  const f = fixture(managedPending());
  const approval = deferred<Awaited<ReturnType<typeof f.deps.managedApprove>>>();
  f.deps.managedApprove.mockReturnValueOnce(approval.promise);
  const creating = f.actions.choose("managed-browser");
  expect(f.deps.managedApprove).toHaveBeenCalledTimes(1);
  await f.actions.decide("stop");
  expect(f.deps.decide).toHaveBeenCalledExactlyOnceWith("synthetic-app", key, "stop");
  if (late === "resolve") approval.resolve({ requestId: key.requestId, revision: key.revision, runId: "obsolete-run", decisionClass: "synthetic" });
  else approval.reject(new Error("obsolete runtime failure"));
  await creating;
  expect(f.actions.getSnapshot()).toMatchObject({ pending: "stop", picker: null, error: null, awaitingHost: true });
  expect(f.deps.begin).not.toHaveBeenCalled();
});

it("managed receipt waits for Host approval and a late failure cannot erase authoritative success", async () => {
  const f = fixture(managedPending());
  const approval = deferred<Awaited<ReturnType<typeof f.deps.managedApprove>>>();
  f.deps.managedApprove.mockReturnValueOnce(approval.promise);
  const creating = f.actions.choose("managed-browser");
  expect(f.deps.managedApprove).toHaveBeenCalledTimes(1);
  f.actions.update(snapshot({ ...managedPending(), request: { ...managedPending().request!, phase: "approved" } }));
  approval.reject(new Error("reply lost after commit")); await creating;
  expect(f.actions.getSnapshot()).toMatchObject({ pending: null, error: null, awaitingHost: false });
});

it("foreign managed receipts and replaced requests cannot control the current task", async () => {
  const f = fixture(managedPending());
  f.deps.managedApprove.mockResolvedValueOnce({ requestId: "foreign-request", revision: 1, runId: "foreign-run", decisionClass: "synthetic" });
  await f.actions.choose("managed-browser");
  expect(f.actions.getSnapshot()).toMatchObject({ pending: null, error: "managed", awaitingHost: false });
  const old = deferred<Awaited<ReturnType<typeof f.deps.managedApprove>>>();
  f.deps.managedApprove.mockReturnValueOnce(old.promise);
  const creating = f.actions.choose("managed-browser");
  const nextKey = { ...key, requestId: "new-request", revision: 2 };
  f.actions.update(snapshot({ ...managedPending(), request: { ...managedPending().request!, key: nextKey } }));
  old.reject(new Error("obsolete owner")); await creating;
  expect(f.actions.getSnapshot()).toMatchObject({ pending: null, error: null });
  expect(f.deps.decide).not.toHaveBeenCalled();
});

it("late Host approval clears an earlier managed reply failure without retrying or manufacturing authority", async () => {
  const f = fixture(managedPending());
  f.deps.managedApprove.mockRejectedValueOnce(new Error("synthetic reply lost"));
  await f.actions.choose("managed-browser");
  expect(f.actions.getSnapshot()).toMatchObject({ pending: null, error: "managed" });
  f.actions.update(snapshot({ ...managedPending(), request: { ...managedPending().request!, phase: "approved" } }));
  expect(f.actions.getSnapshot()).toMatchObject({ pending: null, error: null, awaitingHost: false });
  expect(f.deps.managedApprove).toHaveBeenCalledTimes(1);
  expect(f.deps.begin).not.toHaveBeenCalled();
  expect(f.deps.decide).not.toHaveBeenCalled();
});

it("reads pending facts without enumerating, granting or creating an execution", () => {
  const f = fixture();
  const first = f.actions.subscribe(vi.fn());
  const second = f.actions.subscribe(vi.fn());
  f.actions.update(snapshot());
  for (const call of Object.values(f.deps)) expect(call).not.toHaveBeenCalled();
  first(); second();
  expect(f.deps.decide).not.toHaveBeenCalled();
});

it("opens exactly one explicit picker, with bounded valid selections", async () => {
  const f = fixture();
  await Promise.all([f.actions.choose("desktop"), f.actions.choose("desktop")]);
  await f.actions.choose("desktop");
  expect(f.deps.begin).toHaveBeenCalledExactlyOnceWith("synthetic-app", key, "desktop");
  expect(f.deps.load).toHaveBeenCalledExactlyOnceWith(picker());
  f.actions.toggleCandidate("forged");
  expect(f.actions.getSnapshot().selected).toEqual([]);
  f.actions.toggleCandidate("fixture-A"); f.actions.toggleCandidate("fixture-B");
  expect(f.actions.getSnapshot().selected).toEqual(["fixture-A", "fixture-B"]);
  await Promise.all([f.actions.approve(), f.actions.approve()]);
  expect(f.deps.approve).toHaveBeenCalledExactlyOnceWith(candidates(), ["fixture-A", "fixture-B"]);
  expect(f.actions.getSnapshot()).toMatchObject({ pending: "approve", awaitingHost: true });
  // The receipt doesn't make the task green or adopt its runId.
  f.actions.update(snapshot({ ...pending(), request: { ...pending().request!, phase: "approved" }, executions: [execution()] }));
  expect(f.actions.getSnapshot()).toMatchObject({ pending: null, picker: null, awaitingHost: false });
});

it("uses the original picker transaction for App-owned WebView pages", async () => {
  const f = fixture(appWebviewPending());
  const started = { ...picker(), surface: "app-webview" as const };
  const view = { ...started, candidates: [{
    candidateId: "host-local-page", title: "Generated images", appName: "side-browser",
    kind: "webview", surface: "app-webview",
  }], truncated: false };
  f.deps.begin.mockResolvedValueOnce(started);
  f.deps.load.mockResolvedValueOnce(view);
  await f.actions.choose("app-webview", "card");
  expect(f.deps.begin).toHaveBeenCalledExactlyOnceWith("synthetic-app", key, "app-webview");
  expect(f.deps.load).toHaveBeenCalledExactlyOnceWith(started);
  expect(f.actions.getSnapshot()).toMatchObject({ choosing: true, pickerOwner: "card", picker: view });
  f.actions.toggleCandidate("host-local-page");
  await f.actions.approve();
  expect(f.deps.approve).toHaveBeenCalledExactlyOnceWith(view, ["host-local-page"]);
  expect(f.actions.getSnapshot()).toMatchObject({ pending: "approve", awaitingHost: true });
});

it("keeps App WebView approval alive while the Host publishes preparing state", async () => {
  const f = fixture(appWebviewPending());
  const started = { ...picker(), surface: "app-webview" as const };
  const view = { ...started, candidates: [{
    candidateId: "host-local-page", title: "Generated images", appName: "side-browser",
    kind: "webview", surface: "app-webview",
  }], truncated: false };
  f.deps.begin.mockResolvedValueOnce(started);
  f.deps.load.mockResolvedValueOnce(view);
  await f.actions.choose("app-webview", "card");
  f.actions.toggleCandidate("host-local-page");
  const approval = deferred<Awaited<ReturnType<typeof f.deps.approve>>>();
  f.deps.approve.mockReturnValueOnce(approval.promise);
  const approving = f.actions.approve();
  f.actions.update(snapshot({
    ...appWebviewPending(),
    featureEnabled: true,
    request: { ...appWebviewPending().request!, phase: "preparing" },
  }));
  expect(f.deps.cancel).not.toHaveBeenCalled();
  expect(f.actions.getSnapshot()).toMatchObject({
    pending: "approve", choosing: true, picker: view, error: null,
  });
  approval.resolve({
    requestId: key.requestId, revision: key.revision, runId: "synthetic-run", decisionClass: "synthetic",
  });
  await approving;
  expect(f.actions.getSnapshot()).toMatchObject({ pending: "approve", awaitingHost: true, error: null });
});

it("close during begin cancels the original late receipt and never loads it", async () => {
  const f = fixture();
  const begun = deferred<ReturnType<typeof picker>>();
  f.deps.begin.mockReturnValueOnce(begun.promise);
  const choose = f.actions.choose("desktop");
  f.actions.closePicker();
  begun.resolve(picker()); await choose;
  expect(f.deps.cancel).toHaveBeenCalledExactlyOnceWith(picker());
  expect(f.deps.load).not.toHaveBeenCalled();
  expect(f.actions.getSnapshot().choosing).toBe(false);
});

it("Stop overtakes approval and its late error cannot touch the Stop", async () => {
  const f = fixture();
  await f.actions.choose("desktop"); f.actions.toggleCandidate("fixture-A");
  const approval = deferred<Awaited<ReturnType<typeof f.deps.approve>>>();
  f.deps.approve.mockReturnValueOnce(approval.promise);
  const approve = f.actions.approve();
  await f.actions.decide("stop");
  expect(f.deps.decide).toHaveBeenCalledExactlyOnceWith("synthetic-app", key, "stop");
  expect(f.deps.cancel).toHaveBeenCalledExactlyOnceWith(candidates());
  approval.reject(new Error("obsolete approval")); await approve;
  expect(f.actions.getSnapshot()).toMatchObject({ pending: "stop", awaitingHost: true, error: null });
});

it("cancel during loading fences the picker and discards late candidates", async () => {
  const f = fixture();
  const loaded = deferred<ReturnType<typeof candidates>>();
  f.deps.load.mockReturnValueOnce(loaded.promise);
  const choosing = f.actions.choose("desktop");
  await Promise.resolve();
  await f.actions.decide("cancel");
  loaded.resolve(candidates()); await choosing;
  expect(f.actions.getSnapshot()).toMatchObject({ pending: "cancel", picker: null, choosing: false });
  expect(f.deps.approve).not.toHaveBeenCalled();
});

it("original task Stop remains available on stale metadata, without approving or pausing", async () => {
  const f = fixture();
  f.actions.update(snapshot(pending(), "stale"));
  await f.actions.choose("desktop"); await f.actions.pause(execution());
  expect(f.deps.begin).not.toHaveBeenCalled(); expect(f.deps.pause).not.toHaveBeenCalled();
  await f.actions.decide("stop");
  expect(f.deps.decide).toHaveBeenCalledExactlyOnceWith("synthetic-app", key, "stop");
});

it("native stopped settles a pending Stop before its transport reply", async () => {
  const f = fixture({ ...pending(), executions: [execution()] });
  const stopped = deferred<Awaited<ReturnType<typeof f.deps.decide>>>();
  f.deps.decide.mockReturnValueOnce(stopped.promise);
  const stop = f.actions.decide("stop");
  f.actions.update(snapshot({ ...pending(), executions: [{ ...execution(), phase: "stopped" }] }));
  stopped.reject(new Error("late transport error")); await stop;
  expect(f.actions.getSnapshot()).toMatchObject({ pending: null, error: null });
});

it("Stop ack and stop_requested stay pending; stale facts never prove completion", async () => {
  const f = fixture({ ...pending(), executions: [execution()] });
  await f.actions.decide("stop");
  f.actions.update(snapshot({ ...pending(), executions: [{ ...execution(), phase: "stop_requested", cleanupPending: true }] }));
  expect(f.actions.getSnapshot()).toMatchObject({ pending: "stop", awaitingHost: true });
  f.actions.update(snapshot({ ...pending(), executions: [{ ...execution(), phase: "stop_requested" }] }, "stale"));
  expect(f.actions.getSnapshot().pending).toBeNull();
  await f.actions.decide("stop");
  expect(f.deps.decide).toHaveBeenCalledTimes(2);
});

it("Pause deduplicates a full generation address while Stop overtakes it", async () => {
  const run = execution();
  const f = fixture({ ...pending(), featureEnabled: true, executions: [run], grants: [{
    key, runId: run.runId, members: [run.target!], capabilities: ["observe", "input"], suspended: false, executable: true,
  }] });
  const paused = deferred<void>(); f.deps.pause.mockReturnValueOnce(paused.promise);
  const pause = f.actions.pause(run); await f.actions.pause(run);
  expect(f.deps.pause).toHaveBeenCalledExactlyOnceWith("synthetic-app", {
    key: run.key, runId: run.runId, generation: run.generation,
  });
  await f.actions.decide("stop");
  paused.reject(new Error("obsolete pause")); await pause;
  expect(f.actions.getSnapshot()).toMatchObject({ pending: "stop", error: null });
});

it("replacement task isolates late Stop failure and is independently stoppable", async () => {
  const f = fixture();
  const reply = deferred<Awaited<ReturnType<typeof f.deps.decide>>>();
  f.deps.decide.mockReturnValueOnce(reply.promise);
  const old = f.actions.decide("stop");
  const replacementKey = { ...key, taskId: "synthetic-next-task", requestId: "synthetic-next-request" };
  const next = { ...pending(), taskId: replacementKey.taskId, request: { ...pending().request!, key: replacementKey } };
  f.actions.update(snapshot(next));
  await f.actions.decide("stop"); reply.reject(new Error("old task")); await old;
  expect(f.deps.decide).toHaveBeenLastCalledWith("synthetic-app", replacementKey, "stop");
  expect(f.actions.getSnapshot()).toMatchObject({ pending: "stop", error: null });
});

it("retirement or last consumer release cancels only the original picker", async () => {
  const f = fixture();
  const first = f.actions.subscribe(vi.fn()); const second = f.actions.subscribe(vi.fn());
  await f.actions.choose("desktop"); first();
  expect(f.deps.cancel).not.toHaveBeenCalled(); second();
  expect(f.deps.cancel).toHaveBeenCalledExactlyOnceWith(candidates());
  expect(f.deps.decide).not.toHaveBeenCalled();
});

it("foreign picker receipts cannot load or cancel another session", async () => {
  const f = fixture();
  f.deps.begin.mockResolvedValueOnce({ ...picker(), sessionId: "foreign-session" });
  await f.actions.choose("desktop");
  expect(f.deps.load).not.toHaveBeenCalled(); expect(f.deps.cancel).not.toHaveBeenCalled();
  expect(f.actions.getSnapshot().error).toBe("picker");
});

it("unimplemented picker surfaces do not call an API", async () => {
  const f = fixture({ ...pending(), pickerSurfaces: [] });
  await f.actions.choose("desktop"); await f.actions.choose("managed-browser");
  expect(f.deps.begin).not.toHaveBeenCalled();
});

it("Host expiry closes a ready picker instead of keeping stale approval controls", async () => {
  const f = fixture();
  await f.actions.choose("desktop");
  f.actions.update(snapshot({ ...pending(), request: { ...pending().request!, phase: "expired" } }));
  expect(f.actions.getSnapshot()).toMatchObject({ choosing: false, picker: null });
  expect(f.deps.cancel).toHaveBeenCalledExactlyOnceWith(candidates());
});

it.each(["deny", "cancel"] as const)("incremental %s settles its request while the original grant continues", async (decision) => {
  const originalKey = { ...key, requestId: "original-approved-request" };
  const originalRun = { ...execution(), key: originalKey, runId: "original-run" };
  const view = { ...pending(), featureEnabled: true, executions: [originalRun], grants: [{
    key: originalKey, runId: originalRun.runId, members: [originalRun.target!], capabilities: ["observe"], suspended: false, executable: true,
  }] };
  const f = fixture(view);
  await f.actions.decide(decision);
  f.actions.update(snapshot({ ...view, request: { ...view.request!, phase: decision === "deny" ? "denied" : "cancelled" } }));
  expect(f.actions.getSnapshot()).toMatchObject({ pending: null, error: null, awaitingHost: false });
  expect(f.deps.decide).toHaveBeenCalledExactlyOnceWith("synthetic-app", key, decision);
  expect(f.deps.pause).not.toHaveBeenCalled();
});

it("incremental expiry closes the picker without stopping the original approved execution", async () => {
  const originalKey = { ...key, requestId: "original-approved-request" };
  const originalRun = { ...execution(), key: originalKey, runId: "original-run" };
  const view = { ...pending(), featureEnabled: true, executions: [originalRun], grants: [{
    key: originalKey, runId: originalRun.runId, members: [originalRun.target!], capabilities: ["observe"], suspended: false, executable: true,
  }] };
  const f = fixture(view);
  await f.actions.choose("desktop"); f.actions.toggleCandidate("fixture-A");
  f.actions.update(snapshot({ ...view, request: { ...view.request!, phase: "expired" } }));
  await f.actions.approve();
  expect(f.actions.getSnapshot()).toMatchObject({ picker: null, choosing: false });
  expect(f.deps.cancel).toHaveBeenCalledExactlyOnceWith(candidates());
  expect(f.deps.approve).not.toHaveBeenCalled(); expect(f.deps.decide).not.toHaveBeenCalled();
});
