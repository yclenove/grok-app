/**
 * Computer Use tool-call facts and the session shot ledger.
 * Screenshots are admitted only from the redacted pipeline.
 */

import { resolveImageSrcSync } from "@/lib/imageSrc";
import { isRealLocalAbsolutePath } from "@/lib/pathNormalize";

const SERVERS = ["grok-computer-use", "computer-use", "computer_use"] as const;

export const COMPUTER_USE_TOOLS = [
  "list_apps",
  "list_windows",
  "launch_app",
  "get_app_state",
  "click",
  "perform_secondary_action",
  "set_value",
  "select_text",
  "type_text",
  "press_key",
  "scroll",
  "drag",
  "wait_for",
  "computer_batch",
  "computer_script",
  "capabilities",
] as const;

export type ComputerUseToolName = (typeof COMPUTER_USE_TOOLS)[number];
export type Outcome = "done" | "refused" | "failed" | "unknown";
export type Effect = "changed" | "no_visible_change" | "not_checked";
export type Delivery =
  | "accessibility"
  | "targeted_event"
  | "foreground"
  | "clipboard"
  | "none";
export type ShotFault = "foreign" | "missing" | "rejected";

const TOOLS = new Set<string>(COMPUTER_USE_TOOLS);
const OUTCOMES = new Set<string>(["done", "refused", "failed", "unknown"]);
const EFFECTS = new Set<string>(["changed", "no_visible_change", "not_checked"]);
const DELIVERIES = new Set<string>([
  "accessibility",
  "targeted_event",
  "foreground",
  "clipboard",
  "none",
]);

export type ToolCallInput = {
  title?: string;
  toolKind?: string;
  toolCallId?: string;
  status?: string;
  detail?: string;
  input?: string;
  output?: string;
  streaming?: boolean;
  isError?: boolean;
};

export type ComputerUseCall = {
  tool: ComputerUseToolName | "unknown";
  app: string;
  element: string;
  outcome: Outcome | null;
  effect: Effect | null;
  delivery: Delivery | null;
  durationMs: number | null;
  screenshotId: string | null;
  raw: string;
  pending: boolean;
};

type Json = Record<string, unknown>;

type Parsed = {
  complete: boolean;
  tool: ComputerUseToolName | null;
  outcome: Outcome | null;
  effect: Effect | null;
  delivery: Delivery | null;
  app: string;
  element: string;
  durationMs: number | null;
  screenshotId: string | null;
};

const EMPTY_PARSED: Parsed = {
  complete: false,
  tool: null,
  outcome: null,
  effect: null,
  delivery: null,
  app: "",
  element: "",
  durationMs: null,
  screenshotId: null,
};

export function isComputerUseTool(tool: ToolCallInput): boolean {
  return readComputerUseCall(tool) != null;
}

/** Facts for one computer-use tool row. Null when the call is some other tool. */
export function readComputerUseCall(tool: ToolCallInput): ComputerUseCall | null {
  const marker = hasServerMarker(tool);
  const parsed = mergeParsed(
    parseText(tool.output),
    mergeParsed(parseText(tool.detail), parseText(tool.input)),
  );
  if (!marker && !parsed.complete) return null;
  const status = fromStatus(tool);
  const outcome = parsed.outcome ?? (status.pending ? null : status.outcome ?? "unknown");
  const pending = outcome == null;
  const effect = outcome == null ? null : parsed.effect ?? "not_checked";
  return {
    tool: parsed.tool ?? toolFromMarker(tool) ?? "unknown",
    app: sanitizeLabel(parsed.app),
    element: sanitizeLabel(parsed.element),
    outcome,
    effect,
    delivery: parsed.delivery,
    durationMs: parsed.durationMs,
    screenshotId: safeKey(parsed.screenshotId ?? "") ? parsed.screenshotId : null,
    raw: sanitizeRaw(tool.output || tool.detail || ""),
    pending,
  };
}

export type AdmittedShot = {
  sessionId: string;
  id: string;
  thumbPath?: string | null;
  fullPath?: string | null;
  source: "redacted";
};

export type ShotPaths = { thumb: string | null; full: string | null };

export type ShotView =
  | { kind: "empty" }
  | { kind: "ready"; thumb: string | null; full: string | null }
  | { kind: "foreign" }
  | { kind: "missing" }
  | { kind: "rejected" };

