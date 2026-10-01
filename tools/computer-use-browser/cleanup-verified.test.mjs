import test from "node:test";
import assert from "node:assert/strict";
import { finishOwnedCleanup } from "./fixtures/cleanup-verified.mjs";

const phases = ["shutdown", "workerExit", "fixtureClose", "browserExit", "removeProfile"];
function fixture(failures = new Map()) {
  const calls = [];
  const callbacks = Object.fromEntries(phases.map(phase => [phase, async () => {
    calls.push(phase);
    if (failures.has(phase)) throw failures.get(phase);
  }]));
  return { calls, callbacks };
}

test("profile removal requires every independent native cleanup check", async () => {
  const f = fixture();
  await finishOwnedCleanup(f.callbacks);
  assert.deepEqual(f.calls, phases);
});

for (const phase of phases.slice(0, -1)) {
  test(`${phase} failure retains the profile without skipping the remaining checks`, async () => {
    const failure = new Error(`${phase} unconfirmed`);
    const f = fixture(new Map([[phase, failure]]));
    await assert.rejects(finishOwnedCleanup(f.callbacks), error => {
      assert.ok(error instanceof AggregateError);
      assert.deepEqual(error.errors, [failure]);
      return true;
    });
    assert.deepEqual(f.calls, phases.slice(0, -1));
  });
}

test("later cleanup errors cannot mask the original shutdown failure", async () => {
  const original = new Error("shutdown pending");
  const later = new Error("native browser still alive");
  const f = fixture(new Map([["shutdown", original], ["browserExit", later]]));
  await assert.rejects(finishOwnedCleanup(f.callbacks), error => {
    assert.deepEqual(error.errors, [original, later]);
    return true;
  });
  assert.deepEqual(f.calls, phases.slice(0, -1));
});

test("profile removal failure stays visible after confirmed native shutdown", async () => {
  const denied = new Error("profile removal denied");
  const f = fixture(new Map([["removeProfile", denied]]));
  await assert.rejects(finishOwnedCleanup(f.callbacks), error => error === denied);
  assert.deepEqual(f.calls, phases);
});
