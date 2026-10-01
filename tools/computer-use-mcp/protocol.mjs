const id = { type: "string", minLength: 1, maxLength: 256 };
const number = { type: "number", minimum: 0 };
const object = (properties, required = []) => ({ type: "object", properties, required, additionalProperties: false });
const target = {
  oneOf: [object({ elementRef: id }, ["elementRef"]), object({ x: number, y: number }, ["x", "y"])],
};
const identity = {
  version: { const: 1 }, actionId: id, runId: id, targetId: id,
  targetGeneration: { type: "integer", minimum: 1 }, snapshotId: id,
  geometryRevision: { type: "integer", minimum: 0 }, target,
};
const text = { type: "string", maxLength: 4000 };
const { target: _waitTarget, ...waitIdentity } = identity;
const actions = [
  ["click", object({ button: { enum: ["left", "right", "middle"] }, count: { enum: [1, 2] } })],
  ["set_value", object({ text }, ["text"])],
  ["type_text", object({ text, via: { const: "clipboard" } }, ["text"])],
  ["key", object({ key: { type: "string", minLength: 1, maxLength: 32 } }, ["key"])],
  ["scroll", object({ delta: { type: "integer", minimum: -2400, maximum: 2400 } }, ["delta"])],
  ["drag", { oneOf: [object({ toX: number, toY: number }, ["toX", "toY"]), object({ x1: number, y1: number }, ["x1", "y1"])] }],
  ["wait", object({ nameEquals: text, timeoutMs: { type: "integer", minimum: 1, maximum: 10000 } }, ["nameEquals"])],
];

export const PROTOCOL_VERSION = 1;
export const MODEL_TOOLS = [
  "computer_status",
  "computer_list_targets",
  "computer_open_target",
  "computer_observe",
  "computer_act",
  "computer_wait",
  "computer_request_handoff",
  "computer_stop",
  "computer_navigate",
  "computer_download",
  "browser_list_tabs",
  "browser_open",
  "browser_observe",
  "browser_act",
];
export const HOST_ONLY_TOOLS = [
  "computer_authorize",
  "computer_resume",
  "computer_reconnect",
  "computer_pause",
];

export const TOOLS = [
  ["computer_status", "Current task, capabilities and control state. Not proof that an action succeeded.", object({})],
  ["computer_list_targets", "List the user-authorized target only. A new window requires the Computer panel.", object({})],
  ["computer_open_target", "Use a target already authorized by the user. This cannot grant access to a new window.", object({ targetId: id }, ["targetId"])],
  ["computer_observe", "Observe the authorized target (observe → act → verify). Copy snapshot and target IDs from this result. Screenshots arrive as image content. Use screenshot:false for text and semantic controls only. Coordinate actions need a visible image; text-only observations cannot click pixels.", object({ screenshot: { type: "boolean", default: true } })],
  ["computer_act", "One action using IDs from the observation you saw. Drag requires a visual observation and one explicit destination pair: toX/toY (or legacy x1/y1), in image pixels. Never mix the pairs. Transport success is not verification. An unknown outcome requires a new observation; do not retry the same side-effecting actionId. Reuse actionId only to retrieve its original result.", {
    type: "object", oneOf: actions.map(([action, parameters]) => object({ ...identity, action: { const: action }, parameters }, [...Object.keys(identity), "action", "parameters"])),
  }],
  ["computer_wait", "Read-only wait for the exact observed element. Copy all identity fields from the observation you saw and use a fresh actionId. Never guesses a new snapshot or matches another element by name. Both success and timeout preserve the current snapshot; observe again for a fresh view. Timeout is not success and consumes the bounded wait budget.", object({ ...waitIdentity, elementRef: id, nameEquals: { type: "string", minLength: 1, maxLength: 256 }, timeoutMs: { type: "integer", minimum: 1, maximum: 10000 } }, [...Object.keys(waitIdentity), "elementRef", "nameEquals"])],
  ["computer_request_handoff", "Pause so the user can pick another target, log in, or handle a permission UI. Only the user can resume.", object({})],
  ["computer_stop", "Request stop for this run. The model cannot resume, reconnect, or authorize a new target.", object({})],
  ["computer_navigate", "Navigate the authorized managed tab. javascript, file, data and metadata URLs are rejected. Requires actionId, tabId and pageGeneration. Browser tools cannot read cookies, tokens or storage.", object({ tabId: id, url: { type: "string", minLength: 1, maxLength: 2048 }, actionId: id, pageGeneration: { type: "integer", minimum: 1 } }, ["tabId", "url", "actionId", "pageGeneration"])],
  ["computer_download", "Download into run staging. Model filesystem paths are not trusted. Requires actionId, tabId, pageGeneration, snapshotId and elementRef.", object({ tabId: id, filename: { type: "string", minLength: 1, maxLength: 256 }, actionId: id, pageGeneration: { type: "integer", minimum: 1 }, snapshotId: id, elementRef: id }, ["tabId", "actionId", "pageGeneration", "snapshotId", "elementRef"])],
  ["browser_list_tabs", "List tabs this run already owns or borrowed. Does not leak other titles or URLs. Does not create grants.", object({})],
  ["browser_open", "Select a tab the user already authorized or this run owns. Does not create a grant or open an arbitrary profile.", object({ tabId: id }, ["tabId"])],
  ["browser_observe", "Observe the selected tab (observe → act → verify). Screenshots arrive as image content; text JSON omits png bytes. Coordinate actions need a visible image.", object({ tabId: id, pageGeneration: { type: "integer", minimum: 1 } }, ["tabId", "pageGeneration"])],
  ["browser_act", "One typed browser action using IDs from the observation you saw. An unknown outcome requires a new observation; do not retry the same side-effecting actionId. Stop cannot resume. Missing screenshots reject coordinate actions. Cannot read cookies, tokens or storage.", object({ tabId: id, actionId: id, pageGeneration: { type: "integer", minimum: 1 }, snapshotId: id, kind: { type: "string", minLength: 1, maxLength: 32 }, elementRef: id, parameters: { type: "object" } }, ["tabId", "actionId", "pageGeneration", "snapshotId", "kind"])],
].map(([name, description, inputSchema]) => ({ name, description, inputSchema }));

