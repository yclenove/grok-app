import { useLayoutEffect, useRef } from "react";
import { GlassModal } from "./GlassModal";
import { hasHost } from "@/lib/api/host";
import { useSessionReceipts } from "@/hooks/useSessionReceipts";
import type { MessageKey, Vars } from "@/i18n";
import "./SessionReceiptPanel.css";
type TFn = (key: MessageKey, vars?: Vars) => string;
function receiptIdentity(row: { sessionId: string; key: string; revision: string }) {
  return JSON.stringify([row.sessionId, row.key, row.revision]);
}
export function SessionReceiptPanel({ t }: { t: TFn }) {
  const r = useSessionReceipts(); const heading = useRef<HTMLParagraphElement>(null);
  const pending = useRef<HTMLParagraphElement>(null), result = useRef<HTMLParagraphElement>(null), failure = useRef<HTMLParagraphElement>(null);
  const rows = useRef(new Map<string, HTMLButtonElement>()), lastSelection = useRef<string | null>(null);
  const selectedId = r.selected ? receiptIdentity(r.selected) : null;
  useLayoutEffect(() => {
    const previous = lastSelection.current;
    lastSelection.current = r.open ? selectedId : null;
    if (!r.open) return;
    if (previous !== selectedId) {
      (selectedId ? heading.current : rows.current.get(previous ?? "") ?? heading.current)?.focus();
      return;
    }
    // Disabled/removed actions must not leave keyboard focus on the page body.
    // Keep a user's still-valid focus (Close, Back, etc.) when async feedback arrives.
    const active = document.activeElement;
    if (active && active !== document.body && active.isConnected && !active.matches(":disabled")) return;
    (failure.current ?? pending.current ?? result.current ?? heading.current)?.focus();
  }, [r.open, selectedId, r.busy, r.loading, r.error, r.resolution]);
  const status = r.resolution?.replacementStatus;
  const statusKey = status === "turn_started" ? "receipt.started" : status === "queued" ? "receipt.queued"
    : status === "unknown" ? "receipt.unknown" : status === "cancelled" ? "receipt.cancelled" : "receipt.failed";
  const errorKey = r.error === "SESSION_RECEIPT_ACTIVE" ? "receipt.active" : r.error === "SESSION_RECEIPT_CHANGED" ? "receipt.changed"
    : r.error === "SESSION_RECEIPT_PROMPT_MISMATCH" ? "receipt.mismatch" : r.error === "SESSION_RECEIPT_CAPACITY" ? "receipt.capacity" : "receipt.unavailable";
  return <>
    <div className="settings-row__actions"><button type="button" className="btn btn--ghost" disabled={!hasHost()} onClick={r.show}>{t("receipt.title")}</button></div>
    <GlassModal open={r.open} onClose={r.close} title={t("receipt.title")} closeLabel={t("common.close")} wrapBody className="queue-review receipt-review"
      footer={<>
        <button type="button" className="btn" onClick={r.close}>{t("common.close")}</button>
        {r.selected && <button type="button" className="btn" onClick={() => r.select(null)}>{t("receipt.back")}</button>}
        <button type="button" className="btn" onClick={() => void r.refresh()} disabled={r.loading || r.busy}>{t("receipt.refresh")}</button>
        {r.selected && !r.resolution && <>
          <button type="button" className="btn" disabled={!r.canReview || !r.reviewed || !r.confirmed} onClick={() => void r.submit("dismiss")}>{t("composer.queueRecoverDismiss")}</button>
          <button type="button" className="btn btn--danger" disabled={!r.canReview || !r.reviewed || !r.confirmed || !r.prompt.trim()} onClick={() => void r.submit("retry")}>{t("receipt.retry")}</button>
        </>}
      </>}>
      <p ref={heading} tabIndex={-1}>{t("receipt.help")}</p>
      {!r.selected && <div className="receipt-review__list">
        {r.verified && !r.items.length && <p role="status">{t("receipt.empty")}</p>}
        {r.items.map(row => <button type="button" className="btn receipt-review__row" key={JSON.stringify([row.sessionId, row.key])}
          ref={element => { const id = receiptIdentity(row); if (element) rows.current.set(id, element); else rows.current.delete(id); }}
          onClick={() => r.select(row)}><span>{row.sessionId}</span><code>{row.key}</code><span>{t(row.review ? "receipt.reviewed" : "receipt.unknown")}</span></button>)}
      </div>}
      {r.selected && <>
        <dl className="queue-review__identity"><dt>{t("composer.queueReviewSession")}</dt><dd>{r.selected.sessionId}</dd><dt>{t("receipt.key")}</dt><dd>{r.selected.key}</dd></dl>
        {r.resolution ? <>
          <p role="status" ref={result} tabIndex={-1}>{t(r.resolution.decision === "dismiss" ? "composer.queueRecoverDismissed" : "receipt.recorded")}</p>
          {r.resolution.replacementKey && <dl className="queue-review__identity"><dt>{t("receipt.newKey")}</dt><dd>{r.resolution.replacementKey}</dd>
            <dt>{t("receipt.result")}</dt><dd>{t(statusKey)}</dd>{r.resolution.queueItemId && <><dt>{t("composer.queueReviewItem")}</dt><dd>{r.resolution.queueItemId}</dd></>}</dl>}
        </> : <>
          {!r.selected.canReview && <p>{t("receipt.legacy")}</p>}
          <label className="receipt-review__prompt">{t("receipt.prompt")}<textarea rows={4} value={r.prompt} disabled={!r.canReview}
            onChange={e => r.setPrompt(e.target.value)} /></label>
          <label className="queue-review__confirm"><input type="checkbox" className="queue-review__checkbox" checked={r.reviewed} disabled={!r.canReview}
            onChange={e => r.setReviewed(e.target.checked)} /><span>{t("composer.queueRecoverReviewed")}</span></label>
          <label className="queue-review__confirm"><input type="checkbox" className="queue-review__checkbox" checked={r.confirmed} disabled={!r.canReview}
            onChange={e => r.setConfirmed(e.target.checked)} /><span>{t("composer.queueRecoverRisk")}</span></label>
        </>}
      </>}
      {(r.loading || r.busy) && <p role="status" ref={pending} tabIndex={-1}>{t(r.busy ? "composer.queueRecovering" : "receipt.loading")}</p>}
      {r.error && <p role="alert" ref={failure} tabIndex={-1}>{t(errorKey)}</p>}
    </GlassModal>
  </>;
}
