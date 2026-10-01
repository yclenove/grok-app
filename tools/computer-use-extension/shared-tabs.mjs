import { observeDocument } from "./observe-document.mjs";
import { inspectDocument, sameView } from "./document-state.mjs";
import { normalizeCapture } from "./capture-document.mjs";
import { actDocument, cancelDocumentOperation } from "./act-document.mjs";
import { fenceDocument } from "./document-execution.mjs";

async function bounded(promise) {
  let timer;
  try {
    return await Promise.race([promise, new Promise((_, reject) => {
      timer = setTimeout(() => reject(new Error("shareUnavailable")), 5000);
      timer.unref?.();
    })]);
  } finally { clearTimeout(timer); }
}

export class SharedTabs {
  #entries = new Map();
  #documentSequence = 0;
  #epoch = 0;
  #activity = 0;
  #actions = new Map();
  #journal;
  #clock;

  constructor({ client, tabs, scripting, journal, clock }) {
    this.client = client; this.tabs = tabs; this.scripting = scripting;
    this.#journal = journal;
    if (!clock || typeof clock.next !== "function") throw new Error("executionUnavailable");
    this.#clock = clock;
  }

  reset() { ++this.#epoch; this.#entries.clear(); this.#cancelActions(); }
  activityChanged() {
    ++this.#activity;
    for (const entry of this.#entries.values()) entry.modelSnapshot = null;
    this.#cancelActions();
  }

  #cancelActions(tabId) {
    for (const operation of this.#actions.values()) {
      if (tabId !== undefined && operation.tabId !== tabId) continue;
      operation.cancelled = true;
      if (!operation.dispatched || operation.cancellation) continue;
      // This promise is joined by act(), including after reset/unshare. Never race it away.
      operation.cancellation = Promise.resolve().then(() => this.scripting.executeScript({
        target: { tabId: operation.tabId, documentIds: [operation.documentId] }, world: "ISOLATED",
        func: cancelDocumentOperation, args: [operation.snapshotId, operation.id],
      })).then(rows => rows?.length === 1 && rows[0].frameId === 0 && rows[0].documentId === operation.documentId
        && rows[0].result?.status === "settled").catch(() => false);
    }
  }

  actionsIdle() { return this.#actions.size === 0; }

  releaseRecovered(binding) {
    const operation = this.#actions.get(Number(binding.tabId));
    if (!operation) return;
    if (!operation.returned || operation.id !== binding.requestId || operation.documentId !== binding.documentId
      || operation.snapshotId !== binding.snapshotId) throw new Error("recoveryOwnershipChanged");
    this.#actions.delete(operation.tabId);
  }

  async current() {
    const rows = await bounded(this.tabs.query({ active: true, lastFocusedWindow: true }));
    if (rows.length !== 1 || !Number.isSafeInteger(rows[0].id) || rows[0].id < 0) throw new Error("shareUnavailable");
    const tab = rows[0];
    const entry = this.#entries.get(tab.id);
    return { tabId: tab.id, title: String(tab.title ?? "").slice(0, 1024), shared: entry?.shared === true && !entry.cancelled };
  }

  #live(entry) {
    return !entry.cancelled && this.#entries.get(entry.tabId) === entry && this.#epoch === entry.epoch
      && this.client.connectionEpoch !== null && this.client.connectionEpoch === entry.connection;
  }

  async #document(tabId, documentId) {
    const results = await bounded(this.scripting.executeScript({
      target: documentId ? { tabId, documentIds: [documentId] } : { tabId, frameIds: [0] },
      world: "ISOLATED", func: inspectDocument,
    }));
    if (results.length !== 1 || results[0].frameId !== 0 || typeof results[0].documentId !== "string"
      || !/^[0-9a-f]{32}$/i.test(results[0].documentId)
      || (documentId && results[0].documentId !== documentId)) throw new Error("shareUnavailable");
    const { result, documentId: currentDocument } = results[0];
    if (!result || result.visible !== true || typeof result.url !== "string" || typeof result.title !== "string") {
      throw new Error("shareUnavailable");
    }
    const url = new URL(result.url);
    if (!["http:", "https:"].includes(url.protocol) || !url.hostname || url.username || url.password) {
      throw new Error("shareUnavailable");
    }
    return { ...result, documentId: currentDocument };
  }