const REQUEST_KEYS = new Set(["version", "actionId", "runId", "targetId", "targetGeneration", "snapshotId", "geometryRevision", "action", "target", "parameters"]);
const ACTION_PARAMS = {
  click: ["button", "count"],
  set_value: ["text"],
  type_text: ["text", "via"],
  key: ["key"],
  scroll: ["delta"],
  drag: ["toX", "toY", "x1", "y1"],
  wait: ["nameEquals", "timeoutMs"],
};
const ALLOWED_KEYS = new Set([
  "enter", "return", "tab", "escape", "esc", "space", "down", "up", "left", "right",
  "backspace", "delete", "home", "end", "pageup", "page_up", "pagedown", "page_down",
]);

function grantKey(params) {
  for (const key of Object.keys(params)) {
    const normalized = key.toLowerCase().replace(/[^a-z0-9]/g, "");
    if (normalized === "yolo" || normalized === "acceptedits" || normalized === "alwaysapprove" || normalized === "alwaysapproveedits") {
      return key;
    }
  }
  return null;
}

export function validateActionRequest(req) {
  if (!req || typeof req !== "object" || Array.isArray(req)) return "parameters must be an object";
  for (const key of Object.keys(req)) {
    if (!REQUEST_KEYS.has(key)) return `unknown field ${key}`;
  }
  if (req.version !== PROTOCOL_VERSION) return `unsupported protocol version ${req.version}`;
  if (!req.parameters || typeof req.parameters !== "object" || Array.isArray(req.parameters)) return "parameters must be an object";
  const grant = grantKey(req.parameters);
  if (grant) return `${grant} never grants desktop control`;
  if (typeof req.actionId !== "string" || !req.actionId.trim()) return "actionId required";
  if (typeof req.runId !== "string" || !req.runId.trim()) return "runId required";
  if (typeof req.targetId !== "string" || !req.targetId.trim()) return "targetId required";
  for (const idValue of [req.actionId, req.runId, req.targetId, req.snapshotId]) {
    if (typeof idValue !== "string" || !idValue.trim() || idValue.length > 256) return "invalid identity length";
  }
  if (typeof req.targetGeneration !== "number" || req.targetGeneration < 1) return "targetGeneration must be >= 1";
  if (!ACTION_PARAMS[req.action]) return "unknown action";
  const target = req.target;
  if (!target || typeof target !== "object") return "target required";
  const tKeys = Object.keys(target);
  let isCoord = false;
  if ("elementRef" in target) {
    if (tKeys.some((k) => k !== "elementRef")) return "unknown field";
    if (typeof target.elementRef !== "string" || !target.elementRef.trim() || target.elementRef.length > 256) return "invalid elementRef";
  } else if ("x" in target && "y" in target) {
    isCoord = true;
    if (tKeys.some((k) => k !== "x" && k !== "y")) return "unknown field";
    if (typeof target.x !== "number" || typeof target.y !== "number" || !Number.isFinite(target.x) || !Number.isFinite(target.y) || target.x < 0 || target.y < 0) {
      return "coordinates must be nonnegative finite numbers";
    }
  } else return "target required";
  const allowed = ACTION_PARAMS[req.action];
  if (Object.keys(req.parameters).some((key) => !allowed.includes(key))) return "unknown action parameter";
  if (req.action === "click") {
    const button = req.parameters.button;
    if (button !== undefined && button !== "left" && button !== "right" && button !== "middle") return "button must be left, right, or middle";
    const count = req.parameters.count;
    if (count !== undefined && count !== 1 && count !== 2) return "count must be 1 or 2";
  }
  if (req.action === "set_value" || req.action === "type_text") {
    const value = req.parameters.text;
    if (typeof value !== "string") return req.action === "type_text" ? "type_text requires parameters.text" : "text required";
    if ([...value].length > 4000 || value.includes("\0")) return req.action === "type_text" ? "type_text too long" : "invalid text length or NUL";
    if (req.action === "type_text" && req.parameters.via !== undefined && req.parameters.via !== "clipboard") return "via must be clipboard when set";
  }
  if (req.action === "set_value" && isCoord) return "set_value requires elementRef";
  if (req.action === "key") {
    const key = req.parameters.key;
    if (typeof key !== "string" || !key.trim()) return "key requires parameters.key";
    if (key.length > 32) return "invalid key";
    if (!ALLOWED_KEYS.has(key.trim().toLowerCase())) return "key is not in the allowed set";
  }
  if (req.action === "scroll") {
    const delta = req.parameters.delta;
    if (typeof delta !== "number" || !Number.isInteger(delta)) return "integer delta required";
    if (delta < -2400 || delta > 2400) return "scroll delta out of range";
  }
  if (req.action === "drag") {
    const canonical = Object.hasOwn(req.parameters, "toX") || Object.hasOwn(req.parameters, "toY");
    const legacy = Object.hasOwn(req.parameters, "x1") || Object.hasOwn(req.parameters, "y1");
    if (canonical && legacy) return "drag destination aliases cannot be mixed";
    const [x, y] = canonical ? [req.parameters.toX, req.parameters.toY] : [req.parameters.x1, req.parameters.y1];
    if (typeof x !== "number" || typeof y !== "number") return "drag requires a complete numeric destination pair";
    if (!Number.isFinite(x) || !Number.isFinite(y) || x < 0 || y < 0) return "invalid drag destination";
  }
  if (req.action === "wait") {
    if (isCoord) return "wait requires elementRef";
    const name = req.parameters.nameEquals;
    if (typeof name !== "string" || !name.trim() || name.length > 256) return "invalid nameEquals";
    if (req.parameters.timeoutMs !== undefined) {
      const ms = req.parameters.timeoutMs;
      if (!Number.isInteger(ms) || ms < 1 || ms > 10000) return "timeoutMs must be between 1 and 10000";
    }
  }
  return null;
}

