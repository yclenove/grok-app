import { parseEndpoint } from "./loopback-endpoint.mjs";

const encoder = new TextEncoder();
export const exactKeys = (value, keys) => value !== null && typeof value === "object" && !Array.isArray(value)
  && Object.keys(value).length === keys.length && keys.every(key => Object.hasOwn(value, key));
export const uuid = value => typeof value === "string"
  && /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i.test(value);
export const positive = value => Number.isSafeInteger(value) && value > 0;
export const boundedText = (value, size) => typeof value === "string" && value.length > 0
  && encoder.encode(value).byteLength <= size && !/[\u0000-\u001f\u007f-\u009f]/u.test(value);

export function copyCompletionProof(proof) {
  const binding = proof?.binding;
  if (!exactKeys(proof, ["binding", "completionKey"])
    || !exactKeys(binding, ["protocol", "requestId", "connection", "session", "runId", "tabId",
      "documentId", "documentGeneration", "grantGeneration", "snapshotId"])
    || binding.protocol !== 2 || !uuid(binding.requestId) || !uuid(binding.snapshotId)
    || !exactKeys(binding.connection, ["instanceId", "connectionNonce", "generation"])
    || !uuid(binding.connection.instanceId) || !uuid(binding.connection.connectionNonce) || !positive(binding.connection.generation)
    || !positive(binding.documentGeneration) || !positive(binding.grantGeneration)
    || !boundedText(binding.session, 256) || !boundedText(binding.runId, 256)
    || !boundedText(binding.tabId, 128) || !boundedText(binding.documentId, 128)
    || typeof proof.completionKey !== "string" || !/^[a-f0-9]{64}$/.test(proof.completionKey)
    || encoder.encode(JSON.stringify(proof)).byteLength > 4096) throw new Error("invalidCompletion");
  return Object.freeze({ binding: Object.freeze({ ...binding, connection: Object.freeze({ ...binding.connection }) }),
    completionKey: proof.completionKey });
}

// No PairingClient/PairingSession reference, key export or browser operation.
export class CompletionScope {
  #endpoint;
  #body;
  #fetch;
  #proof;
  #journal;
  #statusTask = null;
  #settleTask = null;
  #settled = false;

  constructor({ endpoint, proof, fetch = (...args) => globalThis.fetch(...args), journal }) {
    this.#endpoint = parseEndpoint(endpoint);
    this.#proof = copyCompletionProof(proof);
    this.#body = JSON.stringify(this.#proof);
    this.#journal = journal;
    this.#fetch = fetch;
  }

  // Acknowledged before claim. No-op only for the historical, explicitly injected receipt fixture.
  async prepare() { if (this.#journal) await this.#journal.reserve(this.#endpoint, this.#proof); }
  async recover() { return this.#journal ? this.#journal.provePhysical(this.#proof) : false; }

  async #request(route, signal) {
    if (signal?.aborted) throw new Error("completionUnavailable");
    const controller = new AbortController();
    const abort = () => controller.abort();
    signal?.addEventListener("abort", abort, { once: true });
    const timer = setTimeout(abort, 5000);
    let reader;
    try {
      const response = await this.#fetch(this.#endpoint + "/cu/extension-completion/" + route, {
        method: "POST", credentials: "omit", cache: "no-store", redirect: "error",
        headers: { "content-type": "application/json" }, body: this.#body, signal: controller.signal,
      });
      if (!response.ok || controller.signal.aborted) throw new Error("completionUnavailable");
      reader = response.body.getReader();
      const chunks = []; let size = 0;
      for (;;) {
        const { value, done } = await reader.read();
        if (controller.signal.aborted) throw new Error("completionUnavailable");
        if (done) break;
        size += value.byteLength;
        if (size > 1024) throw new Error("completionUnavailable");
        chunks.push(value);
      }
      const bytes = new Uint8Array(size); let offset = 0;
      for (const chunk of chunks) { bytes.set(chunk, offset); offset += chunk.byteLength; }
      return JSON.parse(new TextDecoder("utf-8", { fatal: true }).decode(bytes));
    } catch { throw new Error("completionUnavailable"); }
    finally {
      clearTimeout(timer); signal?.removeEventListener("abort", abort);
      await reader?.cancel().catch(() => {});
    }
  }

  status(signal) {
    if (this.#settled) return Promise.resolve({ phase: "settled", cancelRequested: true });
    if (this.#statusTask) return this.#statusTask;
    this.#statusTask = this.#request("status", signal).then(reply => {
      if (!exactKeys(reply, ["ok", "status"]) || reply.ok !== true
        || !exactKeys(reply.status, ["phase", "cancelRequested"])
        || !["offered", "claimed", "settled"].includes(reply.status.phase)
        || typeof reply.status.cancelRequested !== "boolean") throw new Error("completionUnavailable");
      return { ...reply.status };
    }).finally(() => { this.#statusTask = null; });
    return this.#statusTask;
  }

  settle() {
    if (this.#settled && !this.#journal) return Promise.resolve();
    if (this.#settleTask) return this.#settleTask;
    this.#settleTask = this.#finish().finally(() => { this.#settleTask = null; });
    return this.#settleTask;
  }

  async #finish() {
    if (!this.#settled) {
      if (this.#journal) await this.#journal.physicalFinished(this.#proof);
      try {
        const reply = await this.#request("settle");
        if (!exactKeys(reply, ["ok"]) || reply.ok !== true) throw new Error("completionUnavailable");
      } catch {
        // Absence is meaningful only for a locally persisted physical completion.
        if (!this.#journal) throw new Error("completionUnavailable");
        try {
          const reply = await this.#request("retirement");
          if (!exactKeys(reply, ["ok", "state"]) || reply.ok !== true
            || !["settled", "absent"].includes(reply.state)) throw new Error("completionUnavailable");
        } catch {
          if (!await this.#journal.retiredHost(this.#proof)) throw new Error("completionUnavailable");
        }
      }
      this.#settled = true; this.#body = null;
    }
    // Failed deletion retains the owner and retries only cleanup, even after Host acknowledgement.
    if (this.#journal) await this.#journal.forget(this.#proof);
    this.#journal = null; this.#proof = null;
  }
}
