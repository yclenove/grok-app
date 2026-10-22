import { hasHost, invoke } from "./host";
import { parseExternalQueueSnapshot, type ExternalQueueSnapshot } from "../externalQueueSnapshot";

/** Read-only snapshot, supported by desktop IPC and authenticated mirror RPC. */
export async function sessionQueueSnapshot(): Promise<ExternalQueueSnapshot> {
  if (!hasHost()) throw new Error("SESSION_QUEUE_UNAVAILABLE");
  return parseExternalQueueSnapshot(await invoke<unknown>("session_api_queue_snapshot"));
}


export type QueueCancelRequest = { sessionId: string; itemId: string; prompt: string; createdAt: number };
export type QueueCancelResult = { version: 1; status: "cancelled"; sessionId: string; itemId: string };
/** Host performs compare-and-cancel under the dispatch lock. Never removes a local row. */
export async function sessionQueueCancel(request: QueueCancelRequest): Promise<QueueCancelResult> {
  if (!hasHost()) throw new Error("SESSION_QUEUE_UNAVAILABLE");
  if (!request.sessionId || request.sessionId.trim() !== request.sessionId
    || request.sessionId === "__draft__" || request.sessionId.startsWith("imp:")
    || !request.itemId || request.itemId.trim() !== request.itemId || !request.prompt.trim()
    || !Number.isSafeInteger(request.createdAt)) throw new Error("SESSION_QUEUE_INVALID");
  const payload = { sessionId: request.sessionId, itemId: request.itemId, prompt: request.prompt, createdAt: request.createdAt };
  const value = await invoke<unknown>("session_api_queue_cancel", { request: payload });
  if (!value || typeof value !== "object" || Array.isArray(value)) throw new Error("SESSION_QUEUE_UNAVAILABLE");
  const result = value as Record<string, unknown>;
  if (Object.keys(result).sort().join(",") !== "itemId,sessionId,status,version" || result.version !== 1
    || result.status !== "cancelled" || result.sessionId !== request.sessionId || result.itemId !== request.itemId)
    throw new Error("SESSION_QUEUE_UNAVAILABLE");
  return result as QueueCancelResult;
}

export type QueueRecoveryDecision = "dismiss" | "retry";
export type QueueRecoveryRequest = QueueCancelRequest & { decision: QueueRecoveryDecision; reviewed: boolean; confirmed: boolean };
export type QueueRecoveryResult = { version: 1; decision: QueueRecoveryDecision; sessionId: string; itemId: string; replacementId: string | null };
/** Retires an uncertain attempt only after explicit review. Never resets its claim identity. */
export async function sessionQueueRecover(request: QueueRecoveryRequest): Promise<QueueRecoveryResult> {
  if (!hasHost()) throw new Error("SESSION_QUEUE_UNAVAILABLE");
  if (!request.sessionId || request.sessionId.trim() !== request.sessionId || request.sessionId === "__draft__"
    || request.sessionId.startsWith("imp:") || !request.itemId || request.itemId.trim() !== request.itemId
    || !request.prompt.trim() || !Number.isSafeInteger(request.createdAt) || request.reviewed !== true || request.confirmed !== true
    || (request.decision !== "dismiss" && request.decision !== "retry")) throw new Error("SESSION_QUEUE_INVALID");
  const payload = { sessionId: request.sessionId, itemId: request.itemId, prompt: request.prompt, createdAt: request.createdAt,
    decision: request.decision, reviewed: true, confirmed: true };
  const value = await invoke<unknown>("session_api_queue_recover", { request: payload });
  if (!value || typeof value !== "object" || Array.isArray(value)) throw new Error("SESSION_QUEUE_UNAVAILABLE");
  const result = value as Record<string, unknown>;
  if (Object.keys(result).sort().join(",") !== "decision,itemId,replacementId,sessionId,version"
    || result.version !== 1 || result.decision !== request.decision || result.sessionId !== request.sessionId || result.itemId !== request.itemId
    || (request.decision === "dismiss" ? result.replacementId !== null
      : typeof result.replacementId !== "string" || !result.replacementId.trim()
        || result.replacementId.trim() !== result.replacementId || result.replacementId === request.itemId)) throw new Error("SESSION_QUEUE_UNAVAILABLE");
  return result as QueueRecoveryResult;
}
