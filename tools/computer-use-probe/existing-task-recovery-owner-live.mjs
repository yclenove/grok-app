// Original native return and pinned App owner; each replacement gets its own fixture.
import assert from "node:assert/strict";
import { armOriginalRecoveryReturn, originalRecoveryReturnState,
  releaseOriginalRecoveryReturn, finishOriginalRecoveryReturn } from "./existing-task-recovery-gate.mjs";

export const recoveryOwnerScenarios = Object.freeze(["active-host-resume-process-replaced",
  "active-host-resume-agent-replaced", "active-host-resume-acp-replaced", "active-host-resume-slot-removed"]);

export async function replaceOriginalRecoveryOwner({ scenario, rpc, call, worker, until, fixtureId,
  members, selectedRun, initial, paused, oldClick, dispatchCounts, unchanged, pass, phase }) {
  assert(recoveryOwnerScenarios.includes(scenario));
  const documents = () => worker.evaluate(async ids => Promise.all(ids.map(async tabId =>
    (await chrome.webNavigation.getFrame({ tabId, frameId: 0 })).documentId)), members.map(row => row.id));
  const originalDocuments = await documents();
  for (const document of originalDocuments) assert.equal(typeof document, "string");
  const before = await dispatchCounts();
  await armOriginalRecoveryReturn(worker, fixtureId, originalDocuments[0]);
  try {
    phase("original-recovery-native-return-held", "original-recovery-owner-start");
    assert.equal((await rpc("host-resume-race-start")).started, true);
    await until(async () => (await originalRecoveryReturnState(worker)).entered);
    const held = await originalRecoveryReturnState(worker);
    assert.deepEqual(held, { matched: 1, entered: true, released: false, returned: false,
      documentMatched: true, failed: false, originalWrapperRetained: true });
    const original = await rpc("host-resume-race-held");
    assert.equal(original.originalWorkerRetained, true);
    assert.equal(original.originalWaiterPending, true);
    assert.equal(original.executable, 0);
    assert.equal(original.pendingContinuation, false);
    pass("original-recovery-native-return-held");

    const expectedUnchanged = {
      "active-host-resume-process-replaced": [true, false, true, true],
      "active-host-resume-agent-replaced": [true, true, false, true],
      "active-host-resume-acp-replaced": [true, true, true, false],
      "active-host-resume-slot-removed": null,
    }[scenario];
    const zeroAuthority = (value, joined = false) => {
      assert.equal(value.scenario, scenario);
      assert.equal(value.state.featureEnabled, true);
      assert.equal(value.stored, true);
      assert.equal(value.executable, 0);
      assert.equal(value.pendingContinuation, false);
      assert.equal(value.originalClaimRetired, true);
      assert.equal(value.originalDeliveryConsumed, true);
      assert.deepEqual(value.state.request.key, initial.state.request.key);
      assert.equal(value.state.executions.length, 1);
      const execution = value.state.executions[0];
      assert.equal(execution.runId, selectedRun.runId);
      assert.deepEqual(execution.key, initial.state.request.key);
      assert.equal(execution.phase, joined ? "paused" : "resuming");
      assert.equal(execution.hasFreshObservation, false);
      assert.equal(value.state.grants.length, 1);
      assert.deepEqual(value.state.grants[0].members, paused.state.grants[0].members);
      const witness = value.ownerWitness;
      assert.deepEqual(witness.unchanged, expectedUnchanged);
      assert.equal(witness.slotExists, expectedUnchanged !== null);
      assert.equal(witness.removedOriginalRetained, expectedUnchanged === null);
      assert.equal(witness.replacementPeerExists, scenario === "active-host-resume-acp-replaced");
      for (const field of ["preciseReplacementRetained", "originalAcpAlive", "originalTransportRetained"])
        assert.equal(witness[field], true);
      assert.equal(witness.replacementRequestExists, false);
      for (const field of ["replacementGrants", "replacementExecutions", "replacementAvailableClaims"])
        assert.equal(witness[field], 0);
    };
    phase("original-recovery-owner-replaced-without-authority", "original-recovery-owner-replace");
    const replaced = await rpc("host-resume-owner-replace");
    zeroAuthority(replaced);
    assert.deepEqual(await originalRecoveryReturnState(worker), held);
    await unchanged(); pass("original-recovery-owner-replaced-without-authority");

    phase("original-recovery-owner-late-return-rejected-and-joined", "original-recovery-owner-release-and-join");
    await releaseOriginalRecoveryReturn(worker);
    await until(async () => (await originalRecoveryReturnState(worker)).returned);
    const joined = await rpc("host-resume-owner-finish");
    zeroAuthority(joined, true);
    assert.deepEqual(joined.ownerWitness, replaced.ownerWitness);
    for (const field of ["originalWorkerJoined", "originalWorkerRetired", "originalWaiterJoined",
      "zeroOriginalPeerWire", "zeroReplacementPeerWire"]) assert.equal(joined[field], true);
    assert.equal(joined.exactRejection, "Computer Use request owner retired");
    assert.deepEqual(await originalRecoveryReturnState(worker), { ...held, released: true, returned: true });
    const after = { ...before, reads: before.reads + 1 };
    assert.deepEqual(await dispatchCounts(), after);
    await unchanged(); pass("original-recovery-owner-late-return-rejected-and-joined");

    phase("original-recovery-owner-old-delivery-and-shares-retained", "original-recovery-owner-replay");
    const replay = await rpc("host-resume-owner-replay");
    zeroAuthority(replay, true);
    assert.equal(replay.replayChecked, true);
    for (const screenshot of [false, true])
      assert.equal((await call("computer_observe", { ...selectedRun, screenshot })).isError, true);
    assert.equal((await call("computer_act", { ...oldClick, actionId: "old-after-recovery-owner-replaced" })).isError, true);
    for (const { id } of members)
      assert.equal((await call("computer_open_target", { runId: selectedRun.runId, targetId: `tab:${id}` })).isError, true);
    assert.deepEqual(await dispatchCounts(), after);
    assert.deepEqual(await documents(), originalDocuments);
    assert.equal((await rpc("shared-count")).count, 3);
    assert.deepEqual(await worker.evaluate(() => ({ confirmed: globalThis.__jointPairingConfirmed,
      disconnects: globalThis.__jointDisconnectRequests })), { confirmed: 1, disconnects: 0 });
    const settled = await rpc("host-resume-owner-state");
    zeroAuthority(settled, true);
    assert.deepEqual(settled.ownerWitness, replaced.ownerWitness);
    await unchanged(); pass("original-recovery-owner-old-delivery-and-shares-retained");
    return { scenario, nativeCaptures: held.matched, exactRejection: joined.exactRejection,
      originalDocumentMatched: true, originalWorkerRetained: true, originalWaiterPending: true,
      originalWorkerJoined: true, originalWorkerRetired: true, originalWaiterJoined: true,
      originalNativeResultReturned: true, zeroOriginalPeerWire: true, zeroReplacementPeerWire: true,
      originalClaimRetired: true, oldDeliveryNotRevived: true, originalDocumentsRetained: true,
      threeSharesRetained: true, pairRetained: true, featureAndStoreRetained: true, replacementSlotUnchanged: true };
  } finally {
    await finishOriginalRecoveryReturn(worker);
  }
}
