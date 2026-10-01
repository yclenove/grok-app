import { CompletionScope } from "./completion-scope.mjs";
import { completionIdentity } from "./completion-journal.mjs";

// Owns only records loaded at this worker's startup, never current-worker operations.
export class CompletionRecovery {
  #journal;
  #scripting;
  #fetch;
  #pending = new Map();
  #timer = null;
  #delay = 1000;
  #schedule;
  #notify;
  #cancel;
  #started = false;

  constructor({ journal, scripting, fetch = (...args) => globalThis.fetch(...args),
    schedule = (fn, ms) => globalThis.setTimeout(fn, ms), cancel = id => globalThis.clearTimeout(id), notify = () => {} }) {
    this.#journal = journal; this.#scripting = scripting; this.#fetch = fetch;
    this.#schedule = schedule; this.#notify = notify; this.#cancel = cancel;
  }

  async start(retirePriorPairing) {
    if (this.#started) throw new Error("completionRecoveryUnavailable");
    this.#started = true;
    await retirePriorPairing;
    for (const record of await this.#journal.takeRestored()) {
      this.#pending.set(completionIdentity(record.proof), { record,
        physical: record.phase === "physicallySettled",
        task: null,
        scope: new CompletionScope({ endpoint: record.endpoint, proof: record.proof, fetch: this.#fetch, journal: this.#journal }) });
    }
    await this.retry();
  }

  retry(tabId) {
    const tasks = [];
    for (const [key, entry] of this.#pending) {
      if (tabId !== undefined && entry.record.proof.binding.tabId !== String(tabId)) continue;
      if (!entry.task) {
        entry.task = this.#recover(key, entry).finally(() => {
          entry.task = null; this.#scheduleRetry();
        });
      }
      tasks.push(entry.task);
    }
    return Promise.allSettled(tasks);
  }

  async #recover(key, entry) {
    if (!await this.#journal.retains(entry.record.proof)) {
      // A native-browser-exit registration can retire this exact journal owner
      // independently of the original-document recovery path. Do not keep its
      // secret-bearing proof and retry timer alive after durable retirement.
      this.#pending.delete(key); this.#notify(); return;
    }
    if (!entry.physical) {
      if (!await this.#journal.provePhysical(entry.record.proof, this.#scripting, true)) throw new Error("completionRecoveryUnavailable");
      entry.physical = true;
    }
    await entry.scope.settle();
    this.#pending.delete(key); this.#notify();
  }

  #scheduleRetry() {
    if (!this.#pending.size && this.#timer !== null) {
      this.#cancel(this.#timer); this.#timer = null; this.#delay = 1000;
    }
    // An unresolved original script retains its own slot but cannot starve another tab's cleanup.
    if ([...this.#pending.values()].some(entry => !entry.task) && this.#timer === null) {
      this.#timer = this.#schedule(() => { this.#timer = null; void this.retry(); }, this.#delay);
      this.#timer?.unref?.(); this.#delay = Math.min(30000, this.#delay * 2);
    }
  }
}
