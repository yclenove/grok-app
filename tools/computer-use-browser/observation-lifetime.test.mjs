import test from "node:test";
import assert from "node:assert/strict";
import { createObservationState, disposeObservationState, invalidateObservationState,
  resolveObservationTarget, withObservationLease } from "./observation-state.mjs";
import { replacePageObservation, runPageMutation } from "./page-state.mjs";

function deferred() {
  let resolve;
  const promise = new Promise(done => { resolve = done; });
  return { promise, resolve };
}

function fixture({ rejects = false } = {}) {
  const handle = { calls: 0, async dispose() { this.calls += 1; if (rejects) throw new Error("closed"); } };
  const obs = createObservationState(1, [0, 1].map(i => ({
    target: { handle }, role: "button", name: `button-${i}`,
  })));
  const identity = { pageGeneration: 1, snapshotId: obs.snapshotId, elementRef: obs.nodes[0].elementRef };
  return { handle, obs, identity };
}

test("retirement revokes refs immediately and disposes each shared handle once", async () => {
  for (const rejects of [false, true]) {
    const { handle, obs, identity } = fixture({ rejects });
    invalidateObservationState(obs);
    assert.throws(() => resolveObservationTarget(obs, identity), error => error.workerCode === "observation_required");
    await Promise.all([disposeObservationState(obs), disposeObservationState(obs)]);
    assert.equal(handle.calls, 1);
  }
});

test("retired handles remain pinned until the last physical operation exits", async () => {
  const { handle, obs, identity } = fixture();
  const firstGate = deferred(); const secondGate = deferred();
  const first = withObservationLease(obs, identity, async () => firstGate.promise);
  const second = withObservationLease(obs, identity, async () => secondGate.promise);
  invalidateObservationState(obs);
  await assert.rejects(withObservationLease(obs, identity, async () => assert.fail("retired lease admitted")),
    error => error.workerCode === "observation_required");
  assert.equal(handle.calls, 0);
  firstGate.resolve(); await first;
  assert.equal(handle.calls, 0);
  secondGate.resolve(); await second;
  assert.equal(handle.calls, 1);
});

test("success, cancellation and failure release an admitted mutation lease", async () => {
  for (const outcome of ["success", "cancelled", "failed"]) {
    const { handle, obs, identity } = fixture();
    const state = { generation: 1, page: { url: () => "about:blank", isClosed: () => false }, observation: obs };
    const pending = withObservationLease(obs, identity, async () => runPageMutation(state, async () => {
      assert.equal(handle.calls, 0, "mutation invalidation disposed a live action handle");
      assert.throws(() => resolveObservationTarget(obs, identity));
      if (outcome !== "success") throw new Error(outcome);
      return "applied";
    }, obs));
    if (outcome === "success") assert.equal(await pending, "applied");
    else await assert.rejects(pending, new RegExp(outcome));
    assert.equal(handle.calls, 1);
  }
});

test("replacement during preparation rejects dispatch but not the replacement snapshot", async () => {
  const { handle, obs, identity } = fixture();
  const replacement = fixture();
  const state = { generation: 1, page: { url: () => "about:blank", isClosed: () => false }, observation: obs };
  const gate = deferred();
  const pending = withObservationLease(obs, identity, async () => {
    await gate.promise;
    return runPageMutation(state, async () => assert.fail("stale preparation dispatched"), obs);
  });
  replacePageObservation(state, replacement.obs);
  assert.equal(handle.calls, 0);
  gate.resolve();
  await assert.rejects(pending, error => error.workerCode === "stale_snapshot");
  assert.equal(handle.calls, 1);
  assert.equal(replacement.handle.calls, 0);
  assert.equal(state.observation, replacement.obs);
  await disposeObservationState(replacement.obs);
});
