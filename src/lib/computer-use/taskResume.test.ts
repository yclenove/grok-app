import { expect, it, vi } from "vitest";
import type { ComputerTaskResumeOutcome, ComputerTaskView } from "@/lib/api/computerTask";
import { ComputerTaskActions } from "./taskActions";
import type { ComputerTaskSnapshot } from "./taskSource";

const key = { connectionId: "original-connection", taskId: "original-task", requestId: "original-request", revision: 2 };
const address = { key, runId: "original-run", generation: "18446744073709551614" };
const successor = { ...address, generation: "18446744073709551615" };
function view(): ComputerTaskView {
  return { sessionId: "original-chat", connectionId: key.connectionId, taskId: key.taskId, featureEnabled: true,
    request: { key, phase: "approved", purpose: "Owned task", scopeIntent: "Original fixture", surfaces: ["desktop"], capabilities: ["observe"], remainingMs: 60_000 },
    pickerSurfaces: ["desktop"], grants: [{ key, runId: address.runId, members: [{ targetId: "fixture-window", surface: "desktop" }], capabilities: ["observe"], suspended: true, executable: false }],
    executions: [{ ...address, phase: "paused", target: { targetId: "fixture-window", surface: "desktop" }, targetName: "Original fixture", cleanupPending: false, resumeAvailable: true, hasFreshObservation: false, lastFailure: null }] };
}
function snapshot(next = view(), state: ComputerTaskSnapshot["state"] = "live"): ComputerTaskSnapshot {
  return { state, view: next, owner: { sessionId: next.sessionId, connectionId: key.connectionId, subscriptionId: "original-stream", hostInstanceId: "original-host" } };
}
function deferred<T>() {
  let resolve!: (value: T) => void, reject!: (reason: Error) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}
function fixture(next = view()) {
  const deps = { resume: vi.fn(async (): Promise<ComputerTaskResumeOutcome> => ({ phase: "resumed", address: successor })),
    pause: vi.fn(async () => {}), decide: vi.fn(async (sessionId: string, decidedKey: typeof key, decision: "stop" | "cancel" | "deny") => ({ sessionId, key: decidedKey, decision })),
    begin: vi.fn(async () => ({ sessionId: "original-chat", key, pickerId: "fixture-picker", maxSelection: 1, surface: "desktop" as const })),
    load: vi.fn(async () => ({ sessionId: "original-chat", key, pickerId: "fixture-picker", maxSelection: 1, surface: "desktop" as const, candidates: [], truncated: false })),
    preparation: vi.fn(), confirm: vi.fn(),
    cancel: vi.fn(async () => true), approve: vi.fn(async () => ({ requestId: key.requestId, revision: key.revision, runId: address.runId, decisionClass: "synthetic" })),
    managedApprove: vi.fn(async () => ({ requestId: key.requestId, revision: key.revision, runId: address.runId, decisionClass: "synthetic" })),
  };
  const actions = new ComputerTaskActions("original-chat", deps);
  actions.update(snapshot(next));
  return { deps, actions };
}
function resumed(): ComputerTaskView {
  const next = view();
  next.executions[0] = { ...next.executions[0], ...successor, phase: "idle", resumeAvailable: false, hasFreshObservation: true };
  next.grants[0] = { ...next.grants[0], suspended: false, executable: true };
  return next;
}

it("deduplicates Continue and waits for new Host authority rather than its receipt", async () => {
  const f = fixture();
  await Promise.all([f.actions.resume(address), f.actions.resume(address)]);
  expect(f.deps.resume).toHaveBeenCalledExactlyOnceWith("original-chat", address);
  expect(f.actions.getSnapshot()).toMatchObject({ pending: "resume", awaitingHost: true });
  const incomplete = resumed(); incomplete.executions[0].hasFreshObservation = false;
  f.actions.update(snapshot(incomplete));
  expect(f.actions.getSnapshot().pending).toBe("resume");
  f.actions.update(snapshot(resumed()));
  expect(f.actions.getSnapshot()).toMatchObject({ pending: null, error: null, awaitingHost: false });
  expect(f.deps.begin).not.toHaveBeenCalled(); expect(f.deps.approve).not.toHaveBeenCalled();
});

