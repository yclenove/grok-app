// Trusted-worker memory only. Never persist, broadcast, or include request data.
const routes = new Set([
  "/cu/pairing-challenge", "/cu/pairing-confirm", "/cu/extension-status", "/cu/extension-disconnect",
  "/cu/browser-restart", "/cu/tab-offer", "/cu/tab-unoffer", "/cu/extension-poll", "/cu/extension-result",
  "/cu/extension-actions/negotiate", "/cu/extension-actions/poll", "/cu/extension-actions/claim", "/cu/extension-actions/result",
  "/cu/extension-completion/claim", "/cu/extension-completion/host-retirement",
]);
const reasons = new Set(["http", "network", "timeout", "cancelled", "body_read", "body_limit", "decode", "json", "response_identity", "response_protocol",
  "retire_status", "retire_share", "retire_observation", "retire_action", "retire_transport"]);
const records = [];
export function recordConnectionFailure(route, reason, status = null) {
  if (!routes.has(route) || !reasons.has(reason)) return;
  records.push({ elapsedMs: Math.round(performance.now()), route, reason,
    status: Number.isInteger(status) && status >= 100 && status <= 599 ? status : null });
  if (records.length > 32) records.shift();
}
export function readConnectionDiagnostics() { return records.map(row => ({ ...row })); }
