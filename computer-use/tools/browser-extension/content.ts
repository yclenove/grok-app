import { DomTree } from "./dom";
import { type ContentRuntime, type ResponseMessage, error, identifier, record, validRequest } from "./shared";

/** Isolated-world document session. There is no DOM read until an authorized observe. */
export class ContentSession {
  private share: { id: string; url: string } | null = null;
  private tree: DomTree;
  private documents = new Set<Document>();
  private frames = new Set<Element>();
  private readonly navigated = () => { this.revoke(); this.onRevoke(); };
  constructor(private readonly win: Window, private readonly onRevoke: () => void = () => {}) {
    this.tree = new DomTree(win, doc => {
      if (this.documents.has(doc)) return;
      this.documents.add(doc);
      doc.defaultView?.addEventListener("pagehide", this.navigated, { capture: true });
    }, frame => {
      if (this.frames.has(frame)) return;
      this.frames.add(frame);
      frame.addEventListener("load", this.navigated, { once: true });
    });
  }

  /** Disconnect, navigation, revoke and Stop all discard the exact same state. */
  revoke(): void {
    this.share = null;
    this.tree.clear();
    for (const doc of this.documents) {
      doc.defaultView?.removeEventListener("pagehide", this.navigated, { capture: true });
    }
    this.documents.clear();
    for (const frame of this.frames) frame.removeEventListener("load", this.navigated);
    this.frames.clear();
    this.win.removeEventListener("pagehide", this.navigated, { capture: true });
  }

  /** Called only by a private extension port; never wired to window.postMessage. */
  handle(message: unknown): ResponseMessage | null {
    if (record(message, ["kind", "shareId", "url"]) && message.kind === "activate" &&
      identifier(message.shareId) && typeof message.url === "string" && message.url === this.win.document.URL) {
      this.revoke();
      this.share = { id: message.shareId, url: message.url };
      this.win.addEventListener("pagehide", this.navigated, { capture: true });
      return null;
    }
    if (!record(message, ["kind", "request"]) || message.kind !== "request") return null;
    const req = message.request;
    // A safe request ID is the only data reflected on malformed input.
    if (!req || typeof req !== "object" || !("requestId" in req) || !identifier(req.requestId)) return null;
    const reply = (result: ResponseMessage["result"]): ResponseMessage => ({ kind: "response", requestId: req.requestId as string, result });
    if (!this.share) return reply(error("app_denied"));
    if (!validRequest(req)) return reply(error("capability_unavailable"));
    const live = () => this.share?.id === req.shareId && this.win.document.URL === this.share.url;
    if (!live()) { this.revoke(); return reply(error("app_denied")); }
    try {
      if (req.operation.type === "observe") return reply({ kind: "snapshot", snapshot: this.tree.observe() });
      return reply(this.tree.perform(req.operation, live));
    } catch { this.tree.clear(); return reply(error("driver_error")); }
  }
}

/** Installs an inert port listener. Only this extension's worker may attach. */
export function installContent(runtime: ContentRuntime, win: Window): void {
  let active: { close(): void } | null = null;
  runtime.onConnect.addListener(port => {
    if (port.name !== "gcu.dom.v2" || port.sender?.id !== runtime.id || port.sender.tab) { port.disconnect(); return; }
    active?.close();
    const session = new ContentSession(win, () => {
      try { port.postMessage({ kind: "revoked" }); } finally { port.disconnect(); }
    });
    const onMessage = (message: unknown) => {
      const response = session.handle(message);
      if (response) port.postMessage(response);
    };
    const close = () => { session.revoke(); port.onMessage.removeListener(onMessage); port.disconnect(); };
    const connection = { close };
    active = connection;
    port.onMessage.addListener(onMessage);
    port.onDisconnect.addListener(() => {
      session.revoke(); port.onMessage.removeListener(onMessage);
      if (active === connection) active = null;
    });
  });
}

// Programmatic injection is idempotent. This flag lives in Chrome's ISOLATED world.
const isolated = globalThis as typeof globalThis & { chrome?: { runtime: ContentRuntime }; __gcuDomV2Installed?: boolean };
if (isolated.chrome && !isolated.__gcuDomV2Installed) {
  isolated.__gcuDomV2Installed = true;
  installContent(isolated.chrome.runtime, window);
}
