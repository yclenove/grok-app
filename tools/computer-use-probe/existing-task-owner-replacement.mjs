// Fixed private owner cuts; original MV3/HTTP/MCP and DOM oracles stay with caller.
import assert from "node:assert/strict";
import { validateActionRequest } from "../computer-use-mcp/protocol.mjs";

export const ownerReplacements = Object.freeze([
  "choose-app-process-replaced", "choose-agent-replaced", "choose-acp-replaced", "choose-transport-replaced",
  "reservation-app-process-replaced", "reservation-agent-replaced", "reservation-acp-replaced", "reservation-transport-replaced",
  "publication-app-process-replaced", "publication-agent-replaced", "publication-acp-replaced", "publication-transport-replaced",
]);

const identity = scenario => {
  assert(ownerReplacements.includes(scenario), "fixed original owner scenario required");
  if (scenario.endsWith("-app-process-replaced")) return [false, true, true, true];
  if (scenario.endsWith("-agent-replaced")) return [true, false, true, true];
  if (scenario.endsWith("-acp-replaced")) return [true, true, false, true];
  return [true, true, true, false];
};

const ownerWitness = (scenario, witness) => {
  assert.equal(witness.originalAcpAlive, true);
  assert.equal(witness.originalTransportCurrent, !scenario.endsWith("-transport-replaced"));
  assert.equal(witness.replacementAcpAlive, scenario.endsWith("-acp-replaced"));
  assert.equal(witness.replacementRequestExists, false);
  assert.equal(witness.replacementGrants, 0);
  assert.equal(witness.replacementExecutions, 0);
};

export async function replaceOriginalOwner({ scenario, rpc, worker, initialKey,
  publicationRun, documentId, originalDocument, zeroPageReads }) {
  const publication = scenario.startsWith("publication-");
  const reservation = scenario.startsWith("reservation-");
  const expected = identity(scenario);
  const replacement = await rpc(publication ? "replace-publication-owner" : "replace-picker-owner");
  assert.equal(replacement.replaced, true);
  assert.deepEqual(replacement.unchanged, expected);
  assert.equal(replacement.runId, publication ? publicationRun : null);
  assert.deepEqual(replacement.state.request.key, initialKey);
  assert.equal(replacement.state.featureEnabled, publication);
  assert.equal(replacement.state.grants.length, 0);
  assert.equal(replacement.state.executions.length, publication ? 1 : 0);
  assert.equal(replacement.ownedWorkers, publication ? 1 : 0);
  assert.equal(replacement.reservationWorkers, reservation ? 1 : 0);
  assert.equal(replacement.continuations, 0);
  assert.equal(replacement.stored, publication);
  ownerWitness(scenario, replacement.ownerWitness);
  assert.equal(await documentId(), originalDocument);
  assert.equal((await rpc("shared-count")).count, 3);
  assert.equal(await worker.evaluate(() => globalThis.__jointPairingConfirmed), 1);
  assert.equal(await worker.evaluate(() => globalThis.__jointDisconnectRequests), 0);
  await zeroPageReads();
  return { unchanged: replacement.unchanged, ...replacement.ownerWitness };
}

export async function verifyRejectedOwner({ scenario, rejected, rpc, until, documentId,
  originalDocument, publicationRun }) {
  identity(scenario);
  ownerWitness(scenario, rejected.ownerWitness);
  const publication = scenario.startsWith("publication-");
  const reservation = scenario.startsWith("reservation-");
  const prepared = publication || reservation && !scenario.endsWith("-transport-replaced");
  if (publication) assert.equal(rejected.runId, publicationRun);
  if (reservation) assert.equal(rejected.approvalCalls, prepared ? 1 : 0);
  if (!publication && !reservation) {
    assert.equal(rejected.reservationEntries, 0);
    assert.equal(rejected.approvalCalls, 0);
  }
  if (prepared) {
    assert.equal(typeof rejected.runId, "string");
    assert(rejected.state.executions.length <= 1);
    for (const execution of rejected.state.executions) assert.equal(execution.runId, rejected.runId);
    await until(async () => (await rpc("stopped", { runId: rejected.runId })).ok);
  } else {
    assert.equal(rejected.state.executions.length, 0);
    if (reservation) assert.equal(rejected.runId, null);
    else assert.equal(rejected.runId, undefined);
  }
  assert.equal(await documentId(), originalDocument);
}

export async function verifyDeniedOwnerInput({ scenario, rejected, call, rpc, members, zeroPageReads }) {
  identity(scenario);
  assert.equal(rejected.state.featureEnabled, false);
  assert.equal(rejected.state.grants.length, 0);
  assert.equal(rejected.continuations, 0);
  await zeroPageReads();
  let requests = 0;
  for (const { member, id } of members) {
    for (const action of ["set_value", "type_text"]) {
      // No observation was authorized. These complete synthetic identities must
      // be rejected at the authority gate, before schema/ref lookup or dispatch.
      const request = {
        version: 1, actionId: `retired-owner-${member}-${action}`,
        runId: rejected.runId ?? "never-started-owner-run", targetId: `tab:${id}`,
        targetGeneration: 1, snapshotId: "never-observed-owner-snapshot",
        geometryRevision: 0, target: { elementRef: "never-authorized-owner-ref" },
        action, parameters: { text: "must never enter the owned page 你好" },
      };
      assert.equal(validateActionRequest(request), null, "negative input must use the valid MCP action schema");
      const denied = await call("computer_act", request);
      assert.equal(denied.isError, true);
      assert.equal(denied.code, "feature_disabled", "input must fail at authority, not schema or ref lookup");
      requests++;
      await zeroPageReads();
    }
  }
  assert.equal(requests, 6);
  const final = await rpc("original-owner-authority");
  assert.deepEqual(final.state.request.key, rejected.state.request.key);
  assert.equal(final.state.featureEnabled, false);
  assert.equal(final.state.grants.length, 0);
  assert(final.state.executions.length <= rejected.state.executions.length);
  for (const execution of final.state.executions) assert.equal(execution.runId, rejected.runId);
  assert.equal(final.grants, 0);
  assert.equal(final.continuations, 0);
  assert.equal(final.ownedWorkers, 0);
  assert.equal(final.reservationWorkers, 0);
  assert.equal(final.stored, false);
  ownerWitness(scenario, final.ownerWitness);
  await zeroPageReads();
  return { requests, rejected: requests, code: "feature_disabled", pageEffects: 0, finalAuthorityZero: true };
}
