import { hasHost, invoke } from "./host";
export type ReceiptDecision = "dismiss" | "retry";
export type ReceiptStatus = "turn_started" | "queued" | "cancelled" | "busy" | "not_found" | "app_not_running" | "unknown" | "idempotency_conflict" | "error";
export type ReceiptIdentity = { sessionId: string; key: string; revision: string };
export type ReceiptReviewRequest = ReceiptIdentity & { prompt: string; decision: ReceiptDecision; reviewed: boolean; confirmed: boolean };
export type ReceiptReviewAck = ReceiptIdentity & { version: 1; decision: ReceiptDecision; replacementKey: string | null; replacementStatus: ReceiptStatus | null; queueItemId: string | null };
export type ReceiptRow = ReceiptIdentity & { canReview: boolean; review: ReceiptReviewAck | null };
export type ReceiptSnapshot = { version: 1; items: ReceiptRow[] };
const statuses = new Set(["turn_started", "queued", "cancelled", "busy", "not_found", "app_not_running", "unknown", "idempotency_conflict", "error"]);
function bad(): never { throw new Error("SESSION_RECEIPT_UNAVAILABLE"); }
function record(v: unknown): Record<string, unknown> { if (!v || typeof v !== "object" || Array.isArray(v)) return bad(); return v as Record<string, unknown>; }
function exact(v: Record<string, unknown>, keys: string) { if (Object.keys(v).sort().join(",") !== keys) bad(); }
function text(v: unknown): v is string { return typeof v === "string" && !!v.trim() && v.trim() === v; }
function identity(v: Record<string, unknown>) { if (!text(v.sessionId) || !text(v.key) || typeof v.revision !== "string" || !/^[a-f0-9]{64}$/.test(v.revision)) bad(); }
function same(a: ReceiptIdentity, b: ReceiptIdentity) { return a.sessionId === b.sessionId && a.key === b.key && a.revision === b.revision; }
export function parseReceiptAck(v: unknown): ReceiptReviewAck {
  const a = record(v); exact(a, "decision,key,queueItemId,replacementKey,replacementStatus,revision,sessionId,version"); identity(a);
  if (a.version !== 1 || (a.decision !== "dismiss" && a.decision !== "retry")) bad();
  if (a.decision === "dismiss") { if (a.replacementKey !== null || a.replacementStatus !== null || a.queueItemId !== null) bad(); }
  else {
    if (!text(a.replacementKey) || !/^r-ext-[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12}$/.test(a.replacementKey) || a.replacementKey === a.key
      || typeof a.replacementStatus !== "string" || !statuses.has(a.replacementStatus)) bad();
    if (a.replacementStatus === "queued" ? !text(a.queueItemId) : a.queueItemId !== null) bad();
  }
  return a as ReceiptReviewAck;
}
export function parseReceiptSnapshot(v: unknown): ReceiptSnapshot {
  const s = record(v); exact(s, "items,version"); if (s.version !== 1 || !Array.isArray(s.items) || s.items.length > 200) bad();
  const seen = new Set<string>();
  const items = (s.items as unknown[]).map(raw => {
    const row = record(raw); exact(row, "canReview,key,review,revision,sessionId"); identity(row);
    if (typeof row.canReview !== "boolean") bad();
    const id = JSON.stringify([row.sessionId, row.key]); if (seen.has(id)) bad(); seen.add(id);
    const review = row.review === null ? null : parseReceiptAck(row.review);
    if (review && (row.canReview || !same(review, row as ReceiptIdentity))) bad();
    return { ...row, review } as ReceiptRow;
  });
  return { version: 1, items };
}
export async function sessionReceiptSnapshot(): Promise<ReceiptSnapshot> {
  if (!hasHost()) bad(); return parseReceiptSnapshot(await invoke<unknown>("session_api_receipt_snapshot"));
}
export async function sessionReceiptReview(request: ReceiptReviewRequest): Promise<ReceiptReviewAck> {
  if (!hasHost()) bad();
  const { sessionId, key, revision, prompt, decision, reviewed, confirmed } = request;
  if (!text(sessionId) || sessionId === "__draft__" || sessionId.startsWith("imp:") || !text(key)
    || typeof revision !== "string" || !/^[a-f0-9]{64}$/.test(revision) || typeof prompt !== "string" || (decision === "retry" && !prompt.trim())
    || (decision !== "dismiss" && decision !== "retry") || reviewed !== true || confirmed !== true) throw new Error("SESSION_RECEIPT_INVALID");
  const ack = parseReceiptAck(await invoke<unknown>("session_api_receipt_review", { request: { sessionId, key, revision, prompt, decision, reviewed, confirmed } }));
  if (!same(ack, request) || ack.decision !== decision) bad(); return ack;
}
