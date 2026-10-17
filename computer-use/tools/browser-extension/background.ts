import { type ContentRuntime, type DomRequest, type DomResult, type Listener, type Port, error, record, validRequest } from "./shared";

/** Browser-authenticated scope; the page/model cannot supply tab/document identity. */
export interface SharedScope { tabId: number; documentId: string; shareId: string }
/** P8-02 adapter boundary. No native connection is installed by this card.
 * The adapter must authenticate gcu, bind session/connection/one-shot Router dispatch,
 * verify approval and persistent Stop immediately before dispatch, and signal loss
 * of Stop/connection/session health synchronously through onInvalidate.
 * Sharing is additional browser consent, never an approval decision or resume.
 */
export interface TrustedRouterChannel {
  authorize(request: DomRequest, scope: SharedScope): boolean;
  listen(onRequest: (request: DomRequest) => Promise<DomResult>, onInvalidate: () => void): () => void;
  shared(scope: SharedScope): void;
  revoked(scope: SharedScope): void;
}
/** Programmatic top-frame injection only. No persistent content scripts or hosts. */
export interface BrowserDriver {
  inject(tabId: number): Promise<{ documentId: string; port: Port }>;
  mark(tabId: number, shared: boolean, failed?: boolean): void;
}
interface Share {
  tabId: number; url: string; shareId: string;
  scope?: SharedScope; port?: Port; sessionId?: string;
  seen: Set<string>;
  pending?: { request: DomRequest; finish(result: DomResult): void };
}

/** In-memory scopes fail closed on worker restart, navigation or channel loss. */
export class SharingController {
  private shares = new Map<number, Share>();
  private unlisten?: () => void;
  constructor(private readonly browser: BrowserDriver, private readonly router?: TrustedRouterChannel) {
    this.unlisten = router?.listen(req => this.dispatch(req), () => this.clearAll());
  }

