// Original browser capture/permission rejection. No fake PNG or extra permission.
import assert from "node:assert/strict";

export const captureScenario = "active-capture-permission-denied";

export async function verifyOriginalCaptureDenial({ rpc, call, decode, until, observe, identity,
  worker, fixture, fixtureId, memberB, memberC, selectedRun, initial, pass, phase }) {
  const members = [{ id: fixtureId, page: fixture }, memberB, memberC];
  const documents = await worker.evaluate(async ids => Promise.all(ids.map(async tabId =>
    (await chrome.webNavigation.getFrame({ tabId, frameId: 0 }))?.documentId)), members.map(row => row.id));
  assert(documents.every(value => typeof value === "string"));
  const unchanged = async () => {
    for (const { page } of members) {
      assert.equal(page.isClosed(), false);
      assert.equal(await page.evaluate(() => globalThis.clicks), 0);
      assert.equal(await page.locator("#input").inputValue(), "");
      assert.equal(await page.locator("#scroll").evaluate(element => element.scrollTop), 0);
      assert.deepEqual(await page.evaluate(() => globalThis.textEvents),
        { beforeinput: 0, input: 0, change: 0, inputType: null });
    }
    const current = await worker.evaluate(async ids => Promise.all(ids.map(async tabId =>
      (await chrome.webNavigation.getFrame({ tabId, frameId: 0 }))?.documentId)), members.map(row => row.id));
    assert.deepEqual(current, documents);
    const untouched = await worker.evaluate(ids => globalThis.__jointCalls.filter(row => ids.includes(row.tab)
      && ["observeDocument", "actDocument"].includes(row.function)).length, [memberB.id, memberC.id]);
    assert.equal(untouched, 0);
  };
  const counts = () => worker.evaluate(() => ({
    reads: globalThis.__jointCalls.filter(row => row.function === "observeDocument").length,
    inputs: globalThis.__jointCalls.filter(row => row.function === "actDocument").length,
    captures: globalThis.__jointCaptures, denials: globalThis.__jointCapturePermissionDenials,
    settled: globalThis.__jointCaptureSettled, decodes: globalThis.__jointCaptureDecodes,
    results: globalThis.__jointObservationResults, pairing: globalThis.__jointPairingConfirmed,
    disconnects: globalThis.__jointDisconnectRequests,
  }));
  let stage = "original-grant-fresh-observation-no-input";
  phase(stage);
  const original = await observe();
  assert(original.text.includes("Owned visible fixture 你好"));
  assert.equal(original.image.pngBase64, undefined);
  await until(async () => (await counts()).results === 1);
  assert.deepEqual(await counts(), { reads: 1, inputs: 0, captures: 0, denials: 0,
    settled: 0, decodes: 0, results: 1, pairing: 1, disconnects: 0 });
  await unchanged(); pass(stage);

  stage = "original-native-capture-permission-denied-no-publication";
  phase(stage, "original-explicit-screenshot-request");
  const denied = await call("computer_observe", { ...selectedRun, screenshot: true });
  assert.equal(denied.isError, true, "capture denial cannot become semantic success");
  assert.equal(denied.code, "adapter", "native permission error must not be a timeout or prior authority rejection");
  assert.equal(denied.content.some(row => row.type === "image"), false);
  assert.equal(denied.content.some(row => row.type === "text" && row.text.includes("Owned visible fixture")), false);
  await until(async () => (await counts()).results === 2);
  assert.deepEqual(await counts(), { reads: 2, inputs: 0, captures: 1, denials: 1,
    settled: 1, decodes: 0, results: 2, pairing: 1, disconnects: 0 });
  const authority = await rpc("lifecycle-authority");
  assert.equal(authority.grants, 1);
  assert.equal(authority.continuations, 1);
  assert.equal(authority.ownedWorkers, 0);
  assert.equal(authority.state.grants.length, 1);
  assert.equal(authority.state.executions.length, 1);
  assert.deepEqual(authority.state.executions[0].key, initial.state.request.key);
  assert.equal(authority.state.executions[0].runId, selectedRun.runId);
  await unchanged(); pass(stage);

  stage = "explicit-semantic-observation-reuses-original-authority";
  phase(stage, "user-explicit-semantic-request-not-automatic-fallback");
  const semanticReply = await call("computer_observe", { ...selectedRun, screenshot: false });
  const fresh = decode(semanticReply);
  assert(fresh.text.includes("Owned visible fixture 你好"));
  assert.equal(fresh.image.pngBase64, undefined);
  assert.equal(semanticReply.content.some(row => row.type === "image"), false);
  assert.notEqual(fresh.snapshotId, original.snapshotId);
  await until(async () => (await counts()).results === 3);
  const oldClick = { ...identity(original, "capture-denied-old-ref", "Fixture action"), action: "click", parameters: {} };
  assert.equal((await call("computer_act", oldClick)).isError, true);
  assert.deepEqual(await counts(), { reads: 3, inputs: 0, captures: 1, denials: 1,
    settled: 1, decodes: 0, results: 3, pairing: 1, disconnects: 0 });
  const finalAuthority = await rpc("lifecycle-authority");
  assert.equal(finalAuthority.grants, 1);
  assert.equal(finalAuthority.continuations, 1);
  assert.deepEqual(finalAuthority.state.grants, authority.state.grants);
  assert.equal((await rpc("shared-count")).count, 3);
  await unchanged(); pass(stage);

  stage = "capture-denied-stop-keeps-original-documents-and-shares";
  phase(stage);
  decode(await call("computer_stop", selectedRun));
  await until(async () => (await rpc("stopped", { runId: selectedRun.runId })).ok);
  assert.equal((await rpc("shared-count")).count, 3);
  await unchanged();
  const final = await counts();
  assert.deepEqual(final, { reads: 3, inputs: 0, captures: 1, denials: 1,
    settled: 1, decodes: 0, results: 3, pairing: 1, disconnects: 0 });
  pass(stage);
  return { ...final, nativePermissionDenied: true, captureJoined: true,
    zeroImagePublication: true, noAutomaticSemanticFallback: true,
    originalAuthorityRetained: true, explicitSemanticSucceeded: true,
    originalDocumentsRetained: true, threeSharesRetained: true, originalStopped: true };
}
