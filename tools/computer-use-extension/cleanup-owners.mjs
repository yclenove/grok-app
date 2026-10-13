import { copyCompletionProof } from "./completion-scope.mjs";

const identity = (purpose, proof) => [purpose, proof.binding.connection.instanceId,
  proof.binding.connection.connectionNonce, proof.binding.connection.generation, proof.binding.requestId].join(":");

// Both durable journals share this synchronous admission. A failed write keeps
// its intended owner; only an acknowledged retirement can free capacity.
export class CleanupOwners {
  #entries = new Map();

  claim(purpose, input) {
    if (!["action", "observation"].includes(purpose)) throw new Error("cleanupOwnerUnavailable");
    const proof = copyCompletionProof(input);
    const key = identity(purpose, proof);
    if (this.#entries.size >= 8 || this.#entries.has(key)
      || [...this.#entries.values()].some(tab => tab === proof.binding.tabId)) throw new Error("cleanupOwnerUnavailable");
    this.#entries.set(key, proof.binding.tabId);
  }

  release(purpose, proof) { this.#entries.delete(identity(purpose, copyCompletionProof(proof))); }
}