  /** Called exclusively by chrome.action.onClicked, never by a runtime/page request. */
  async toggle(tabId: number, url: string): Promise<void> {
    if (this.shares.has(tabId)) { this.clearTab(tabId); return; }
    // Browser internal/file/extension pages have no supported DOM route.
    if (!/^https?:\/\//i.test(url)) { this.browser.mark(tabId, false, true); return; }
    const share: Share = { tabId, url, shareId: crypto.randomUUID(), seen: new Set() };
    this.shares.set(tabId, share);
    try {
      const injected = await this.browser.inject(tabId);
      if (this.shares.get(tabId) !== share) { injected.port.disconnect(); return; }
      const scope = { tabId, documentId: injected.documentId, shareId: share.shareId };
      share.scope = scope; share.port = injected.port;
      injected.port.onDisconnect.addListener(() => { if (this.shares.get(tabId) === share) this.clearTab(tabId); });
      injected.port.onMessage.addListener(message => {
        if (this.shares.get(tabId) !== share) return;
        if (record(message, ["kind"]) && message.kind === "revoked") { this.clearTab(tabId); return; }
        if (record(message, ["kind", "requestId", "result"]) && message.kind === "response" &&
          message.requestId === share.pending?.request.requestId && this.shares.get(tabId) === share) {
          share.pending?.finish(message.result as DomResult);
        }
      });
      injected.port.postMessage({ kind: "activate", shareId: share.shareId, url });
      this.browser.mark(tabId, true);
      this.router?.shared(scope);
    } catch { if (this.shares.get(tabId) === share) { this.clearTab(tabId); this.browser.mark(tabId, false, true); } }
  }

  /** Clears first, then closes transport; late replies/injections cannot resurrect it. */
  clearTab(tabId: number): void {
    const share = this.shares.get(tabId);
    if (!share) return;
    this.shares.delete(tabId);
    share.pending?.finish(share.pending.request.operation.type === "observe" ? error("app_denied") : { kind: "action", outcome: "unknown" });
    share.seen.clear();
    try { share.port?.disconnect(); } catch { /* Scope is already irreversibly removed. */ }
    this.browser.mark(tabId, false);
    try { if (share.scope) this.router?.revoked(share.scope); } catch { /* Native connection loss must not prevent other scopes being cleared. */ }
  }
  /** Refreshes browser-owned indicators after worker restart without reading content. */
  refreshIndicator(tabId: number): void { this.browser.mark(tabId, this.shares.has(tabId)); }
  /** Stop/transport/session loss has no queue and no automatic recovery path. */
  clearAll(): void { for (const tabId of this.shares.keys()) this.clearTab(tabId); }
  /** Test/adapter teardown; does not clear Host Stop or preserve scopes. */
  dispose(): void { this.clearAll(); this.unlisten?.(); }

  /** Private Router ingress. Intentionally not exposed via runtime.onMessage. */
  async dispatch(request: DomRequest): Promise<DomResult> {
    if (!this.router) return error("capability_unavailable");
    if (!validRequest(request)) return error("capability_unavailable");
    const share = [...this.shares.values()].find(s => s.shareId === request.shareId);
    if (!share?.scope || !share.port) return error("app_denied");
    if (share.sessionId && share.sessionId !== request.sessionId) return error("app_denied");
    if (share.pending) return error("busy");
    if (share.seen.has(request.requestId)) return error("stale_element");
    if (share.seen.size >= 256) { this.clearTab(share.tabId); return error("app_denied"); }
    if (this.router.authorize(request, share.scope) !== true) return error("approval_denied");
    share.sessionId = request.sessionId;
    share.seen.add(request.requestId);
    return new Promise(resolve => {
      const timer = setTimeout(() => this.clearTab(share.tabId), 5000);
      share.pending = { request, finish: result => { clearTimeout(timer); share.pending = undefined; resolve(result); } };
      try {
        // No await between preflight and postMessage; invalidation clears all queued capabilities.
        if (this.shares.get(share.tabId) !== share) { share.pending.finish(error("app_denied")); return; }
        share.port!.postMessage({ kind: "request", request });
      } catch { this.clearTab(share.tabId); }
    });
  }
}

interface Tab { id?: number; url?: string }
interface Navigation { tabId: number }
interface ChromeApi {
  runtime: ContentRuntime;
  i18n: { getMessage(key: string): string };
  action: { onClicked: Listener<(tab: Tab) => void>; setTitle(details: { tabId?: number; title: string }): Promise<void>; setBadgeText(details: { tabId: number; text: string }): Promise<void> };
  tabs: {
    onRemoved: Listener<(tabId: number) => void>;
    onUpdated: Listener<(tabId: number, change: { status?: string; url?: string }) => void>;
    onReplaced: Listener<(addedTabId: number, removedTabId: number) => void>;
    connect(tabId: number, details: { name: string; documentId: string }): Port;
    query(query: Record<string, never>): Promise<Tab[]>;
  };
  scripting: { executeScript(details: { target: { tabId: number; frameIds: number[] }; files: string[]; world: "ISOLATED" }): Promise<{ frameId: number; documentId: string }[]> };
  webNavigation: { onBeforeNavigate: Listener<(details: Navigation) => void>; onCommitted: Listener<(details: Navigation) => void>; onHistoryStateUpdated: Listener<(details: Navigation) => void>; onReferenceFragmentUpdated: Listener<(details: Navigation) => void> };
}

/** Registers browser-owned consent and lifecycle events; leaves Router disconnected. */
export function installBackground(api: ChromeApi, router?: TrustedRouterChannel): SharingController {
  const controller = new SharingController({
    inject: async tabId => {
      const results = await api.scripting.executeScript({ target: { tabId, frameIds: [0] }, files: ["content.js"], world: "ISOLATED" });
      const top = results.find(result => result.frameId === 0);
      if (!top?.documentId) throw new Error("missing_document");
      return { documentId: top.documentId, port: api.tabs.connect(tabId, { name: "gcu.dom.v2", documentId: top.documentId }) };
    },
    mark: (tabId, shared, failed) => {
      void api.action.setTitle({ tabId, title: api.i18n.getMessage(failed ? "unavailable" : shared ? "revoke" : "share") }).catch(() => {});
      void api.action.setBadgeText({ tabId, text: failed ? "!" : shared ? "●" : "" }).catch(() => {});
    },
  }, router);
  // Tab-specific action titles/badges can outlive the worker's in-memory scopes.
  void api.tabs.query({}).then(tabs => {
    for (const tab of tabs) if (tab.id !== undefined) controller.refreshIndicator(tab.id);
  }).catch(() => { controller.clearAll(); });
  api.action.onClicked.addListener(tab => { if (tab.id !== undefined) void controller.toggle(tab.id, tab.url ?? ""); });
  api.tabs.onRemoved.addListener(tabId => controller.clearTab(tabId));
  api.tabs.onReplaced.addListener((_added, removed) => controller.clearTab(removed));
  api.tabs.onUpdated.addListener((tabId, change) => { if (change.status === "loading" || change.url !== undefined) controller.clearTab(tabId); });
  for (const navigation of [api.webNavigation.onBeforeNavigate, api.webNavigation.onCommitted,
    api.webNavigation.onHistoryStateUpdated, api.webNavigation.onReferenceFragmentUpdated]) {
    navigation.addListener(details => controller.clearTab(details.tabId));
  }
  return controller;
}
const browser = (globalThis as typeof globalThis & { chrome?: ChromeApi }).chrome;
if (browser) installBackground(browser);