export function validateNavigateUrl(url) {
  const trimmed = String(url ?? "").trim();
  if (!trimmed || trimmed.length > 2048) return "navigate url is empty or too long";
  if (/[\0\n\r]/.test(trimmed)) return "navigate url contains control characters";
  const lower = trimmed.toLowerCase();
  if (lower === "about:blank") return null;
  const scheme = lower.split(":")[0] || "";
  if (["javascript", "data", "file", "vbscript", "blob"].includes(scheme)) return `${scheme}: navigation is not allowed`;
  if (scheme !== "http" && scheme !== "https") return "navigate url must be http(s)";
  const rest = trimmed.slice(scheme.length + 1);
  if (!rest.startsWith("//")) return "navigate url must be http(s) with a host";
  const authority = rest.slice(2).split("/")[0] || "";
  if (!authority || authority.includes("@")) return "navigate url must not include credentials";
  let host = authority;
  if (host.startsWith("[")) host = host.slice(1).split("]")[0] || host;
  else {
    const colon = host.lastIndexOf(":");
    if (colon > 0 && /^\d+$/.test(host.slice(colon + 1))) host = host.slice(0, colon);
  }
  host = host.trim().replace(/\.+$/, "");
  if (host === "169.254.169.254" || host === "100.100.100.200" || host === "metadata.google.internal" || host.endsWith(".metadata.google.internal")) {
    return "navigate url must not target instance metadata";
  }
  return null;
}
