// Original source MV3/App/MCP; never a fabricated stopped run or fresh receipt.
import assert from "node:assert/strict";
import { validateActionRequest } from "../computer-use-mcp/protocol.mjs";

export const terminalScenarios = Object.freeze([
  "choose-picker-cancel", "choose-task-stop", "discovery-picker-cancel",
  "discovery-task-stop", "discovery-waiter-drop",
]);

export async function verifyOriginalTerminal({ scenario, rpc, call, worker, members,
  initial, until, zeroPageReads, pass, phase }) {
  assert(terminalScenarios.includes(scenario));
  const discovering = scenario.startsWith("discovery-");
  const documents = async () => worker.evaluate(async ids => Promise.all(ids.map(async tabId =>
    (await chrome.webNavigation.getFrame({ tabId, frameId: 0 })).documentId)), members.map(row => row.id));
  // Capture document identities before the original four/five-second cut.
  const originalDocuments = await documents();
  for (const id of originalDocuments) assert.equal(typeof id, "string");
  const pairingCounters = () => worker.evaluate(() => ({ confirmed: globalThis.__jointPairingConfirmed,
    disconnects: globalThis.__jointDisconnectRequests }));
  const originalPairing = await pairingCounters();
  process.stderr.write(`terminal initial pairing witness: confirmed=${originalPairing.confirmed}, disconnects=${originalPairing.disconnects}\n`);
  assert.equal(originalPairing.confirmed, 1);
  assert.equal(originalPairing.disconnects, 0);
  let candidates = [];
  if (!discovering) {
    const selected = await rpc("load");
    assert.equal(selected.ok, true);
    assert.equal(selected.view.candidates.length, 3);
    candidates = ["CU New TaskGrant Fixture", "CU New TaskGrant Fixture B"].map(title => {
      const rows = selected.view.candidates.filter(row => row.title === title);
      assert.equal(rows.length, 1);
      return rows[0].candidateId;
    });
  }
  await zeroPageReads();
  pass("real-unshare-reshare-original-picker");
  phase("terminal-original-call-fenced-and-joined");
  assert.equal((await rpc("terminal-arm")).armed, true);
  if (discovering) {
    assert.equal((await rpc("start-discovery")).started, true);
    await until(async () => (await rpc("discovery-ready")).ready);
  }
  const rejected = await rpc("terminal-reject", { candidates });
  assert.equal(rejected.rejected, true);
  assert.equal(rejected.at, discovering ? "original-owned-discovery" : "original-app-picker");
  assert.equal(rejected.outerDropped, scenario === "discovery-waiter-drop");
  assert.equal(rejected.originalInnerJoined, discovering);
  assert.equal(rejected.exactRejection, scenario.endsWith("picker-cancel")
    ? "Computer Use original App picker owner changed" : discovering
      ? "Computer Use picker not ready or retired" : "Computer Use picker selection unavailable or retired");
  const zeroAuthority = value => {
    assert.deepEqual(value.state.request.key, initial.state.request.key);
    assert.equal(value.state.request.phase, scenario.endsWith("task-stop") ? "cancelled" : "pending");
    assert.equal(value.state.featureEnabled, false);
    assert.equal(value.state.grants.length, 0);
    assert.equal(value.state.executions.length, 0);
    for (const name of ["reservationEntries", "approvalCalls", "ownedWorkers", "discoveryWorkers",
      "reservationWorkers", "grants", "continuations"]) assert.equal(value[name], 0);
    assert.equal(value.stored, false);
  };
  zeroAuthority(rejected);
  await zeroPageReads();
  pass("terminal-original-call-fenced-and-joined");
  const isolation = await verifyTerminalIsolation({ rpc, call, worker, members,
    originalDocuments, zeroAuthority, zeroPageReads, pass, phase });
  return { ...isolation, outerDropped: rejected.outerDropped,
    originalInnerJoined: rejected.originalInnerJoined, noRunCreated: true };
}

export async function verifyTerminalIsolation({ rpc, call, worker, members, originalDocuments,
  zeroAuthority, zeroPageReads, pass, phase, authorityCommand = "terminal-authority",
  isolationStage = "terminal-zero-authority-shares-retained", denialCode = "feature_disabled", runId }) {
  phase(isolationStage);
  const deniedAtAuthority = (tool, denied) => {
    const code = ["feature_disabled", "target_unauthorized", "identity_mismatch", "schema", "rate_limited"]
      .includes(denied.code) ? denied.code : denied.code === undefined ? "missing" : "other";
    process.stderr.write(`terminal permission witness: tool=${tool}, isError=${denied.isError === true}, code=${code}\n`);
    assert.equal(denied.isError, true);
    assert.equal(denied.code, denialCode);
  };
  const runClaim = runId === undefined ? {} : { runId };
  for (const screenshot of [false, true]) {
    phase(isolationStage, screenshot ? "terminal-observe-image-denied" : "terminal-observe-text-denied");
    const denied = await call("computer_observe", { ...runClaim, screenshot });
    deniedAtAuthority("computer_observe", denied);
  }
  phase(isolationStage, "terminal-list-denied");
  const list = await call("computer_list_targets", runClaim);
  deniedAtAuthority("computer_list_targets", list);
  let inputRequests = 0;
  for (const { member, id } of members) {
    for (const action of ["set_value", "type_text"]) {
      const request = {
        version: 1, actionId: `terminal-${member}-${action}`, runId: runId ?? "never-started-terminal-run",
        targetId: `tab:${id}`, targetGeneration: 1, snapshotId: "never-observed-terminal-snapshot",
        geometryRevision: 0, target: { elementRef: "never-authorized-terminal-ref" },
        action, parameters: { text: "must never enter the owned page 你好" },
      };
      assert.equal(validateActionRequest(request), null);
      phase(isolationStage, "terminal-input-denied");
      const denied = await call("computer_act", request);
      deniedAtAuthority("computer_act", denied);
      inputRequests++;
      await zeroPageReads();
    }
  }
  assert.equal(inputRequests, 6);
  phase(isolationStage, "terminal-authority-retained");
  zeroAuthority(await rpc(authorityCommand));
  phase(isolationStage, "terminal-documents-retained");
  const documents = await worker.evaluate(async ids => Promise.all(ids.map(async tabId =>
    (await chrome.webNavigation.getFrame({ tabId, frameId: 0 })).documentId)), members.map(row => row.id));
  assert.deepEqual(documents, originalDocuments);
  assert.equal((await rpc("shared-count")).count, 3);
  for (const { page } of members) assert.equal(page.isClosed(), false);
  phase(isolationStage, "terminal-pair-retained");
  const pairing = await worker.evaluate(() => ({ confirmed: globalThis.__jointPairingConfirmed,
    disconnects: globalThis.__jointDisconnectRequests }));
  process.stderr.write(`terminal final pairing witness: confirmed=${pairing.confirmed}, disconnects=${pairing.disconnects}\n`);
  assert.equal(pairing.confirmed, 1);
  assert.equal(pairing.disconnects, 0);
  await zeroPageReads();
  pass(isolationStage);
  return { inputRequests, rejection: denialCode, originalDocumentsRetained: true,
    threeSharesRetained: true, finalAuthorityZero: true };
}
