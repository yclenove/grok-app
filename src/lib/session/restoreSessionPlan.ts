import { emptySessionPlan, invalidatePlanGate, shouldReopenClosedPlan, type SessionPlanState } from "@/lib/planSession";
import type { ParkedGates } from "@/lib/sessionGateRestore";
import { loadRestoredPlan } from "./restorePlan";

/** Show memory immediately, but validate its approval identity with the Host.
 * Event/reply mutations and newer pulls take precedence over the whole restore. */
export async function restoreSessionPlan(
  sessionId: string,
  stillOpen: () => boolean,
  { parked, apply, markPending, readyTitle }: {
    parked: ParkedGates<SessionPlanState>;
    apply: (plan: SessionPlanState) => void;
    markPending: (sessionId: string, plan: SessionPlanState) => void;
    readyTitle: string;
  },
): Promise<void> {
  const cache = parked.current;
  const lease = cache.beginRestore(sessionId);
  const cached = cache.get(sessionId);
  apply(cached ?? emptySessionPlan(readyTitle));
  const canCommit = () => stillOpen() && parked.current === cache && lease.isCurrent();
  const commit = (next: SessionPlanState) => {
    cache.set(sessionId, next);
    markPending(sessionId, next);
    if (stillOpen() && parked.current === cache && cache.get(sessionId) === next) apply(next);
  };
  try {
    const restored = await loadRestoredPlan(sessionId, readyTitle);
    if (!canCommit()) return;
    if (cached?.userClosed && !shouldReopenClosedPlan(cached, restored, "agent")) {
      // Disk may lag a local dismissal. A distinct plan tool or live approval
      // identity is needed to reopen, matching the normal event lifecycle.
      commit(cached);
    } else if (cached && !restored.visible && !restored.userClosed && !restored.body && !restored.entries.length) {
      commit(invalidatePlanGate(cached));
    } else {
      commit(restored);
    }
  } catch {
    // Optional Host queries soft-fail in the loader. Keep any remaining memory
    // read-only if malformed host data or an unexpected query failure escapes.
    if (cached && canCommit()) commit(invalidatePlanGate(cached));
  } finally {
    lease.finish();
  }
}
