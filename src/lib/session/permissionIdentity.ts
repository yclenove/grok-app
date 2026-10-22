import { gateClockKey } from "@/lib/gateClock";

export type PermissionIdentity = {
  sessionId: string;
  rpcId: number;
  requestGeneration?: string | null;
};

/** CLI RPC ids and all descriptive fields can repeat on the same chat. */
export function samePermissionRequest(
  current: PermissionIdentity | null | undefined,
  answered: PermissionIdentity,
): boolean {
  if (!current) return false;
  if (!current.requestGeneration || !answered.requestGeneration) {
    // Legacy hosts have no reusable identity; compare the displayed object.
    return current === answered;
  }
  return current.sessionId === answered.sessionId &&
    current.rpcId === answered.rpcId &&
    current.requestGeneration === answered.requestGeneration;
}

export function permissionClockKey(request: PermissionIdentity): string {
  const key = gateClockKey(request.sessionId, request.rpcId);
  return request.requestGeneration ? `${key}:${request.requestGeneration}` : key;
}
