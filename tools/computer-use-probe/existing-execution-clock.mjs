import assert from "node:assert/strict";

// This database exists only in the owner-marked test profile, separate from the SW's production counter.
export async function verifyExecutionClock(popup) {
  const result = await popup.evaluate(async () => {
    const { allocateWorkerEpoch } = await import(chrome.runtime.getURL("execution-clock.mjs"));
    const database = "grok-cu-owned-clock-fixture";
    const allocate = () => allocateWorkerEpoch({ database });
    const values = await Promise.all(Array.from({ length: 12 }, allocate));
    const modify = (value, abort = false) => new Promise((resolve, reject) => {
      const open = indexedDB.open(database, 1); open.onerror = () => reject(new Error("fixture database unavailable"));
      open.onsuccess = () => {
        const db = open.result; const tx = db.transaction("metadata", "readwrite", { durability: "strict" });
        const store = tx.objectStore("metadata");
        const change = value === null ? store.delete("epoch") : store.put(value, "epoch");
        if (abort) change.onsuccess = () => tx.abort();
        tx.oncomplete = () => { db.close(); resolve(); };
        tx.onabort = () => { db.close(); if (abort) resolve(); else reject(new Error("fixture transaction aborted")); };
      };
    });
    // Rollback uses a real native transaction, not a replacement allocator.
    await modify({ version: 1, epoch: 1000 }, true);
    const afterAbort = await allocate();
    const rejected = [];
    for (const value of [null, { version: 1, epoch: Number.MAX_SAFE_INTEGER }, { version: 2, epoch: 0 },
      { version: 1, epoch: -1 }, { version: 1, epoch: 0, extra: true }]) {
      await modify(value);
      rejected.push(await allocate().then(() => false, error => error.message === "executionUnavailable"));
    }
    await modify({ version: 1, epoch: 13 });
    const restored = await allocate();
    return { values, afterAbort, rejected, restored };
  });
  assert.deepEqual(result.values.slice().sort((a, b) => a - b), Array.from({ length: 12 }, (_, i) => i + 1));
  assert.equal(result.afterAbort, 13); assert.equal(result.restored, 14);
  assert.deepEqual(result.rejected, Array(5).fill(true));
}
