import assert from "node:assert/strict";
import { armOriginalRecoveryReturn, originalRecoveryReturnState,
  releaseOriginalRecoveryReturn, finishOriginalRecoveryReturn } from "./existing-task-recovery-gate.mjs";

export async function resumeOriginalConcurrently({ rpc, worker, until, fixtureId, pass, phase }) {
  const document = await worker.evaluate(async tabId =>
    (await chrome.webNavigation.getFrame({ tabId, frameId: 0 })).documentId, fixtureId);
  assert.equal(typeof document, "string");
  await armOriginalRecoveryReturn(worker, fixtureId, document);
  try {
    phase("original-recovery-native-return-held", "original-recovery-start");
    assert.equal((await rpc("host-resume-race-start")).started, true);
    await until(async () => (await originalRecoveryReturnState(worker)).entered);
    const held = await originalRecoveryReturnState(worker);
    assert.deepEqual(held, { matched: 1, entered: true, released: false, returned: false,
      documentMatched: true, failed: false, originalWrapperRetained: true });
    phase("original-recovery-native-return-held", "original-recovery-registry-held");
    const original = await rpc("host-resume-race-held");
    assert.equal(original.originalWorkerRetained, true);
    assert.equal(original.originalWaiterPending, true);
    assert.equal(original.executable, 0);
    assert.equal(original.pendingContinuation, false);
    assert.equal(original.state.executions[0].phase, "resuming");
    assert.equal(original.state.executions[0].hasFreshObservation, false);
    pass("original-recovery-native-return-held");

    phase("concurrent-continue-retains-one-original-worker", "original-recovery-concurrent-continue");
    const duplicate = await rpc("host-resume-race-duplicate");
    assert.equal(duplicate.inProgress, true);
    assert.equal(duplicate.originalWorkerRetained, true);
    assert.equal(duplicate.originalWaiterPending, true);
    assert.equal(duplicate.executable, 0);
    assert.equal(duplicate.pendingContinuation, false);
    assert.deepEqual(await originalRecoveryReturnState(worker), held);
    pass("concurrent-continue-retains-one-original-worker");

    phase("original-host-resume-fresh-semantic-observation-once", "original-recovery-release-and-join");
    await releaseOriginalRecoveryReturn(worker);
    const resumed = await rpc("host-resume-race-finish");
    assert.equal(resumed.originalWorkerJoined, true);
    assert.equal(resumed.originalWorkerRetired, true);
    assert.equal(resumed.originalWaiterJoined, true);
    assert.equal(resumed.inProgressReplies, 1);
    assert.deepEqual(await originalRecoveryReturnState(worker), { ...held, released: true, returned: true });
    resumed.raceEvidence = { inProgressReplies: resumed.inProgressReplies, nativeCaptures: held.matched,
      originalDocumentMatched: held.documentMatched, originalWorkerRetained: original.originalWorkerRetained,
      originalWaiterPending: original.originalWaiterPending, duplicateDidNotCapture: true,
      originalWorkerJoined: resumed.originalWorkerJoined, originalWorkerRetired: resumed.originalWorkerRetired,
      originalWaiterJoined: resumed.originalWaiterJoined, originalNativeResultReturned: true };
    return resumed;
  } finally {
    await finishOriginalRecoveryReturn(worker);
  }
}