it("Stop overtakes an in-flight Continue and isolates its late failure", async () => {
  const f = fixture(); const reply = deferred<ComputerTaskResumeOutcome>();
  f.deps.resume.mockReturnValueOnce(reply.promise);
  const continuing = f.actions.resume(address);
  await f.actions.decide("stop");
  reply.reject(new Error("late recovery error")); await continuing;
  expect(f.actions.getSnapshot()).toMatchObject({ pending: "stop", error: null });
  expect(f.deps.decide).toHaveBeenCalledExactlyOnceWith("original-chat", key, "stop");
});

it("new Host authority settles Continue before a late receipt or transport error", async () => {
  const f = fixture(); const reply = deferred<ComputerTaskResumeOutcome>();
  f.deps.resume.mockReturnValueOnce(reply.promise);
  const continuing = f.actions.resume(address);
  f.actions.update(snapshot(resumed()));
  reply.reject(new Error("obsolete reply")); await continuing;
  expect(f.actions.getSnapshot()).toMatchObject({ pending: null, error: null });
});

it("failed native recovery settles an in-progress receipt without another user click", async () => {
  const f = fixture(); f.deps.resume.mockResolvedValueOnce({ phase: "in_progress" });
  await f.actions.resume(address);
  const preparing = view(); preparing.executions[0].phase = "resuming";
  f.actions.update(snapshot(preparing));
  f.actions.update(snapshot(view()));
  expect(f.actions.getSnapshot()).toMatchObject({ pending: null, error: "resume", awaitingHost: false });
});

it.each(["run", "task", "generation", "noncanonical", "overflow"])("rejects a changed %s in a recovery receipt", async (field) => {
  const f = fixture(); const changed = { ...successor, key: { ...key } };
  if (field === "run") changed.runId = "other-run";
  if (field === "task") changed.key.taskId = "other-task";
  if (field === "generation") changed.generation = address.generation;
  if (field === "noncanonical") changed.generation = "0002";
  if (field === "overflow") changed.generation = "18446744073709551616";
  f.deps.resume.mockResolvedValueOnce({ phase: "resumed", address: changed });
  await f.actions.resume(address);
  expect(f.actions.getSnapshot()).toMatchObject({ pending: null, error: "resume" });
});

it.each(["unavailable", "feature", "grant", "running", "stale"])("never requests recovery for %s metadata", async (reason) => {
  const next = view();
  if (reason === "unavailable") next.executions[0].resumeAvailable = false;
  if (reason === "feature") next.featureEnabled = false;
  if (reason === "grant") next.grants = [];
  if (reason === "running") next.executions[0].phase = "idle";
  const f = fixture(next);
  if (reason === "stale") f.actions.update(snapshot(next, "stale"));
  await f.actions.resume(address);
  expect(f.deps.resume).not.toHaveBeenCalled();
});

it("a replacement task cannot receive an old recovery failure or automatic retry", async () => {
  const f = fixture(); const reply = deferred<ComputerTaskResumeOutcome>();
  f.deps.resume.mockReturnValueOnce(reply.promise);
  const continuing = f.actions.resume(address);
  const next = view(); next.taskId = "replacement-task";
  f.actions.update(snapshot(next));
  reply.reject(new Error("obsolete")); await continuing;
  expect(f.actions.getSnapshot()).toMatchObject({ pending: null, error: null });
  expect(f.deps.resume).toHaveBeenCalledTimes(1);
});

it.each(["stop_requested", "stopped", "target_closed"] as const)("Host %s wins before the recovery reply", async (phase) => {
  const f = fixture(); const reply = deferred<ComputerTaskResumeOutcome>();
  f.deps.resume.mockReturnValueOnce(reply.promise);
  const continuing = f.actions.resume(address);
  const next = view(); next.executions[0].phase = phase; next.executions[0].resumeAvailable = false;
  f.actions.update(snapshot(next));
  reply.reject(new Error("obsolete recovery")); await continuing;
  expect(f.actions.getSnapshot()).toMatchObject({ pending: null, error: null });
});
