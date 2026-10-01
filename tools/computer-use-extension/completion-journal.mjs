import { durableCleanupFromJournal } from "./browser-exit.mjs";
import { copyCompletionProof, exactKeys } from "./completion-scope.mjs";
import { parseEndpoint } from "./loopback-endpoint.mjs";
import { proveDocumentCompletion } from "./document-lifetime.mjs";
import { armDocumentCompletion } from "./document-completion.mjs";
import { DocumentReceipts, isTerminalToken, newTerminalToken, terminalRequest } from "./document-receipts.mjs";
import { copyHostLifetime } from "./host-lifetime.mjs";

const storageKey = "cuActionCleanup";
export const completionIdentity = proof => [proof.binding.connection.instanceId,
  proof.binding.connection.connectionNonce, proof.binding.connection.generation, proof.binding.requestId].join(":");

function recordCopy(record) {
  if (!exactKeys(record, ["endpoint", "proof", "phase", "documentContext", "terminalToken", "hostLifetime"])
    || !["prepared", "physicallySettled"].includes(record.phase)
    || (record.terminalToken !== null && !isTerminalToken(record.terminalToken))) throw new Error("completionJournalUnavailable");
  const context = record.documentContext;
  if (context !== null && (!exactKeys(context, ["incognito"]) || typeof context.incognito !== "boolean")) {
    throw new Error("completionJournalUnavailable");
  }
  const proof = copyCompletionProof(record.proof);
  if (!/^(0|[1-9]\d{0,15})$/.test(proof.binding.tabId) || !Number.isSafeInteger(Number(proof.binding.tabId))
    || !/^[0-9a-f]{32}$/i.test(proof.binding.documentId)) throw new Error("completionJournalUnavailable");
  return Object.freeze({ endpoint: parseEndpoint(record.endpoint), proof, phase: record.phase,
    hostLifetime: record.hostLifetime === null ? null : copyHostLifetime(record.hostLifetime, proof.binding.connection.instanceId),
    terminalToken: record.terminalToken,
    documentContext: context === null ? null : Object.freeze({ incognito: context.incognito }) });
}

// Trusted storage.local cleanup journal survives a full browser exit. It is not
// input/read authority; neither the pairing Bearer nor command input is recorded.
export class CompletionJournal {
  #storage;
  #entries = new Map();
  #restored = [];
  #queue = Promise.resolve();
  #lifetime;
  #scripting;
  #receipts;
  #hostLifetime;

  constructor({ storage, lifetime, scripting, receipts, hostLifetime }) {
    this.#storage = storage;
    if (!lifetime || !scripting || !receipts) throw new Error("documentLifetimeUnavailable");
    this.#lifetime = lifetime; this.#scripting = scripting;
    this.#receipts = new DocumentReceipts(receipts);
    this.#hostLifetime = hostLifetime;
    this.ready = this.#initialize();
    // Initialization errors remain visible to every caller without an unhandled rejection.
    void this.ready.catch(() => {});
  }

