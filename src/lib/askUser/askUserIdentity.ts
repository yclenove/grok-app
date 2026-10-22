import { gateClockKey } from "@/lib/gateClock";
import type { AskUserPayload } from "@/lib/session";

type AskUserIdentity = Pick<AskUserPayload, "sessionId" | "rpcId" | "requestGeneration">;
const legacyClocks = new WeakMap<AskUserIdentity, number>();
let legacySequence = 0;

/** RPC numbers and question text can repeat; legacy hosts only have object identity. */
export function sameAskUserRequest(current: AskUserIdentity | null | undefined, answered: AskUserIdentity): boolean {
  if (!current) return false;
  if (!current.requestGeneration || !answered.requestGeneration) return current === answered;
  return current.sessionId === answered.sessionId && current.rpcId === answered.rpcId &&
    current.requestGeneration === answered.requestGeneration;
}

export function askUserClockKey(request: AskUserIdentity): string {
  const base = gateClockKey(request.sessionId, request.rpcId);
  if (request.requestGeneration) return `${base}:${request.requestGeneration}`;
  let identity = legacyClocks.get(request);
  if (identity == null) {
    identity = ++legacySequence;
    legacyClocks.set(request, identity);
  }
  return `${base}:legacy-${identity}`;
}
