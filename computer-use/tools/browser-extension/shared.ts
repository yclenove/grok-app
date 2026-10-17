/** Private L2 transport DTOs, not a replacement for gcu-protocol or ActionRouter. */
export type Role = "document" | "web_area" | "group" | "unknown" | "button" | "link" | "check_box" |
  "radio" | "switch" | "text_field" | "search_field" | "text_area" | "combo_box" | "slider" |
  "progress_bar" | "list" | "list_item" | "table" | "row" | "cell" | "column" | "tree" |
  "tree_item" | "tab" | "tab_list" | "scroll_area" | "scroll_bar" | "image" | "static_text" |
  "heading" | "dialog" | "menu" | "menu_item" | "menu_bar" | "toolbar" | "stepper";
/** The core Rect shape; see DomSnapshot.frameSpace before interpreting coordinates. */
export interface Rect { x: number; y: number; w: number; h: number }
/** NodeValue uses the existing core externally tagged representation. */
export type NodeValue = { text: string } | { bool: boolean } | { number: number };
/** Driver-side node. IDs are opaque, single-snapshot handles, never selectors. */
export interface DomNode {
  id: string; role: Role; native_role: string; name: string | null; value: NodeValue | null;
  states: string[]; frame: Rect | null; children: DomNode[];
}
/** No screenshots are captured here. P8-03 must map viewport frames to window pixels. */
export interface DomSnapshot {
  id: string; root: DomNode;
  frameSpace: "tab_viewport_physical";
  geometry: { width: number; height: number; scale: number };
  incomplete: { reason: "node_cap" | "depth_cap" | "deadline" | "partial_error"; read_nodes: number; unread_nodes: null } | null;
  handoff: boolean;
}
/** Only these four semantic actions are available; there is no script/key/selector API. */
export type DomOperation = { type: "observe" } | ({ snapshotId: string; nodeId: string } & (
  { type: "click" } | { type: "set_value"; value: string } |
  { type: "scroll"; dx: number; dy: number } | { type: "select"; value: string }
));
/** P8-02 supplies this only after the complete Router authorization/Stop preflight. */
export interface DomRequest { requestId: string; shareId: string; sessionId: string; operation: DomOperation }
export type ErrorCode = "app_denied" | "approval_denied" | "capability_unavailable" | "stale_element" |
  "handoff_required" | "busy" | "not_settable" | "element_not_actionable" | "driver_error";
/** Unknown means delivery may have started and must never be automatically replayed. */
export type DomResult = { kind: "snapshot"; snapshot: DomSnapshot } |
  { kind: "action"; outcome: "done" | "unknown" } | { kind: "error"; code: ErrorCode };
export type WireMessage = { kind: "activate"; shareId: string; url: string } |
  { kind: "request"; request: DomRequest } | ResponseMessage | { kind: "revoked" };
export interface ResponseMessage { kind: "response"; requestId: string; result: DomResult }
export interface Listener<T extends (...args: never[]) => unknown> {
  addListener(listener: T): void; removeListener(listener: T): void;
}
/** Narrow subset of Chrome APIs; no cookie, storage, network or page-message bridge. */
export interface Sender { id?: string; tab?: { id?: number } }
export interface Port {
  name: string; sender?: Sender;
  onMessage: Listener<(message: unknown) => void>; onDisconnect: Listener<() => void>;
  postMessage(message: WireMessage): void; disconnect(): void;
}
export interface ContentRuntime { id: string; onConnect: Listener<(port: Port) => void> }

/** Reject unknown fields and bound all untrusted wire strings before dispatch. */
export function record(value: unknown, keys: readonly string[]): value is Record<string, unknown> {
  return !!value && typeof value === "object" && !Array.isArray(value) &&
    Object.keys(value).length === keys.length && keys.every(key => Object.hasOwn(value, key));
}
export function identifier(value: unknown): value is string {
  return typeof value === "string" && /^[a-zA-Z0-9_-]{1,128}$/.test(value);
}
/** Runtime validation is required even though the ingress itself is authenticated. */
export function validRequest(value: unknown): value is DomRequest {
  if (!record(value, ["requestId", "shareId", "sessionId", "operation"]) ||
    !identifier(value.requestId) || !identifier(value.shareId) || !identifier(value.sessionId)) return false;
  if (!value.operation || typeof value.operation !== "object" || Array.isArray(value.operation)) return false;
  const op = value.operation as Record<string, unknown>;
  if (op.type === "observe") return record(op, ["type"]);
  const base = ["type", "snapshotId", "nodeId"];
  const keys = op.type === "click" ? base : op.type === "scroll" ? [...base, "dx", "dy"] : [...base, "value"];
  if (!record(op, keys) || !identifier(op.snapshotId) || !identifier(op.nodeId)) return false;
  if (op.type === "set_value" || op.type === "select") return typeof op.value === "string" && op.value.length <= 8192 && !op.value.includes("\0");
  return op.type === "click" || (op.type === "scroll" && typeof op.dx === "number" && typeof op.dy === "number" &&
    Number.isFinite(op.dx) && Number.isFinite(op.dy) && Math.abs(op.dx) <= 2400 && Math.abs(op.dy) <= 2400);
}
/** Errors contain codes only; DOM values and page exceptions never enter diagnostics. */
export const error = (code: ErrorCode): DomResult => ({ kind: "error", code });
