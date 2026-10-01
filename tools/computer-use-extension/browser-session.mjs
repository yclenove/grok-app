import { exactKeys, uuid } from "./completion-scope.mjs";

const key = "cuBrowserSession";
function copy(value) {
  if (!exactKeys(value, ["version", "id", "started", "previousId"]) || value.version !== 1 || !uuid(value.id)
    || (value.previousId !== null && !uuid(value.previousId)) || value.previousId === value.id
    || typeof value.started !== "boolean") throw new Error("browserSessionUnavailable");
  return Object.freeze({ ...value });
}

// This marker is NOT physical completion evidence by itself. It lives in
// storage.local. Service-worker restart and runtime.reload do not rotate it.
// Only Chrome's real onStartup does. A record created in this worker binds the
// first startup without inventing a previous owner. A persisted started=false
// record (extension installed into an already-running browser) and every later
// startup rotate to a new id. A failed write does not publish that rotation.
export class BrowserSession {
  #storage;
  #current;
  #queue;
  #loadedPersisted = false;

  constructor({ storage, runtime, crypto = globalThis.crypto }) {
    this.#storage = storage;
    this.ready = this.#initialize(crypto);
    this.#queue = this.ready;
    // Synchronous listener registration during module evaluation, before any awaited work.
    runtime.onStartup.addListener(() => {
      this.#queue = this.#queue.then(async () => {
        const current = this.#current;
        const rotate = this.#loadedPersisted || current.started;
        const next = rotate
          ? copy({ version: 1, id: crypto.randomUUID(), previousId: current.id, started: true })
          : copy({ ...current, started: true });
        await this.#storage.set({ [key]: next });
        this.#current = next;
      });
      void this.#queue.catch(() => {});
    });
    void this.ready.catch(() => {});
  }

  async #initialize(crypto) {
    await this.#storage.setAccessLevel({ accessLevel: "TRUSTED_CONTEXTS" });
    const stored = (await this.#storage.get(key))[key];
    if (stored !== undefined) {
      this.#current = copy(stored);
      this.#loadedPersisted = true;
      return;
    }
    const fresh = copy({ version: 1, id: crypto.randomUUID(), previousId: null, started: false });
    await this.#storage.set({ [key]: fresh });
    this.#current = fresh;
    this.#loadedPersisted = false;
  }

  async snapshot() {
    await this.#queue;
    return copy(this.#current);
  }

  acknowledgeRestart(expected) {
    const registered = copy(expected);
    // Pairing/cleanup can finish after another startup. Only consume the exact
    // boundary registered with the Host, and serialize the read with the write.
    const task = this.#queue.then(async () => {
      const current = this.#current;
      if (!registered.started || !registered.previousId || current.id !== registered.id
        || current.previousId !== registered.previousId) return null;
      const next = copy({ ...current, previousId: null });
      await this.#storage.set({ [key]: next });
      this.#current = next;
      return registered.previousId;
    });
    this.#queue = task;
    void task.catch(() => {});
    return task;
  }
}
