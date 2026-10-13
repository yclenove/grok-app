// Original owned reservation, before an approval/run exists.
import assert from "node:assert/strict";
import { verifyTerminalIsolation } from "./existing-task-terminals-live.mjs";

export const reservationTerminalScenarios = Object.freeze([
  "reservation-picker-cancel", "reservation-task-stop", "reservation-waiter-drop",
]);

export async function verifyReservationTerminal({ scenario, rpc, call, worker, members,
  initial, until, zeroPageReads, pass, phase }) {
  assert(reservationTerminalScenarios.includes(scenario));
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
  assert.equal((await rpc("start-reservation", { candidates })).started, true);
  await until(async () => (await rpc("reservation-ready")).ready);
  const ready = await rpc("reservation-ready");
  assert.equal(ready.reservationWorkers, 1);
  assert.equal(ready.originalRegistryCaptured, true);
  assert.equal(ready.state.executions.length, 0);
  assert.equal(ready.state.featureEnabled, false);
  assert.equal(ready.stored, false);
  const rejected = await rpc("reservation-terminal-reject");
  assert.equal(rejected.rejected, true);
  assert.equal(rejected.at, "original-owned-reservation");
  assert.equal(rejected.exactRejection, "Computer Use selected window instance retired or changed");
  assert.equal(rejected.outerDropped, scenario === "reservation-waiter-drop");
  assert.equal(rejected.originalInnerJoined, true);
  const zeroAuthority = value => {
    assert.deepEqual(value.state.request.key, initial.state.request.key);
    assert.equal(value.state.request.phase, scenario.endsWith("task-stop") ? "cancelled" : "pending");
    assert.equal(value.state.featureEnabled, false);
    assert.equal(value.state.grants.length, 0);
    assert.equal(value.state.executions.length, 0);
    assert.equal(value.reservationEntries, 1);
    for (const field of ["approvalCalls", "ownedWorkers", "discoveryWorkers", "reservationWorkers",
      "grants", "continuations"]) assert.equal(value[field], 0);
    assert.equal(value.stored, false);
  };
  zeroAuthority(rejected);
  await zeroPageReads();
  pass("terminal-original-call-fenced-and-joined");
  const isolation = await verifyTerminalIsolation({ rpc, call, worker, members, originalDocuments,
    zeroAuthority, zeroPageReads, pass, phase, authorityCommand: "reservation-terminal-authority" });
  return { ...isolation, outerDropped: rejected.outerDropped, originalInnerJoined: true, noRunCreated: true };
}
