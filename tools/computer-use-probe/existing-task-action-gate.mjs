// Private owned MV3 instrumentation. Original request/proof/Promises stay in this worker.
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";

export async function armOriginalActionReturn(worker, binding, { popup, phase }) {
  // MV3 service workers cannot dynamically import modules. Capture each ORIGINAL
  // statically imported class at an exact source module frame, then resume it.
  for (const [name, file, marker] of [
    ["SharedTabs", "shared-tabs.mjs", "const query = () => this.tabs.query("],
    ["PairingClient", "pairing-client.mjs", "async pollAction(epoch, signal)"],
    ["ActionCompletions", "action-transport.mjs", "dispatch = await this.#client.pollAction"],
  ]) {
    const source = await readFile(new URL(`../computer-use-extension/${file}`, import.meta.url), "utf8");
    const rows = source.split(/\r?\n/);
    const matches = rows.flatMap((row, index) => row.includes(marker) ? [index] : []);
    assert.equal(matches.length, 1, "one exact original module cut required");
    const line = matches[0] + (name === "PairingClient" ? 1 : 0);
    phase("original-action-source-module-arm", `original-action-bind-${name}`);
    // current() is event-driven. Ask the actual owned popup for passive tab state;
    // this reads original metadata only, never Share/observe/grant or new refs.
    const cue = name === "SharedTabs" ? () => popup.evaluate(() =>
      chrome.runtime.sendMessage({ type: "cu-tab-state" })) : undefined;
    assert.equal(await worker.captureActionModuleBinding(name, new URL(file, worker.url()).href, line, cue), true);
  }
  await worker.evaluate(expected => {
    if (globalThis.__jointActionGate) throw new Error("original action gate already exists");
    const { SharedTabs, ActionCompletions, PairingClient } = globalThis.__jointActionTypes;
    let release;
    const held = new Promise(resolve => { release = resolve; });
    const gate = { expected, release, entered: false, released: false, returned: false,
      matched: 0, nativeInvokes: 0, cancelInvokes: 0, cancelJoined: 0, otherReaders: 0,
      claimed: false, documentMatched: false, actPending: false, completionPending: false,
      originalActJoined: false, originalCompletionJoined: false, nativeStatus: null, nativeDetail: null,
      wrappers: [], originals: [] };
    const matches = request => request?.runId === expected.runId && request.tabId === String(expected.tabId)
      && request.documentId === expected.documentId && request.command?.snapshotId === expected.snapshotId
      && request.command.action === expected.action;
    const wrap = (prototype, name, fn) => {
      const original = prototype[name];
      const wrapper = fn(original);
      prototype[name] = wrapper;
      gate.wrappers.push({ prototype, name, original, wrapper });
    };
    wrap(PairingClient.prototype, "pollAction", original => async function (...args) {
      const dispatch = await original.apply(this, args);
      if (dispatch && matches(dispatch.request)) {
        if (gate.dispatch) throw new Error("original action dispatched twice");
        gate.dispatch = dispatch; gate.client = this; gate.epoch = args[0];
      }
      return dispatch;
    });
    wrap(PairingClient.prototype, "claimAction", original => async function (epoch, dispatch, signal) {
      const reply = await original.call(this, epoch, dispatch, signal);
      if (matches(dispatch.request)) {
        if (dispatch.request.requestId !== gate.dispatch?.request.requestId) throw new Error("original claim changed");
        gate.claimed = true;
      }
      return reply;
    });
    wrap(PairingClient.prototype, "completionScope", original => function (epoch, proof, journal) {
      const scope = original.call(this, epoch, proof, journal);
      if (proof.binding.requestId === gate.dispatch?.request.requestId) {
        // Production creates a transient scope before consulting its finished ledger.
        // A replay after join must reach that ledger; keep the ORIGINAL pin unchanged.
        if (gate.scope && !gate.originalCompletionJoined) throw new Error("original scope replaced");
        gate.scope ??= scope;
      }
      return scope;
    });
    for (const name of ["share", "observe"]) wrap(SharedTabs.prototype, name, original => async function (...args) {
      gate.otherReaders++;
      try { return await original.apply(this, args); } finally { gate.otherReaders--; }
    });
    wrap(ActionCompletions.prototype, "run", original => function (epoch, request, proof, signal) {
      if (!matches(request)) return original.call(this, epoch, request, proof, signal);
      if (gate.completions) throw new Error("original completion owner repeated");
      // The same original transport may already have started its poll before arming.
      // Pin its real returned request/proof at completion admission in that case.
      gate.dispatch ??= { request, proof }; gate.epoch = epoch;
      gate.completions = this; gate.completionPending = true;
      const task = original.call(this, epoch, request, proof, signal);
      gate.completionTask = task; gate.originals.push(task);
      task.then(() => { gate.completionPending = false; gate.originalCompletionJoined = true; },
        () => { gate.completionPending = false; });
      return task;
    });
    wrap(SharedTabs.prototype, "act", original => function (request, signal) {
      if (!matches(request)) return original.call(this, request, signal);
      if (!gate.claimed || gate.actTask || request.requestId !== gate.dispatch.request.requestId) {
        throw new Error("original action owner changed");
      }
      gate.tabs = this; gate.signal = signal; gate.actPending = true;
      const scripting = this.scripting;
      // Only this exact SharedTabs instance's original act owns this facade.
      // Share/observe are counted independently; any competing document reader fails the witness.
      const facade = { ...scripting, executeScript: async input => {
        const target = input.target;
        const bound = target.tabId === expected.tabId && target.documentIds?.length === 1
          && target.documentIds[0] === expected.documentId;
        const command = input.args?.[0];
        const act = bound && input.func?.name === "actDocument" && command?.operationId === request.requestId
          && command.snapshotId === expected.snapshotId && command.action === expected.action;
        const pre = bound && input.func?.name === "inspectDocument" && gate.actPending && gate.otherReaders === 0;
        const cancel = bound && input.func?.name === "cancelDocumentOperation"
          && input.args?.[0] === expected.snapshotId && input.args?.[1] === request.requestId;
        if (act) gate.nativeInvokes++;
        if (cancel) gate.cancelInvokes++;
        const task = scripting.executeScript(input);
        if (cancel) gate.originals.push(task);
        const rows = await task;
        if (cancel) gate.cancelJoined++;
        if ((expected.cut === "pre" && pre) || (expected.cut === "post" && act)) {
          if (gate.matched++ !== 0 || gate.otherReaders !== 0) throw new Error("original action cut ambiguous");
          gate.documentMatched = rows.length === 1 && rows[0].frameId === 0
            && rows[0].documentId === expected.documentId;
          if (act) { gate.nativeStatus = rows[0].result?.status; gate.nativeDetail = rows[0].result?.detail; }
          gate.entered = true;
          await held;
          gate.returned = true;
        }
        return rows;
      } };
      this.scripting = facade;
      const task = original.call(this, request, signal);
      gate.actTask = task; gate.originals.push(task);
      task.then(result => { gate.actResult = result; gate.originalActJoined = true; }, () => {}).finally(() => {
        gate.actPending = false;
        if (this.scripting === facade) this.scripting = scripting;
      });
      return task;
    });
    globalThis.__jointActionGate = gate;
  }, binding);
}

