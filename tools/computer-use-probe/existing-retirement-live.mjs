import assert from "node:assert/strict";

export async function verifyTombstoneRetirement(env) {
  const { getWorker, fixture, rpc, until, enqueue, result, idle, stats, popup, setStage, setStep, pass } = env;
  setStage("expired-completion-tombstone-cleans-without-replay"); setStep("dispatch-once");
  const before = await stats(); const clicks = await fixture.evaluate(() => window.clicks);
  await getWorker().evaluate(() => {
    const faults = globalThis.__cuDispatchFaults;
    faults.loseSettle = true; faults.delayRetirement = true;
  });
  await enqueue("Count", "click", {});
  await until(() => getWorker().evaluate(() => typeof globalThis.__cuDispatchFaults.releaseRetirement === "function"));
  assert.equal(await fixture.evaluate(() => window.clicks), clicks + 1, "the original action must execute exactly once");
  assert.equal((await stats()).settlements, before.settlements + 1, "Host must have accepted the lost settlement");
  assert.equal((await rpc("dispatch-state")).idle, false, "lost cleanup reply still owns the business result");

  setStep("expire-host-tombstone");
  assert((await rpc("dispatch-expire-completion-tombstones")).ok);
  setStep("release-retirement-query");
  await getWorker().evaluate(() => globalThis.__cuDispatchFaults.releaseRetirement());
  setStep("await-retired-action-result");
  const reply = await result(); assert(reply.ok); assert.equal(reply.outcome.status, "applied");
  setStep("await-retired-host-idle"); await idle();
  await until(() => popup.evaluate(async () => !(await chrome.storage.local.get("cuActionCleanup")).cuActionCleanup));
  const after = await stats();
  assert.equal(after.claims, before.claims + 1); assert.equal(after.settlements, before.settlements + 1);
  assert.equal(after.retirements, before.retirements + 1);
  assert.equal(await fixture.evaluate(() => window.clicks), clicks + 1, "terminal cleanup must not replay the action");

  setStep("fresh-action-after-absent-terminal");
  await enqueue("Count", "click", {}); assert((await result()).ok); await idle();
  assert.equal(await fixture.evaluate(() => window.clicks), clicks + 2); pass();
}
