import { GlassModal } from "./GlassModal";
import type { ExternalQueueReview } from "@/hooks/useExternalQueueReview";
import type { createT } from "@/i18n";

export function ExternalQueueReviewModal({ review: r, tr }: { review: ExternalQueueReview; tr: ReturnType<typeof createT> }) {
  const target = r.target;
  if (!target) return null;
  const uncertain = r.currentState === "pending" || r.currentState === "held";
  const stateKey = r.currentState === "pending" ? "composer.queueHostPending"
    : r.currentState === "held" ? "composer.queueHostHeld" : "composer.queueHostQueued";
  const busyKey = r.operation === "cancel" ? "composer.queueCancelling" : "composer.queueRecovering";
  const doneKey = r.resolution?.decision === "retry" ? "composer.queueRecoverRetried"
    : r.resolution?.decision === "dismiss" ? "composer.queueRecoverDismissed" : "composer.queueCancelled";
  return <GlassModal open onClose={r.close} title={tr("composer.queueReviewTitle")} closeLabel={tr("common.close")}
    wrapBody className="queue-review" footer={<>
      <button type="button" className="btn" onClick={r.close}>{tr("common.close")}</button>
      {!r.done && <button type="button" className="btn" onClick={r.refresh} disabled={r.refreshing}>{tr("composer.queueSyncRefresh")}</button>}
      {!r.done && r.currentState === "queued" && <button type="button" className="btn btn--danger" disabled={!r.canCancel || !r.confirmed}
        onClick={() => void r.cancel()}>{tr(r.busy ? busyKey : "composer.queueCancelAction")}</button>}
      {!r.done && uncertain && <>
        <button type="button" className="btn" data-queue-decision="dismiss" disabled={!r.canRecover || !r.reviewed || !r.riskConfirmed}
          onClick={() => void r.recover("dismiss")}>{tr("composer.queueRecoverDismiss")}</button>
        <button type="button" className="btn btn--danger" data-queue-decision="retry" disabled={!r.canRecover || !r.reviewed || !r.riskConfirmed}
          onClick={() => void r.recover("retry")}>{tr("composer.queueRecoverRetry")}</button>
      </>}
    </>}>
    {!r.done && <p>{tr(stateKey)}</p>}
    <dl className="queue-review__identity"><dt>{tr("composer.queueReviewSession")}</dt><dd>{target.sessionId}</dd>
      <dt>{tr("composer.queueReviewItem")}</dt><dd>{target.itemId}</dd></dl>
    <pre className="queue-review__prompt">{target.prompt}</pre>
    <p>{tr("composer.queueHostReview")}</p>
    {!r.done && <p>{tr(uncertain ? "composer.queueRecoverExplain" : "composer.queueCancelLimit")}</p>}
    {!r.verified && !r.done && <p role="status">{tr("composer.queueSyncError")}</p>}
    {!r.done && r.currentState === "queued" && <label className="queue-review__confirm">
      <input type="checkbox" className="queue-review__checkbox" checked={r.confirmed} disabled={!r.canCancel}
        onChange={e => r.setConfirmed(e.target.checked)} /><span>{tr("composer.queueCancelConfirm")}</span>
    </label>}
    {!r.done && uncertain && <>
      <label className="queue-review__confirm">
        <input type="checkbox" className="queue-review__checkbox" checked={r.reviewed} disabled={!r.canRecover}
          onChange={e => r.setReviewed(e.target.checked)} /><span>{tr("composer.queueRecoverReviewed")}</span>
      </label>
      <label className="queue-review__confirm">
        <input type="checkbox" className="queue-review__checkbox" checked={r.riskConfirmed} disabled={!r.canRecover}
          onChange={e => r.setRiskConfirmed(e.target.checked)} /><span>{tr("composer.queueRecoverRisk")}</span>
      </label>
    </>}
    {r.busy && <p role="status">{tr(busyKey)}</p>}
    {r.done && <p role="status">{tr(doneKey)}</p>}
    {r.resolution?.replacementId && <dl className="queue-review__identity">
      <dt>{tr("composer.queueRecoverReplacement")}</dt><dd>{r.resolution.replacementId}</dd>
    </dl>}
    {r.error && <p role="alert">{tr(r.error === "changed" ? "composer.queueCancelChanged"
      : r.error === "claimed" ? "composer.queueCancelClaimed" : r.error === "active" ? "composer.queueRecoverActive"
      : r.operation === "cancel" ? "composer.queueCancelUnavailable" : "composer.queueRecoverUnavailable")}</p>}
  </GlassModal>;
}