export async function originalActionReturnState(worker) {
  return worker.evaluate(async () => {
    const gate = globalThis.__jointActionGate;
    if (!gate) throw new Error("original action gate missing");
    const scope = gate.scope ? await gate.scope.status().catch(() => null) : null;
    return { matched: gate.matched, entered: gate.entered, released: gate.released, returned: gate.returned,
      claimed: gate.claimed, documentMatched: gate.documentMatched, otherReaders: gate.otherReaders,
      nativeInvokes: gate.nativeInvokes, nativeStatus: gate.nativeStatus, nativeDetail: gate.nativeDetail,
      originalActResult: gate.actResult ? { status: gate.actResult.status,
        detail: gate.actResult.detail, physicallySettled: gate.actResult.physicallySettled === true } : null,
      cancelInvokes: gate.cancelInvokes, cancelJoined: gate.cancelJoined,
      actPending: gate.actPending, completionPending: gate.completionPending,
      originalActJoined: gate.originalActJoined, originalCompletionJoined: gate.originalCompletionJoined,
      signalAborted: gate.signal?.aborted === true, scope,
      actionsIdle: gate.tabs?.actionsIdle() === true, completionsIdle: gate.completions?.idle() === true,
      originalWrappersRetained: gate.wrappers.every(row => row.prototype[row.name] === row.wrapper) };
  });
}

export async function releaseOriginalActionReturn(worker) {
  await worker.evaluate(() => {
    const gate = globalThis.__jointActionGate;
    if (!gate?.entered || gate.released) throw new Error("original action release misplaced");
    gate.released = true; gate.release();
  });
}

export async function replayOriginalCompletion(worker) {
  return worker.evaluate(async () => {
    const gate = globalThis.__jointActionGate;
    if (!gate?.originalCompletionJoined || !gate.completions.idle()) throw new Error("original completion not joined");
    // Restore only run wrapper to allow the same ORIGINAL finished ledger to reject its own replay.
    const row = gate.wrappers.find(row => row.name === "run");
    const result = await row.original.call(gate.completions, gate.epoch,
      gate.dispatch.request, gate.dispatch.proof, new AbortController().signal);
    return { status: result.status, detail: result.detail };
  });
}

export async function finishOriginalActionReturn(worker) {
  await worker.evaluate(async () => {
    const gate = globalThis.__jointActionGate;
    if (!gate) return;
    if (!gate.released) { gate.released = true; gate.release(); }
    await Promise.allSettled(gate.originals);
    for (const row of gate.wrappers) {
      if (row.prototype[row.name] !== row.wrapper) throw new Error("original action wrapper replaced");
      row.prototype[row.name] = row.original;
    }
    delete globalThis.__jointActionGate;
    delete globalThis.__jointActionTypes;
  });
}