const EMPTY_VIEW: ShotView = { kind: "empty" };
const FOREIGN_VIEW: ShotView = { kind: "foreign" };
const MISSING_VIEW: ShotView = { kind: "missing" };
const REJECTED_VIEW: ShotView = { kind: "rejected" };

type Row = { sessionId: string; thumb: string | null; full: string | null };

/** Mirrors host ownership so one chat cannot paint another chat's thumbnail. */
export class ShotBook {
  private rows = new Map<string, Row>();
  private faults = new Map<string, ShotFault>();
  private listeners = new Set<() => void>();
  private generation = 0;
  private cache = new Map<string, ShotView>();

  subscribe = (listener: () => void): (() => void) => {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  };

  viewFor(sessionId: string | null | undefined, id: string | null): ShotView {
    const key = `${this.generation}|${sessionId ?? ""}|${id ?? ""}`;
    const hit = this.cache.get(key);
    if (hit) return hit;
    const view = this.makeView(sessionId ?? null, id);
    this.cache.set(key, view);
    return view;
  }

  admit(shot: AdmittedShot): ShotFault | null {
    if (shot.source !== "redacted") return "rejected";
    if (!safeKey(shot.sessionId) || !safeKey(shot.id)) return "rejected";
    const thumb = acceptPath(shot.thumbPath);
    const full = acceptPath(shot.fullPath);
    if (!thumb && !full) return "rejected";
    const existing = this.rows.get(shot.id);
    if (existing && existing.sessionId !== shot.sessionId) return "foreign";
    this.rows.set(shot.id, {
      sessionId: shot.sessionId,
      thumb: thumb ?? existing?.thumb ?? null,
      full: full ?? existing?.full ?? null,
    });
    this.faults.delete(faultKey(shot.sessionId, shot.id));
    this.emit();
    return null;
  }

  resolve(sessionId: string, id: string): ShotPaths | ShotFault {
    if (!safeKey(sessionId) || !safeKey(id)) return "rejected";
    const row = this.rows.get(id);
    if (!row) return this.faults.get(faultKey(sessionId, id)) ?? "missing";
    if (row.sessionId !== sessionId) return "foreign";
    return { thumb: row.thumb, full: row.full };
  }

  noteFault(sessionId: string, id: string, fault: ShotFault): void {
    if (!safeKey(sessionId) || !safeKey(id)) return;
    const row = this.rows.get(id);
    if (row && row.sessionId !== sessionId) {
      this.emit();
      return;
    }
    if (row && fault !== "foreign") return;
    this.faults.set(faultKey(sessionId, id), fault);
    this.emit();
  }

  dropSession(sessionId: string): void {
    for (const [id, row] of this.rows) {
      if (row.sessionId === sessionId) this.rows.delete(id);
    }
    const prefix = `${sessionId}\0`;
    for (const key of this.faults.keys()) {
      if (key.startsWith(prefix)) this.faults.delete(key);
    }
    this.emit();
  }

  private makeView(sessionId: string | null, id: string | null): ShotView {
    if (!sessionId || !id) return EMPTY_VIEW;
    const resolved = this.resolve(sessionId, id);
    if (resolved === "foreign") return FOREIGN_VIEW;
    if (resolved === "rejected") return REJECTED_VIEW;
    if (resolved === "missing") {
      return this.faults.has(faultKey(sessionId, id)) ? MISSING_VIEW : EMPTY_VIEW;
    }
    return { kind: "ready", thumb: resolved.thumb, full: resolved.full };
  }

  private emit(): void {
    this.generation += 1;
    this.cache.clear();
    for (const listener of this.listeners) listener();
  }
}

export const sharedShotBook = new ShotBook();

/** Loopback media URL only. `media://`, `data:`, and raw paths are not previews. */
export function previewUrl(path: string | null): string | null {
  if (!path || !acceptPath(path)) return null;
  const url = resolveImageSrcSync(path);
  if (!url || !isLoopbackMedia(url)) return null;
  return url;
}

export function sanitizeLabel(input: string): string {
  let out = "";
  for (const ch of input) {
    if (out.length >= 280) break;
    if (ch === "\n" || ch === "\r" || ch === "\t") {
      out += " ";
      continue;
    }
    out += hidden(ch.codePointAt(0) ?? 0) ? "\uFFFD" : ch;
  }
  return out;
}

