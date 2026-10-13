// Actual source MV3/new TaskGrant/HTTP effects. Host gestures and ACP are synthetic.
import assert from "node:assert/strict";
import { resumeOriginalConcurrently } from "./existing-task-recovery-race-live.mjs";
import { interruptOriginalRecovery } from "./existing-task-recovery-terminal-live.mjs";
import { replaceOriginalRecoveryOwner } from "./existing-task-recovery-owner-live.mjs";

export async function verifyOriginalHostRecovery({ rpc, call, decode, until, observe, identity,
  fixture, fixtureId, memberB, memberC, selectedRun, worker, initial,
  noMemberCReadOrInput, pass, phase, concurrent = false, onRaceEvidence,
  interruption, onTerminalEvidence, ownerScenario, onOwnerEvidence }) {
  const unchanged = async () => {
    for (const page of [fixture, memberB.page, memberC.page]) {
      assert.equal(page.isClosed(), false);
      assert.equal(await page.evaluate(() => globalThis.clicks), 0);
      assert.equal(await page.locator("#input").inputValue(), "");
      assert.equal(await page.locator("#scroll").evaluate(element => element.scrollTop), 0);
    }
    await noMemberCReadOrInput();
  };
  const dispatchCounts = () => worker.evaluate(() => ({
    reads: globalThis.__jointCalls.filter(row => row.function === "observeDocument").length,
    inputs: globalThis.__jointCalls.filter(row => row.function === "actDocument").length,
    captures: globalThis.__jointCaptures,
  }));
  let stage = "original-grant-fresh-observation-no-input";
  phase(stage, "old-original-observation");
  const old = await observe();
  assert(old.text.includes("Owned visible fixture 你好"));
  const oldClick = { ...identity(old, "original-before-pause", "Fixture action"), action: "click", parameters: {} };
  const before = await dispatchCounts();
  assert.equal(before.captures, 0);
  await unchanged(); pass(stage);

  stage = "original-host-pause-denies-model-dispatch";
  phase(stage, "original-manager-pause");
  const paused = await rpc("host-pause");
  assert.equal(paused.ok, true);
  assert.equal(paused.executable, 0);
  assert.equal(paused.continuations, 1);
  assert.equal(paused.pendingContinuation, false);
  assert.equal(paused.state.executions[0].phase, "paused");
  assert.equal(paused.state.executions[0].hasFreshObservation, false);
  assert.deepEqual(paused.state.executions[0].key, initial.state.request.key);
  assert.equal(paused.state.executions[0].runId, selectedRun.runId);
  for (const screenshot of [false, true]) {
    assert.equal((await call("computer_observe", { ...selectedRun, screenshot })).isError, true);
  }
  assert.equal((await call("computer_act", oldClick)).isError, true);
  assert.equal((await call("computer_open_target", { runId: selectedRun.runId, targetId: `tab:${memberB.id}` })).isError, true);
  assert.deepEqual(await dispatchCounts(), before);
  await unchanged(); pass(stage);

  if (ownerScenario) {
    onOwnerEvidence(await replaceOriginalRecoveryOwner({ scenario: ownerScenario, rpc, call, worker,
      until, fixtureId, members: [{ id: fixtureId, page: fixture }, memberB, memberC], selectedRun,
      initial, paused, oldClick, dispatchCounts, unchanged, pass, phase }));
    return;
  }

  if (interruption) {
    const evidence = await interruptOriginalRecovery({ rpc, call, worker, until, fixtureId,
      members: [{ id: fixtureId, page: fixture }, memberB, memberC], selectedRun, initial, oldClick,
      dispatchCounts, unchanged, interruption, pass, phase });
    onTerminalEvidence(evidence);
    if (interruption === "chat-stop") return;
  }

  stage = "original-host-resume-fresh-semantic-observation-once";
  phase(stage, "original-manager-resume");
  const resumed = concurrent
    ? await resumeOriginalConcurrently({ rpc, worker, until, fixtureId, pass, phase })
    : await rpc("host-resume");
  if (concurrent) onRaceEvidence(resumed.raceEvidence);
  assert.equal(resumed.ok, true, "original Host must resume a semantic ExistingTab task without capture permission");
  assert.equal(resumed.outcome.phase, "resumed");
  assert.equal(resumed.outcome.address.runId, selectedRun.runId);
  assert.deepEqual(resumed.outcome.address.key, initial.state.request.key);
  assert.notEqual(resumed.outcome.address.generation, paused.state.executions[0].generation);
  assert.equal(resumed.state.executions[0].hasFreshObservation, true);
  assert.equal(resumed.executable, 1);
  assert.equal(resumed.continuations, 2);
  assert.equal(resumed.pendingContinuation, true);
  assert.equal(resumed.state.grants.length, 1);
  assert.deepEqual(resumed.state.grants[0].members, paused.state.grants[0].members);
  const captured = await dispatchCounts();
  assert.equal(captured.reads, before.reads + (interruption ? 2 : 1));
  assert.equal(captured.inputs, before.inputs);
  assert.equal(captured.captures, 0);
  phase(stage, "original-address-duplicate-rejected");
  const duplicate = await rpc("host-resume-duplicate");
  assert.equal(duplicate.ok, true);
  assert.equal(duplicate.continuations, 2);
  assert.deepEqual(await dispatchCounts(), captured);
  assert.equal((await rpc("delivery")).ok, true);
  assert.equal((await rpc("host-recovery-state")).pendingContinuation, false);
  await unchanged(); pass(stage);

  stage = "resumed-old-refs-denied-fresh-click-deduped";
  phase(stage, "pre-pause-ref-denied");
  assert.equal((await call("computer_act", { ...oldClick, actionId: "old-after-host-resume" })).isError, true);
  assert.deepEqual(await dispatchCounts(), captured);
  const fresh = await observe();
  assert(fresh.targetGeneration > old.targetGeneration);
  assert.notEqual(fresh.snapshotId, old.snapshotId);
  assert.equal(fresh.image.pngBase64, undefined);
  // A DOM recovery does not expand the adapter's semantic-only input capability.
  const coordinate = { ...identity(fresh, "semantic-recovery-no-coordinates", "Fixture action"),
    action: "click", target: { x: 10, y: 10 }, parameters: {} };
  const coordinateReply = await call("computer_act", coordinate);
  assert.equal(coordinateReply.isError, true);
  assert(coordinateReply.content.some(row => row.type === "text"
    && row.text.includes("unsupported action Click for backend chromium-extension; not dispatched")));
  assert.deepEqual(await dispatchCounts(), { ...captured, reads: captured.reads + 1 });
  const click = { ...identity(fresh, "fresh-after-host-resume", "Fixture action"), action: "click", parameters: {} };
  const clicked = decode(await call("computer_act", click));
  assert.equal(clicked.kind, "applied");
  assert.deepEqual(decode(await call("computer_act", click)), clicked);
  assert.equal(await fixture.evaluate(() => globalThis.clicks), 1);
  await noMemberCReadOrInput(); pass(stage);

  stage = "resumed-original-group-switch-retains-unselected-isolation";
  phase(stage, "original-group-member-b");
  await memberB.page.bringToFront();
  phase(stage, "recovered-group-b-open-request");
  const openReply = await call("computer_open_target", { runId: selectedRun.runId, targetId: `tab:${memberB.id}` });
  phase(stage, "recovered-group-b-open-response");
  const permissionCode = reply => ["target_unauthorized", "identity_mismatch", "dead_target", "lease_held",
    "adapter", "timeout", "rate_limited", "feature_disabled", "stop_requested"].includes(reply.code) ? reply.code : "other";
  process.stderr.write(`original recovered B open witness: error=${openReply.isError === true}, code=${permissionCode(openReply)}\n`);
  const opened = decode(openReply);
  assert.equal(opened.targetId, `tab:${memberB.id}`);
  selectedRun.targetId = opened.targetId;
  phase(stage, "recovered-group-b-old-ref-denied");
  assert.equal((await call("computer_act", { ...click, actionId: "a-ref-after-resumed-switch" })).isError, true);
  phase(stage, "recovered-group-b-observe-request");
  const bReply = await call("computer_observe", { ...selectedRun, screenshot: false });
  phase(stage, "recovered-group-b-observe-response");
  process.stderr.write(`original recovered B observation witness: error=${bReply.isError === true}, code=${permissionCode(bReply)}\n`);
  phase(stage, "recovered-group-b-observe-text");
  assert(decode(bReply).text.includes("member B"));
  assert.equal(await fixture.evaluate(() => globalThis.clicks), 1);
  assert.equal(await memberB.page.evaluate(() => globalThis.clicks), 0);
  await noMemberCReadOrInput(); pass(stage);

  stage = "resumed-stop-keeps-real-shares-and-tabs";
  phase(stage, "original-run-stop");
  decode(await call("computer_stop", selectedRun));
  await until(async () => (await rpc("stopped", { runId: selectedRun.runId })).ok);
  assert.equal((await rpc("shared-count")).count, 3);
  assert.equal((await rpc("host-recovery-state")).pendingContinuation, false);
  for (const page of [fixture, memberB.page, memberC.page]) assert.equal(page.isClosed(), false);
  assert.equal((await call("computer_observe", { ...selectedRun, screenshot: false })).isError, true);
  assert.equal(await fixture.evaluate(() => globalThis.clicks), 1);
  assert.equal(await memberB.page.evaluate(() => globalThis.clicks), 0);
  await noMemberCReadOrInput(); pass(stage);
}
