import { copyCompletionProof, exactKeys } from "./completion-scope.mjs";
import { parseEndpoint } from "./loopback-endpoint.mjs";
import { copyHostLifetime } from "./host-lifetime.mjs";
import { durableCleanupFromJournal } from "./browser-exit.mjs";
import { completionIdentity } from "./completion-journal.mjs";

const storageKey = "cuObservationCleanup";
function copyRecord(input) {
  if (!exactKeys(input, ["endpoint", "purpose", "proof", "phase", "hostLifetime"]) || input.purpose !== "observation"
    || !["prepared", "executing", "physicallySettled"].includes(input.phase)) throw new Error("observationJournalUnavailable");
  const proof = copyCompletionProof(input.proof);
  if (!/^(0|[1-9]\d{0,15})$/.test(proof.binding.tabId) || !Number.isSafeInteger(Number(proof.binding.tabId))
    || !/^[0-9a-f]{32}$/i.test(proof.binding.documentId)) throw new Error("observationJournalUnavailable");
  return Object.freeze({ endpoint: parseEndpoint(input.endpoint), purpose: "observation", proof, phase: input.phase,
    hostLifetime: input.hostLifetime === null ? null : copyHostLifetime(input.hostLifetime, proof.binding.connection.instanceId) });
}

// No DOM terminal nonce, capture data, model content, command, or Pair Bearer.
// A worker restart cannot prove a browser API/decoder has physically returned.
export class ObservationJournal {
  #storage;
  #hostLifetime;
  #owners;
  #entries = new Map();
  #restored = [];
  #queue = Promise.resolve();

  constructor({ storage, hostLifetime, owners }) {
    this.#storage = storage; this.#hostLifetime = hostLifetime; this.#owners = owners;
    this.ready = this.#initialize(); void this.ready.catch(() => {});
  }

  async #initialize() {
    try {
      await this.#storage.setAccessLevel({ accessLevel: "TRUSTED_CONTEXTS" });
      const stored = (await this.#storage.get(storageKey))[storageKey];
      if (stored === undefined) return;
      if (!exactKeys(stored, ["version", "entries"]) || stored.version !== 1 || !Array.isArray(stored.entries)
        || stored.entries.length > 8 || new TextEncoder().encode(JSON.stringify(stored)).byteLength > 36 * 1024) throw new Error("invalid journal");
      const tabs = new Set();
      for (const input of stored.entries) {
        const record = copyRecord(input); const key = completionIdentity(record.proof);
        if (this.#entries.has(key) || tabs.has(record.proof.binding.tabId)) throw new Error("duplicate journal owner");
        this.#owners?.claim("observation", record.proof);
        tabs.add(record.proof.binding.tabId); this.#entries.set(key, record);
      }
      this.#restored = [...this.#entries.values()];
    } catch { throw new Error("observationJournalUnavailable"); }
  }

  async takeRestored() { await this.ready; const records = this.#restored; this.#restored = []; return records; }
  async hasTab(tabId) { await this.ready; return [...this.#entries.values()].some(row => row.proof.binding.tabId === String(tabId)); }
  async retains(proof) { await this.ready; return this.#current(proof) !== undefined; }

  #current(input) {
    const proof = copyCompletionProof(input);
    const record = this.#entries.get(completionIdentity(proof));
    if (record && (record.proof.completionKey !== proof.completionKey
      || Object.keys(proof.binding).filter(key => key !== "connection").some(key => proof.binding[key] !== record.proof.binding[key])
      || Object.keys(proof.binding.connection).some(key => proof.binding.connection[key] !== record.proof.binding.connection[key]))) {
      throw new Error("observationJournalUnavailable");
    }
    return record;
  }

  #write(mutate, retiring = false) {
    const task = this.#queue.then(async () => {
      await this.ready; const next = new Map(this.#entries); mutate(next);
      if (!retiring) this.#entries = next;
      if (next.size) await this.#storage.set({ [storageKey]: { version: 1, entries: [...next.values()] } });
      else await this.#storage.remove(storageKey);
      this.#entries = next;
    }).catch(() => { throw new Error("observationJournalUnavailable"); });
    this.#queue = task.catch(() => {}); return task;
  }

  async reserve(endpoint, input) {
    const proof = copyCompletionProof(input);
    const hostLifetime = this.#hostLifetime ? await this.#hostLifetime.capture(endpoint, proof) : null;
    const record = copyRecord({ endpoint, purpose: "observation", proof, phase: "prepared", hostLifetime });
    await this.#write(next => {
      if (this.#entries.size >= 8 || this.#entries.has(completionIdentity(proof))
        || [...this.#entries.values()].some(row => row.proof.binding.tabId === proof.binding.tabId)) throw new Error("observationJournalUnavailable");
      this.#owners?.claim("observation", proof); next.set(completionIdentity(proof), record);
    });
  }

  // Durable acknowledgement must precede invoking the original observer. A
  // restored prepared record therefore proves no browser read was ever started.
  async begin(proof) {
    await this.#write(next => {
      const record = this.#current(proof);
      if (!record || record.phase !== "prepared") throw new Error("observationJournalUnavailable");
      next.set(completionIdentity(proof), Object.freeze({ ...record, phase: "executing" }));
    });
  }

  async provePhysical(proof) {
    await this.ready; const record = this.#current(proof);
    if (!record) throw new Error("observationJournalUnavailable");
    return record.phase === "prepared" || record.phase === "physicallySettled";
  }

  async physicalFinished(proof) {
    await this.#write(next => {
      const record = this.#current(proof);
      if (record) next.set(completionIdentity(proof), Object.freeze({ ...record, phase: "physicallySettled" }));
    });
  }

  async forget(proof) {
    await this.#write(next => {
      const record = this.#current(proof);
      if (record && record.phase !== "physicallySettled") throw new Error("observationJournalUnavailable");
      next.delete(completionIdentity(proof));
    }, true);
    this.#owners?.release("observation", proof);
  }

  async retiredHost(proof) {
    await this.ready; const record = this.#current(proof);
    if (!record || record.phase !== "physicallySettled" || !record.hostLifetime || !this.#hostLifetime) return false;
    return await this.#hostLifetime.retired(record.hostLifetime) === true && this.#current(proof) === record;
  }

  async cleanupRecords(browserId) {
    await this.ready;
    return [...this.#entries.values()].map(record => durableCleanupFromJournal({ ...record,
      phase: record.phase === "executing" ? "prepared" : record.phase }, browserId));
  }

  async forgetBrowserExit(requestIds) {
    const ids = new Set(requestIds); const removed = [];
    await this.#write(next => {
      for (const [key, record] of next) {
        if (ids.has(record.proof.binding.requestId)) { next.delete(key); removed.push(record.proof); }
      }
    }, true);
    for (const proof of removed) this.#owners?.release("observation", proof);
  }
}
