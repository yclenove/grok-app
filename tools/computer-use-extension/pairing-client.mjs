import { parseEndpoint } from "./loopback-endpoint.mjs";
import { CompletionScope, copyCompletionProof, exactKeys, uuid } from "./completion-scope.mjs";
import { copyBoundAction } from "./action-completions.mjs";
import { copyHostLifetime } from "./host-lifetime.mjs";
import { recordConnectionFailure } from "./connection-diagnostics.mjs";
export { parseEndpoint } from "./loopback-endpoint.mjs";

export const EXTENSION_ID = "bgegbabkegkanjbmjbeaockdijnbkjgi";
const encoder = new TextEncoder();
const keyName = "cuPairing";

export function normalizeCode(value) {
  if (typeof value !== "string" || !/^[0-9a-f -]+$/i.test(value)) throw new Error("invalidCode");
  const normalized = value.replace(/[ -]/g, "").toUpperCase();
  if (!/^[0-9A-F]{20}$/.test(normalized)) throw new Error("invalidCode");
  return normalized;
}

export async function createProof(challenge, code, connectionNonce, crypto = globalThis.crypto, now = Date.now()) {
  if (!challenge || challenge.protocol !== 1 || challenge.ext !== EXTENSION_ID
    || typeof challenge.nonce !== "string" || challenge.nonce.length > 64
    || typeof challenge.instance !== "string" || challenge.instance.length > 64
    || !Number.isSafeInteger(challenge.expiresAt) || challenge.expiresAt <= now
    || challenge.expiresAt > now + 300_000
    || !/^[0-9a-f-]{36}$/i.test(connectionNonce)) throw new Error("challengeChanged");
  const material = JSON.stringify(["grok-cu-pairing-v1", challenge.nonce,
    challenge.instance, challenge.ext, challenge.expiresAt, connectionNonce]);
  const key = await crypto.subtle.importKey("raw", encoder.encode(normalizeCode(code)),
    { name: "HMAC", hash: "SHA-256" }, false, ["sign"]);
  const signature = new Uint8Array(await crypto.subtle.sign("HMAC", key, encoder.encode(material)));
  return { ...challenge, connectionNonce, response: [...signature].map(b => b.toString(16).padStart(2, "0")).join("") };
}

function identity(session) {
  return { instanceId: session.instanceId, connectionNonce: session.connectionNonce, generation: session.generation };
}

export class PairingClient {
  #session = null;
  #revision = 0;
  #busy = false;
  #controller = null;
  #heartbeatTimer = null;
  #statusRequest = null;
  #storageQueue = Promise.resolve();
  #shareSequence = 0;
  #actionsEpoch = null;

  constructor({ storage, fetch = (...args) => globalThis.fetch(...args), crypto = globalThis.crypto,
    now = Date.now, schedule = (fn, ms) => globalThis.setTimeout(fn, ms),
    cancel = id => globalThis.clearTimeout(id), onSessionChanged = () => {} }) {
    this.storage = storage;
    this.fetch = fetch;
    this.crypto = crypto;
    this.now = now;
    this.schedule = schedule;
    this.cancel = cancel;
    this.onSessionChanged = onSessionChanged;
    this.ready = this.#initialize();
  }

