import { copyCompletionProof, exactKeys, positive, uuid } from "./completion-scope.mjs";

// Version-2 action shape; Host claim additionally compares the complete queued command.
export function copyBoundAction(request, suppliedProof, now = Date.now()) {
  const proof = copyCompletionProof(suppliedProof);
  const binding = proof.binding;
  const command = request?.command;
  if (!exactKeys(request, ["protocol", "requestId", "sequence", "deadlineMs", "connection", "session", "runId",
    "tabId", "documentId", "documentGeneration", "grantGeneration", "command"])
    || request.protocol !== 2 || !positive(request.sequence) || !positive(request.deadlineMs)
    || request.deadlineMs <= now || request.deadlineMs > now + 11000
    || !exactKeys(request.connection, ["instanceId", "connectionNonce", "generation"])
    || ["instanceId", "connectionNonce", "generation"].some(key => request.connection[key] !== binding.connection[key])
    || ["protocol", "requestId", "session", "runId", "tabId", "documentId", "documentGeneration", "grantGeneration"]
      .some(key => request[key] !== binding[key])
    || !/^(0|[1-9]\d{0,15})$/.test(request.tabId) || !Number.isSafeInteger(Number(request.tabId))
    || !/^[0-9a-f]{32}$/i.test(request.documentId)
    || !exactKeys(command, ["kind", "snapshotId", "elementRef", "action", "parameters"])
    || command.kind !== "act" || !uuid(command.snapshotId) || command.snapshotId !== binding.snapshotId
    || typeof command.elementRef !== "string" || command.elementRef.length > 128
    || command.elementRef.length <= command.snapshotId.length + 1
    || !command.elementRef.startsWith(command.snapshotId + "-") || !/^[a-zA-Z0-9-]+$/.test(command.elementRef)) {
    throw new Error("invalidCompletionAction");
  }
  const params = command.parameters;
  let valid = false;
  switch (command.action) {
    case "click":
      valid = params && typeof params === "object" && !Array.isArray(params)
        && Object.keys(params).every(key => ["button", "count"].includes(key))
        && (!Object.hasOwn(params, "button") || params.button === "left")
        && (!Object.hasOwn(params, "count") || params.count === 1);
      break;
    case "set_value": case "type_text":
      valid = exactKeys(params, ["text"]) && typeof params.text === "string"
        && [...params.text].length <= 4000 && !params.text.includes("\0") && !/[\uD800-\uDFFF]/u.test(params.text);
      break;
    case "scroll":
      valid = exactKeys(params, ["delta"]) && Number.isInteger(params.delta) && Math.abs(params.delta) <= 2400;
      break;
    case "wait":
      valid = (exactKeys(params, ["nameEquals"]) || exactKeys(params, ["nameEquals", "timeoutMs"]))
        && typeof params.nameEquals === "string" && !!params.nameEquals.trim() && params.nameEquals.length <= 256
        && !/[\uD800-\uDFFF]/u.test(params.nameEquals)
        && (!Object.hasOwn(params, "timeoutMs") || (positive(params.timeoutMs) && params.timeoutMs <= 10000));
      break;
  }
  if (!valid) throw new Error("invalidCompletionAction");
  return { proof, request: Object.freeze({ ...request, connection: proof.binding.connection,
    command: Object.freeze({ ...command, parameters: Object.freeze({ ...params }) }) }) };
}

function wait(signal) {
  return new Promise(resolve => {
    const done = () => { clearTimeout(timer); signal.removeEventListener("abort", done); resolve(); };
    const timer = setTimeout(done, 250);
    signal.addEventListener("abort", done, { once: true });
    if (signal.aborted) done();
  });
}

export class ActionCompletions {
  #entries = new Map();
  #finished = new Set();
  #retryTimer = null;
  #retryDelay = 1000;
  #client;
  #act;
  #wait;
  #schedule;
  #cancel;
  #claim;
  #journal;
  #recovered;

  constructor({ client, act, journal, recovered = () => {}, wait: pause = wait,
    claim = (epoch, request, proof, signal) => client.claimAction(epoch, { request, proof }, signal),
    schedule = (fn, ms) => globalThis.setTimeout(fn, ms), cancel = id => globalThis.clearTimeout(id) }) {
    this.#client = client; this.#act = act; this.#wait = pause;
    this.#schedule = schedule; this.#cancel = cancel;
    this.#claim = claim;
    this.#journal = journal;
    this.#recovered = recovered;
  }

