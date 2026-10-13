import assert from "node:assert/strict";
import { armOriginalActionReturn, originalActionReturnState, releaseOriginalActionReturn,
  replayOriginalCompletion, finishOriginalActionReturn } from "./existing-task-action-gate.mjs";
import { lateActionSpec, expectedPageEffects, originalPageEffects } from "./existing-task-late-action-spec.mjs";

export { lateActionScenarios } from "./existing-task-late-action-spec.mjs";

export async function verifyOriginalLateAction({ scenario, rpc, call, decode, until, observe, identity,
  fixture, fixtureId, memberB, memberC, selectedRun, worker, popup, initial, noMemberCReadOrInput, pass, phase }) {
  const spec = lateActionSpec(scenario);
  const { post, interruption } = spec;
  const members = [{ id: fixtureId, page: fixture }, memberB, memberC];
  const documents = () => worker.evaluate(async ids => Promise.all(ids.map(async tabId =>
    (await chrome.webNavigation.getFrame({ tabId, frameId: 0 })).documentId)), members.map(row => row.id));
  const counts = () => worker.evaluate(() => ({
    reads: globalThis.__jointCalls.filter(row => row.function === "observeDocument").length,
    nativeActions: globalThis.__jointCalls.filter(row => row.function === "actDocument").length,
    captures: globalThis.__jointCaptures,
  }));
  const effects = async applied => {
    let original;
    for (const { page } of members) {
      assert.equal(page.isClosed(), false);
      const actual = await originalPageEffects(page);
      assert.deepEqual(actual, expectedPageEffects(spec, page === fixture && applied));
      if (page === fixture) original = actual;
    }
    await noMemberCReadOrInput();
    return original;
  };
  phase("original-grant-fresh-observation-no-input", "original-action-observe");
  const old = await observe();
  assert(old.text.includes("Owned visible fixture 你好"));
  const action = { ...identity(old, `original-late-${spec.action}`, spec.name),
    action: spec.action, parameters: spec.parameters };
  const originalDocuments = await documents();
  const before = await counts();
  assert.equal(before.captures, 0);
  await effects(false); pass("original-grant-fresh-observation-no-input");
  await armOriginalActionReturn(worker, { runId: selectedRun.runId, tabId: fixtureId,
    documentId: originalDocuments[0], snapshotId: old.snapshotId, action: spec.action, cut: post ? "post" : "pre" }, { popup, phase });
  let originalReply;
  try {
    phase("original-action-native-return-held", "original-action-start");
    originalReply = call("computer_act", action);
    originalReply.catch(() => {});
    await until(async () => (await originalActionReturnState(worker)).entered);
    const held = await originalActionReturnState(worker);
    for (const field of ["claimed", "documentMatched", "actPending", "completionPending", "originalWrappersRetained"])
      assert.equal(held[field], true);
    assert.equal(held.matched, 1);
    assert.equal(held.otherReaders, 0);
    assert.equal(held.nativeInvokes, Number(post));
    assert.equal(held.nativeStatus, post ? spec.status : null);
    assert.equal(held.nativeDetail, post ? spec.detail : null);
    assert.equal(held.originalActResult, null);
    assert.equal(held.signalAborted, false);
    assert.deepEqual(held.scope, { phase: "claimed", cancelRequested: false });
    const pin = await rpc("late-action-held");
    assert.deepEqual(pin.state.executions[0].key, initial.state.request.key);
    assert.equal(pin.state.executions[0].runId, selectedRun.runId);
    // Read the original page BEFORE interruption too: held post rows already happened.
    await effects(post);
    pass("original-action-native-return-held");
    phase("original-action-interrupted-without-authority", "original-action-host-interruption");
    assert.equal((await rpc("late-action-interrupt")).started, true);
    await until(async () => (await rpc("late-action-state")).executable === 0);
    await until(async () => (await originalActionReturnState(worker)).signalAborted);
    const fenced = await originalActionReturnState(worker);
    assert.deepEqual(fenced.scope, { phase: "claimed", cancelRequested: true });
    assert.equal(fenced.actPending, true);
    assert.equal(fenced.completionPending, true);
    assert.equal(fenced.returned, false);
    const authority = value => {
      for (const field of ["stored", "originalTransportRetained", "originalClaimRetired", "originalDeliveryConsumed"])
        assert.equal(value[field], true);
      assert.equal(value.state.featureEnabled, true);
      assert.equal(value.executable, 0);
      assert.equal(value.pendingContinuation, false);
      assert.equal(value.state.executions[0].hasFreshObservation, false);
      assert.equal(value.state.executions[0].runId, selectedRun.runId);
      assert.deepEqual(value.state.executions[0].key, initial.state.request.key);
      if (interruption === "chat-stop") assert.equal(value.state.grants.length, 0);
    };
    authority(await rpc("late-action-state"));
    pass("original-action-interrupted-without-authority");
    phase("original-action-late-return-rejected-and-joined", "original-action-release-and-join");
    await releaseOriginalActionReturn(worker);
    const reply = await originalReply;
    let outcome = "error";
    if (reply.isError === false) {
      outcome = decode(reply).kind;
      assert(["unknown", "rejected"].includes(outcome));
      if (post) assert.equal(outcome, "unknown");
    }
    await until(async () => {
      const value = await originalActionReturnState(worker);
      return value.originalActJoined && value.originalCompletionJoined && value.actionsIdle && value.completionsIdle;
    });
    const native = await originalActionReturnState(worker);
    assert.equal(native.returned, true);
    assert.equal(native.scope.phase, "settled");
    assert.equal(native.nativeInvokes, Number(post));
    assert.equal(native.cancelInvokes, Number(post));
    assert.equal(native.cancelJoined, native.cancelInvokes);
    assert.deepEqual(native.originalActResult, { status: post ? "unknown" : "rejected",
      detail: post ? "cancelled_after_dispatch" : "stale_snapshot", physicallySettled: true });
    const joined = await rpc("late-action-finish");
    authority(joined);
    assert.equal(joined.originalWaiterJoined, true);
    assert.equal(joined.state.executions[0].cleanupPending, false);
    if (interruption === "chat-stop") assert.equal(joined.originalStopped, true);
    else assert.equal(joined.state.executions[0].phase, "paused");
    const after = { ...before, nativeActions: before.nativeActions + Number(post) };
    assert.deepEqual(await counts(), after);
    const actualEffects = await effects(post); pass("original-action-late-return-rejected-and-joined");
    phase("original-action-old-delivery-and-shares-retained", "original-action-replay");
    assert.deepEqual(await replayOriginalCompletion(worker), { status: "rejected", detail: "action_busy_or_duplicate" });
    for (const actionId of [action.actionId, "old-ref-after-action-interruption"])
      assert.equal((await call("computer_act", { ...action, actionId })).isError, true);
    for (const screenshot of [false, true])
      assert.equal((await call("computer_observe", { ...selectedRun, screenshot })).isError, true);
    authority(await rpc("late-action-replay"));
    assert.equal((await rpc("late-action-state")).replayChecked, true);
    assert.deepEqual(await counts(), after);
    assert.deepEqual(await documents(), originalDocuments);
    assert.equal((await rpc("shared-count")).count, 3);
    assert.deepEqual(await worker.evaluate(() => ({ confirmed: globalThis.__jointPairingConfirmed,
      disconnects: globalThis.__jointDisconnectRequests })), { confirmed: 1, disconnects: 0 });
    await effects(post); pass("original-action-old-delivery-and-shares-retained");
    const evidence = { action: spec.action, cut: post ? "post" : "pre", interruption,
      nativeInvokes: native.nativeInvokes, clicks: actualEffects.clicks, outcome,
      nativeStatus: held.nativeStatus, nativeDetail: held.nativeDetail,
      originalActResult: native.originalActResult, textValueMatched: true,
      textEvents: actualEffects.textEvents, scrollTop: actualEffects.scrollTop,
      originalActJoined: true, originalCompletionJoined: true, originalWaiterJoined: true,
      scopeSettled: true, cancelJoined: true, oldDeliveryNotRevived: true, zeroPeerWire: true,
      pairRetained: true, threeSharesRetained: true, originalDocumentsRetained: true,
      featureAndStoreRetained: true, preflightOwnerMatched: true,
      retainedContinuationRecords: joined.retainedContinuationRecords };
    if (interruption === "pause") {
      phase("original-action-new-explicit-continue", "original-action-new-continue");
      const resumed = await rpc("late-action-resume");
      assert.equal(resumed.outcome.phase, "resumed");
      assert.equal(resumed.executable, 1);
      assert.equal(resumed.pendingContinuation, true);
      assert.equal(resumed.state.executions[0].hasFreshObservation, true);
      assert.notEqual(resumed.state.executions[0].generation, pin.state.executions[0].generation);
      assert.equal((await rpc("delivery")).ok, true);
      assert.equal((await call("computer_act", { ...action, actionId: "old-ref-after-new-continue" })).isError, true);
      assert.deepEqual(await counts(), { ...after, reads: after.reads + 1 });
      await effects(post); pass("original-action-new-explicit-continue");
    }
    phase("original-action-final-stop-keeps-shares", "original-action-final-stop");
    if (interruption === "pause") decode(await call("computer_stop", selectedRun));
    await until(async () => (await rpc("stopped", { runId: selectedRun.runId })).ok);
    assert.equal((await rpc("shared-count")).count, 3);
    assert.deepEqual(await documents(), originalDocuments);
    await effects(post); pass("original-action-final-stop-keeps-shares");
    return evidence;
  } finally {
    await finishOriginalActionReturn(worker);
    if (originalReply) await originalReply.catch(() => {});
  }
}