  async #initialize() {
    await this.storage.setAccessLevel({ accessLevel: "TRUSTED_CONTEXTS" });
    const prior = (await this.storage.get(keyName))[keyName];
    // Worker restart requires a new user pairing. Never replay an old grant.
    await this.#retire(prior);
  }

  async #request(endpoint, path, body, token, signal, responseLimit = 8192) {
    signal ??= AbortSignal.timeout(5000);
    let failure = "network";
    let status = null;
    try {
      const response = await this.fetch(parseEndpoint(endpoint) + path, {
        method: "POST", credentials: "omit", cache: "no-store", redirect: "error",
        headers: { "content-type": "application/json", ...(token ? { authorization: "Bearer " + token } : {}) },
        body: JSON.stringify(body), signal,
      });
      status = response.status;
      if (!response.ok) { failure = "http"; throw new Error("pairingRejected"); }
      failure = "body_read";
      // Read a bounded response even if the loopback process was replaced.
      const reader = response.body.getReader();
      const chunks = [];
      let size = 0;
      try {
        for (;;) {
          const { done, value } = await reader.read();
          if (done) break;
          size += value.byteLength;
          if (size > responseLimit) { failure = "body_limit"; throw new Error("pairingRejected"); }
          chunks.push(value);
        }
      } finally { await reader.cancel().catch(() => {}); }
      const data = new Uint8Array(size);
      let offset = 0;
      for (const chunk of chunks) { data.set(chunk, offset); offset += chunk.byteLength; }
      failure = "decode";
      const text = new TextDecoder("utf-8", { fatal: true }).decode(data);
      failure = "json";
      return JSON.parse(text);
    } catch (error) {
      recordConnectionFailure(path, signal.aborted
        ? signal.reason?.name === "TimeoutError" ? "timeout" : "cancelled" : failure, status);
      throw error;
    }
  }

  async #disconnect(session) {
    try { await this.#request(session.endpoint, "/cu/extension-disconnect", identity(session), session.sessionKey); }
    catch { /* Local authority is removed even if the App has already exited. */ }
  }

  async #retire(session) {
    // A session-storage failure must not prevent revocation at the Host.
    try {
      await this.#withStorage(async () => {
        const stored = (await this.storage.get(keyName))[keyName];
        // A slow cleanup of A must not delete B's newly stored connection.
        if (!session || (stored?.sessionKey === session.sessionKey
          && stored?.connectionNonce === session.connectionNonce)) await this.storage.remove(keyName);
      });
    }
    finally { if (session) await this.#disconnect(session); }
  }

  #withStorage(action) {
    const work = this.#storageQueue.then(action);
    this.#storageQueue = work.catch(() => {});
    return work;
  }

  #stopHeartbeat() {
    if (this.#heartbeatTimer !== null) this.cancel(this.#heartbeatTimer);
    this.#heartbeatTimer = null;
    this.#statusRequest?.controller.abort();
  }

  #publicState() {
    return this.#session ? { paired: true, endpoint: this.#session.endpoint } : { paired: false };
  }

  get connectionEpoch() { return this.#session ? this.#revision : null; }

  #setSession(session) {
    this.#actionsEpoch = null;
    this.#session = session;
    this.onSessionChanged();
  }

  #scheduleHeartbeat(session) {
    if (this.#session !== session) return;
    this.#heartbeatTimer = this.schedule(async () => {
      this.#heartbeatTimer = null;
      // Popup status and renewal share one bounded in-flight request.
      try { await this.status(); }
      catch { /* Status already retired local authority, including storage faults. */ }
      if (this.#session === session) this.#scheduleHeartbeat(session);
    }, 10000);
    // Node unit tests must not be kept alive by a browser-lifecycle timer.
    this.#heartbeatTimer?.unref?.();
  }

  async pair(endpoint, code) {
    const requestedRevision = this.#revision;
    await this.ready;
    if (requestedRevision !== this.#revision) throw new Error("cancelled");
    if (this.#busy) throw new Error("busy");
    endpoint = parseEndpoint(endpoint);
    normalizeCode(code);
    this.#busy = true;
    const revision = ++this.#revision;
    const controller = new AbortController();
    this.#controller = controller;
    const timeout = setTimeout(() => controller.abort(new DOMException("Request timed out", "TimeoutError")), 10_000);
    let minted = null;
    try {
      const old = this.#session;
      this.#setSession(null);
      this.#stopHeartbeat();
      await this.#retire(old);
      const challenge = await this.#request(endpoint, "/cu/pairing-challenge", {}, null, controller.signal);
      const proof = await createProof(challenge, code, this.crypto.randomUUID(), this.crypto, this.now());
      const session = await this.#request(endpoint, "/cu/pairing-confirm", proof, null, controller.signal);
      if (!session || !/^[a-f0-9]{64}$/.test(session.sessionKey)
        || session.instanceId !== challenge.instance || session.connectionNonce !== proof.connectionNonce
        || !Number.isSafeInteger(session.generation) || session.generation < 1) throw new Error("pairingRejected");
      const record = { ...identity(session), sessionKey: session.sessionKey, endpoint };
      minted = record;
      if (revision !== this.#revision) { await this.#disconnect(record); throw new Error("cancelled"); }
      await this.#withStorage(() => this.storage.set({ [keyName]: record }));
      if (revision !== this.#revision) {
        await this.#retire(record); throw new Error("cancelled");
      }
      this.#setSession(record);
      this.#shareSequence = 0;
      this.#scheduleHeartbeat(record);
      return { paired: true, endpoint };
    } catch (error) {
      if (minted && this.#session === minted) {
        this.#setSession(null);
        this.#stopHeartbeat();
      }
      if (minted) await this.#retire(minted);
      throw error;
    } finally {
      clearTimeout(timeout);
      this.#busy = false;
      if (this.#controller === controller) this.#controller = null;
    }
  }

  async status() {
    await this.ready;
    const session = this.#session;
    if (!session) return { paired: false };
    if (this.#statusRequest?.session === session) return this.#statusRequest.promise;
    const request = { session, controller: new AbortController(), promise: null };
    this.#statusRequest = request;
    request.promise = this.#readStatus(request);
    return request.promise;
  }

  async shareCurrentTab(tab, expectedEpoch = this.connectionEpoch) {
    const session = this.#session;
    await this.ready;
    if (!session) throw new Error("pairingRejected");
    if (this.#session !== session || this.connectionEpoch !== expectedEpoch) throw new Error("cancelled");
    if (!tab || !/^\d{1,16}$/.test(String(tab.tabId)) || typeof tab.url !== "string"
      || (!tab.url.startsWith("http://") && !tab.url.startsWith("https://"))
      || tab.url.length > 8192 || String(tab.title ?? "").length > 1024
      || !Number.isSafeInteger(tab.documentGeneration) || tab.documentGeneration < 1) {
      throw new Error("shareUnavailable");
    }
    const url = new URL(tab.url);
    if (!url.hostname || url.username || url.password) throw new Error("shareUnavailable");
    await this.#shareUpdate(session, "/cu/tab-offer", {
      ...identity(session), tabId: String(tab.tabId), title: String(tab.title ?? ""),
      url: tab.url, browserId: "chromium", profileId: session.connectionNonce,
      documentGeneration: tab.documentGeneration, focused: tab.focused === true,
    });
    return { shared: true, tabId: String(tab.tabId) };
  }

  async unshareTab(tabId, documentGeneration, expectedEpoch = this.connectionEpoch) {
    const session = this.#session;
    await this.ready;
    if (!session) throw new Error("pairingRejected");
    if (this.#session !== session || this.connectionEpoch !== expectedEpoch) throw new Error("cancelled");
    if (!/^\d{1,16}$/.test(String(tabId)) || !Number.isSafeInteger(documentGeneration)
      || documentGeneration < 1) throw new Error("shareUnavailable");
    const result = await this.#shareUpdate(session, "/cu/tab-unoffer", {
      ...identity(session), tabId: String(tabId), documentGeneration,
    });
    return { shared: false, tabId: String(tabId), removed: result.removed === true };
  }

  async #shareUpdate(session, route, body) {
    const sequence = ++this.#shareSequence;
    try {
      if (!Number.isSafeInteger(sequence)) throw new Error("shareUnavailable");
      const result = await this.#request(session.endpoint, route, { ...body, sequence }, session.sessionKey);
      if (this.#session !== session) throw new Error("cancelled");
      if (result?.ok !== true) throw new Error("shareUnavailable");
      return result;
    } catch (error) {
      if (this.#session === session) {
        // Failed/ambiguous unshare must not leave a renewing App grant behind.
        recordConnectionFailure(route, "retire_share");
        this.#setSession(null);
        this.#stopHeartbeat();
        await this.#retire(session);
      }
      throw error;
    }
  }

  async #extensionRequest(epoch, path, makeBody, signal) {
    const session = this.#session;
    await this.ready;
    if (!session || this.#session !== session || this.connectionEpoch !== epoch || signal.aborted) throw new Error("cancelled");
    const body = makeBody(session);
    const limit = path === "/cu/extension-result" ? 768 * 1024 : 64 * 1024;
    if (encoder.encode(JSON.stringify(body)).byteLength > limit) throw new Error("invalid extension result");
    const controller = new AbortController();
    const abort = () => controller.abort();
    signal.addEventListener("abort", abort, { once: true });
    const timer = setTimeout(() => controller.abort(new DOMException("Request timed out", "TimeoutError")), 5000);
    try {
      const response = await this.#request(session.endpoint, path, body, session.sessionKey, controller.signal,
        path === "/cu/extension-actions/poll" ? 64 * 1024 : 8192);
      if (this.#session !== session || this.connectionEpoch !== epoch || signal.aborted || controller.signal.aborted) {
        recordConnectionFailure(path, "response_identity"); throw new Error("cancelled");
      }
      if (response?.ok !== true) { recordConnectionFailure(path, "response_protocol"); throw new Error("invalid extension response"); }
      return response;
    } finally { clearTimeout(timer); signal.removeEventListener("abort", abort); }
  }

  async pollExtension(epoch, signal) {
    const result = await this.#extensionRequest(epoch, "/cu/extension-poll", identity, signal);
    if (result.request === null) return null;
    const connection = result.request?.connection;
    const session = this.#session;
    if (!session || this.connectionEpoch !== epoch || connection?.instanceId !== session.instanceId
      || connection.connectionNonce !== session.connectionNonce || connection.generation !== session.generation) {
      throw new Error("invalid extension connection");
    }
    return result.request;
  }

  async retiredHost(input) {
    const lifetime = copyHostLifetime(input);
    const reply = await this.#extensionRequest(this.connectionEpoch, "/cu/extension-completion/host-retirement",
      session => ({ connection: identity(session), lifetime }), new AbortController().signal);
    if (!exactKeys(reply, ["ok", "state", "lifetime"]) || !["live", "retired", "unavailable"].includes(reply.state)) {
      throw new Error("hostLifetimeUnavailable");
    }
    const received = copyHostLifetime(reply.lifetime, lifetime.instanceId);
    if (Object.keys(lifetime).some(key => lifetime[key] !== received[key])) throw new Error("hostLifetimeUnavailable");
    return reply.state === "retired";
  }

  async registerBrowser(currentId, previousId = null, cleanups = []) {
    if (!uuid(currentId) || (previousId !== null && (!uuid(previousId) || currentId === previousId))
      || !Array.isArray(cleanups)) {
      throw new Error("browserSessionUnavailable");
    }
    const reply = await this.#extensionRequest(this.connectionEpoch, "/cu/browser-restart",
      session => ({ connection: identity(session), currentId, previousId, cleanups }), new AbortController().signal);
    if (!exactKeys(reply, ["ok", "abandoned", "results"]) || reply.ok !== true
      || !Number.isSafeInteger(reply.abandoned) || reply.abandoned < 0 || reply.abandoned > 8
      || !Array.isArray(reply.results) || reply.results.length !== reply.abandoned
      || reply.results.some((item) => !exactKeys(item, ["requestId", "result"]) || !uuid(item.requestId)
        || (item.result !== "cleanup" && item.result !== "unknown"))) {
      throw new Error("browserSessionUnavailable");
    }
    return reply;
  }

  async retireBrowser(currentId, previousId) {
    return this.registerBrowser(currentId, previousId);
  }

  async completeExtension(epoch, result, signal) {
    return this.#extensionRequest(epoch, "/cu/extension-result", session => {
      const connection = result.request?.connection;
      if (connection?.instanceId !== session.instanceId || connection.connectionNonce !== session.connectionNonce
        || connection.generation !== session.generation) throw new Error("cancelled");
      return result;
    }, signal);
  }

  async #actionRequest(epoch, route, makeBody, signal, negotiation = false) {
    try {
      if (!negotiation && this.#actionsEpoch !== epoch) throw new Error("unnegotiated");
      return await this.#extensionRequest(epoch, "/cu/extension-actions/" + route, makeBody, signal);
    } catch { throw new Error("actionUnavailable"); }
  }

  async negotiateActions(epoch, signal) {
    if (this.connectionEpoch !== epoch) throw new Error("actionUnavailable");
    this.#actionsEpoch = null;
    const reply = await this.#actionRequest(epoch, "negotiate", session => ({ protocol: 2, completionProtocol: 2,
      connection: identity(session), actions: ["click", "set_value", "type_text", "scroll", "wait"] }), signal, true);
    if (!exactKeys(reply, ["ok", "protocol", "completionProtocol"]) || reply.protocol !== 2 || reply.completionProtocol !== 2
      || this.connectionEpoch !== epoch || signal.aborted) throw new Error("actionUnavailable");
    this.#actionsEpoch = epoch;
  }

  #boundAction(epoch, dispatch) {
    try {
      if (!exactKeys(dispatch, ["request", "proof"]) || this.connectionEpoch !== epoch || this.#actionsEpoch !== epoch) {
        throw new Error("unnegotiated");
      }
      const copied = copyBoundAction(dispatch.request, dispatch.proof);
      const current = this.#session;
      const connection = copied.request.connection;
      if (!current || connection.instanceId !== current.instanceId || connection.connectionNonce !== current.connectionNonce
        || connection.generation !== current.generation) throw new Error("connection changed");
      return copied;
    } catch { throw new Error("actionUnavailable"); }
  }

  async pollAction(epoch, signal) {
    const reply = await this.#actionRequest(epoch, "poll", identity, signal);
    try {
      if (!exactKeys(reply, ["ok", "dispatch"])) throw new Error("actionUnavailable");
      return reply.dispatch === null ? null : this.#boundAction(epoch, reply.dispatch);
    } catch (error) { recordConnectionFailure("/cu/extension-actions/poll", "response_protocol"); throw error; }
  }

  async claimAction(epoch, dispatch, signal) {
    const copied = this.#boundAction(epoch, dispatch);
    const reply = await this.#actionRequest(epoch, "claim", () => copied, signal);
    if (!exactKeys(reply, ["ok"])) throw new Error("actionUnavailable");
  }

  async completeAction(epoch, result, signal) {
    if (!exactKeys(result, ["request", "proof", "outcome"]) || !exactKeys(result.outcome, ["status", "detail"])
      || !["rejected", "unknown", "applied", "verified"].includes(result.outcome.status)
      || typeof result.outcome.detail !== "string" || !/^[a-z_]{1,64}$/.test(result.outcome.detail)) throw new Error("actionUnavailable");
    const copied = this.#boundAction(epoch, { request: result.request, proof: result.proof });
    const outcome = { ...result.outcome };
    const reply = await this.#actionRequest(epoch, "result", () => ({ ...copied, outcome }), signal);
    if (!exactKeys(reply, ["ok"])) throw new Error("actionUnavailable");
  }

  // Capture cleanup authority before attempting claim, including when its reply is lost.
  completionScope(epoch, proof, journal) {
    const session = this.#session;
    const copied = copyCompletionProof(proof);
    const connection = copied.binding.connection;
    if (!session || this.connectionEpoch !== epoch || connection.instanceId !== session.instanceId
      || connection.connectionNonce !== session.connectionNonce || connection.generation !== session.generation) {
      throw new Error("cancelled");
    }
    return new CompletionScope({ endpoint: session.endpoint, proof: copied, fetch: this.fetch, journal });
  }

  async claimCompletion(epoch, proof, signal) {
    const copied = copyCompletionProof(proof);
    const reply = await this.#extensionRequest(epoch, "/cu/extension-completion/claim", session => {
      const connection = copied.binding.connection;
      if (connection.instanceId !== session.instanceId || connection.connectionNonce !== session.connectionNonce
        || connection.generation !== session.generation) throw new Error("cancelled");
      return copied;
    }, signal);
    if (!exactKeys(reply, ["ok"])) throw new Error("invalidCompletion");
  }

  async retireTransport(epoch, source = "transport") {
    if (this.connectionEpoch !== epoch) return;
    const observation = source === "observation";
    recordConnectionFailure(observation ? "/cu/extension-poll" : "/cu/extension-actions/poll",
      observation ? "retire_observation" : source === "action" ? "retire_action" : "retire_transport");
    const session = this.#session;
    this.#setSession(null); this.#stopHeartbeat();
    await this.#retire(session);
  }

  async #readStatus(request) {
    const { session, controller } = request;
    const timer = setTimeout(() => controller.abort(new DOMException("Request timed out", "TimeoutError")), 5000);
    try {
      const result = await this.#request(session.endpoint, "/cu/extension-status", identity(session), session.sessionKey, controller.signal);
      if (result?.ok !== true) throw new Error("pairingRejected");
      return this.#publicState();
    } catch {
      if (this.#session === session) {
        recordConnectionFailure("/cu/extension-status", "retire_status");
        this.#setSession(null);
        this.#stopHeartbeat();
        await this.#retire(session);
      }
      return this.#publicState();
    } finally {
      clearTimeout(timer);
      if (this.#statusRequest === request) this.#statusRequest = null;
    }
  }

  async forget() {
    ++this.#revision;
    this.#controller?.abort();
    await this.ready;
    const old = this.#session;
    this.#setSession(null);
    this.#stopHeartbeat();
    await this.#retire(old);
    return this.#publicState();
  }
}
