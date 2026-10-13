// Private owned-worker cut. Keep the original request and capture Promise in MV3.
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";

export async function armOriginalCaptureReturn(worker, expected, { popup, phase }) {
  for (const [name, file, marker, offset] of [
    ["SharedTabs", "shared-tabs.mjs", "const query = () => this.tabs.query(", 0],
    ["PairingClient", "pairing-client.mjs", "async pollExtension(epoch, signal)", 1],
    ["ObservationCompletions", "extension-transport.mjs", "request = await this.client.pollExtension", 0],
  ]) {
    const source = await readFile(new URL(`../computer-use-extension/${file}`, import.meta.url), "utf8");
    const matches = source.split(/\r?\n/).flatMap((row, index) => row.includes(marker) ? [index] : []);
    assert.equal(matches.length, 1, "one exact original capture module cut required");
    phase("original-capture-source-module-arm", `original-capture-bind-${name}`);
    const cue = name === "SharedTabs" ? () => popup.evaluate(() =>
      chrome.runtime.sendMessage({ type: "cu-tab-state" })) : undefined;
    assert.equal(await worker.captureActionModuleBinding(name, new URL(file, worker.url()).href,
      matches[0] + offset, cue), true);
  }
  await worker.evaluate(binding => {
    if (globalThis.__jointCaptureGate) throw new Error("original capture gate repeated");
    const { SharedTabs, PairingClient, ObservationCompletions } = globalThis.__jointActionTypes;
    let release;
    const held = new Promise(resolve => { release = resolve; });
    const gate = { expected: binding, release, released: false, entered: false, returned: false,
      matched: 0, nativeInvokes: 0, nativeSettled: 0, documentMatched: false,
      observePending: false, observeJoined: false, observationReturned: false,
      completionInvokes: 0, completionPending: false, completionJoined: false,
      ownerPending: false, ownerJoined: false, statusPending: 0, statusInvokes: 0, statusJoined: 0,
      settleInvokes: 0, settlePending: false, settleJoined: false, scopeMatched: false, claimed: false,
      completionImage: false, wrappers: [], originals: [], successorReads: [] };
    const matches = request => request?.runId === binding.runId && request.tabId === String(binding.tabId)
      && request.documentId === binding.documentId && request.command?.kind === "observe" && request.command.screenshot === true
      && request.command.preview === false;
    const sameRequest = request => matches(request) && gate.request
      && ["protocol", "requestId", "sequence", "deadlineMs", "session", "runId", "tabId", "documentId", "documentGeneration", "grantGeneration"]
        .every(key => request[key] === gate.request[key])
      && ["instanceId", "connectionNonce", "generation"].every(key => request.connection[key] === gate.request.connection[key])
      && request.command.snapshotId === gate.request.command.snapshotId
      && request.completion.purpose === "observation"
      && request.completion.proof.completionKey === gate.request.completion.proof.completionKey;
    const wrap = (prototype, name, factory) => {
      const original = prototype[name]; const wrapper = factory(original);
      prototype[name] = wrapper;
      gate.wrappers.push({ prototype, name, original, wrapper });
    };
    wrap(ObservationCompletions.prototype, "run", original => function (epoch, request, signal) {
      if (!matches(request)) return original.call(this, epoch, request, signal);
      if (gate.request) throw new Error("original observation owner repeated");
      gate.request = request; gate.epoch = epoch; gate.ownerPending = true;
      const task = original.call(this, epoch, request, signal); gate.originals.push(task);
      return task.finally(() => { gate.ownerPending = false; gate.ownerJoined = true; });
    });
    wrap(PairingClient.prototype, "observationScope", original => function (epoch, completion, journal) {
      const scope = original.call(this, epoch, completion, journal);
      if (completion.proof.binding.requestId !== gate.request?.requestId) return scope;
      if (gate.scope || epoch !== gate.epoch || completion.purpose !== "observation"
        || completion.proof.binding.documentId !== binding.documentId
        || completion.proof.binding.snapshotId !== gate.request.command.snapshotId) throw new Error("original observation scope changed");
      gate.scope = scope; gate.scopeMatched = true;
      const status = scope.status; const settle = scope.settle;
      scope.status = function (...args) {
        gate.statusInvokes++; gate.statusPending++;
        const task = status.apply(this, args); gate.originals.push(task);
        return task.finally(() => { gate.statusPending--; gate.statusJoined++; });
      };
      scope.settle = function (...args) {
        if (gate.observePending || !gate.observeJoined || gate.statusPending !== 0 || gate.settleInvokes++ !== 0)
          throw new Error("original observer or watcher not physically joined before settle");
        gate.settlePending = true;
        const task = settle.apply(this, args); gate.originals.push(task);
        return task.finally(() => { gate.settlePending = false; gate.settleJoined = true; });
      };
      return scope;
    });
    wrap(PairingClient.prototype, "claimObservation", original => async function (epoch, completion, signal) {
      const result = await original.call(this, epoch, completion, signal);
      if (completion.proof.binding.requestId === gate.request?.requestId) {
        if (!gate.scopeMatched || gate.claimed) throw new Error("original observation claim changed");
        gate.claimed = true;
      }
      return result;
    });
    wrap(SharedTabs.prototype, "observe", original => function (request, ...rest) {
      if (!matches(request)) {
        if (gate.entered && request.runId === binding.runId && request.tabId === String(binding.tabId)
          && request.command?.screenshot === false && request.command.preview === false) {
          gate.successorReads.push({ ownerJoined: gate.ownerJoined, observeJoined: gate.observeJoined,
            settleJoined: gate.settleJoined, statusPending: gate.statusPending,
            snapshotChanged: request.command.snapshotId !== gate.request.command.snapshotId });
        }
        return original.call(this, request, ...rest);
      }
      if (!sameRequest(request) || !gate.claimed || gate.observePending) throw new Error("original capture observation repeated or unbound");
      gate.tabs = this; gate.observePending = true;
      gate.signal = rest[0];
      const scripting = this.scripting; const tabs = this.tabs;
      const scripts = { ...scripting, executeScript: async input => {
        const rows = await scripting.executeScript(input);
        if (input.func?.name === "inspectDocument" && input.target.tabId === binding.tabId
          && input.target.documentIds?.[0] === binding.documentId && rows.length === 1
          && rows[0].frameId === 0 && rows[0].documentId === binding.documentId) {
          gate.view = rows[0].result;
        }
        if (input.func?.name === "observeDocument") {
          if (gate.observedDocument) throw new Error("original capture document reader repeated");
          gate.observedDocument = true;
          gate.documentMatched = input.target.tabId === binding.tabId
            && input.target.documentIds?.length === 1 && input.target.documentIds[0] === binding.documentId
            && input.args?.[0] === request.command.snapshotId && input.args?.[1] === true
            && rows.length === 1 && rows[0].frameId === 0 && rows[0].documentId === binding.documentId
            && rows[0].result?.snapshotId === request.command.snapshotId;
        }
        return rows;
      } };
      const captureTabs = { ...tabs, captureVisibleTab: async (...args) => {
        if (!gate.documentMatched || !gate.observePending || args[0] !== binding.windowId
          || args[1]?.format !== "png" || gate.nativeInvokes++ !== 0) {
          throw new Error("original native capture binding changed");
        }
        // Await precisely the original native API before holding its actual return.
        const task = tabs.captureVisibleTab(...args); gate.originals.push(task);
        let dataUrl;
        try { dataUrl = await task; } finally { gate.nativeSettled++; }
        if (typeof dataUrl !== "string" || !dataUrl.startsWith("data:image/png;base64,")
          || dataUrl.length > 24 * 1024 * 1024) throw new Error("original native PNG missing");
        const raw = atob(dataUrl.slice(22));
        const bytes = Uint8Array.from(raw.slice(0, 24), char => char.charCodeAt(0));
        if (bytes.length < 24) throw new Error("original native PNG header missing");
        const header = new DataView(bytes.buffer);
        if (header.getUint32(0) !== 0x89504e47 || header.getUint32(4) !== 0x0d0a1a0a
          || header.getUint32(8) !== 13 || header.getUint32(12) !== 0x49484452) {
          throw new Error("original native PNG header changed");
        }
        gate.png = { bytes: raw.length, width: header.getUint32(16), height: header.getUint32(20) };
        gate.originalPng = dataUrl;
        gate.geometryMatched = Number.isFinite(gate.view?.pixelRatio) && gate.view.pixelRatio > 0
          && gate.view.visualScale === 1 && gate.view.visualX === 0 && gate.view.visualY === 0
          && Math.abs(gate.png.width - gate.view.viewportWidth * gate.view.pixelRatio) <= 2
          && Math.abs(gate.png.height - gate.view.viewportHeight * gate.view.pixelRatio) <= 2;
        gate.matched++; gate.entered = true;
        await held;
        gate.returned = true;
        return dataUrl;
      } };
      this.scripting = scripts; this.tabs = captureTabs;
      const task = original.call(this, request, ...rest); gate.observeTask = task; gate.originals.push(task);
      task.then(observation => { gate.observationReturned = true; gate.normalizedPng = observation.screenshot?.pngBase64; }, () => {}).finally(() => {
        gate.observePending = false; gate.observeJoined = true;
        if (this.scripting === scripts) this.scripting = scripting;
        if (this.tabs === captureTabs) this.tabs = tabs;
      });
      return task;
    });
    wrap(PairingClient.prototype, "completeExtension", original => function (epoch, result, signal) {
      if (!sameRequest(result.request)) return original.call(this, epoch, result, signal);
      if (gate.completionInvokes++ !== 0) throw new Error("original capture completion repeated");
      gate.completionPending = true;
      gate.completionImage = typeof result.outcome?.observation?.screenshot?.pngBase64 === "string";
      const task = original.call(this, epoch, result, signal); gate.originals.push(task);
      task.then(() => {}, () => {}).finally(() => {
        gate.completionPending = false; gate.completionJoined = true;
      });
      return task;
    });
    globalThis.__jointCaptureGate = gate;
  }, expected);
}

