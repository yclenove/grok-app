// Original prepared run and approval owner, before the original App/ACP pin commits.
import assert from "node:assert/strict";
import { verifyTerminalIsolation } from "./existing-task-terminals-live.mjs";

export const publicationTerminalScenarios = Object.freeze([
  "publication-picker-cancel", "publication-task-stop", "publication-waiter-drop",
]);

export async function verifyPublicationTerminal({ scenario, rpc, call, worker, members,
  initial, until, zeroPageReads, pass, phase }) {
  assert(publicationTerminalScenarios.includes(scenario));
  const originalDocuments = await worker.evaluate(async ids => Promise.all(ids.map(async tabId =>
    (await chrome.webNavigation.getFrame({ tabId, frameId: 0 })).documentId)), members.map(row => row.id));
  for (const document of originalDocuments) assert.equal(typeof document, "string");
  const pairing = await worker.evaluate(() => ({ confirmed: globalThis.__jointPairingConfirmed,
    disconnects: globalThis.__jointDisconnectRequests }));
  assert.equal(pairing.confirmed, 1);
  assert.equal(pairing.disconnects, 0);
  const selected = await rpc("load");
  assert.equal(selected.ok, true);
  assert.equal(selected.view.candidates.length, 3);
  const candidates = ["CU New TaskGrant Fixture", "CU New TaskGrant Fixture B"].map(title => {
    const rows = selected.view.candidates.filter(row => row.title === title);
    assert.equal(rows.length, 1);
    return rows[0].candidateId;
  });
  await zeroPageReads();
  pass("real-unshare-reshare-original-picker");
  phase("terminal-original-call-fenced-and-joined");
  assert.equal((await rpc("start-publication", { candidates })).started, true);
  await until(async () => (await rpc("publication-ready")).ready);
  const ready = await rpc("publication-ready");
  assert.equal(ready.ownedWorkers, 1);
  assert.equal(ready.originalRegistryCaptured, true);
  assert.equal(ready.publicationEntries, 1);
  assert.equal(ready.state.featureEnabled, true);
  assert.equal(ready.stored, true);
  assert.equal(ready.state.grants.length, 0);
  assert.equal(ready.state.executions.length, 1);
  const originalRun = ready.runId;
  assert.equal(ready.state.executions[0].runId, originalRun);
  phase("terminal-original-call-fenced-and-joined", "publication-original-execution-poll-entered");
  // Enter the real execution loop while the original pin is still held. A
  // successful terminal must now survive rollback of an already-running poll.
  await until(async () => worker.connectionEvidence().http.some(row =>
    row.route === "/cu/extension-poll" && row.outcome === "pending"));
  const rejected = await rpc("publication-terminal-reject");
  assert.equal(rejected.rejected, true);
  assert.equal(rejected.at, "original-publication");
  assert.equal(rejected.exactRejection, "Computer Use approval retired before publication");
  assert.equal(rejected.outerDropped, scenario === "publication-waiter-drop");
  assert.equal(rejected.originalInnerJoined, true);
  const zeroAuthority = value => {
    assert.equal(value.runId, originalRun);
    assert.deepEqual(value.state.request.key, initial.state.request.key);
    assert.equal(value.state.request.phase, scenario.endsWith("task-stop") ? "cancelled" : "pending");
    assert.equal(value.state.featureEnabled, false);
    assert.equal(value.state.grants.length, 0);
    assert.equal(value.state.executions.length, 0);
    assert.equal(value.publicationEntries, 1);
    assert.equal(value.approvalCalls, 1);
    for (const field of ["ownedWorkers", "discoveryWorkers", "reservationWorkers", "grants", "continuations"])
      assert.equal(value[field], 0);
    assert.equal(value.stored, false);
  };
  zeroAuthority(rejected);
  phase("terminal-original-call-fenced-and-joined", "publication-original-execution-poll-suspended");
  await until(async () => worker.connectionEvidence().http.some(row =>
    row.route === "/cu/extension-poll" && row.status === 403));
  assert.equal(worker.connectionEvidence().disconnectRequests, 0);
  phase("terminal-original-call-fenced-and-joined", "original-run-stopped");
  await until(async () => (await rpc("stopped", { runId: originalRun })).ok);
  const final = await rpc("publication-terminal-authority");
  zeroAuthority(final);
  assert.equal(final.originalStopped, true);
  await zeroPageReads();
  pass("terminal-original-call-fenced-and-joined");
  const isolation = await verifyTerminalIsolation({ rpc, call, worker, members, originalDocuments,
    zeroAuthority, zeroPageReads, pass, phase, authorityCommand: "publication-terminal-authority" });
  return { ...isolation, outerDropped: rejected.outerDropped, originalInnerJoined: true, originalRunStopped: true,
    originalExecutionPollObserved: true, forbiddenObservationPollWaited: true };
}
