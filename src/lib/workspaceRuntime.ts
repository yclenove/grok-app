/** A refused turn never retries automatically in a different directory. */
import type { createT } from "@/i18n";
import { importErrorKey } from "@/lib/sessionImport/errorCopy";
export const WORKSPACE_REVIEW_EVENT = "grok:workspace-runtime-review";
const ROLLED_BACK_UNSENT_PREFIX = "TURN_NOT_DISPATCHED_ROLLED_BACK: ";

function commandErrorText(error: unknown): string {
  return String(error).replace(/^Error: /, "");
}

export function rolledBackUnsentCause(error: unknown): string | null {
  const raw = commandErrorText(error);
  if (!raw.startsWith(ROLLED_BACK_UNSENT_PREFIX)) return null;
  const cause = raw.slice(ROLLED_BACK_UNSENT_PREFIX.length).trim();
  return cause.length > 0 ? cause : null;
}

export function isWorkspaceChangedError(error: unknown): boolean {
  return /^(?:Error: )?WORKSPACE_CHANGED:/.test(String(error));
}

export function requestWorkspaceRuntimeReview(sessionId: string): void {
  window.dispatchEvent(new CustomEvent(WORKSPACE_REVIEW_EVENT, { detail: sessionId }));
}

export function isUnsentTurnError(error: unknown): boolean {
  return isWorkspaceChangedError(error) ||
    /^(?:Error: )?SEND_CANCELLED:/.test(String(error)) ||
    rolledBackUnsentCause(error) != null;
}

/** Roll back both optimistic rows only when Host refused before journaling. */
export function describeWorkspaceSendFailure(
  error: unknown, tr: ReturnType<typeof createT>, dropIds: Set<string>,
  userMessageId: string, viewedSessionId: string | null,
): string {
  const rolledBackCause = rolledBackUnsentCause(error);
  const unsent = isUnsentTurnError(error);
  if (unsent) dropIds.add(userMessageId);
  if (rolledBackCause) return tr(importErrorKey(rolledBackCause));
  if (!isWorkspaceChangedError(error)) {
    if (unsent) return tr("activity.cancelledByUser");
    const key = importErrorKey(commandErrorText(error));
    return key === "sessionImport.err.generic" ? String(error) : tr(key);
  }
  if (viewedSessionId) requestWorkspaceRuntimeReview(viewedSessionId);
  return tr("workspace.runtime.explain");
}
