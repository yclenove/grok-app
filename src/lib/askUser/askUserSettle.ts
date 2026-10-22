/** Transport result; the requesting domain owns cache, view and retry policy. */

export type AskUserSettleDecision = "accepted" | "cancelled";

export type AskUserSettlePayload = {
  requestGeneration?: string;
  rpcId: number;
  sessionId: string;
};

export type AskUserActionOutcome =
  | { kind: "delivered" }
  | { kind: "failed"; error: unknown }
  | { kind: "skipped" };
export type AskUserActionResult = void | AskUserActionOutcome;

/** Dismiss / X stay clickable while an accept IPC is in flight. */
export function askUserDismissLocked(_busy: boolean): boolean {
  return false;
}

export async function settleAskUserDecision(input: {
  payload: AskUserSettlePayload;
  decision: AskUserSettleDecision;
  answers?: Record<string, string> | null;
  resolve: (args: {
    decision: AskUserSettleDecision;
    answers?: Record<string, string> | null;
    rpcId: number;
    requestGeneration?: string;
    sessionId: string;
  }) => Promise<unknown>;
}): Promise<AskUserActionOutcome> {
  try {
    await input.resolve({
      decision: input.decision,
      answers: input.answers ?? null,
      rpcId: input.payload.rpcId,
      ...(input.payload.requestGeneration
        ? { requestGeneration: input.payload.requestGeneration }
        : {}),
      sessionId: input.payload.sessionId,
    });
    return { kind: "delivered" };
  } catch (error) {
    return { kind: "failed", error };
  }
}