export async function originalCaptureReturnState(worker) {
  return worker.evaluate(() => {
    const gate = globalThis.__jointCaptureGate;
    if (!gate) throw new Error("original capture gate missing");
    return { entered: gate.entered, released: gate.released, returned: gate.returned,
      matched: gate.matched, nativeInvokes: gate.nativeInvokes, nativeSettled: gate.nativeSettled,
      documentMatched: gate.documentMatched, observePending: gate.observePending,
      geometryMatched: gate.geometryMatched === true,
      observeJoined: gate.observeJoined, observationReturned: gate.observationReturned,
      completionInvokes: gate.completionInvokes, completionPending: gate.completionPending,
      completionJoined: gate.completionJoined, completionImage: gate.completionImage,
      ownerPending: gate.ownerPending, ownerJoined: gate.ownerJoined,
      scopeMatched: gate.scopeMatched, claimed: gate.claimed, statusPending: gate.statusPending,
      statusInvokes: gate.statusInvokes, statusJoined: gate.statusJoined,
      settleInvokes: gate.settleInvokes, settlePending: gate.settlePending, settleJoined: gate.settleJoined,
      signalAborted: gate.signal?.aborted === true, png: gate.png ?? null,
      successorReads: gate.successorReads,
      requestBound: typeof gate.request?.requestId === "string" && Number.isSafeInteger(gate.request?.sequence)
        && typeof gate.request?.command?.snapshotId === "string",
      originalWrappersRetained: gate.wrappers.every(row => row.prototype[row.name] === row.wrapper),
      captures: globalThis.__jointCaptures, decodes: globalThis.__jointCaptureDecodes,
      settled: globalThis.__jointCaptureSettled, denials: globalThis.__jointCapturePermissionDenials };
  });
}

