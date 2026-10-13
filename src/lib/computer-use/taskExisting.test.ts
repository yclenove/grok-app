import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { ComputerTaskActions } from "./taskActions";
import type { ComputerTaskSnapshot } from "./taskSource";
import type { ComputerTaskPickerPreparation, ComputerTaskPickerView } from "@/lib/api/computerTaskPicker";

const key = { connectionId: "synthetic", taskId: "task", requestId: "request", revision: 1 };
const picker = { sessionId: "chat", key, pickerId: "original-picker", maxSelection: 2, surface: "existing-tabs" as const };
const host = (): ComputerTaskSnapshot => ({ state: "live", owner: { sessionId: "chat", connectionId: key.connectionId, subscriptionId: "stream", hostInstanceId: "host" }, view: {
  sessionId: "chat", connectionId: key.connectionId, taskId: key.taskId, featureEnabled: false, grants: [], executions: [], pickerSurfaces: ["existing-tabs"],
  request: { key, phase: "pending", purpose: "Owned tab", scopeIntent: "Shared fixture only", capabilities: ["observe", "input"], surfaces: ["existing-tabs"], remainingMs: 60_000 },
} });
let pairingExpiresAtMs = 0;
const challenge = () => ({ nonce: "original-nonce", instanceId: "instance", verificationCode: "123456", endpoint: "http://127.0.0.1:32100", installedExtensionId: "extension", expiresAtMs: pairingExpiresAtMs });
const prep = (patch: Partial<ComputerTaskPickerPreparation> = {}): ComputerTaskPickerPreparation => ({ ...picker, pairing: challenge(), pairingConfirmed: false, ready: false, availableTabs: 0, incompatibleTabs: 0, borrowedTabs: 0, ...patch });
const rows = (): ComputerTaskPickerView => ({ ...picker, truncated: false, candidates: [{ candidateId: "tab@original", title: "Shared fixture", appName: "Browser", surface: "existing-tabs", kind: "tab" }] });
function deferred<T>() { let resolve!: (value: T) => void; const promise = new Promise<T>(yes => { resolve = yes; }); return { promise, resolve }; }
function fixture(initial = prep()) {
  const deps = {
    begin: vi.fn(async () => picker), load: vi.fn(async () => rows()), cancel: vi.fn(async () => true),
    preparation: vi.fn(async () => initial), confirm: vi.fn(async () => {}),
    approve: vi.fn(async () => ({ requestId: key.requestId, revision: 1, runId: "run", decisionClass: "synthetic" })),
    managedApprove: vi.fn(), pause: vi.fn(), resume: vi.fn(),
    decide: vi.fn(async (sessionId: string, decidedKey: typeof key, decision: "stop" | "deny" | "cancel") => ({ sessionId, key: decidedKey, decision })),
  };
  const actions = new ComputerTaskActions("chat", deps); actions.update(host());
  return { actions, deps };
}
beforeEach(() => { vi.useFakeTimers(); pairingExpiresAtMs = Date.now() + 60_000; });
afterEach(() => vi.useRealTimers());

it("ExistingTab begins preparation, never discovery, confirm or approval before Pair/Share", async () => {
  const f = fixture(); await f.actions.choose("existing-tabs", "card");
  expect(f.deps.preparation).toHaveBeenCalledExactlyOnceWith(picker);
  expect(f.deps.load).not.toHaveBeenCalled(); expect(f.deps.confirm).not.toHaveBeenCalled(); expect(f.deps.approve).not.toHaveBeenCalled();
  expect(f.actions.getSnapshot()).toMatchObject({ pending: null, choosing: true, preparation: prep(), pickerOwner: "card" });
  f.actions.dispose();
});

it("a ready Share loads candidate IDs but still needs explicit selection and approval", async () => {
  const f = fixture(prep({ pairing: null, ready: true, availableTabs: 1 }));
  await f.actions.choose("existing-tabs", "card");
  expect(f.deps.load).toHaveBeenCalledExactlyOnceWith(picker); expect(f.deps.approve).not.toHaveBeenCalled();
  await f.actions.approve(); expect(f.deps.approve).not.toHaveBeenCalled();
  f.actions.toggleCandidate("tab@original"); await f.actions.approve();
  expect(f.deps.approve).toHaveBeenCalledExactlyOnceWith(rows(), ["tab@original"]);
  expect(f.actions.getSnapshot()).toMatchObject({ pending: "approve", awaitingHost: true });
  f.actions.dispose();
});

it("only the original visible owner polls, hidden replies cannot discover or publish", async () => {
  const f = fixture(); await f.actions.choose("existing-tabs", "card");
  f.actions.setPickerVisible("panel", true); await vi.advanceTimersByTimeAsync(2000);
  expect(f.deps.preparation).toHaveBeenCalledTimes(1);
  const reply = deferred<ComputerTaskPickerPreparation>(); f.deps.preparation.mockReturnValueOnce(reply.promise);
  f.actions.setPickerVisible("card", true); await vi.advanceTimersByTimeAsync(1100);
  const before = f.actions.getSnapshot().preparation;
  f.actions.setPickerVisible("card", false);
  reply.resolve(prep({ pairing: null, ready: true, availableTabs: 1 })); await Promise.resolve(); await Promise.resolve();
  expect(f.actions.getSnapshot().preparation).toBe(before); expect(f.deps.load).not.toHaveBeenCalled();
  await vi.advanceTimersByTimeAsync(5000); expect(f.deps.preparation).toHaveBeenCalledTimes(2);
  f.actions.setPickerVisible("card", true); await vi.advanceTimersByTimeAsync(1100);
  expect(f.deps.preparation).toHaveBeenCalledTimes(3); f.actions.dispose();
});

