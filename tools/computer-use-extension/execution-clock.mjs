// Only non-sensitive monotonic metadata persists. A completed transaction is the authority to use an epoch.
export function allocateWorkerEpoch({ indexedDB = globalThis.indexedDB, database = "grok-cu-execution" } = {}) {
  return new Promise((resolve, reject) => {
    let db; let transaction; let done = false;
    const finish = (error, epoch) => {
      if (done) return;
      done = true; clearTimeout(timer);
      if (error) { try { transaction?.abort(); } catch {} }
      db?.close();
      if (error) reject(new Error("executionUnavailable")); else resolve(epoch);
    };
    const timer = setTimeout(() => finish(true), 5000);
    let request;
    try { request = indexedDB.open(database, 1); }
    catch { finish(true); return; }
    request.onerror = () => finish(true);
    request.onblocked = () => finish(true);
    request.onupgradeneeded = event => {
      if (done || event.oldVersion !== 0) { request.transaction.abort(); return; }
      const store = request.result.createObjectStore("metadata");
      store.add({ version: 1, epoch: 0 }, "epoch");
    };
    request.onsuccess = () => {
      db = request.result;
      if (done) { db.close(); return; }
      db.onversionchange = () => { finish(true); db.close(); };
      try {
        transaction = db.transaction("metadata", "readwrite", { durability: "strict" });
        transaction.onabort = () => finish(true);
        transaction.onerror = () => finish(true);
        const store = transaction.objectStore("metadata");
        const read = store.get("epoch"); let next;
        read.onsuccess = () => {
          if (done) return;
          const value = read.result;
          if (!value || typeof value !== "object" || Array.isArray(value) || Object.keys(value).length !== 2 || value.version !== 1
            || !Number.isSafeInteger(value.epoch) || value.epoch < 0 || value.epoch >= Number.MAX_SAFE_INTEGER) {
            finish(true); return;
          }
          next = value.epoch + 1;
          try { store.put({ version: 1, epoch: next }, "epoch"); } catch { finish(true); }
        };
        transaction.oncomplete = () => { if (!Number.isSafeInteger(next)) finish(true); else finish(false, next); };
      } catch { finish(true); }
    };
  });
}

export class ExecutionClock {
  #sequence = 0;
  constructor(epoch) {
    this.ready = Promise.resolve(epoch).then(value => {
      if (!Number.isSafeInteger(value) || value <= 0) throw new Error("executionUnavailable");
      return value;
    });
    void this.ready.catch(() => {});
  }
  async next() {
    const epoch = await this.ready;
    if (!Number.isSafeInteger(this.#sequence + 1)) throw new Error("executionUnavailable");
    return Object.freeze({ epoch, sequence: ++this.#sequence });
  }
}
