import { copyCompletionProof, exactKeys, positive, uuid } from "./completion-scope.mjs";
import { completionIdentity } from "./completion-journal.mjs";

export function copyObservationProof(input) {
  if (!exactKeys(input, ["purpose", "proof"]) || input.purpose !== "observation") throw new Error("invalidObservationScope");
  return Object.freeze({ purpose: "observation", proof: copyCompletionProof(input.proof) });
}

export function copyBoundObservation(input, now = Date.now()) {
  const completion = copyObservationProof(input?.completion); const binding = completion.proof.binding;
  if (!exactKeys(input, ["protocol", "requestId", "sequence", "deadlineMs", "connection", "session", "runId",
    "tabId", "documentId", "documentGeneration", "grantGeneration", "command", "completion"])
    || input.protocol !== 2 || !positive(input.sequence) || !positive(input.deadlineMs)
    || input.deadlineMs <= now || input.deadlineMs > now + 11000
    || !exactKeys(input.connection, ["instanceId", "connectionNonce", "generation"])
    || ["instanceId", "connectionNonce", "generation"].some(key => input.connection[key] !== binding.connection[key])
    || ["protocol", "requestId", "session", "runId", "tabId", "documentId", "documentGeneration", "grantGeneration"]
      .some(key => input[key] !== binding[key])
    || !/^(0|[1-9]\d{0,15})$/.test(input.tabId) || !Number.isSafeInteger(Number(input.tabId))
    || !/^[0-9a-f]{32}$/i.test(input.documentId)
    || !exactKeys(input.command, ["kind", "snapshotId", "screenshot", "preview"]) || input.command.kind !== "observe"
    || !uuid(input.command.snapshotId) || input.command.snapshotId !== binding.snapshotId
    || typeof input.command.screenshot !== "boolean" || typeof input.command.preview !== "boolean") throw new Error("invalidObservationScope");
  return Object.freeze({ ...input, connection: binding.connection, command: Object.freeze({ ...input.command }), completion });
}

function pause(signal) {
  return new Promise(resolve => {
    const done = () => { clearTimeout(timer); signal.removeEventListener("abort", done); resolve(); };
    const timer = setTimeout(done, 250); signal.addEventListener("abort", done, { once: true });
    if (signal.aborted) done();
  });
}

// One owner includes the original read, its cancellation watcher and durable
// cleanup. Reset retires results; none of those promises is raced or detached.
export class ObservationCompletions {
  #entries = new Map();
  #finished = new Set();
  #client;
  #observe;
  #journal;
  #wait;
  #schedule;
  #cancel;
  #retryTimer = null;
  #retryDelay = 1000;

  constructor({ client, observe, journal, wait = pause,
    schedule = (fn, ms) => setTimeout(fn, ms), cancel = id => clearTimeout(id) }) {
    if (!journal) throw new Error("observationJournalRequired");
    this.#client = client; this.#observe = observe; this.#journal = journal;
    this.#wait = wait; this.#schedule = schedule; this.#cancel = cancel;
  }

  idle() { return this.#entries.size === 0; }
  reset() { for (const entry of this.#entries.values()) entry.controller.abort(); }

  run(epoch, input, signal = new AbortController().signal) {
    let request; let scope;
    try {
      request = copyBoundObservation(input);
      scope = this.#client.observationScope(epoch, request.completion, this.#journal);
    } catch { return Promise.resolve(null); }
    const key = completionIdentity(request.completion.proof);
    if (this.#entries.size >= 8 || this.#entries.has(key) || this.#finished.has(key)
      || [...this.#entries.values()].some(entry => entry.tabId === request.tabId)) return Promise.resolve(null);
    const entry = { key, tabId: request.tabId, proof: request.completion.proof, scope,
      controller: new AbortController(), ready: false, task: null, settling: null };
    this.#entries.set(key, entry);
    entry.task = this.#execute(entry, epoch, request, signal).finally(() => {
      entry.task = null; if (this.#entries.has(key)) this.#scheduleRetry();
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

  async #execute(entry, epoch, request, signal) {
    const cancel = () => entry.controller.abort();
    signal.addEventListener("abort", cancel, { once: true }); if (signal.aborted) cancel();
    const deadline = setTimeout(cancel, Math.max(0, request.deadlineMs - Date.now()));
    const watcher = new AbortController(); const stopWatch = () => watcher.abort();
    entry.controller.signal.addEventListener("abort", stopWatch, { once: true });
    let watching = Promise.resolve(); let outcome = { kind: "rejected", reason: "documentChanged" };
    try {
      await entry.scope.prepare();
      // Claim ambiguity never replays the reader. Cleanup already owns its proof.
      await this.#client.claimObservation(epoch, request.completion, entry.controller.signal);
      if (entry.controller.signal.aborted || this.#client.connectionEpoch !== epoch) return null;
      await this.#journal.begin(entry.proof);
      if (entry.controller.signal.aborted || this.#client.connectionEpoch !== epoch) return null;
      watching = this.#watch(entry, watcher.signal);
      const checkpoint = async () => {
        try {
          if (entry.controller.signal.aborted || this.#client.connectionEpoch !== epoch) throw new Error("cancelled");
          const state = await entry.scope.recheck(entry.controller.signal);
          if (state.phase !== "claimed" || state.cancelRequested || entry.controller.signal.aborted
            || this.#client.connectionEpoch !== epoch) throw new Error("cancelled");
        } catch { entry.controller.abort(); throw new Error("cancelled"); }
      };
      const observation = await this.#observe(request, entry.controller.signal, checkpoint);
      outcome = { kind: "observation", observation };
    } catch {
      // Original observe rejection means its owned promises have joined. The
      // durable executing marker is never recovered from a DOM action nonce.
    } finally {
      clearTimeout(deadline); signal.removeEventListener("abort", cancel);
      watcher.abort(); await watching;
      entry.controller.signal.removeEventListener("abort", stopWatch);
      entry.ready = true;
      await this.#settle(entry);
    }
    if (this.#entries.has(entry.key)) return null; // Result cannot precede physical acknowledgement.
    if (entry.controller.signal.aborted || this.#client.connectionEpoch !== epoch) return { kind: "rejected", reason: "cancelled" };
    if (Date.now() >= request.deadlineMs) return { kind: "rejected", reason: "deadline" };
    return outcome;
  }

  #settle(entry) {
    if (entry.settling) return entry.settling;
    entry.settling = (async () => {
      if (!entry.ready) return;
      await entry.scope.settle();
      this.#entries.delete(entry.key); this.#finished.add(entry.key);
      if (this.#finished.size > 64) this.#finished.delete(this.#finished.values().next().value);
      entry.scope = null; entry.proof = null;
      if (this.idle() && this.#retryTimer !== null) {
        this.#cancel(this.#retryTimer); this.#retryTimer = null; this.#retryDelay = 1000;
      }
    })().catch(() => {}).finally(() => { entry.settling = null; if (this.#entries.has(entry.key)) this.#scheduleRetry(); });
    return entry.settling;
  }

  #scheduleRetry() {
    if (this.#retryTimer !== null || ![...this.#entries.values()].some(entry => !entry.task && !entry.settling)) return;
    this.#retryTimer = this.#schedule(() => { this.#retryTimer = null; void this.retryCleanup(); }, this.#retryDelay);
    this.#retryTimer?.unref?.(); this.#retryDelay = Math.min(30000, this.#retryDelay * 2);
  }

  retryCleanup(tabId) {
    return Promise.allSettled([...this.#entries.values()].filter(entry => !entry.task
      && (tabId === undefined || entry.tabId === String(tabId))).map(entry => this.#settle(entry)));
  }
}
