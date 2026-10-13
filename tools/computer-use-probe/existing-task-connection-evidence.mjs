// Passive metadata from the exact owned native worker; never headers/body/URLs/IDs.
const routes = new Set([
  "/cu/pairing-challenge", "/cu/pairing-confirm", "/cu/extension-status", "/cu/extension-disconnect",
  "/cu/browser-restart", "/cu/tab-offer", "/cu/tab-unoffer", "/cu/extension-poll", "/cu/extension-result",
  "/cu/extension-actions/negotiate", "/cu/extension-actions/poll", "/cu/extension-actions/claim", "/cu/extension-actions/result",
  "/cu/extension-completion/claim", "/cu/extension-completion/host-retirement",
]);
const outcomes = new Set(["pending", "response", "failed", "cancelled"]);
const networkErrors = new Set([
  "net::ERR_ABORTED", "net::ERR_FAILED", "net::ERR_ACCESS_DENIED", "net::ERR_NETWORK_ACCESS_DENIED",
  "net::ERR_UNSAFE_PORT", "net::ERR_CONNECTION_REFUSED", "net::ERR_CONNECTION_RESET",
  "net::ERR_CONNECTION_CLOSED", "net::ERR_CONNECTION_TIMED_OUT", "net::ERR_TIMED_OUT",
  "net::ERR_ADDRESS_INVALID", "net::ERR_ADDRESS_UNREACHABLE", "net::ERR_NAME_NOT_RESOLVED",
  "net::ERR_EMPTY_RESPONSE", "net::ERR_INVALID_RESPONSE", "net::ERR_INTERNET_DISCONNECTED",
  "net::ERR_NETWORK_CHANGED", "net::ERR_BLOCKED_BY_CLIENT", "net::ERR_BLOCKED_BY_RESPONSE",
  "net::ERR_BLOCKED_BY_ADMINISTRATOR", "net::ERR_BLOCKED_BY_CSP", "net::ERR_BLOCKED_BY_ORB",
]);
const blockedReasons = new Set([
  "other", "csp", "mixed-content", "origin", "inspector", "subresource-filter", "content-type",
  "coep-frame-resource-needs-coep-header", "coop-sandboxed-iframe-cannot-navigate-to-coop-page",
  "corp-not-same-origin", "corp-not-same-origin-after-defaulted-to-same-origin-by-coep", "corp-not-same-site",
]);
const corsErrors = new Set([
  "DisallowedByMode", "InvalidResponse", "WildcardOriginNotAllowed", "MissingAllowOriginHeader",
  "MultipleAllowOriginValues", "InvalidAllowOriginValue", "AllowOriginMismatch", "InvalidAllowCredentials",
  "CorsDisabledScheme", "PreflightInvalidStatus", "PreflightDisallowedRedirect",
  "PreflightWildcardOriginNotAllowed", "PreflightMissingAllowOriginHeader", "PreflightMultipleAllowOriginValues",
  "PreflightInvalidAllowOriginValue", "PreflightAllowOriginMismatch", "PreflightInvalidAllowCredentials",
  "PreflightMissingAllowExternal", "PreflightInvalidAllowExternal", "InvalidAllowMethodsPreflightResponse",
  "InvalidAllowHeadersPreflightResponse", "MethodDisallowedByPreflightResponse", "HeaderDisallowedByPreflightResponse",
  "RedirectContainsCredentials", "InsecurePrivateNetwork", "InvalidPrivateNetworkAccess",
  "PreflightMissingAllowPrivateNetwork", "PreflightInvalidAllowPrivateNetwork",
  "LocalNetworkAccessPermissionDenied",
]);
const count = value => Number.isSafeInteger(value) && value >= 0 ? value : null;
const status = value => Number.isInteger(value) && value >= 100 && value <= 599 ? value : null;
const optionalEnum = (value, allowed) => value == null ? null : allowed.has(value) ? value : "other";
const failure = value => ({
  networkError: optionalEnum(value?.networkError, networkErrors),
  blockedReason: optionalEnum(value?.blockedReason, blockedReasons),
  corsError: optionalEnum(value?.corsError, corsErrors),
});

export function sanitizeConnectionEvidence(value) {
  if (!value) return null;
  return {
    detached: value.detached === true, workerStopped: value.workerStopped === true,
    contextsCleared: count(value.contextsCleared),
    pairingConfirmed: count(value.pairingConfirmed), disconnectRequests: count(value.disconnectRequests),
    http: Array.isArray(value.http) ? value.http.slice(-32).filter(row => routes.has(row?.route))
      .map(row => ({ route: row.route, status: status(row.status),
        outcome: outcomes.has(row.outcome) ? row.outcome : "pending", elapsedMs: count(row.elapsedMs),
        ...(row.failure ? { failure: failure(row.failure) } : {}) })) : [],
  };
}

export class NativeConnectionEvidence {
  #requests = new Map();
  #records = [];
  #started = performance.now();
  #pairingConfirmed = 0;
  #disconnectRequests = 0;
  #contextsCleared = 0;
  #detached = false;
  #workerStopped = false;

  accept(message) {
    const params = message.params;
    if (message.method === "Runtime.executionContextsCleared") { this.#contextsCleared++; return; }
    if (message.method === "Network.requestWillBeSent") {
      let url;
      try { url = new URL(params?.request?.url); } catch { return; }
      if (url.protocol !== "http:" || url.hostname !== "127.0.0.1" || !routes.has(url.pathname)) return;
      const record = { route: url.pathname, status: null, outcome: "pending",
        elapsedMs: Math.round(performance.now() - this.#started) };
      // CDP request IDs are private lookup keys and never leave this cache.
      this.#requests.delete(params.requestId);
      this.#requests.set(params.requestId, record);
      this.#records.push(record);
      if (this.#records.length > 32) this.#records.shift();
      if (this.#requests.size > 32) this.#requests.delete(this.#requests.keys().next().value);
      if (record.route === "/cu/extension-disconnect") this.#disconnectRequests++;
      return;
    }
    const record = this.#requests.get(params?.requestId);
    if (!record) return;
    if (message.method === "Network.responseReceived") {
      // Only count the first response for this original request.
      if (record.outcome !== "pending") return;
      record.status = status(params.response?.status);
      record.outcome = "response";
      if (record.route === "/cu/pairing-confirm" && record.status === 200) this.#pairingConfirmed++;
    } else if (message.method === "Network.loadingFailed") {
      record.outcome = params.canceled === true ? "cancelled" : "failed";
      // Only fixed CDP enums survive: errorText/failedParameter may contain URLs or credentials.
      record.failure = failure({ networkError: params.errorText, blockedReason: params.blockedReason,
        corsError: params.corsErrorStatus?.corsError });
      this.#requests.delete(params.requestId);
    } else if (message.method === "Network.loadingFinished") {
      this.#requests.delete(params.requestId);
    }
  }

  markDetached() { this.#detached = true; }
  markWorkerStopped() { this.#workerStopped = true; }
  snapshot() {
    return sanitizeConnectionEvidence({ detached: this.#detached, workerStopped: this.#workerStopped,
      contextsCleared: this.#contextsCleared, pairingConfirmed: this.#pairingConfirmed,
      disconnectRequests: this.#disconnectRequests, http: this.#records });
  }
}