  async #initialize() {
    try {
      await this.#storage.setAccessLevel({ accessLevel: "TRUSTED_CONTEXTS" });
      const value = (await this.#storage.get(storageKey))[storageKey];
      if (value === undefined) { await this.#receipts.prune([]); return; }
      if (!exactKeys(value, ["version", "entries"]) || ![1, 2, 3, 4].includes(value.version) || !Array.isArray(value.entries)
        || value.entries.length > 8 || new TextEncoder().encode(JSON.stringify(value)).byteLength > 36 * 1024) {
        throw new Error("invalid record");
      }
      const tabs = new Set();
      for (const input of value.entries) {
        if (value.version === 1 && !exactKeys(input, ["endpoint", "proof", "phase"])) throw new Error("invalid legacy record");
        if (value.version === 2 && !exactKeys(input, ["endpoint", "proof", "phase", "documentContext"])) throw new Error("invalid legacy record");
        if (value.version === 3 && !exactKeys(input, ["endpoint", "proof", "phase", "documentContext", "terminalToken"])) throw new Error("invalid legacy record");
        const migrated = value.version === 1 ? { ...input, documentContext: null, terminalToken: null }
          : value.version === 2 ? { ...input, terminalToken: null } : input;
        const record = recordCopy(value.version < 4 ? { ...migrated, hostLifetime: null } : migrated);
        const key = completionIdentity(record.proof);
        if (this.#entries.has(key) || tabs.has(record.proof.binding.tabId)) throw new Error("duplicate record");
        tabs.add(record.proof.binding.tabId); this.#entries.set(key, record);
      }
      this.#restored = [...this.#entries.values()];
      await this.#receipts.prune(this.#restored);
    } catch { throw new Error("completionJournalUnavailable"); }
  }

  async takeRestored() {
    await this.ready;
    const records = this.#restored; this.#restored = []; return records;
  }
  async hasTab(tabId) {
    await this.ready;
    return [...this.#entries.values()].some(record => record.proof.binding.tabId === String(tabId));
  }

  async retains(proof) {
    await this.ready;
    // Only an acknowledged journal retirement removes an existing local owner.
    // Failed storage deletion and mismatched proof still retain/fail closed.
    return this.#current(proof) !== undefined;
  }

  #write(mutate, retiring = false) {
    const task = this.#queue.then(async () => {
      await this.ready;
      const next = new Map(this.#entries);
      mutate(next);
      // Retain the intended state even on ambiguous storage failure; never silently free capacity.
      if (!retiring) this.#entries = next;
      if (next.size) await this.#storage.set({ [storageKey]: { version: 4, entries: [...next.values()] } });
      else await this.#storage.remove(storageKey);
      this.#entries = next;
    }).catch(() => { throw new Error("completionJournalUnavailable"); });
    this.#queue = task.catch(() => {});
    return task;
  }

  async reserve(endpoint, proof) {
    let record;
    try {
      const copied = copyCompletionProof(proof);
      const documentContext = await this.#lifetime.attest(copied.binding);
      const hostLifetime = this.#hostLifetime ? await this.#hostLifetime.capture(endpoint, copied) : null;
      record = recordCopy({ endpoint, proof: copied, phase: "prepared", documentContext, terminalToken: newTerminalToken(), hostLifetime });
    }
    catch { return Promise.reject(new Error("completionJournalUnavailable")); }
    await this.#write(next => {
      if (this.#entries.size >= 8 || this.#entries.has(completionIdentity(record.proof))
        || [...this.#entries.values()].some(old => old.proof.binding.tabId === record.proof.binding.tabId)) {
        throw new Error("completionJournalUnavailable");
      }
      next.set(completionIdentity(record.proof), record);
    });
    const { binding } = record.proof;
    const rows = await this.#scripting.executeScript({
      target: { tabId: Number(binding.tabId), documentIds: [binding.documentId] }, world: "ISOLATED",
      func: armDocumentCompletion, args: [binding.snapshotId, binding.requestId, record.terminalToken],
    });
    if (rows?.length !== 1 || rows[0].frameId !== 0 || rows[0].documentId !== binding.documentId
      || rows[0].result?.status !== "armed") throw new Error("completionJournalUnavailable");
  }

  async provePhysical(proof, scripting = this.#scripting, retireExecution = false) {
    await this.ready;
    const record = this.#current(proof);
    if (!record) throw new Error("completionJournalUnavailable");
    if (record.phase === "physicallySettled") return true;
    if (await this.#receipts.matches(record)) { await this.physicalFinished(proof); return true; }
    return proveDocumentCompletion({ lifetime: this.#lifetime, scripting, binding: record.proof.binding,
      context: record.documentContext, retireExecution });
  }

  async storedReceipt(key) {
    const requestId = terminalRequest(key);
    if (!requestId) return null;
    await this.ready;
    const record = [...this.#entries.values()].find(record => record.proof.binding.requestId === requestId);
    if (!record) { await this.#receipts.forget(requestId); return null; }
    if (!await this.#receipts.matches(record)) return null;
    await this.physicalFinished(record.proof); return record.proof.binding.tabId;
  }

  // Called only from the private runtime listener after it checks the extension sender ID.
  async documentFinished(message, sender) {
    await this.ready;
    if (!exactKeys(message, ["type", "snapshotId", "requestId"]) || message.type !== "cu-document-finished"
      || sender?.frameId !== 0 || !Number.isSafeInteger(sender.tab?.id)) return false;
    const record = [...this.#entries.values()].find(({ proof: { binding }, documentContext }) => documentContext
      && binding.requestId === message.requestId && binding.snapshotId === message.snapshotId
      && binding.documentId === sender.documentId && binding.tabId === String(sender.tab.id)
      && documentContext.incognito === sender.tab.incognito);
    if (!record) return false;
    await this.physicalFinished(record.proof); return true;
  }

  #current(proof) {
    const copied = copyCompletionProof(proof);
    const record = this.#entries.get(completionIdentity(copied));
    if (record && (record.proof.completionKey !== copied.completionKey
      || Object.keys(copied.binding).filter(key => key !== "connection").some(key => record.proof.binding[key] !== copied.binding[key])
      || Object.keys(copied.binding.connection).some(key => record.proof.binding.connection[key] !== copied.binding.connection[key]))) {
      throw new Error("completionJournalUnavailable");
    }
    return record;
  }

  async physicalFinished(proof) {
    await this.#write(next => {
      const record = this.#current(proof);
      // A reserve failure before insertion grants no execution permission and needs no stored cleanup.
      if (record) next.set(completionIdentity(proof), Object.freeze({ ...record, phase: "physicallySettled" }));
    });
    await this.#receipts.forget(proof.binding.requestId);
  }

  async forget(proof) {
    await this.ready;
    const record = this.#current(proof);
    if (record && record.phase !== "physicallySettled") throw new Error("completionJournalUnavailable");
    await this.#receipts.forget(proof.binding.requestId);
    return this.#write(next => {
      const record = this.#current(proof);
      if (record && record.phase !== "physicallySettled") throw new Error("completionJournalUnavailable");
      next.delete(completionIdentity(proof));
    }, true);
  }

  async retiredHost(proof) {
    await this.ready;
    const record = this.#current(proof);
    if (!record || record.phase !== "physicallySettled" || !record.hostLifetime || !this.#hostLifetime) return false;
    const retired = await this.#hostLifetime.retired(record.hostLifetime);
    // A slow response cannot free a changed/deleted local owner.
    return retired === true && this.#current(proof) === record;
  }

  async cleanupRecords(browserId) {
    await this.ready;
    return [...this.#entries.values()].map((record) => durableCleanupFromJournal(record, browserId));
  }

  // Removes only request ids the host has already labeled cleanup or unknown.
  // A storage failure leaves the local owner in place.
  async forgetBrowserExit(requestIds) {
    const ids = new Set(requestIds);
    await this.#write((next) => {
      for (const [key, record] of [...next]) {
        if (ids.has(record.proof.binding.requestId)) next.delete(key);
      }
    }, true);
  }
}
