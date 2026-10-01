/** Client-side Computer Use protocol helpers. Host broker is authoritative. */

export const COMPUTER_USE_PROTOCOL_VERSION = 1 as const;
export const COMPUTER_USE_ID_MAX_LEN = 256;
export const COMPUTER_USE_TEXT_MAX_CHARS = 4000;

export const COMPUTER_USE_MODEL_TOOLS = [
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
] as const;

export const COMPUTER_USE_HOST_ONLY_TOOLS = [
  "computer_authorize",
  "computer_resume",
  "computer_reconnect",
  "computer_pause",
] as const;

export const COMPUTER_USE_ERROR_CODES = [
  "feature_disabled",
  "run_not_found",
  "target_unauthorized",
  "identity_mismatch",
  "dead_target",
  "stop_requested",
  "lease_held",
  "duplicate_action",
  "schema",
  "adapter",
  "timeout",
] as const;

const REQUEST_KEYS = new Set([
  "version",
  "actionId",
  "runId",
  "targetId",
  "targetGeneration",
  "snapshotId",
  "geometryRevision",
  "action",
  "target",
  "parameters",
]);

const ACTION_PARAMS: Record<ActionKind, readonly string[]> = {
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

export type ActionKind =
  | "click"
  | "set_value"
  | "type_text"
  | "key"
  | "scroll"
  | "drag"
  | "wait";

export type ActionTarget =
  | { elementRef: string }
  | { x: number; y: number };

export type ActionRequest = {
  version: number;
  actionId: string;
  runId: string;
  targetId: string;
  targetGeneration: number;
  snapshotId: string;
  geometryRevision: number;
  action: ActionKind;
  target: ActionTarget;
  parameters: Record<string, unknown>;
};

export type OutcomeKind = "applied" | "verified" | "rejected" | "unknown";

export type ActionOutcome = {
  actionId: string;
  runId: string;
  kind: OutcomeKind;
  executed: boolean;
  reason?: string | null;
  generation: number;
};

export function isCoordTarget(
  target: ActionTarget,
): target is { x: number; y: number } {
  return "x" in target && "y" in target;
}

function permissionGrantKey(params: Record<string, unknown>): string | null {
  for (const key of Object.keys(params)) {
    const normalized = key.toLowerCase().replace(/[^a-z0-9]/g, "");
    if (
      normalized === "yolo" ||
      normalized === "acceptedits" ||
      normalized === "alwaysapprove" ||
      normalized === "alwaysapproveedits"
    ) {
      return key;
    }
  }
  return null;
}

function identityLen(id: unknown): string | null {
  if (typeof id !== "string" || !id.trim() || id.length > COMPUTER_USE_ID_MAX_LEN) {
    return "invalid identity length";
  }
  return null;
}

export function validateActionRequest(
  req: ActionRequest | Record<string, unknown>,
): string | null {
  const raw = req as Record<string, unknown>;
  for (const key of Object.keys(raw)) {
    if (!REQUEST_KEYS.has(key)) return `unknown field ${key}`;
  }
  const version = raw.version;
  if (version !== COMPUTER_USE_PROTOCOL_VERSION) {
    return `unsupported protocol version ${String(version)}`;
  }
  const parameters =
    raw.parameters && typeof raw.parameters === "object" && !Array.isArray(raw.parameters)
      ? (raw.parameters as Record<string, unknown>)
      : null;
  if (!parameters) return "parameters must be an object";
  const grant = permissionGrantKey(parameters);
  if (grant) return `${grant} never grants desktop control`;
  if (typeof raw.actionId !== "string" || !raw.actionId.trim()) return "actionId required";
  if (typeof raw.runId !== "string" || !raw.runId.trim()) return "runId required";
  if (typeof raw.targetId !== "string" || !raw.targetId.trim()) return "targetId required";
  for (const id of [raw.actionId, raw.runId, raw.targetId, raw.snapshotId]) {
    const err = identityLen(id);
    if (err) return err;
  }
  if (typeof raw.targetGeneration !== "number" || raw.targetGeneration < 1) {
    return "targetGeneration must be >= 1";
  }
  const action = raw.action;
  if (
    action !== "click" &&
    action !== "set_value" &&
    action !== "type_text" &&
    action !== "key" &&
    action !== "scroll" &&
    action !== "drag" &&
    action !== "wait"
  ) {
    return "unknown action";
  }
  const target = raw.target;
  if (!target || typeof target !== "object" || Array.isArray(target)) {
    return "target required";
  }
  const t = target as Record<string, unknown>;
  const tKeys = Object.keys(t);
  let parsed: ActionTarget;
  if ("elementRef" in t) {
    if (tKeys.some((k) => k !== "elementRef")) return "unknown field";
    if (typeof t.elementRef !== "string" || !t.elementRef.trim() || t.elementRef.length > COMPUTER_USE_ID_MAX_LEN) {
      return "invalid elementRef";
    }
    parsed = { elementRef: t.elementRef };
  } else if ("x" in t && "y" in t) {
    if (tKeys.some((k) => k !== "x" && k !== "y")) return "unknown field";
    if (typeof t.x !== "number" || typeof t.y !== "number" || !Number.isFinite(t.x) || !Number.isFinite(t.y) || t.x < 0 || t.y < 0) {
      return "coordinates must be nonnegative finite numbers";
    }
    parsed = { x: t.x, y: t.y };
  } else {
    return "target required";
  }
  const allowed = ACTION_PARAMS[action];
  if (Object.keys(parameters).some((key) => !allowed.includes(key))) {
    return "unknown action parameter";
  }
  if (action === "click") {
    const button = parameters.button;
    if (button !== undefined && button !== "left" && button !== "right" && button !== "middle") {
      return "button must be left, right, or middle";
    }
    const count = parameters.count;
    if (count !== undefined && count !== 1 && count !== 2) {
      return "count must be 1 or 2";
    }
  }
  if (action === "set_value" || action === "type_text") {
    const text = parameters.text;
    if (typeof text !== "string") {
      return action === "type_text" ? "type_text requires parameters.text" : "text required";
    }
    if ([...text].length > COMPUTER_USE_TEXT_MAX_CHARS || text.includes("\0")) {
      return action === "type_text" ? "type_text too long" : "invalid text length or NUL";
    }
    if (action === "type_text" && parameters.via !== undefined && parameters.via !== "clipboard") {
      return "via must be clipboard when set";
    }
  }
  if (action === "set_value" && isCoordTarget(parsed)) {
    return "set_value requires elementRef";
  }
  if (action === "key") {
    const key = parameters.key;
    if (typeof key !== "string" || !key.trim()) return "key requires parameters.key";
    if (key.length > 32) return "invalid key";
    if (!ALLOWED_KEYS.has(key.trim().toLowerCase())) return "key is not in the allowed set";
  }
  if (action === "scroll") {
    const delta = parameters.delta;
    if (typeof delta !== "number" || !Number.isInteger(delta)) return "integer delta required";
    if (delta < -2400 || delta > 2400) return "scroll delta out of range";
  }
  if (action === "drag") {
    const dest = ["toX", "toY", "x1", "y1"].some((key) => typeof parameters[key] === "number");
    if (!dest && isCoordTarget(parsed)) {
      return "drag requires destination coordinates or a drop element";
    }
    for (const key of ["toX", "toY", "x1", "y1"]) {
      const n = parameters[key];
      if (n !== undefined && (typeof n !== "number" || !Number.isFinite(n) || n < 0)) {
        return "invalid drag destination";
      }
    }
  }
  if (action === "wait") {
    if (isCoordTarget(parsed)) return "wait requires elementRef";
    const name = parameters.nameEquals;
    if (typeof name !== "string" || !name.trim()) return "nameEquals required";
    if (name.length > COMPUTER_USE_ID_MAX_LEN) return "invalid nameEquals";
    const timeout = parameters.timeoutMs;
    if (timeout !== undefined) {
      if (
        typeof timeout !== "number" ||
        !Number.isInteger(timeout) ||
        timeout < 1 ||
        timeout > 10000
      ) {
        return "timeoutMs must be between 1 and 10000";
      }
    }
  }
  return null;
}

export function validateNavigateUrl(url: string): string | null {
  const trimmed = url.trim();
  if (!trimmed || trimmed.length > 2048) return "navigate url is empty or too long";
  if (/[\0\n\r]/.test(trimmed)) return "navigate url contains control characters";
  const lower = trimmed.toLowerCase();
  if (lower === "about:blank") return null;
  const split = lower.split(":");
  const scheme = split[0] ?? "";
  if (scheme === "javascript" || scheme === "data" || scheme === "file" || scheme === "vbscript" || scheme === "blob") {
    return `${scheme}: navigation is not allowed`;
  }
  if (scheme !== "http" && scheme !== "https") return "navigate url must be http(s)";
  const rest = trimmed.slice(scheme.length + 1);
  if (!rest.startsWith("//")) return "navigate url must be http(s) with a host";
  const authority = rest.slice(2).split("/")[0] ?? "";
  if (!authority || authority.includes("@")) return "navigate url must not include credentials";
  let host = authority;
  if (host.startsWith("[")) host = host.slice(1).split("]")[0] ?? host;
  else {
    const colon = host.lastIndexOf(":");
    if (colon > 0 && /^\d+$/.test(host.slice(colon + 1))) host = host.slice(0, colon);
  }
  host = host.trim().replace(/\.+$/, "");
  if (
    host === "169.254.169.254" ||
    host === "100.100.100.200" ||
    host === "metadata.google.internal" ||
    host.endsWith(".metadata.google.internal")
  ) {
    return "navigate url must not target instance metadata";
  }
  return null;
}

export function staleSnapshotCannotClick(opts: {
  previewHidden: boolean;
  snapshotId: string | null;
  clickSnapshotId: string;
}): boolean {
  if (!opts.snapshotId) return true;
  if (opts.previewHidden) return opts.clickSnapshotId !== opts.snapshotId;
  return opts.clickSnapshotId !== opts.snapshotId;
}

/** Coordinate actions are blocked when the last observation had no image. */
export function coordActionBlockedWithoutImage(opts: {
  target: ActionTarget;
  hasImage: boolean;
}): boolean {
  return isCoordTarget(opts.target) && !opts.hasImage;
}
