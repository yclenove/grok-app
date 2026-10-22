import { useCallback, type Dispatch, type RefObject, type SetStateAction } from "react";
import type { createT } from "@/i18n";
import * as api from "@/lib/api";
import { dropAskUserClock, pauseAskUserClock } from "@/lib/askUser/askUserClocks";
import { askUserClockKey, sameAskUserRequest } from "@/lib/askUser/askUserIdentity";
import { settleAskUserDecision, type AskUserActionOutcome, type AskUserSettleDecision } from "@/lib/askUser/askUserSettle";
import type { AskUserPayload } from "@/lib/session";

export type AskUserResolutionContext = {
  pendingAskUserBySessionRef: RefObject<Map<string, AskUserPayload>>;
  viewingSessionIdRef: RefObject<string | null>;
  setAskUser: Dispatch<SetStateAction<AskUserPayload | null>>;
};
const replyClaims = new WeakMap<Map<string, AskUserPayload>, Set<AskUserPayload>>();

/** Optimistically dismiss only this card. ACK/failure cannot affect another human gate. */
export function useAskUserResolution(c: AskUserResolutionContext & {
  showToast: (message: string, duration: number) => void;
  tr: ReturnType<typeof createT>;
}) {
  const { pendingAskUserBySessionRef, viewingSessionIdRef, setAskUser, showToast, tr } = c;
  return useCallback(async (
    payload: AskUserPayload,
    decision: AskUserSettleDecision,
    answers?: Record<string, string>,
  ): Promise<AskUserActionOutcome> => {
    const cache = pendingAskUserBySessionRef.current;
    const claims = replyClaims.get(cache) ?? new Set<AskUserPayload>();
    if (!sameAskUserRequest(pendingAskUserBySessionRef.current.get(payload.sessionId), payload) ||
        [...claims].some((claimed) => sameAskUserRequest(claimed, payload))) return { kind: "skipped" };
    replyClaims.set(cache, claims);
    claims.add(payload);
    setAskUser((current) => sameAskUserRequest(current, payload) ? null : current);
    const clockKey = askUserClockKey(payload);
    try {
      const outcome = await settleAskUserDecision({ payload, decision, answers, resolve: api.sessionResolveAskUser });
      if (outcome.kind === "delivered") {
        if (sameAskUserRequest(pendingAskUserBySessionRef.current.get(payload.sessionId), payload)) {
          pendingAskUserBySessionRef.current.delete(payload.sessionId);
        }
        dropAskUserClock(clockKey);
        setAskUser((current) => sameAskUserRequest(current, payload) ? null : current);
      } else if (outcome.kind === "failed") {
        if (!sameAskUserRequest(pendingAskUserBySessionRef.current.get(payload.sessionId), payload)) {
          dropAskUserClock(clockKey);
          return { kind: "skipped" };
        }
        pauseAskUserClock(clockKey);
        if (viewingSessionIdRef.current === payload.sessionId) {
          setAskUser((current) => {
            if (viewingSessionIdRef.current !== payload.sessionId ||
                !sameAskUserRequest(pendingAskUserBySessionRef.current.get(payload.sessionId), payload)) return current;
            return !current || sameAskUserRequest(current, payload) ? payload : current;
          });
          const error = outcome.error;
          const code = error && typeof error === "object" && "code" in error ? String(error.code) : "";
          showToast(code === "UNSUPPORTED" ? tr("mirror.unsupported") : String(error), 4500);
        }
      }
      return outcome;
    } finally {
      claims.delete(payload);
    }
  }, [pendingAskUserBySessionRef, viewingSessionIdRef, setAskUser, showToast, tr]);
}