it("explicit Pair confirm uses original nonce, deduplicates and never auto-approves", async () => {
  const f = fixture(); await f.actions.choose("existing-tabs", "card");
  f.deps.preparation.mockResolvedValueOnce(prep({ pairingConfirmed: true }));
  await Promise.all([f.actions.confirmPairing(), f.actions.confirmPairing()]);
  expect(f.deps.confirm).toHaveBeenCalledExactlyOnceWith(picker, "original-nonce");
  expect(f.actions.getSnapshot().preparation?.pairingConfirmed).toBe(true);
  expect(f.deps.load).not.toHaveBeenCalled(); expect(f.deps.approve).not.toHaveBeenCalled(); f.actions.dispose();
});

it("readiness racing Unshare retries an empty receipt on the same picker without new authority", async () => {
  const f = fixture(prep({ pairing: null, ready: true, availableTabs: 1 }));
  f.deps.load.mockResolvedValueOnce({ ...rows(), candidates: [] });
  await f.actions.choose("existing-tabs", "card");
  await f.actions.refreshPreparation();
  expect(f.deps.begin).toHaveBeenCalledTimes(1); expect(f.deps.load).toHaveBeenCalledTimes(2);
  expect(f.actions.getSnapshot().picker).toEqual(rows()); expect(f.deps.approve).not.toHaveBeenCalled(); f.actions.dispose();
});

it.each(["close", "stop", "replace", "release"] as const)("%s fences late original preparation and leaves no timer", async kind => {
  const f = fixture(); const reply = deferred<ComputerTaskPickerPreparation>(); f.deps.preparation.mockReturnValueOnce(reply.promise);
  const release = f.actions.subscribe(vi.fn()); const choosing = f.actions.choose("existing-tabs", "card"); await Promise.resolve();
  if (kind === "close") f.actions.closePicker();
  if (kind === "stop") await f.actions.decide("stop");
  if (kind === "replace") { const next = host(); next.owner!.hostInstanceId = "next"; f.actions.update(next); }
  if (kind === "release") release();
  reply.resolve(prep({ pairing: null, ready: true, availableTabs: 1 })); await choosing; await vi.advanceTimersByTimeAsync(5000);
  expect(f.deps.cancel).toHaveBeenCalledExactlyOnceWith(picker); expect(f.deps.load).not.toHaveBeenCalled(); expect(f.actions.getSnapshot().preparation).toBeNull();
  expect(vi.getTimerCount()).toBe(0); f.actions.dispose();
});

it("invalid or cross-surface preparation fails closed, without loading or approving", async () => {
  const f = fixture(prep({ surface: "desktop" })); await f.actions.choose("existing-tabs", "card");
  expect(f.deps.load).not.toHaveBeenCalled(); expect(f.deps.approve).not.toHaveBeenCalled();
  expect(f.actions.getSnapshot()).toMatchObject({ error: "preparation", preparation: null }); f.actions.dispose();
});

it("an explicit confirm during a slow metadata read is queued once for the original nonce", async () => {
  const f = fixture(); await f.actions.choose("existing-tabs", "card");
  const reply = deferred<ComputerTaskPickerPreparation>(); f.deps.preparation.mockReturnValueOnce(reply.promise);
  f.actions.setPickerVisible("card", true); await vi.advanceTimersByTimeAsync(1000);
  await Promise.all([f.actions.confirmPairing(), f.actions.confirmPairing()]);
  expect(f.deps.confirm).not.toHaveBeenCalled();
  reply.resolve(prep()); await vi.advanceTimersByTimeAsync(0);
  expect(f.deps.confirm).toHaveBeenCalledExactlyOnceWith(picker, "original-nonce");
  expect(f.deps.preparation).toHaveBeenCalledTimes(3);
  expect(f.deps.load).not.toHaveBeenCalled(); expect(f.deps.approve).not.toHaveBeenCalled(); f.actions.dispose();
});

it.each(["nonce", "hide", "stop", "ready", "failure", "expired"] as const)("%s retires a queued confirm without confirming another challenge", async kind => {
  const f = fixture(); await f.actions.choose("existing-tabs", "card");
  const reply = deferred<ComputerTaskPickerPreparation>(); f.deps.preparation.mockReturnValueOnce(reply.promise);
  f.actions.setPickerVisible("card", true); await vi.advanceTimersByTimeAsync(1000);
  await f.actions.confirmPairing();
  if (kind === "hide") f.actions.setPickerVisible("card", false);
  if (kind === "stop") await f.actions.decide("stop");
  if (kind === "expired") await vi.advanceTimersByTimeAsync(61_000);
  reply.resolve(kind === "nonce" ? prep({ pairing: { ...challenge(), nonce: "replacement" } })
    : kind === "ready" ? prep({ pairing: null, ready: true, availableTabs: 1 })
      : kind === "failure" ? prep({ surface: "desktop" })
        : kind === "expired" ? prep({ pairing: { ...challenge(), expiresAtMs: Date.now() + 60_000 } }) : prep());
  await vi.advanceTimersByTimeAsync(0);
  expect(f.deps.confirm).not.toHaveBeenCalled(); expect(f.deps.approve).not.toHaveBeenCalled(); f.actions.dispose();
});

it("a second gesture during the original confirm never queues a duplicate, even if readback is still unconfirmed", async () => {
  const f = fixture(); await f.actions.choose("existing-tabs", "card");
  const reply = deferred<void>(); f.deps.confirm.mockReturnValueOnce(reply.promise);
  const first = f.actions.confirmPairing(); await f.actions.confirmPairing();
  reply.resolve(); await first;
  expect(f.deps.confirm).toHaveBeenCalledExactlyOnceWith(picker, "original-nonce");
  expect(f.deps.approve).not.toHaveBeenCalled(); f.actions.dispose();
});