export function sanitizeRaw(input: string): string {
  let out = "";
  let count = 0;
  for (const ch of input) {
    if (count >= 8000) break;
    count += 1;
    const code = ch.codePointAt(0) ?? 0;
    if (ch === "\n" || ch === "\t") {
      out += ch;
      continue;
    }
    if (ch === "\r") {
      out += "\n";
      continue;
    }
    out += hidden(code) ? "\uFFFD" : ch;
  }
  return out;
}

function hasServerMarker(tool: ToolCallInput): boolean {
  const blob = [tool.title, tool.toolKind, tool.toolCallId].filter(Boolean).join("\n").toLowerCase();
  return SERVERS.some((name) => blob.includes(name));
}

function toolFromMarker(tool: ToolCallInput): ComputerUseToolName | null {
  const blob = `${tool.title ?? ""} ${tool.toolKind ?? ""}`.toLowerCase();
  for (const server of SERVERS) {
    let from = 0;
    while (from < blob.length) {
      const at = blob.indexOf(server, from);
      if (at < 0) break;
      const rest = blob.slice(at + server.length).replace(/^[^a-z]+/, "");
      const name = /^[a-z_]+/.exec(rest)?.[0] ?? "";
      if (isToolName(name)) return name;
      from = at + server.length;
    }
  }
  return null;
}

function fromStatus(tool: ToolCallInput): { pending: boolean; outcome: Outcome | null } {
  if (tool.streaming) return { pending: true, outcome: null };
  const status = (tool.status ?? "").toLowerCase().trim();
  if (
    status === "" ||
    status === "pending" ||
    status === "in_progress" ||
    status === "running"
  ) {
    return { pending: true, outcome: null };
  }
  if (status === "failed" || status === "error" || tool.isError) {
    return { pending: false, outcome: "failed" };
  }
  if (status === "rejected" || status === "denied") {
    return { pending: false, outcome: "refused" };
  }
  return { pending: false, outcome: null };
}

function parseText(text: string | undefined): Parsed | null {
  if (!text) return null;
  let best: Parsed | null = null;
  for (const value of jsonObjects(text)) {
    const parsed = fromJson(value);
    if (!parsed) continue;
    best = best ? mergeParsed(best, parsed) : parsed;
  }
  const line = actionLine(text);
  if (!line) return best;
  return best ? mergeParsed(best, line) : line;
}

function fromJson(value: unknown): Parsed | null {
  const root = asRecord(value);
  if (!root) return null;
  const action = asRecord(root.action);
  const state = asRecord(root.state);
  if (!action && !state && typeof root.app !== "string") return null;
  const target = elementOf(action?.target);
  const toolName = action?.tool;
  const outcomeName = action?.outcome;
  const effectName = action?.effect;
  const deliveryName = action?.delivery;
  const tool = isToolName(toolName) ? toolName : null;
  const outcome = isOutcome(outcomeName) ? outcomeName : null;
  const effect = isEffect(effectName) ? effectName : null;
  const delivery = isDelivery(deliveryName) ? deliveryName : null;
  const duration = action?.duration_ms;
  return {
    complete: tool != null && outcome != null && effect != null && delivery != null,
    tool,
    outcome,
    effect,
    delivery,
    app: textOf(state?.app_id) || textOf(root.app),
    element: target.element,
    durationMs:
      typeof duration === "number" && Number.isFinite(duration) && duration >= 0
        ? Math.floor(duration)
        : null,
    screenshotId: safeShot(shotId(state) ?? target.screenshotId),
  };
}

function actionLine(text: string): Parsed | null {
  const match = /Action:\s*([a-z_]+)\s+[—–-]\s*(done|refused|failed|unknown)\s+\(([a-z_]+)\)/.exec(
    text,
  );
  if (!match) return null;
  const tool = isToolName(match[1]) ? match[1] : null;
  const outcome = isOutcome(match[2]) ? match[2] : null;
  const effect = isEffect(match[3]) ? match[3] : null;
  if (!tool && !outcome) return null;
  return { ...EMPTY_PARSED, tool, outcome, effect };
}

