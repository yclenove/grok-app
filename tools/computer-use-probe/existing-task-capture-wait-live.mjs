// Actual original PNG admission with synthetic Host Continue/Stop and ACP peer.
import assert from "node:assert/strict";
import { originalCaptureReturnState, releaseOriginalCaptureReturn } from "./existing-task-capture-gate.mjs";
import { originalPageEffects, expectedPageEffects } from "./existing-task-late-action-spec.mjs";

export async function verifyEarlyCaptureContinue({ stopped, rpc, call, decode, until, observe, identity,
  worker, originalReply, originalDocuments, unchanged, counts, pin, oldAction, selectedRun,
  fixture, memberB, memberC, evidence, pass, step }) {
  const heldCounts = await counts();
  step("early-continue-waits-for-original-capture");
  await until(async () => (await rpc("late-capture-state")).pauseWaiterReady);
  const started = await rpc("late-capture-wait-start");
  assert.equal(started.started, true);
  await until(async () => (await rpc("late-capture-wait-state")).state.executions[0].phase === "resuming");
  const waiting = await rpc("late-capture-wait-held");
  for (const key of ["originalWorkerRetained", "originalWaiterPending", "zeroPeerWire"])
    assert.equal(waiting[key], true);
  assert.equal(waiting.state.executions[0].hasFreshObservation, false);
  assert.equal(waiting.state.executions[0].cleanupPending, true);
  assert.equal(waiting.state.executions[0].generation, started.address.generation);
  assert.deepEqual(await counts(), heldCounts);
  assert.equal((await originalCaptureReturnState(worker)).returned, false);
  await unchanged(originalDocuments);
  evidence.earlyContinueWhileHeld = true;
  evidence.originalPauseWaiterJoined = waiting.originalWaiterJoined;
  evidence.zeroPeerWireWhileHeld = waiting.zeroPeerWire;
  pass("early-continue-waits-for-original-capture");

  step("duplicate-early-continue-shares-reservation");
  const duplicate = await rpc("late-capture-wait-duplicate");
  assert.equal(duplicate.inProgress, true);
  assert.equal(duplicate.originalWorkerRetained, true);
  assert.equal(duplicate.originalWaiterPending, true);
  assert.equal(duplicate.state.executions[0].cleanupPending, true);
  assert.equal((await call("computer_act", oldAction)).isError, true);
  for (const screenshot of [false, true])
    assert.equal((await call("computer_observe", { ...selectedRun, screenshot })).isError, true);
  assert.equal((await rpc("late-capture-replay")).replayChecked, true);
  assert.deepEqual(await counts(), heldCounts);
  const stillHeld = await originalCaptureReturnState(worker);
  assert.equal(stillHeld.returned, false);
  assert.deepEqual(stillHeld.successorReads, []);
  evidence.duplicateSharedReservation = true;
  evidence.zeroNewWorkWhileHeld = true;
  evidence.oldDeliveryNotRevived = true;
  evidence.zeroPeerWire = true;
  pass("duplicate-early-continue-shares-reservation");

  evidence.waitingStop = stopped;
  if (stopped) {
    step("stop-during-original-capture-recovery-wait");
    const fenced = await rpc("late-capture-wait-stop");
    assert.equal(fenced.executable, 0);
    assert.equal(fenced.pendingContinuation, false);
    assert.equal(fenced.state.executions[0].cleanupPending, true);
    assert.equal((await originalCaptureReturnState(worker)).returned, false);
    assert.deepEqual(await counts(), heldCounts);
    evidence.stopFencedBeforeRelease = true;
    pass("stop-during-original-capture-recovery-wait");
  }

  step("original-capture-late-return-rejected-and-joined");
  await releaseOriginalCaptureReturn(worker);
  const reply = await originalReply;
  evidence.originalMcpError = reply.isError === true;
  evidence.originalMcpImage = reply.content.some(row => row.type === "image");
  evidence.originalMcpOldText = reply.content.some(row => row.type === "text" && row.text.includes("Owned visible fixture"));
  await until(async () => {
    const state = await originalCaptureReturnState(worker);
    return state.observeJoined && state.ownerJoined && state.settleJoined
      && !state.settlePending && state.statusPending === 0 && !state.completionPending;
  });
  const joined = await rpc("late-capture-wait-finish");
  evidence.final = await originalCaptureReturnState(worker);
  evidence.counts = await counts();
  evidence.originalWaiterJoined = joined.originalWaiterJoined;
  evidence.originalWorkerJoined = joined.originalWorkerJoined;
  evidence.originalWorkerRetired = joined.originalWorkerRetired;
  evidence.hostIdleAfterJoin = joined.existingIdle;
  process.stderr.write(`original early capture witness: ${JSON.stringify(evidence)}\n`);
  assert.equal(evidence.hostIdleBeforeRelease, false);
  assert.equal(evidence.originalMcpError, true);
  assert.equal(evidence.originalMcpImage, false);
  assert.equal(evidence.originalMcpOldText, false);
  assert.equal(evidence.final.decodes, 0);
  assert.equal(evidence.final.observationReturned, false);
  assert.equal(evidence.final.completionImage, false);
  assert.equal(joined.originalWorkerJoined, true);
  assert.equal(joined.originalWorkerRetired, true);
  assert.equal(joined.originalWaiterJoined, true);
  assert.equal(joined.existingIdle, true);
  assert.equal(joined.state.executions[0].cleanupPending, false);
  await unchanged(originalDocuments);
  pass("original-capture-late-return-rejected-and-joined");

  step("early-continue-fresh-publication-or-terminal-stop");
  evidence.freshRecoveryReads = evidence.counts.reads - heldCounts.reads;
  evidence.freshActions = 0;
  evidence.continuationsDelivered = 0;
  assert.equal(evidence.freshRecoveryReads, Number(!stopped));
  if (stopped) {
    assert.equal(joined.rejected, true);
    assert.equal(joined.executable, 0);
    assert.equal(joined.pendingContinuation, false);
    assert.deepEqual(evidence.final.successorReads, []);
    for (const screenshot of [false, true])
      assert.equal((await call("computer_observe", { ...selectedRun, screenshot })).isError, true);
    assert.equal((await call("computer_act", { ...oldAction, actionId: "after-waiting-stop" })).isError, true);
    assert.deepEqual(await counts(), evidence.counts);
    await unchanged(originalDocuments);
  } else {
    assert.equal(joined.outcome.phase, "resumed");
    assert.equal(joined.executable, 1);
    assert.equal(joined.pendingContinuation, true);
    assert.equal(joined.state.executions[0].hasFreshObservation, true);
    assert.notEqual(joined.state.executions[0].generation, pin.state.executions[0].generation);
    assert.deepEqual(evidence.final.successorReads, [{ ownerJoined: true, observeJoined: true,
      settleJoined: true, statusPending: 0, snapshotChanged: true }]);
    evidence.successorReadsAfterOriginalJoin = true;
    assert.equal((await rpc("delivery")).ok, true);
    evidence.continuationsDelivered = 1;
    assert.equal((await call("computer_act", { ...oldAction, actionId: "after-early-continue" })).isError, true);
    const fresh = await observe();
    assert(fresh.text.includes("Owned visible fixture 你好"));
    const action = { ...identity(fresh, "one-fresh-click-after-early-continue", "Fixture action"), action: "click", parameters: {} };
    const result = decode(await call("computer_act", action));
    assert.equal(result.kind, "applied");
    assert.deepEqual(decode(await call("computer_act", action)), result);
    assert.deepEqual(await originalPageEffects(fixture), expectedPageEffects({ action: "click" }, true));
    for (const member of [memberB, memberC])
      assert.deepEqual(await originalPageEffects(member.page), expectedPageEffects({ action: "click" }, false));
    assert.equal((await counts()).inputs - evidence.counts.inputs, 1);
    evidence.freshActions = 1;
    evidence.newExplicitContinue = true;
  }
  pass("early-continue-fresh-publication-or-terminal-stop");

  step("original-capture-final-stop-keeps-shares");
  if (!stopped) decode(await call("computer_stop", selectedRun));
  await until(async () => (await rpc("stopped", { runId: selectedRun.runId })).ok);
  assert.equal((await rpc("shared-count")).count, 3);
  evidence.originalDocumentsRetained = true;
  evidence.threeSharesRetained = true;
  evidence.originalStopped = true;
  pass("original-capture-final-stop-keeps-shares");
  return evidence;
}
