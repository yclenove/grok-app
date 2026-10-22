import type { Dispatch, RefObject, SetStateAction } from "react";
import { invalidatePlanGate, type SessionPlanState } from "@/lib/planSession";
import type { SessionHostEventsCtx } from "./useSessionHostEvents";
import type { registerGateAndMetaSubscriptions } from "./sessionHostGateSubscriptions";

type Request = { sessionId?: string; rpcId: number; requestGeneration?: string | null };
type Row = {
  sessionId?: string;
  identityVersion?: number;
  permissionRpcId?: number | null;
  permissionRequestGeneration?: string | null;
  askUserRpcId?: number | null;
  askUserRequestGeneration?: string | null;
  planRpcId?: number | null;
  planRequestGeneration?: string | null;
  previousPlanChromeRevision?: string | null;
  publishedPlanChromeRevision?: string | null;
  planChromeRevision?: string | null;
};

function matchesRequest(current: Request, rpc: number | null | undefined, generation: string | null | undefined, modern: boolean) {
  if (generation != null) return typeof generation === "string" && !!generation.trim() && current.rpcId === rpc && current.requestGeneration === generation;
  return !current.requestGeneration && (rpc != null ? current.rpcId === rpc : !modern);
}

function matchesPlan(current: SessionPlanState, row: Row) {
  if (row.planRequestGeneration != null) {
    return typeof row.planRequestGeneration === "string" && !!row.planRequestGeneration.trim() && current.rpcId === row.planRpcId && current.requestGeneration === row.planRequestGeneration;
  }
  if (current.chromeRevision) {
    return current.chromeRevision === row.previousPlanChromeRevision || current.chromeRevision === row.publishedPlanChromeRevision;
  }
  return !current.requestGeneration && (row.planRpcId != null ? current.rpcId === row.planRpcId : row.identityVersion !== 1);
}

/** Share the production subscription lifecycle; callbacks only retire their captured gates. */
export function registerGateInvalidationSubscriptions(c: SessionHostEventsCtx, env: Parameters<typeof registerGateAndMetaSubscriptions>[1]) {
  function clearRequest<T extends Request>(
    sid: string, parked: RefObject<Map<string, T>>, apply: Dispatch<SetStateAction<T | null>>,
    rpc: number | null | undefined, generation: string | null | undefined, modern: boolean,
  ) {
    const cache = parked.current;
    const current = cache.get(sid);
    if (current && !matchesRequest(current, rpc, generation, modern)) return;
    if (!current && modern && rpc == null) return;
    cache.delete(sid);
    if (c.viewingSessionIdRef.current === sid) apply((prev) => {
      if (env.isCancelled() || parked.current !== cache || c.viewingSessionIdRef.current !== sid) return prev;
      const replacement = cache.get(sid);
      if (replacement && !matchesRequest(replacement, rpc, generation, modern)) return prev;
      return prev?.sessionId === sid && matchesRequest(prev, rpc, generation, modern) ? null : prev;
    });
  }
  function invalidateRow(row: Row) {
    if (!row || (row.identityVersion != null && row.identityVersion !== 1)) return;
    const sid = typeof row.sessionId === "string" ? row.sessionId.trim() : "";
    if (!sid) return;
    const modern = row.identityVersion === 1;
    clearRequest(sid, c.pendingPermBySessionRef, c.setPerm, row.permissionRpcId, row.permissionRequestGeneration, modern);
    clearRequest(sid, c.pendingAskUserBySessionRef, c.setAskUser, row.askUserRpcId, row.askUserRequestGeneration, modern);
    const cache = c.planBySessionRef?.current;
    const base = cache?.get(sid);
    if (!cache || (base && !matchesPlan(base, row))) return;
    if (!base && modern && !row.previousPlanChromeRevision && row.planRpcId == null) return;
    if (!base) cache.delete(sid); // Also invalidate an outstanding restore query.
    const stalePlan = (prev: SessionPlanState) => {
      const replacement = cache.get(sid);
      if (replacement && !matchesPlan(replacement, row)) return prev;
      if (!matchesPlan(prev, row)) return prev;
      const next = { ...invalidatePlanGate(prev), rpcId: null, requestGeneration: null,
        waiting: false, gateStale: true, chromeRevision: row.planChromeRevision ?? null };
      cache.set(sid, next);
      c.markPlanPendingBadge?.(sid, next);
      return next;
    };
    if (sid === c.viewingSessionIdRef.current) {
      c.setPlan((prev) => env.isCancelled() || c.planBySessionRef?.current !== cache || c.viewingSessionIdRef.current !== sid ? prev : stalePlan(prev));
    } else if (base) stalePlan(base);
  }
  env.track(env.listenWithRetry<{ sessions?: Row[] }>("session://permissions_invalidated", (payload) => {
    if (env.isCancelled() || !payload) return;
    if (Array.isArray(payload.sessions) && payload.sessions.length) {
      payload.sessions.forEach(invalidateRow);
    } else {
      // Older Hosts omitted identities. They may retire only legacy cards.
      const sessions = new Set([...c.pendingPermBySessionRef.current.keys(), ...c.pendingAskUserBySessionRef.current.keys(),
        ...(c.planBySessionRef?.current.keys() ?? [])]);
      if (c.viewingSessionIdRef.current) sessions.add(c.viewingSessionIdRef.current);
      sessions.forEach((sessionId) => invalidateRow({ sessionId }));
    }
  }));
  env.track(env.listenWithRetry<{ sessionId?: string; rpcId?: number; requestGeneration?: string }>("session://ask_user_cleared", (payload) => {
    if (env.isCancelled()) return;
    const sid = typeof payload?.sessionId === "string" ? payload.sessionId.trim() : "";
    if (sid) clearRequest(sid, c.pendingAskUserBySessionRef, c.setAskUser, payload.rpcId, payload.requestGeneration, false);
  }));
}
