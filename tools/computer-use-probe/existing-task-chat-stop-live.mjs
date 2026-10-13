// Original manager reservation/Chat Stop stdin fence; synthetic Host and ACP peer.
import assert from "node:assert/strict";
import { verifyTerminalIsolation } from "./existing-task-terminals-live.mjs";

export async function verifyOriginalChatStop({ rpc, call, worker, members, initial,
  approved, until, zeroPageReads, pass, phase }) {
  const runId = approved.outcome.runId;
  const originalDocuments = await worker.evaluate(async ids => Promise.all(ids.map(async tabId =>
    (await chrome.webNavigation.getFrame({ tabId, frameId: 0 })).documentId)), members.map(row => row.id));
  for (const document of originalDocuments) assert.equal(typeof document, "string");
  await zeroPageReads();
  pass("new-task-grant-original-app-publication");
  phase("original-chat-stop-queued-prompt-fenced-and-joined", "original-chat-stop-stdin-fence");
  const stopped = await rpc("chat-stop-queued");
  for (const field of ["queuedAtOriginalStdin", "notStarted", "originalPromptJoined", "zeroPeerWire", "originalClaimRetired"])
    assert.equal(stopped[field], true);
  assert.equal(stopped.pendingBeforeStop, 1);
  assert.equal(stopped.continuationBeforeStop, 1);
  const zeroAuthority = value => {
    assert.equal(value.runId, runId);
    if (value.state.request) assert.deepEqual(value.state.request.key, initial.state.request.key);
    assert.equal(value.state.featureEnabled, true);
    assert.equal(value.stored, true);
    assert.equal(value.state.grants.length, 0);
    assert(value.state.executions.every(row => row.runId === runId));
    for (const field of ["grants", "continuations", "ownedWorkers"]) assert.equal(value[field], 0);
    assert.equal(value.approvalCalls, 1);
    assert.equal(value.originalTransportRetained, true);
  };
  zeroAuthority(stopped);
  phase("original-chat-stop-queued-prompt-fenced-and-joined", "original-run-stopped");
  await until(async () => (await rpc("stopped", { runId })).ok);
  const final = await rpc("chat-stop-authority");
  zeroAuthority(final);
  assert.equal(final.originalStopped, true);
  await zeroPageReads();
  pass("original-chat-stop-queued-prompt-fenced-and-joined");
  const isolation = await verifyTerminalIsolation({ rpc, call, worker, members, originalDocuments,
    zeroAuthority, zeroPageReads, pass, phase, authorityCommand: "chat-stop-authority",
    isolationStage: "chat-stop-zero-authority-shares-retained", denialCode: "target_unauthorized", runId });
  phase("chat-stop-old-delivery-cannot-revive");
  const replay = await rpc("chat-stop-replay");
  zeroAuthority(replay);
  assert.equal(replay.originalStopped, true);
  assert.equal(replay.replayChecked, true);
  await zeroPageReads();
  pass("chat-stop-old-delivery-cannot-revive");
  return { ...isolation, queuedAtOriginalStdin: true, pendingBeforeStop: 1, continuationBeforeStop: 1,
    notStarted: true, originalPromptJoined: true, zeroPeerWire: true, originalClaimRetired: true, originalRunStopped: true,
    oldDeliveryNotRevived: true, featureAndStoreRetained: true };
}
