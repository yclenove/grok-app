// Fixed native acceptance only. Pause original owned MV3, never edit its clock/key.
import assert from "node:assert/strict";
import { setTimeout as delay } from "node:timers/promises";
import { pauseOwnedWorker } from "./existing-task-worker-pause.mjs";

export async function preparePreapprovalLease({ context, popup, worker, rpc, registerPause, phase }) {
  const disconnects = await worker.evaluate(() => globalThis.__jointDisconnectRequests);
  assert.equal(disconnects, 0, "no disconnect request before original worker pause");
  phase("pause-original-worker-outside-approval-barrier");
  const paused = await pauseOwnedWorker(context, popup, worker.url());
  registerPause(paused); // Failure cleanup must own it before the first witness RPC.
  const started = performance.now();
  const witness = async () => {
    const value = await rpc("pending-lease-witness");
    assert.equal(typeof value.leaseStored, "boolean");
    assert(Number.isInteger(value.leaseRemainingMs) && value.leaseRemainingMs >= 0 && value.leaseRemainingMs <= 30000);
    assert(Number.isInteger(value.leaseStoredShares) && value.leaseStoredShares >= 0 && value.leaseStoredShares <= 3);
    assert.equal(value.leaseMs, 30000);
    return value;
  };
  const first = await witness();
  assert.equal(first.leaseStored, true);
  assert.equal(first.leaseStoredShares, 3);
  assert(first.leaseRemainingMs > 10000, "original lease must be aged outside the 15-second barrier");
  const evidence = {
    leaseMs: first.leaseMs, pausedRemainingMs: first.leaseRemainingMs,
    disconnectRequestsBeforePause: disconnects,
  };
  phase("age-original-lease-outside-approval-barrier");
  let window;
  while (performance.now() - started <= 32000) {
    window = await witness();
    assert.equal(window.leaseStored, true, "connection retired before original approval entry");
    assert.equal(window.leaseStoredShares, 3);
    assert(window.leaseRemainingMs >= 8000, "original approval entry window missed; do not retry");
    if (window.leaseRemainingMs <= 10000) break;
    await delay(50);
  }
  assert(window.leaseRemainingMs <= 10000);
  evidence.entryWindowRemainingMs = window.leaseRemainingMs;
  return {
    evidence,
    async entered(ready) {
      // ready is the actual original worker gate, not a predicted start timestamp.
      const value = ready || await witness();
      assert.equal(value.leaseStored, true, "lease expired before original worker actually entered");
      assert(value.leaseRemainingMs > 0 && value.leaseRemainingMs <= 10000);
      assert.equal(value.leaseStoredShares, 3);
      evidence.enteredStored = value.leaseStored;
      evidence.enteredRemainingMs = value.leaseRemainingMs;
    },
    async retire() {
      phase("wait-original-production-maintenance-before-release");
      let retired;
      while (performance.now() - started <= 32000) {
        retired = await witness(); // Raw read only; no task view/list/sweep/worker eval.
        if (!retired.leaseStored) break;
        await delay(50);
      }
      assert.equal(retired.leaseStored, false, "original production maintenance did not retire connection");
      assert.equal(retired.leaseRemainingMs, 0);
      assert.equal(retired.leaseStoredShares, 0);
      evidence.pauseToRetirementMs = Math.round(performance.now() - started);
      assert(evidence.pauseToRetirementMs >= first.leaseRemainingMs);
      evidence.retiredStored = retired.leaseStored;
      evidence.retiredRemainingMs = retired.leaseRemainingMs;
      evidence.retiredShares = retired.leaseStoredShares;
      evidence.retiredBeforeResume = true;
    },
    async resume() {
      assert.equal(evidence.retiredBeforeResume, true);
      phase("resume-original-worker-after-retirement-and-rejection");
      await paused.resume();
      registerPause(undefined);
    },
  };
}
