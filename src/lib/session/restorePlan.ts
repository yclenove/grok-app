import * as api from "@/lib/api";
import { mergePlanFromEvent, restorePlanFromPersistence } from "@/lib/planSession";

/** Live approval identity comes only from the current Host, never from disk. */
export async function loadRestoredPlan(sessionId: string, readyTitle: string) {
  const [chrome, agent, live] = await Promise.all([
    api.sessionPlanChromeGet(sessionId).catch(() => null),
    api.sessionAgentPlanSnapshot(sessionId).catch(() => null),
    api.sessionPendingPlan(sessionId).catch(() => null),
  ]);
  const restored = restorePlanFromPersistence(chrome, agent, readyTitle);
  if (live?.sessionId === sessionId && Number.isSafeInteger(live.rpcId) &&
      live.rpcId! >= 0 && typeof live.requestGeneration === "string" && live.requestGeneration.trim()) {
    return mergePlanFromEvent(restored, live, readyTitle);
  }
  return restored;
}
