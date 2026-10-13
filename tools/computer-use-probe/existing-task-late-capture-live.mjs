// Real owned CFT action grant -> original App/MCP observation -> original Host fence.
import assert from "node:assert/strict";
import { writeFile } from "node:fs/promises";
import path from "node:path";
import { createHash } from "node:crypto";
import { EXTENSION_ID } from "../computer-use-extension/pairing-client.mjs";
import { armOriginalCaptureReturn, originalCaptureReturnState, releaseOriginalCaptureReturn,
  finishOriginalCaptureReturn, originalCapturePng } from "./existing-task-capture-gate.mjs";
import { originalPageEffects, expectedPageEffects } from "./existing-task-late-action-spec.mjs";
import { verifyEarlyCaptureContinue } from "./existing-task-capture-wait-live.mjs";

export const lateCaptureScenarios = Object.freeze([
  "active-late-capture-pause", "active-late-capture-chat-stop",
  "active-late-capture-early-continue", "active-late-capture-early-stop",
]);

export async function verifyOriginalLateCapture({ scenario, rpc, call, decode, until, observe, identity,
  fixture, fixtureId, memberB, memberC, selectedRun, worker, popup, nativeBrowser, profile, initial, pass, phase }) {
  assert(lateCaptureScenarios.includes(scenario));
  const chatStop = scenario.endsWith("chat-stop");
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
  const unchanged = async originalDocuments => {
    for (const { page } of members) {
      assert.equal(page.isClosed(), false);
      assert.deepEqual(await originalPageEffects(page), expectedPageEffects({ action: "click" }, false));
    }
    assert.deepEqual(await documents(), originalDocuments);
    assert.equal(await worker.evaluate(ids => globalThis.__jointCalls.filter(row => ids.includes(row.tab)
      && ["observeDocument", "actDocument"].includes(row.function)).length, [memberB.id, memberC.id]), 0);
  };
  let stage = "original-grant-fresh-observation-no-input";
  const step = (name, detail = name) => { stage = name; phase(name, detail); };
  const evidence = { interruption: chatStop ? "chat-stop" : "pause", actionDriver: "owned-cdp-test-driver" };
  let originalReply, replySettled = false, armed = false;
  try {
    step(stage);
    const old = await observe();
    assert(old.text.includes("Owned visible fixture 你好"));
    const oldAction = { ...identity(old, "old-ref-after-capture-interruption", "Fixture action"),
      action: "click", parameters: {} };
    const originalDocuments = await documents();
    assert(originalDocuments.every(value => typeof value === "string"));
    const before = await counts();
    assert.deepEqual(before, { reads: 1, inputs: 0, captures: 0, denials: 0,
      settled: 0, decodes: 0, results: 1, pairing: 1, disconnects: 0 });
    await unchanged(originalDocuments); pass(stage);

    step("original-capture-owned-action-grant", "original-browser-extension-action");
    assert(nativeBrowser, "capture requires the original owned browser-level driver");
    const version = await nativeBrowser.send("Browser.getVersion");
    evidence.browserVersion = version.product;
    assert.equal(version.product, "Chrome/154.0.8037.92", "independent pinned CFT154 required");
    const active = await worker.evaluate(async () => {
      const [row] = await chrome.tabs.query({ active: true, lastFocusedWindow: true });
      return { id: row.id, url: row.url, windowId: row.windowId };
    });
    assert.equal(active.id, fixtureId);
    const tabs = (await nativeBrowser.send("Target.getTargets", { filter: [{ type: "tab" }] })).targetInfos
      .filter(row => row.type === "tab" && row.url === active.url);
    assert.equal(tabs.length, 1, "original capture fixture tab target must be unique");
    await nativeBrowser.send("Extensions.triggerAction", { id: EXTENSION_ID, targetId: tabs[0].targetId });
    assert.deepEqual(await worker.evaluate(async () => {
      const [row] = await chrome.tabs.query({ active: true, lastFocusedWindow: true });
      return { id: row.id, url: row.url, windowId: row.windowId };
    }), active);
    await unchanged(originalDocuments);
    assert.deepEqual(await counts(), before);
    evidence.ownedActionGranted = true; pass(stage);

    await armOriginalCaptureReturn(worker, { runId: selectedRun.runId, tabId: fixtureId,
      documentId: originalDocuments[0], windowId: active.windowId }, { popup, phase: step });
    armed = true;
    step("original-capture-native-return-held", "original-explicit-screenshot-request");
    originalReply = call("computer_observe", { ...selectedRun, screenshot: true });
    originalReply.then(() => { replySettled = true; }, () => { replySettled = true; });
    await until(async () => (await originalCaptureReturnState(worker)).entered);
    const held = await originalCaptureReturnState(worker);
    evidence.held = held;
    for (const field of ["documentMatched", "geometryMatched", "requestBound", "observePending", "ownerPending", "scopeMatched", "claimed", "originalWrappersRetained"])
      assert.equal(held[field], true);
    assert.equal(held.observeJoined, false);
    assert.equal(held.matched, 1);
    assert.equal(held.nativeInvokes, 1);
    assert.equal(held.nativeSettled, 1);
    assert.equal(held.captures, 1);
    assert.equal(held.settled, 1);
    assert.equal(held.denials, 0);
    assert.equal(held.decodes, 0);
    assert(held.png.bytes > 33 && held.png.width > 0 && held.png.height > 0);
    const png = Buffer.from((await originalCapturePng(worker)).slice(22), "base64");
    assert.equal(png.length, held.png.bytes);
    assert.equal(png.readUInt32BE(16), held.png.width);
    assert.equal(png.readUInt32BE(20), held.png.height);
    await writeFile(path.join(profile, "joint-capture-original.png"), png, { flag: "wx" });
    evidence.pngSha256 = createHash("sha256").update(png).digest("hex");
    const pin = await rpc("late-capture-held");
    assert.deepEqual(pin.state.executions[0].key, initial.state.request.key);
    assert.equal(pin.state.executions[0].runId, selectedRun.runId);
    assert.equal(pin.existingIdle, false);
    await unchanged(originalDocuments); pass(stage);

    step("original-capture-interrupted-without-authority", "original-capture-host-interruption");
    assert.equal((await rpc("late-capture-interrupt")).started, true);
    await until(async () => (await rpc("late-capture-state")).executable === 0);
    const fenced = await rpc("late-capture-state");
    const fencedNative = await originalCaptureReturnState(worker);
    evidence.hostIdleBeforeRelease = fenced.existingIdle;
    evidence.hostCleanupPendingBeforeRelease = fenced.state.executions[0].cleanupPending;
    assert.equal(evidence.hostCleanupPendingBeforeRelease, true,
      "Host must report the original retained capture cleanup while PNG is held");
    evidence.hostReplySettledBeforeRelease = replySettled;
    evidence.fenced = fencedNative;
    assert.equal(fenced.pendingContinuation, false);
    assert.equal(fenced.state.executions[0].hasFreshObservation, false);
    assert.equal(fencedNative.observePending, true);
    assert.equal(fencedNative.returned, false);
    assert.equal(fencedNative.decodes, 0);
    pass(stage);

    if (scenario.includes("-early-")) {
      return await verifyEarlyCaptureContinue({ stopped: scenario.endsWith("early-stop"), rpc, call, decode,
        until, observe, identity, worker, originalReply, originalDocuments, unchanged, counts, pin, oldAction,
        selectedRun, fixture, memberB, memberC, evidence, pass, step });
    }

    step("original-capture-late-return-rejected-and-joined", "original-capture-release-and-join");
    await releaseOriginalCaptureReturn(worker);
    const reply = await originalReply;
    evidence.originalMcpError = reply.isError === true;
    evidence.originalMcpImage = reply.content.some(row => row.type === "image");
    evidence.originalMcpOldText = reply.content.some(row => row.type === "text"
      && row.text.includes("Owned visible fixture"));
    await until(async () => {
      const state = await originalCaptureReturnState(worker);
      return state.observeJoined && state.ownerJoined && state.settleJoined && !state.settlePending
        && state.statusPending === 0 && !state.completionPending;
    });
    const final = await originalCaptureReturnState(worker);
    evidence.final = final;
    evidence.counts = await counts();
    const joined = await rpc("late-capture-finish");
    evidence.originalWaiterJoined = joined.originalWaiterJoined;
    evidence.hostIdleAfterJoin = joined.existingIdle;
    // Save a sanitized native witness before any expectation can become first failure.
    process.stderr.write(`original late capture native witness: ${JSON.stringify(evidence)}\n`);
    assert.equal(evidence.hostIdleBeforeRelease, false, "Host must not report idle while original capture return is held");
    assert.equal(final.decodes, 0, "retired original PNG must be dropped before decoding");
    assert.equal(final.observationReturned, false, "retired observation must not return success to transport");
    assert.equal(final.completionImage, false, "retired image must not be posted to Host");
    assert.equal(final.ownerJoined, true); assert.equal(final.ownerPending, false);
    assert.equal(final.settleInvokes, 1); assert.equal(final.settleJoined, true);
    assert.equal(final.statusPending, 0); assert(final.statusInvokes > 0); assert.equal(final.statusInvokes, final.statusJoined);
    assert.equal(evidence.originalMcpError, true);
    assert.equal(evidence.originalMcpImage, false);
    assert.equal(evidence.originalMcpOldText, false);
    assert.equal(final.nativeInvokes, 1);
    assert.equal(final.nativeSettled, 1);
    assert.equal(final.matched, 1);
    assert.equal(final.returned, true);
    assert.equal(evidence.hostIdleAfterJoin, true);
    assert.equal(joined.state.executions[0].cleanupPending, false);
    assert.equal(joined.pendingContinuation, false);
    assert.equal(joined.executable, 0);
    assert.equal(joined.originalWaiterJoined, true);
    if (chatStop) assert.equal(joined.originalStopped, true);
    else assert.equal(joined.state.executions[0].phase, "paused");
    await unchanged(originalDocuments); pass(stage);

    step("original-capture-old-delivery-and-shares-retained", "original-capture-replay");
    assert.equal((await call("computer_act", oldAction)).isError, true);
    for (const screenshot of [false, true])
      assert.equal((await call("computer_observe", { ...selectedRun, screenshot })).isError, true);
    assert.equal((await rpc("late-capture-replay")).replayChecked, true);
    assert.equal((await rpc("shared-count")).count, 3);
    assert.deepEqual(await counts(), evidence.counts);
    assert.equal(evidence.counts.inputs, 0);
    assert.equal(evidence.counts.pairing, 1);
    assert.equal(evidence.counts.disconnects, 0);
    await unchanged(originalDocuments); pass(stage);
    evidence.oldDeliveryNotRevived = true;
    evidence.originalDocumentsRetained = true;
    evidence.threeSharesRetained = true;
    evidence.zeroPeerWire = true;

    if (!chatStop) {
      step("original-capture-new-explicit-continue", "original-capture-new-continue");
      const resumed = await rpc("late-capture-resume");
      assert.equal(resumed.outcome.phase, "resumed");
      assert.equal(resumed.executable, 1);
      assert.equal(resumed.pendingContinuation, true);
      assert.equal(resumed.state.executions[0].hasFreshObservation, true);
      assert.notEqual(resumed.state.executions[0].generation, pin.state.executions[0].generation);
      assert.equal((await rpc("delivery")).ok, true);
      assert.equal((await call("computer_act", { ...oldAction, actionId: "old-ref-after-capture-continue" })).isError, true);
      const resumedCounts = await counts();
      assert.equal(resumedCounts.reads, evidence.counts.reads + 1);
      for (const field of ["captures", "denials", "settled", "decodes", "inputs", "pairing", "disconnects"])
        assert.equal(resumedCounts[field], evidence.counts[field]);
      await unchanged(originalDocuments); pass(stage);
      evidence.newExplicitContinue = true;
    }
    step("original-capture-final-stop-keeps-shares", "original-capture-final-stop");
    if (!chatStop) decode(await call("computer_stop", selectedRun));
    await until(async () => (await rpc("stopped", { runId: selectedRun.runId })).ok);
    assert.equal((await rpc("shared-count")).count, 3);
    await unchanged(originalDocuments); pass(stage);
    evidence.originalStopped = true;
    return evidence;
  } catch (error) {
    // Preserve the original cut before releasing held native work or outer teardown.
    await writeFile(path.join(profile, "joint-capture-primary.json"), JSON.stringify({
      stage, errorClass: error?.code === "ERR_ASSERTION" ? "AssertionError" : "Error", evidence,
    }) + "\n", { flag: "wx" });
    throw error;
  } finally {
    if (armed) await finishOriginalCaptureReturn(worker);
    if (originalReply) await originalReply.catch(() => {});
  }
}
