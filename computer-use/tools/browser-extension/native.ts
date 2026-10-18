import type { SharedScope, TrustedRouterChannel } from "./background";
import { type DomRequest, type DomResult, type Listener, error, identifier, record, validRequest } from "./shared";
import manifest from "./manifest.json";

export const NATIVE_HOST = "ai.grok.computer_use";
const MAX_BYTES = 1024 * 1024;
export interface NativePort {
  onMessage: Listener<(message: unknown) => void>;
  onDisconnect: Listener<() => void>;
  postMessage(message: unknown): void;
  disconnect(): void;
}
export interface NativeRuntime {
  id: string;
  connectNative(host: string): NativePort;
  // Reading lastError inside disconnect consumes Chrome's diagnostic without logging it.
  lastError?: { message?: string };
}

/** Derive Chrome's ID from the checked-in public key, never from page/runtime messages. */
export async function fixedExtensionId(key: unknown): Promise<string | undefined> {
  if (typeof key !== "string" || !/^[A-Za-z0-9+/]+={0,2}$/.test(key) || key.length > 11_000) return undefined;
  try {
    const decoded = atob(key);
    if (!decoded.length || decoded.length > 8192 || btoa(decoded) !== key) return undefined;
    const bytes = Uint8Array.from(decoded, c => c.charCodeAt(0));
    const digest = new Uint8Array(await crypto.subtle.digest("SHA-256", bytes));
    return [...digest.slice(0, 16)].flatMap(b => [String.fromCharCode(97 + (b >> 4)), String.fromCharCode(97 + (b & 15))]).join("");
  } catch { return undefined; }
}

/** Native-only background dependency. No runtime/page listener, HTTP or reconnect loop.
 * Native requests originate in gcu's in-process RouterGate; sharing grants no approval.
 * The current P8-02 executable has no RouterGate assembly and sends no DOM requests.
 */
export class NativeChannel implements TrustedRouterChannel {
  private port?: NativePort;
  private connectionId?: string;
  private scopes = new Map<string, { scope: SharedScope; announced: boolean; ack: boolean; session?: string }>();
  private dispatch?: (request: DomRequest) => Promise<DomResult>;
  private invalidate?: () => void;
  private current?: { request: DomRequest; scope: SharedScope };
  private generation = 0;
  private connecting = false;
  private handshakeTimer?: ReturnType<typeof setTimeout>;
  constructor(private readonly runtime: NativeRuntime, private readonly publicKey: unknown = (manifest as { key?: string }).key) {}

  listen(dispatch: (request: DomRequest) => Promise<DomResult>, invalidate: () => void): () => void {
    this.dispatch = dispatch; this.invalidate = invalidate;
    return () => { this.close(); this.dispatch = undefined; this.invalidate = undefined; };
  }
  /** Accept only the exact one-shot request currently delivered by this native pipe. */
  authorize(request: DomRequest, scope: SharedScope): boolean {
    const shared = this.scopes.get(scope.shareId);
    if (!this.port || !this.connectionId || !shared?.ack || this.current?.request !== request ||
      this.current.scope !== shared.scope || scope.tabId !== shared.scope.tabId || scope.documentId !== shared.scope.documentId ||
      (shared.session !== undefined && shared.session !== request.sessionId)) return false;
    this.current = undefined;
    shared.session = request.sessionId;
    return true;
  }
  shared(scope: SharedScope): void {
    this.scopes.set(scope.shareId, { scope, announced: false, ack: false });
    if (this.connectionId) this.announce();
    else void this.connect();
  }
  revoked(scope: SharedScope): void {
    const shared = this.scopes.get(scope.shareId);
    this.scopes.delete(scope.shareId);
    if (shared?.announced && this.connectionId) this.send({ kind: "revoked", connectionId: this.connectionId, shareId: scope.shareId });
    if (!this.scopes.size && (this.port || this.connecting)) this.close();
  }
  private async connect(): Promise<void> {
    if (this.connecting || this.port || !this.scopes.size) return;
    this.connecting = true;
    const generation = this.generation;
    const expected = await fixedExtensionId(this.publicKey);
    if (generation !== this.generation) return;
    this.connecting = false;
    if (!expected || this.runtime.id !== expected) { this.close(); return; }
    try {
      const port = this.runtime.connectNative(NATIVE_HOST);
      this.port = port;
      port.onMessage.addListener(message => { if (this.port === port) this.receive(message); });
      port.onDisconnect.addListener(() => {
        void this.runtime.lastError;
        if (this.port === port) this.close();
      });
      this.handshakeTimer = setTimeout(() => this.close(), 5000);
    } catch { this.close(); }
  }
  private send(message: unknown): void {
    try {
      if (!this.port || new TextEncoder().encode(JSON.stringify(message)).length > MAX_BYTES) { this.close(); return; }
      this.port.postMessage(message);
    } catch { this.close(); }
  }
  private announce(): void {
    for (const shared of this.scopes.values()) {
      if (shared.announced) continue;
      shared.announced = true;
      this.send({ kind: "shared", connectionId: this.connectionId, scope: { ...shared.scope, frameId: 0 } });
    }
  }
  private receive(message: unknown): void {
    if (record(message, ["kind", "protocol", "connectionId"]) && message.kind === "hello" &&
      message.protocol === 1 && typeof message.connectionId === "string" && /^[a-f0-9]{64}$/.test(message.connectionId) && !this.connectionId) {
      clearTimeout(this.handshakeTimer);
      this.connectionId = message.connectionId;
      this.send({ kind: "ready", protocol: 1, connectionId: this.connectionId });
      this.announce();
      return;
    }
    if (!message || typeof message !== "object" || (message as { connectionId?: unknown }).connectionId !== this.connectionId || !this.connectionId) { this.close(); return; }
    if (record(message, ["kind", "connectionId", "shareId"]) && message.kind === "sharedAck" && identifier(message.shareId)) {
      const shared = this.scopes.get(message.shareId);
      if (!shared?.announced || shared.ack) { this.close(); return; }
      shared.ack = true;
      return;
    }
    if (record(message, ["kind", "connectionId", "request"]) && message.kind === "request" && validRequest(message.request)) {
      const request = message.request;
      const shared = this.scopes.get(request.shareId);
      if (!shared?.ack || !this.dispatch || this.current) { this.close(); return; }
      const port = this.port;
      const connectionId = this.connectionId;
      this.current = { request, scope: shared.scope };
      let result: Promise<DomResult>;
      try { result = this.dispatch(request); } catch { this.close(); return; }
      this.current = undefined;
      void result.catch(() => error("driver_error")).then(result => {
        // Never publish late data to a replacement connection or a revoked share.
        if (this.port === port && this.connectionId === connectionId && this.scopes.get(request.shareId) === shared) {
          this.send({ kind: "response", connectionId, requestId: request.requestId, result });
        }
      });
      return;
    }
    this.close();
  }
  /** Immediate invalidation, no queue, retry, persistent share or automatic resume. */
  private close(): void {
    const port = this.port;
    this.port = undefined; this.connectionId = undefined; this.current = undefined;
    this.scopes.clear(); this.generation++; this.connecting = false;
    clearTimeout(this.handshakeTimer);
    this.invalidate?.();
    try { port?.disconnect(); } catch { /* All capability state is already gone. */ }
  }
}