export async function releaseOriginalCaptureReturn(worker) {
  await worker.evaluate(() => {
    const gate = globalThis.__jointCaptureGate;
    if (!gate?.entered || gate.released) throw new Error("original capture release misplaced");
    gate.released = true; gate.release();
  });
}

export async function originalCapturePng(worker) {
  return worker.evaluate(() => {
    const gate = globalThis.__jointCaptureGate;
    if (!gate?.entered || gate.matched !== 1 || !gate.geometryMatched || gate.released) {
      throw new Error("original capture PNG is not held at the validated cut");
    }
    return gate.originalPng;
  });
}

export async function finishOriginalCaptureReturn(worker) {
  await worker.evaluate(async () => {
    const gate = globalThis.__jointCaptureGate;
    if (!gate) return;
    if (!gate.released) { gate.released = true; gate.release(); }
    await Promise.allSettled(gate.originals);
    for (const row of gate.wrappers) {
      if (row.prototype[row.name] !== row.wrapper) throw new Error("original capture wrapper replaced");
      row.prototype[row.name] = row.original;
    }
    delete globalThis.__jointCaptureGate; delete globalThis.__jointActionTypes;
  });
}

export async function originalCaptureOutputPng(worker) {
  return worker.evaluate(() => {
    const gate = globalThis.__jointCaptureGate;
    if (!gate?.scopeMatched || !gate.observationReturned || !gate.ownerJoined || !gate.settleJoined
      || typeof gate.normalizedPng !== "string") throw new Error("original capture output not physically joined");
    return gate.normalizedPng;
  });
}
