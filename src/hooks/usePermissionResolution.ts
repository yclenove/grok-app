import { useCallback, type Dispatch, type RefObject, type SetStateAction } from "react";
import type { createT } from "@/i18n";
import * as api from "@/lib/api";
import { dropGateClock } from "@/lib/gateClock";
import type { PermissionPayload } from "@/lib/session";
import { permissionClockKey, samePermissionRequest } from "@/lib/session/permissionIdentity";

export type PermissionDecision = "allow_once" | "allow_session" | "deny";

/** A reply settles its permission, leaving questions and plans untouched. */
export function usePermissionResolution(c: {
  pendingPermBySessionRef: RefObject<Map<string, PermissionPayload>>;
  permRaisedAtRef: RefObject<Map<string, number>>;
  viewingSessionIdRef: RefObject<string | null>;
  setPerm: Dispatch<SetStateAction<PermissionPayload | null>>;
  showToast: (message: string, duration: number) => void;
  tr: ReturnType<typeof createT>;
}) {
  const { pendingPermBySessionRef, permRaisedAtRef, viewingSessionIdRef, setPerm, showToast, tr } = c;
  return useCallback(async (p: PermissionPayload, decision: PermissionDecision, optionId: string) => {
    try {
      await api.sessionResolvePermission({
        rpcId: p.rpcId,
        requestGeneration: p.requestGeneration,
        decision,
        optionId,
        scopeKey: p.scopeKey,
        sessionId: p.sessionId,
        options: p.options,
        toolName: p.toolName,
      });
      const matchesCached = samePermissionRequest(pendingPermBySessionRef.current.get(p.sessionId), p);
      if (matchesCached) {
        pendingPermBySessionRef.current.delete(p.sessionId);
      }
      if (p.requestGeneration || matchesCached) {
        dropGateClock(permRaisedAtRef.current, permissionClockKey(p));
      }
      setPerm((current) => samePermissionRequest(current, p) ? null : current);
    } catch (e) {
      const code = e && typeof e === "object" && "code" in e
        ? String((e as { code?: string }).code) : "";
      const msg = code === "UNSUPPORTED" ? tr("mirror.unsupported") : String(e);
      if (viewingSessionIdRef.current === p.sessionId &&
        samePermissionRequest(pendingPermBySessionRef.current.get(p.sessionId), p)) {
        showToast(msg, 4000);
      }
      throw e instanceof Error ? e : new Error(msg);
    }
  }, [pendingPermBySessionRef, permRaisedAtRef, viewingSessionIdRef, setPerm, showToast, tr]);
}
