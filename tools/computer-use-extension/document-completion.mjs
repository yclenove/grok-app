// Fixed isolated-world guardian, installed and acknowledged before Host claim.
// No cleanup key or action input enters the document. Chrome supplies sender identity.
export function armDocumentCompletion(snapshotId, requestId, terminalToken) {
  const uuid = value => typeof value === "string" && /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i.test(value);
  const state = globalThis.__grokComputerUseSnapshot;
  if (!uuid(snapshotId) || !uuid(requestId) || typeof terminalToken !== "string" || !/^[a-f0-9]{64}$/.test(terminalToken)
    || !state || state.snapshotId !== snapshotId || state.retired
    || state.operation || !state.execution?.current() || typeof state.execution.close !== "function") {
    throw new Error("completionGuardUnavailable");
  }
  const prior = globalThis.__grokComputerUseCompletion;
  if (prior?.busy()) throw new Error("completionGuardUnavailable");
  prior?.dispose();
  let terminal = false;
  const guard = { busy: () => !!state.operation, dispose: () => globalThis.removeEventListener("pagehide", hidden, true) };
  function hidden(event) {
    // A cached document can be restored. Its missing navigation metadata is not a terminal receipt.
    if (!event.isTrusted || event.persisted !== false || terminal) return;
    terminal = true; guard.dispose();
    const operation = state.operation;
    if (operation && operation.id !== requestId) return;
    state.execution.close(); state.retire();
    void (async () => {
      if (operation) await operation.finished;
      // Browser-owned storage survives the sender document and a sleeping worker. A plain
      // runtime message can be dropped when navigation destroys its response channel.
      // Receipts are one-use, session-scoped evidence written by the page
      // guardian. The cleanup journal stays in trusted local storage; mixing
      // the two areas would make this write fail closed.
      await chrome.storage.session.set({ ["cuDocumentFinished." + requestId]: { token: terminalToken } });
      await chrome.runtime.sendMessage({ type: "cu-document-finished", snapshotId, requestId });
    })().catch(() => {}); // A lost receipt cannot free occupancy; recovery remains conservative.
  }
  globalThis.__grokComputerUseCompletion = guard;
  globalThis.addEventListener("pagehide", hidden, { capture: true, passive: true });
  return { status: "armed" };
}
