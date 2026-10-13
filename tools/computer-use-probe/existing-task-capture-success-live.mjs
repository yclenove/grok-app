// Original owned CFT154 capture, real App/MCP image and original task preview.
import assert from "node:assert/strict";
import { writeFile } from "node:fs/promises";
import path from "node:path";
import { createHash } from "node:crypto";
import { EXTENSION_ID } from "../computer-use-extension/pairing-client.mjs";
import { armOriginalCaptureReturn, originalCaptureReturnState, releaseOriginalCaptureReturn,
  finishOriginalCaptureReturn, originalCapturePng, originalCaptureOutputPng } from "./existing-task-capture-gate.mjs";

export const captureSuccessScenario = "active-capture-success";
export async function verifyOriginalCaptureSuccess({ rpc, call, decode, until, observe, identity, worker, popup,
  nativeBrowser, profile, fixture, fixtureId, memberB, memberC, selectedRun, initial, pass, phase }) {
  const evidence = { actionDriver: "owned-cdp-test-driver" }; let armed = false; let originalReply;
  const members = [{ id: fixtureId, page: fixture }, memberB, memberC];
  const documents = () => worker.evaluate(async ids => Promise.all(ids.map(async tabId =>
    (await chrome.webNavigation.getFrame({ tabId, frameId: 0 }))?.documentId)), members.map(row => row.id));
  const counts = () => worker.evaluate(() => ({
    reads: globalThis.__jointCalls.filter(row => row.function === "observeDocument").length,
    inputs: globalThis.__jointCalls.filter(row => row.function === "actDocument").length,
    captures: globalThis.__jointCaptures, denials: globalThis.__jointCapturePermissionDenials,
    settled: globalThis.__jointCaptureSettled, decodes: globalThis.__jointCaptureDecodes,
    results: globalThis.__jointObservationResults, pairing: globalThis.__jointPairingConfirmed,
    disconnects: globalThis.__jointDisconnectRequests,
  }));
  const step = name => phase(name, name);
  try {
    step("original-grant-fresh-observation-no-input");
    const first = await observe(); assert(first.text.includes("Owned visible fixture 你好"));
    assert.equal(first.image.pngBase64, undefined);
    const originalDocuments = await documents(); assert(originalDocuments.every(value => typeof value === "string"));
    pass("original-grant-fresh-observation-no-input");

    step("original-successful-capture-owned-action-grant");
    assert.equal((await nativeBrowser.send("Browser.getVersion")).product, "Chrome/154.0.8037.92");
    const active = await worker.evaluate(async () => {
      const [row] = await chrome.tabs.query({ active: true, lastFocusedWindow: true });
      return { id: row.id, url: row.url, windowId: row.windowId };
    });
    assert.equal(active.id, fixtureId);
    const tabs = (await nativeBrowser.send("Target.getTargets", { filter: [{ type: "tab" }] })).targetInfos
      .filter(row => row.type === "tab" && row.url === active.url);
    assert.equal(tabs.length, 1);
    await nativeBrowser.send("Extensions.triggerAction", { id: EXTENSION_ID, targetId: tabs[0].targetId });
    pass("original-successful-capture-owned-action-grant");

    await armOriginalCaptureReturn(worker, { runId: selectedRun.runId, tabId: fixtureId,
      documentId: originalDocuments[0], windowId: active.windowId }, { popup, phase }); armed = true;
    step("original-successful-capture-held-before-physical-settle");
    let replySettled = false;
    originalReply = call("computer_observe", { ...selectedRun, screenshot: true });
    originalReply.then(() => { replySettled = true; }, () => { replySettled = true; });
    await until(async () => (await originalCaptureReturnState(worker)).entered);
    const held = await originalCaptureReturnState(worker); evidence.held = held;
    for (const key of ["documentMatched", "geometryMatched", "requestBound", "ownerPending", "observePending", "scopeMatched", "claimed"])
      assert.equal(held[key], true);
    assert.equal(held.nativeInvokes, 1); assert.equal(held.nativeSettled, 1); assert.equal(held.decodes, 0);
    assert.equal(held.settleInvokes, 0); assert.equal(replySettled, false);
    const authority = await rpc("lifecycle-authority");
    assert.equal(authority.state.executions[0].phase, "busy");
    assert.deepEqual(authority.state.executions[0].key, initial.state.request.key);
    const png = Buffer.from((await originalCapturePng(worker)).slice(22), "base64");
    await writeFile(path.join(profile, "joint-capture-original.png"), png, { flag: "wx" });
    evidence.pngSha256 = createHash("sha256").update(png).digest("hex"); pass("original-successful-capture-held-before-physical-settle");

    step("original-model-image-published-after-scope-join");
    await releaseOriginalCaptureReturn(worker); const reply = await originalReply; const model = decode(reply);
    const image = reply.content.filter(row => row.type === "image"); assert.equal(image.length, 1);
    assert.equal(image[0].mimeType, "image/png");
    assert.equal(model.image.pngBase64, undefined, "MCP sends pixels only in image content");
    await until(async () => (await originalCaptureReturnState(worker)).completionJoined);
    const joined = await originalCaptureReturnState(worker); evidence.joined = joined;
    assert.equal(joined.ownerJoined, true); assert.equal(joined.observeJoined, true);
    assert.equal(joined.settleJoined, true); assert.equal(joined.settleInvokes, 1); assert.equal(joined.statusPending, 0);
    assert.equal(joined.statusInvokes, joined.statusJoined); assert.equal(joined.decodes, 1);
    const output = Buffer.from(await originalCaptureOutputPng(worker), "base64");
    assert.deepEqual(Buffer.from(image[0].data, "base64"), output, "model image must be the original normalized capture");
    assert.equal(output.readUInt32BE(16), held.png.width); assert.equal(output.readUInt32BE(20), held.png.height);
    evidence.outputPngSha256 = createHash("sha256").update(output).digest("hex");
    await writeFile(path.join(profile, "joint-capture-model.png"), output, { flag: "wx" });
    evidence.modelImageSucceeded = true; pass("original-model-image-published-after-scope-join");

    step("original-task-preview-preserves-model-ref");
    const preview = await rpc("capture-preview"); evidence.preview = preview;
    assert.equal(preview.pngSha256, evidence.outputPngSha256); assert.equal(preview.pngBytes, output.length);
    assert.equal(preview.width, held.png.width); assert.equal(preview.height, held.png.height);
    assert.notEqual(preview.snapshotId, model.snapshotId);
    assert.deepEqual(preview.address.key, initial.state.request.key); assert.equal(preview.address.runId, selectedRun.runId);
    const click = decode(await call("computer_act", { ...identity(model, "capture-model-ref-after-preview", "Fixture action"), action: "click", parameters: {} }));
    assert.equal(click.kind, "applied"); assert.equal(await fixture.evaluate(() => globalThis.clicks), 1);
    for (const { id, page } of [memberB, memberC]) {
      assert.equal(page.isClosed(), false); assert.equal(await page.evaluate(() => globalThis.clicks), 0);
      assert.equal(await worker.evaluate(tabId => globalThis.__jointCalls.filter(row => row.tab === tabId
        && ["observeDocument", "actDocument"].includes(row.function)).length, id), 0);
    }
    const after = await rpc("lifecycle-authority");
    assert.equal(after.grants, 1); assert.equal(after.continuations, 1); assert.equal(after.ownedWorkers, 0);
    evidence.previewImageSucceeded = true; evidence.modelRefSurvivedPreview = true; evidence.originalAuthorityRetained = true;
    pass("original-task-preview-preserves-model-ref");

    step("original-successful-capture-stop-keeps-shares");
    decode(await call("computer_stop", selectedRun)); await until(async () => (await rpc("stopped", { runId: selectedRun.runId })).ok);
    assert.equal((await rpc("shared-count")).count, 3); assert.deepEqual(await documents(), originalDocuments);
    evidence.originalDocumentsRetained = true; evidence.threeSharesRetained = true; evidence.originalStopped = true;
    evidence.counts = await counts(); pass("original-successful-capture-stop-keeps-shares");
    return evidence;
  } catch (error) {
    await writeFile(path.join(profile, "joint-capture-success-primary.json"), JSON.stringify({
      errorClass: error?.code === "ERR_ASSERTION" ? "AssertionError" : "Error", evidence,
    }) + "\n", { flag: "wx" });
    throw error;
  } finally {
    if (armed) await finishOriginalCaptureReturn(worker);
    if (originalReply) await originalReply.catch(() => {});
  }
}
