import { exactKeys, uuid } from "./completion-scope.mjs";

const prefix = "cuDocumentFinished.";
export const isTerminalToken = value => typeof value === "string" && /^[a-f0-9]{64}$/.test(value);
export const terminalRequest = key => typeof key === "string" && key.startsWith(prefix) && uuid(key.slice(prefix.length))
  ? key.slice(prefix.length) : null;
export function newTerminalToken() {
  return Array.from(globalThis.crypto.getRandomValues(new Uint8Array(32)), value => value.toString(16).padStart(2, "0")).join("");
}

// Only one-use terminal nonces enter session storage; all Host proofs remain in
// trusted local storage. Storage access level is per storage area, not per key.
export class DocumentReceipts {
  constructor(storage) {
    this.storage = storage;
    // The page guardian writes the receipt from an isolated/content context.
    // Explicitly expose this short-lived nonce store to those contexts.
    this.ready = Promise.resolve(typeof storage.setAccessLevel === "function"
      ? storage.setAccessLevel({ accessLevel: "TRUSTED_AND_UNTRUSTED_CONTEXTS" })
      : undefined).catch(() => { throw new Error("documentReceiptStorageUnavailable"); });
  }
  async prune(records) {
    await this.ready;
    const owned = new Set(records.map(record => record.proof.binding.requestId));
    const values = await this.storage.get(null);
    const stale = Object.keys(values).filter(key => terminalRequest(key) && !owned.has(terminalRequest(key)));
    if (stale.length) await this.storage.remove(stale);
  }
  async matches(record) {
    await this.ready;
    if (!isTerminalToken(record.terminalToken)) return false;
    const key = prefix + record.proof.binding.requestId;
    const value = (await this.storage.get(key))[key];
    return exactKeys(value, ["token"]) && value.token === record.terminalToken;
  }
  async forget(requestId) { await this.ready; await this.storage.remove(prefix + requestId); }
}
