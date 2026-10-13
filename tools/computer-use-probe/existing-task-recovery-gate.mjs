// Owned native fixture only: hold the original document-bound executeScript result.
// No replacement result, script, clock change, browser permission or transport grant.
export async function armOriginalRecoveryReturn(worker, tabId, documentId) {
  await worker.evaluate(({ tabId, documentId }) => {
    if (globalThis.__jointRecoveryGate) throw new Error("original recovery gate already exists");
    const original = chrome.scripting.executeScript.bind(chrome.scripting);
    let release;
    const held = new Promise(resolve => { release = resolve; });
    const gate = { matched: 0, entered: false, released: false, returned: false,
      documentMatched: false, failed: false, original, release, wrapper: null };
    gate.wrapper = async request => {
      const match = request.func?.name === "observeDocument" && request.target.tabId === tabId
        && request.target.documentIds?.length === 1 && request.target.documentIds[0] === documentId;
      const first = match && gate.matched++ === 0;
      let rows;
      try { rows = await original(request); }
      catch (error) { if (first) gate.failed = true; throw error; }
      if (first) {
        gate.documentMatched = rows.length === 1 && rows[0].documentId === documentId;
        gate.entered = true;
        await held;
        gate.returned = true;
      }
      return rows;
    };
    globalThis.__jointRecoveryGate = gate;
    chrome.scripting.executeScript = gate.wrapper;
  }, { tabId, documentId });
}

export async function originalRecoveryReturnState(worker) {
  return worker.evaluate(() => {
    const gate = globalThis.__jointRecoveryGate;
    if (!gate) throw new Error("original recovery gate missing");
    return { matched: gate.matched, entered: gate.entered, released: gate.released,
      returned: gate.returned, documentMatched: gate.documentMatched, failed: gate.failed,
      originalWrapperRetained: chrome.scripting.executeScript === gate.wrapper };
  });
}

export async function releaseOriginalRecoveryReturn(worker) {
  await worker.evaluate(() => {
    const gate = globalThis.__jointRecoveryGate;
    if (!gate || gate.released) throw new Error("original recovery release misplaced");
    gate.released = true;
    gate.release();
  });
}

export async function finishOriginalRecoveryReturn(worker) {
  await worker.evaluate(() => {
    const gate = globalThis.__jointRecoveryGate;
    if (!gate) return;
    // Failure cleanup releases the same original Promise; never replays a capture.
    if (!gate.released) { gate.released = true; gate.release(); }
    if (chrome.scripting.executeScript !== gate.wrapper) throw new Error("original recovery wrapper replaced");
    chrome.scripting.executeScript = gate.original;
    delete globalThis.__jointRecoveryGate;
  });
}
