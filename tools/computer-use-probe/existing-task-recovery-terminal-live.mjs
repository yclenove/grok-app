// Same original native Promise, current Host address, real Pause/Chat Stop and join.
import assert from "node:assert/strict";
import { armOriginalRecoveryReturn, originalRecoveryReturnState,
  releaseOriginalRecoveryReturn, finishOriginalRecoveryReturn } from "./existing-task-recovery-gate.mjs";

export const recoveryTerminalScenarios = Object.freeze(["active-host-resume-pause", "active-host-resume-chat-stop"]);

export async function interruptOriginalRecovery({ rpc, call, worker, until, fixtureId, members,
  selectedRun, initial, oldClick, dispatchCounts, unchanged, interruption, pass, phase }) {
  assert(["pause", "chat-stop"].includes(interruption));
  const documents = () => worker.evaluate(async ids => Promise.all(ids.map(async tabId =>
    (await chrome.webNavigation.getFrame({ tabId, frameId: 0 })).documentId)), members.map(row => row.id));
  const originalDocuments = await documents();
  for (const document of originalDocuments) assert.equal(typeof document, "string");
  const documentId = originalDocuments[0];
  const before = await dispatchCounts();
  await armOriginalRecoveryReturn(worker, fixtureId, documentId);
  try {
    phase("original-recovery-native-return-held", "original-recovery-start");
    assert.equal((await rpc("host-resume-race-start")).started, true);
    await until(async () => (await originalRecoveryReturnState(worker)).entered);
    const held = await originalRecoveryReturnState(worker);
    assert.deepEqual(held, { matched: 1, entered: true, released: false, returned: false,
      documentMatched: true, failed: false, originalWrapperRetained: true });
    const owned = await rpc("host-resume-race-held");
    assert.equal(owned.originalWorkerRetained, true);
    assert.equal(owned.originalWaiterPending, true);
    assert.equal(owned.executable, 0);
    assert.equal(owned.pendingContinuation, false);
    pass("original-recovery-native-return-held");

    const zeroAuthority = value => {
      assert.equal(value.interruption, interruption);
      assert.equal(value.state.featureEnabled, true);
      assert.equal(value.stored, true);
      assert.equal(value.originalTransportRetained, true);
      assert.equal(value.originalClaimRetired, true);
      assert.equal(value.originalDeliveryConsumed, true);
      assert.equal(value.executable, 0);
      assert.equal(value.pendingContinuation, false);
      if (value.state.request) assert.deepEqual(value.state.request.key, initial.state.request.key);
      for (const row of value.state.executions) {
        assert.equal(row.runId, selectedRun.runId);
        assert.deepEqual(row.key, initial.state.request.key);
        assert.equal(row.hasFreshObservation, false);
      }
      if (interruption === "chat-stop") assert.equal(value.state.grants.length, 0);
    };
    phase("original-recovery-interrupted-without-authority", "original-recovery-host-interruption");
    const fenced = await rpc(interruption === "pause" ? "host-resume-terminal-pause" : "host-resume-terminal-stop");
    zeroAuthority(fenced);
    // Cancellation may naturally settle the Host waiter before native rows return.
    // The native gate must still own exactly the original unresolved result.
    assert.deepEqual(await originalRecoveryReturnState(worker), held);
    await unchanged();
    pass("original-recovery-interrupted-without-authority");

    phase("original-recovery-late-return-rejected-and-joined", "original-recovery-terminal-release-and-join");
    await releaseOriginalRecoveryReturn(worker);
    await until(async () => (await originalRecoveryReturnState(worker)).returned);
    const joined = await rpc("host-resume-terminal-finish");
    zeroAuthority(joined);
    for (const field of ["originalWorkerJoined", "originalWorkerRetired", "originalWaiterJoined", "zeroPeerWire"])
      assert.equal(joined[field], true);
    assert.equal(joined.exactRejection, "Computer Use original scope or recovery observation unavailable");
    if (interruption === "pause") {
      assert.equal(joined.state.executions.length, 1);
      assert.equal(joined.state.executions[0].phase, "paused");
    } else {
      assert.equal(joined.originalStopped, true);
    }
    assert.deepEqual(await originalRecoveryReturnState(worker), { ...held, released: true, returned: true });
    const after = { ...before, reads: before.reads + 1 };
    assert.deepEqual(await dispatchCounts(), after);
    await unchanged(); pass("original-recovery-late-return-rejected-and-joined");

    phase("original-recovery-old-delivery-and-shares-retained", "original-recovery-old-delivery-replay");
    zeroAuthority(await rpc("host-resume-terminal-replay"));
    assert.equal((await rpc("host-resume-terminal-state")).replayChecked, true);
    for (const screenshot of [false, true])
      assert.equal((await call("computer_observe", { ...selectedRun, screenshot })).isError, true);
    assert.equal((await call("computer_act", { ...oldClick, actionId: "old-after-recovery-interruption" })).isError, true);
    for (const { id } of members)
      assert.equal((await call("computer_open_target", { runId: selectedRun.runId, targetId: `tab:${id}` })).isError, true);
    assert.deepEqual(await dispatchCounts(), after);
    assert.deepEqual(await documents(), originalDocuments);
    assert.equal((await rpc("shared-count")).count, 3);
    const pair = await worker.evaluate(() => ({ confirmed: globalThis.__jointPairingConfirmed,
      disconnects: globalThis.__jointDisconnectRequests }));
    assert.deepEqual(pair, { confirmed: 1, disconnects: 0 });
    await unchanged();
    zeroAuthority(await rpc("host-resume-terminal-state"));
    pass("original-recovery-old-delivery-and-shares-retained");
    return { interruption, nativeCaptures: held.matched, originalDocumentMatched: true,
      originalWorkerRetained: true, originalWaiterPending: true, originalWorkerJoined: true,
      originalWorkerRetired: true, originalWaiterJoined: true, originalNativeResultReturned: true,
      zeroPeerWire: true, originalClaimRetired: true, oldDeliveryNotRevived: true,
      originalDocumentsRetained: true, threeSharesRetained: true, pairRetained: true, featureAndStoreRetained: true };
  } finally {
    await finishOriginalRecoveryReturn(worker);
  }
}