  idle() { return this.#entries.size === 0; }
  reset() { for (const entry of this.#entries.values()) entry.controller.abort(); }

  run(epoch, input, suppliedProof, signal = new AbortController().signal) {
    let request; let proof; let scope;
    try {
      ({ request, proof } = copyBoundAction(input, suppliedProof));
      scope = this.#client.completionScope(epoch, proof, this.#journal);
    } catch { return Promise.resolve({ status: "rejected", detail: "invalid_action_scope" }); }
    const key = [proof.binding.connection.instanceId, proof.binding.connection.connectionNonce,
      proof.binding.connection.generation, proof.binding.requestId].join(":");
    if (this.#entries.size >= 8 || this.#entries.has(key) || this.#finished.has(key)
      || [...this.#entries.values()].some(entry => entry.tabId === request.tabId)) {
      return Promise.resolve({ status: "rejected", detail: "action_busy_or_duplicate" });
    }
    const entry = { key, tabId: request.tabId, binding: proof.binding, scope, controller: new AbortController(), ready: false,
      task: null, settling: null };
    this.#entries.set(key, entry);
    entry.task = this.#execute(entry, epoch, request, proof, signal).finally(() => {
      entry.task = null;
      if (this.#entries.has(key)) this.#scheduleRetry();
    });
    return entry.task;
  }

  async #watch(entry, signal) {
    try {
      while (!signal.aborted) {
        const state = await entry.scope.status(signal);
        if (signal.aborted) return;
        if (state.phase !== "claimed" || state.cancelRequested) { entry.controller.abort(); return; }
        await this.#wait(signal);
      }
    } catch { if (!signal.aborted) entry.controller.abort(); }
  }

  async #execute(entry, epoch, request, proof, signal) {
    const cancel = () => entry.controller.abort();
    signal.addEventListener("abort", cancel, { once: true });
    if (signal.aborted) cancel();
    const deadline = setTimeout(cancel, Math.max(0, request.deadlineMs - Date.now()));
    const watcher = new AbortController();
    const stopWatch = () => watcher.abort();
    entry.controller.signal.addEventListener("abort", stopWatch, { once: true });
    let watching = Promise.resolve(); let invoked = false;
    let outcome = { status: "rejected", detail: "claim_unconfirmed" };
    try {
      await entry.scope.prepare();
      // Never retry this call. Scope already exists if cancellation loses the claim reply.
      await this.#claim(epoch, request, proof, entry.controller.signal);
      if (entry.controller.signal.aborted || this.#client.connectionEpoch !== epoch || Date.now() >= request.deadlineMs) return outcome;
      watching = this.#watch(entry, watcher.signal);
      invoked = true;
      const result = await this.#act(request, entry.controller.signal);
      if (!exactKeys(result, ["status", "detail", "physicallySettled"])
        || !["rejected", "unknown", "applied", "verified"].includes(result.status)
        || typeof result.detail !== "string" || !/^[a-z_]{1,64}$/.test(result.detail)
        || typeof result.physicallySettled !== "boolean") throw new Error("invalidActionCompletion");
      entry.ready = result.physicallySettled;
      outcome = { status: result.status, detail: result.detail };
      if (entry.controller.signal.aborted || this.#client.connectionEpoch !== epoch) {
        outcome = { status: "unknown", detail: "cancelled_after_dispatch" };
      }
    } catch {
      outcome = { status: invoked ? "unknown" : "rejected", detail: invoked ? "action_completion_unknown" : "claim_unconfirmed" };
    } finally {
      clearTimeout(deadline); signal.removeEventListener("abort", cancel);
      watcher.abort(); await watching;
      entry.controller.signal.removeEventListener("abort", stopWatch);
      if (!invoked) entry.ready = true;
      if (entry.ready) await this.#settle(entry);
    }
    if (invoked && (entry.controller.signal.aborted || this.#client.connectionEpoch !== epoch)) {
      outcome = { status: "unknown", detail: "cancelled_after_dispatch" };
    }
    return outcome;
  }

  #settle(entry) {
    if (entry.settling) return entry.settling;
    entry.settling = (async () => {
      if (!entry.ready) {
        if (!await entry.scope.recover()) return;
        this.#recovered(entry.binding); entry.ready = true;
      }
      await entry.scope.settle();
      this.#entries.delete(entry.key);
      this.#finished.add(entry.key);
      if (this.#finished.size > 64) this.#finished.delete(this.#finished.values().next().value);
      entry.scope = null;
      if (this.idle() && this.#retryTimer !== null) {
        this.#cancel(this.#retryTimer); this.#retryTimer = null; this.#retryDelay = 1000;
      }
    })().catch(() => {}).finally(() => {
      entry.settling = null;
      if (this.#entries.has(entry.key)) this.#scheduleRetry();
    });
    return entry.settling;
  }

  #scheduleRetry() {
    if (this.#retryTimer !== null || ![...this.#entries.values()].some(entry => !entry.task && !entry.settling)) return;
    this.#retryTimer = this.#schedule(() => {
      this.#retryTimer = null;
      void this.retryCleanup();
    }, this.#retryDelay);
    this.#retryTimer?.unref?.();
    this.#retryDelay = Math.min(30000, this.#retryDelay * 2);
  }

  retryCleanup(tabId) {
    // Each original proof owns its retry; a retained document cannot starve another tab's cleanup.
    return Promise.allSettled([...this.#entries.values()].filter(entry => !entry.task
      && (tabId === undefined || entry.tabId === String(tabId))).map(entry => this.#settle(entry)));
  }
}
