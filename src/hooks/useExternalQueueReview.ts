import { useEffect, useRef, useState } from "react";
import { sessionQueueCancel, sessionQueueRecover, type QueueCancelRequest, type QueueRecoveryDecision, type QueueRecoveryResult } from "@/lib/api/sessionQueue";
import type { QueuedSend } from "@/lib/sendQueue";
import type { ExternalQueueSync } from "./useExternalSendQueue";

type Options = { sessionId: string | null; queue: readonly QueuedSend[]; sync: ExternalQueueSync };
type Target = QueueCancelRequest & { state: QueuedSend["externalState"] };
export function useExternalQueueReview(options: Options) {
  const latest = useRef(options); latest.current = options;
  const generation = useRef(0), inFlight = useRef(false), alive = useRef(true);
  const [target, setTarget] = useState<Target | null>(null);
  const [confirmed, setConfirmed] = useState(false);
  const [reviewed, setReviewed] = useState(false);
  const [riskConfirmed, setRiskConfirmed] = useState(false);
  const [busy, setBusy] = useState(false), [done, setDone] = useState(false);
  const [operation, setOperation] = useState<"cancel" | QueueRecoveryDecision | null>(null);
  const [resolution, setResolution] = useState<QueueRecoveryResult | null>(null);
  const [error, setError] = useState<"changed" | "claimed" | "active" | "unavailable" | null>(null);
  useEffect(() => { alive.current = true; return () => { alive.current = false; generation.current++; }; }, []);
  useEffect(() => {
    generation.current++; setTarget(null); setConfirmed(false); setReviewed(false); setRiskConfirmed(false);
    setError(null); setDone(false); setResolution(null); setOperation(null);
  }, [options.sessionId]);
  const visible = target?.sessionId === options.sessionId ? target : null;
  const current = visible && options.queue.find(q => q.source === "external" && q.id === visible.itemId
    && q.storedDisplay === visible.prompt && q.createdAt === visible.createdAt);
  const mutable = !!visible && !!current && options.sync.enabled && options.sync.ready && !options.sync.error && !busy && !done;
  const canCancel = mutable && current?.externalState === "queued";
  const canRecover = mutable && (current?.externalState === "pending" || current?.externalState === "held");
  function open(item: QueuedSend) {
    const sid = latest.current.sessionId;
    if (!sid || sid === "__draft__" || sid.startsWith("imp:") || item.source !== "external") return;
    generation.current++;
    setTarget({ sessionId: sid, itemId: item.id, prompt: item.storedDisplay, createdAt: item.createdAt, state: item.externalState });
    setConfirmed(false); setReviewed(false); setRiskConfirmed(false); setDone(false); setError(null); setResolution(null); setOperation(null);
  }
  function close() { generation.current++; setTarget(null); setConfirmed(false); setReviewed(false); setRiskConfirmed(false); }
  async function submit(action: "cancel" | QueueRecoveryDecision) {
    if (!visible || inFlight.current) return;
    if (action === "cancel" ? !canCancel || !confirmed : !canRecover || !reviewed || !riskConfirmed) return;
    const attempt = generation.current;
    const request = { sessionId: visible.sessionId, itemId: visible.itemId, prompt: visible.prompt, createdAt: visible.createdAt };
    inFlight.current = true; setBusy(true); setError(null); setOperation(action);
    try {
      let ack: QueueRecoveryResult | null = null;
      if (action === "cancel") await sessionQueueCancel(request);
      else ack = await sessionQueueRecover({ ...request, decision: action, reviewed: true, confirmed: true });
      if (alive.current && attempt === generation.current) {
        setDone(true); setResolution(ack); setConfirmed(false); setReviewed(false); setRiskConfirmed(false);
      }
    } catch (err) {
      if (alive.current && attempt === generation.current) {
        const message = err instanceof Error ? err.message : typeof err === "string" ? err : "";
        setError(message === "SESSION_QUEUE_CHANGED" ? "changed" : message === "SESSION_QUEUE_CLAIMED" ? "claimed"
          : message === "SESSION_QUEUE_ACTIVE" ? "active" : "unavailable");
      }
    } finally {
      inFlight.current = false;
      if (alive.current) { setBusy(false); latest.current.sync.refresh(); }
    }
  }
  return { target: visible, currentState: current ? current.externalState : visible?.state, canCancel, canRecover,
    confirmed, setConfirmed, reviewed, setReviewed, riskConfirmed, setRiskConfirmed, busy, done, operation, resolution, error,
    open, close, cancel: () => submit("cancel"), recover: (decision: QueueRecoveryDecision) => submit(decision),
    refresh: options.sync.refresh, verified: options.sync.ready && !options.sync.error, refreshing: options.sync.loading };
}
export type ExternalQueueReview = ReturnType<typeof useExternalQueueReview>;