function mergeParsed(primary: Parsed | null, extra: Parsed | null): Parsed {
  if (!primary) return extra ?? EMPTY_PARSED;
  if (!extra) return primary;
  return {
    complete: primary.complete || extra.complete,
    tool: primary.tool ?? extra.tool,
    outcome: primary.outcome ?? extra.outcome,
    effect: primary.effect ?? extra.effect,
    delivery: primary.delivery ?? extra.delivery,
    app: primary.app || extra.app,
    element: primary.element || extra.element,
    durationMs: primary.durationMs ?? extra.durationMs,
    screenshotId: primary.screenshotId ?? extra.screenshotId,
  };
}

function elementOf(target: unknown): { element: string; screenshotId: string | null } {
  const record = asRecord(target);
  if (!record) return { element: "", screenshotId: null };
  if (typeof record.label === "string") return { element: record.label, screenshotId: null };
  if (typeof record.x === "number" && typeof record.y === "number") {
    return {
      element: `(${formatNum(record.x)}, ${formatNum(record.y)})`,
      screenshotId: textOf(record.screenshot_id) || null,
    };
  }
  return { element: "", screenshotId: null };
}

function shotId(state: Json | null): string | null {
  if (!state || state.screenshot == null) return null;
  const shot = asRecord(state.screenshot);
  return textOf(shot?.id) || null;
}

function jsonObjects(text: string): unknown[] {
  const found: unknown[] = [];
  for (let i = 0; i < text.length; i += 1) {
    if (text[i] !== "{") continue;
    const end = balancedEnd(text, i);
    if (end < 0) continue;
    try {
      found.push(JSON.parse(text.slice(i, end + 1)));
    } catch {
      /* keep scanning */
    }
    i = end;
  }
  return found;
}

function balancedEnd(text: string, start: number): number {
  let depth = 0;
  let quote = false;
  let escape = false;
  for (let i = start; i < text.length; i += 1) {
    const ch = text[i];
    if (quote) {
      if (escape) {
        escape = false;
        continue;
      }
      if (ch === "\\") {
        escape = true;
        continue;
      }
      if (ch === '"') quote = false;
      continue;
    }
    if (ch === '"') {
      quote = true;
      continue;
    }
    if (ch === "{") depth += 1;
    else if (ch === "}") {
      depth -= 1;
      if (depth === 0) return i;
    }
  }
  return -1;
}

function asRecord(value: unknown): Json | null {
  if (!value || typeof value !== "object" || Array.isArray(value)) return null;
  return value as Json;
}

function textOf(value: unknown): string {
  return typeof value === "string" ? value : "";
}

function formatNum(value: number): string {
  return Number.isInteger(value) ? String(value) : String(value);
}

function isToolName(value: unknown): value is ComputerUseToolName {
  return typeof value === "string" && TOOLS.has(value);
}

function isOutcome(value: unknown): value is Outcome {
  return typeof value === "string" && OUTCOMES.has(value);
}

function isEffect(value: unknown): value is Effect {
  return typeof value === "string" && EFFECTS.has(value);
}

function isDelivery(value: unknown): value is Delivery {
  return typeof value === "string" && DELIVERIES.has(value);
}

function safeShot(id: string | null): string | null {
  return id && safeKey(id) ? id : null;
}

function safeKey(value: string): boolean {
  return value.length >= 1 && value.length <= 128 && /^[A-Za-z0-9][A-Za-z0-9._-]*$/.test(value);
}

function acceptPath(path: string | null | undefined): string | null {
  if (!path) return null;
  const trimmed = path.trim();
  if (!trimmed || trimmed.includes("..")) return null;
  if (/^[a-z][a-z0-9+.-]*:/i.test(trimmed)) return null;
  if (!isRealLocalAbsolutePath(trimmed)) return null;
  return trimmed;
}

function hidden(code: number): boolean {
  return (
    code < 0x20 ||
    (code >= 0x7f && code <= 0x9f) ||
    code === 0x61c ||
    code === 0x200e ||
    code === 0x200f ||
    (code >= 0x202a && code <= 0x202e) ||
    (code >= 0x2066 && code <= 0x2069)
  );
}

function faultKey(sessionId: string, id: string): string {
  return `${sessionId}\0${id}`;
}

function isLoopbackMedia(url: string): boolean {
  try {
    const parsed = new URL(url);
    if (parsed.protocol !== "http:") return false;
    const host = parsed.hostname;
    if (host !== "127.0.0.1" && host !== "localhost" && host !== "::1") return false;
    return parsed.pathname === "/v1/media" || parsed.pathname.endsWith("/v1/media");
  } catch {
    return false;
  }
}
