import { expect, it, vi } from "vitest";
import { ComputerTaskUploadChoice } from "./taskUpload";
import type { ComputerTaskExecution, ComputerTaskUploadCompletion } from "@/lib/api/computerTask";
import type { ComputerTaskSnapshot } from "./taskSource";

const key = { connectionId: "connection", taskId: "task", requestId: "scope-request", revision: 1 };
const run = (): ComputerTaskExecution => ({ key, runId: "run", generation: "18446744073709551614", phase: "idle",
  target: { targetId: "page", surface: "app-webview" }, targetName: "Fixture", cleanupPending: false,
  resumeAvailable: false, hasFreshObservation: true, lastFailure: null,
  upload: { requestId: "file-request", actionId: "file-action", phase: "awaiting_file", targetName: "Fixture", fieldName: "File" },
});
function snapshot(execution = run()): ComputerTaskSnapshot {
  return { state: "live", owner: { hostInstanceId: "host", subscriptionId: "sub", sessionId: "chat", connectionId: "connection" },
    view: { sessionId: "chat", connectionId: "connection", taskId: "task", featureEnabled: true, pickerSurfaces: [], request: null,
      executions: [execution], grants: [{ key, runId: "run", members: [execution.target!], capabilities: ["input"], suspended: false, executable: true }] } };
}
function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (value: Error) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}
const completion = (kind: "verified" | "unknown" | "rejected" = "verified"): ComputerTaskUploadCompletion => ({
  outcome: { runId: "run", actionId: "file-action", kind, executed: kind === "verified", reason: kind === "rejected" ? "user cancelled file selection" : null },
  bytes: kind === "verified" ? 0 : null,
});
function fixture() {
  const pending = deferred<ComputerTaskUploadCompletion>();
  const deps = { choose: vi.fn(() => pending.promise), cancel: vi.fn(async () => completion("rejected")) };
  const actions = new ComputerTaskUploadChoice("chat", deps);
  actions.update(snapshot());
  return { actions, deps, pending };
}

it("shares one original chooser and passes only exact Host correlation fields", async () => {
  const { actions, deps, pending } = fixture();
  const call = actions.choose(run(), "Choose a file");
  await actions.choose(run(), "Second picker");
  expect(deps.choose).toHaveBeenCalledExactlyOnceWith("chat", { key, runId: "run", generation: run().generation }, "file-request", "Choose a file");
  pending.resolve(completion());
  await call;
  expect(actions.getSnapshot()).toMatchObject({ pending: false, result: "assigned" });
});

it.each(["stale", "paused", "stopped", "owner", "feature", "generation", "request"])("fences late chooser after %s without releasing its physical owner", async cause => {
  const { actions, deps, pending } = fixture();
  const call = actions.choose(run(), "Choose");
  let next = snapshot();
  if (cause === "stale") next = { ...next, state: "stale" };
  if (cause === "owner") next = { ...next, owner: { ...next.owner!, hostInstanceId: "replacement" } };
  if (cause === "feature") next.view!.featureEnabled = false;
  if (cause === "paused" || cause === "stopped") next.view!.executions[0].phase = cause;
  if (cause === "generation") next.view!.executions[0].generation = "18446744073709551615";
  if (cause === "request") next.view!.executions[0].upload!.requestId = "new-file-request";
  actions.update(next);
  expect(actions.getSnapshot()).toMatchObject({ pending: true, choice: null, result: null });
  expect(deps.cancel).toHaveBeenCalledExactlyOnceWith("chat", { key, runId: "run", generation: run().generation }, "file-request");
  actions.update(snapshot());
  await actions.choose(run(), "Do not reopen");
  expect(deps.choose).toHaveBeenCalledTimes(1);
  pending.resolve(completion());
  await call;
  expect(actions.getSnapshot()).toMatchObject({ pending: false, result: null });
});

it("explicit cancel cannot create a new chooser while the original one is open", async () => {
  const { actions, deps, pending } = fixture();
  const call = actions.choose(run(), "Choose");
  await actions.cancel(run());
  expect(actions.getSnapshot()).toMatchObject({ pending: true, result: "cancelled" });
  actions.release();
  expect(actions.hasPendingCall()).toBe(true);
  await actions.choose(run(), "No duplicate");
  expect(deps.choose).toHaveBeenCalledTimes(1);
  pending.resolve(completion("rejected"));
  await call;
  expect(actions.hasPendingCall()).toBe(false);
});

it.each(["foreign", "missing-bytes", "unknown", "error"])("does not claim assignment from %s reply", async kind => {
  const { actions, pending } = fixture();
  const call = actions.choose(run(), "Choose");
  const result = completion();
  if (kind === "foreign") result.outcome.runId = "other";
  if (kind === "missing-bytes") result.bytes = null;
  if (kind === "unknown") result.outcome.kind = "unknown";
  if (kind === "error") pending.reject(new Error("native picker failed"));
  else pending.resolve(result);
  await call;
  expect(actions.getSnapshot().result).toBe(kind === "unknown" ? "unknown" : "failed");
});

it("rejects forged and old UI rows before calling the Host", async () => {
  const { actions, deps } = fixture();
  await actions.choose({ ...run(), generation: "3" }, "Choose");
  await actions.choose({ ...run(), upload: { ...run().upload!, actionId: "different" } }, "Choose");
  actions.update({ ...snapshot(), state: "stale" });
  await actions.choose(run(), "Choose");
  await actions.cancel(run());
  expect(deps.choose).not.toHaveBeenCalled();
  expect(deps.cancel).not.toHaveBeenCalled();
});

it("cannot cancel assignment using an older awaiting-file presentation", async () => {
  const { actions, deps } = fixture();
  actions.update(snapshot({ ...run(), upload: { ...run().upload!, phase: "assigning" } }));
  await actions.cancel(run());
  expect(deps.cancel).not.toHaveBeenCalled();
});