  async share(expectedTabId) {
    if (this.client.connectionEpoch === null) throw new Error("pairingRejected");
    const epoch = this.#epoch;
    if (this.#journal && await this.#journal.hasTab(expectedTabId)) throw new Error("busy");
    const tab = await this.current();
    if (epoch !== this.#epoch || tab.tabId !== expectedTabId) throw new Error("cancelled");
    if (this.#entries.has(tab.tabId) || this.#actions.has(tab.tabId)) throw new Error("busy");
    if (this.#entries.size >= 64 || !Number.isSafeInteger(this.#documentSequence + 1)) throw new Error("shareUnavailable");
    const entry = { tabId: tab.tabId, epoch, connection: this.client.connectionEpoch,
      documentGeneration: ++this.#documentSequence, offered: false, shared: false, retirement: null };
    this.#entries.set(tab.tabId, entry);
    try {
      const executionIdentity = await this.#clock.next();
      if (!this.#live(entry)) throw new Error("cancelled");
      const snapshot = await this.#document(tab.tabId);
      if (!this.#live(entry)) throw new Error("cancelled");
      entry.documentId = snapshot.documentId;
      entry.url = snapshot.url;
      const active = await this.current();
      if (!this.#live(entry) || active.tabId !== tab.tabId) throw new Error("cancelled");
      const currentDocument = await this.#document(tab.tabId, entry.documentId);
      if (!this.#live(entry) || currentDocument.url !== snapshot.url) throw new Error("cancelled");
      const fenced = await bounded(this.scripting.executeScript({
        target: { tabId: entry.tabId, documentIds: [entry.documentId] }, world: "ISOLATED",
        func: fenceDocument, args: [executionIdentity],
      }));
      if (!this.#live(entry) || fenced?.length !== 1 || fenced[0].frameId !== 0 || fenced[0].documentId !== entry.documentId
        || fenced[0].result?.epoch !== executionIdentity.epoch || fenced[0].result?.sequence !== executionIdentity.sequence) {
        throw new Error("shareUnavailable");
      }
      entry.offered = true;
      await this.client.shareCurrentTab({ tabId: tab.tabId, title: snapshot.title, url: snapshot.url,
        documentGeneration: entry.documentGeneration, focused: true }, entry.connection);
      if (!this.#live(entry)) throw new Error("cancelled");
      // Even same-URL reloads have a new Chrome documentId. Never inherit authority.
      await this.#document(tab.tabId, entry.documentId);
      if (!this.#live(entry)) throw new Error("cancelled");
      entry.shared = true;
      return { tabId: tab.tabId, shared: true };
    } catch (error) {
      entry.cancelled = true;
      try { await this.#retire(entry); }
      finally { if (this.#entries.get(tab.tabId) === entry) this.#entries.delete(tab.tabId); }
      throw error;
    }
  }

  #retire(entry) {
    if (entry.retirement) return entry.retirement;
    if (!entry.offered || entry.connection !== this.client.connectionEpoch) return Promise.resolve();
    entry.retirement = this.client.unshareTab(entry.tabId, entry.documentGeneration, entry.connection);
    return entry.retirement;
  }

  async invalidate(tabId) {
    const entry = this.#entries.get(tabId);
    if (!entry) return;
    // Local fence precedes any network await or navigation completion.
    entry.cancelled = true;
    entry.modelSnapshot = null;
    this.#cancelActions(tabId);
    try { await this.#retire(entry); }
    finally { if (this.#entries.get(tabId) === entry) this.#entries.delete(tabId); }
  }

  async unshare(expectedTabId) {
    const epoch = this.#epoch;
    const tab = await this.current();
    if (epoch !== this.#epoch || tab.tabId !== expectedTabId) throw new Error("cancelled");
    await this.invalidate(tab.tabId);
    return { tabId: tab.tabId, shared: false };
  }

  async observe(request) {
    const entry = this.#entries.get(Number(request.tabId));
    const activity = this.#activity;
    const live = () => entry && entry.shared && this.#live(entry)
      && activity === this.#activity && entry.documentGeneration === request.documentGeneration && Date.now() < request.deadlineMs;
    if (!live() || request.command.kind !== "observe") throw new Error("cancelled");
    if (this.#actions.has(entry.tabId)) throw new Error("busy");
    const executionIdentity = await this.#clock.next();
    if (!live()) throw new Error("cancelled");
    const active = await this.current();
    if (!live() || active.tabId !== entry.tabId) throw new Error("cancelled");
    const before = await this.#document(entry.tabId, entry.documentId);
    if (before.url !== entry.url) throw new Error("cancelled");
    if (!live()) throw new Error("cancelled");
    const results = await bounded(this.scripting.executeScript({
      target: { tabId: entry.tabId, documentIds: [entry.documentId] }, world: "ISOLATED",
      func: observeDocument, args: [request.command.snapshotId, request.command.preview !== true, executionIdentity],
    }));
    if (!live() || results.length !== 1 || results[0].frameId !== 0 || results[0].documentId !== entry.documentId
      || results[0].result?.snapshotId !== request.command.snapshotId) throw new Error("cancelled");
    let screenshot = null;
    if (request.command.screenshot === true) {
      const captureView = await this.#document(entry.tabId, entry.documentId);
      const [captureTab] = await bounded(this.tabs.query({ active: true, lastFocusedWindow: true }));
      if (!live() || !sameView(before, captureView) || captureTab?.id !== entry.tabId
        || !Number.isSafeInteger(captureTab.windowId) || captureTab.windowId < 0) throw new Error("cancelled");
      const dataUrl = await bounded(this.tabs.captureVisibleTab(captureTab.windowId, { format: "png" }));
      // Drop a suspect image before decoding or returning it to the Host.
      if (!live()) throw new Error("cancelled");
      const capturedView = await this.#document(entry.tabId, entry.documentId);
      if (!live() || !sameView(before, capturedView)) throw new Error("cancelled");
      screenshot = await bounded(normalizeCapture(dataUrl, before, live));
    }
    if (!live()) throw new Error("cancelled");
    const after = await this.#document(entry.tabId, entry.documentId);
    if (!sameView(before, after) || results[0].result.url !== entry.url) throw new Error("cancelled");
    const finalActive = await this.current();
    if (!live() || finalActive.tabId !== entry.tabId) throw new Error("cancelled");
    if (!request.command.preview) entry.modelSnapshot = {
      id: request.command.snapshotId, session: request.session, runId: request.runId,
      grantGeneration: request.grantGeneration, activity, view: before,
    };
    return { ...results[0].result, documentId: entry.documentId, screenshot };
  }

  // Only negotiated v2 dispatch reaches this. Host owns grant and completion-receipt checks.
  async act(request, signal = new AbortController().signal) {
    const entry = this.#entries.get(Number(request.tabId));
    const snapshot = entry?.modelSnapshot;
    const command = request.command;
    const rejected = { status: "rejected", detail: "stale_snapshot", physicallySettled: true };
    if (!entry || !snapshot || command?.kind !== "act" || this.#actions.size >= 8 || this.#actions.has(entry.tabId)
      || Object.keys(command).some(key => !["kind", "snapshotId", "elementRef", "action", "parameters"].includes(key))) return rejected;
    const operation = { id: request.requestId, tabId: entry.tabId, documentId: entry.documentId,
      snapshotId: snapshot.id, cancelled: signal.aborted, dispatched: false, cancellation: null };
    const live = () => !operation.cancelled && entry.shared && this.#live(entry) && entry.modelSnapshot === snapshot
      && request.documentId === entry.documentId
      && snapshot.activity === this.#activity && snapshot.id === command.snapshotId
      && snapshot.session === request.session && snapshot.runId === request.runId && snapshot.grantGeneration === request.grantGeneration
      && entry.documentGeneration === request.documentGeneration && Date.now() < request.deadlineMs;
    if (!live()) return rejected;
    this.#actions.set(entry.tabId, operation);
    const cancel = () => this.#cancelActions(entry.tabId);
    signal.addEventListener("abort", cancel, { once: true });
    const timer = setTimeout(cancel, Math.max(0, Math.min(10000, request.deadlineMs - Date.now())));
    let proof = false;
    let outcome = rejected;
    try {
      const active = await this.current();
      if (!live() || active.tabId !== entry.tabId) return rejected;
      const view = await this.#document(entry.tabId, entry.documentId);
      if (!live() || !sameView(snapshot.view, view)) return rejected;
      operation.dispatched = true;
      outcome = { status: "unknown", detail: "invalid_action_response" };
      // Await the ORIGINAL scripting promise. A timer only asks for cancellation; it never frees this slot.
      const rows = await this.scripting.executeScript({
        target: { tabId: entry.tabId, documentIds: [entry.documentId] }, world: "ISOLATED", func: actDocument,
        args: [{ operationId: request.requestId, snapshotId: command.snapshotId, elementRef: command.elementRef,
          action: command.action, parameters: command.parameters, deadlineMs: request.deadlineMs }],
      });
      if (rows.length === 1 && rows[0].frameId === 0 && rows[0].documentId === entry.documentId
        && ["rejected", "unknown", "applied", "verified"].includes(rows[0].result?.status)
        && typeof rows[0].result.detail === "string" && /^[a-z_]{1,64}$/.test(rows[0].result.detail)) {
        proof = true; outcome = rows[0].result;
      }
      if (!live()) outcome = { status: "unknown", detail: "cancelled_after_dispatch" };
    } catch {
      outcome = { status: operation.dispatched ? "unknown" : "rejected", detail: "document_unavailable" };
    } finally {
      clearTimeout(timer); signal.removeEventListener("abort", cancel);
      // Always fence an ambiguous native result; failure to confirm retirement keeps the slot busy.
      if (operation.dispatched && !proof && !operation.cancellation) cancel();
      if (operation.cancellation) proof = (await operation.cancellation) || proof;
      if (command.action !== "wait" || outcome.status === "unknown") entry.modelSnapshot = null;
      if (!operation.dispatched || proof) this.#actions.delete(entry.tabId);
      operation.returned = true;
    }
    return { ...outcome, physicallySettled: !operation.dispatched || proof };
  }
}
