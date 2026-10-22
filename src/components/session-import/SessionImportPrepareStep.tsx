import { useLayoutEffect, useRef } from "react";
import type {
  PrepareBatchItem,
  PrepareBatchStatus,
} from "@/lib/api/sessionMigration";
import { isDialogFocusAllowed } from "@/lib/a11yFocus";
import { importErrorKey } from "@/lib/sessionImport/errorCopy";
import type { SessionImportTFn } from "./sessionImportHostLabels";
import { SessionImportActionPortal } from "./SessionImportActionPortal";

function statusKey(status: PrepareBatchStatus): Parameters<SessionImportTFn>[0] {
  switch (status.state) {
    case "queued":
      return "sessionImport.prepare.queued";
    case "preparing":
      return "sessionImport.prepare.running";
    case "cancelling":
      return "sessionImport.job.cancelling";
    case "completed":
      return "sessionImport.prepare.completed";
    case "completed-with-issues":
      return "sessionImport.job.completedWithIssues";
    case "failed":
      return "sessionImport.job.failed";
    case "cancelled":
      return "sessionImport.job.cancelled";
    case "planned":
      return "sessionImport.prepare.completed";
  }
}

export function SessionImportPrepareStep(props: {
  tr: SessionImportTFn;
  status: PrepareBatchStatus;
  items: PrepareBatchItem[];
  itemsError: boolean;
  nextCursor: string | null;
  busy: boolean;
  active: boolean;
  onCancel: () => void;
  onRetry: () => void;
  onContinue: () => void;
  onBack: () => void;
  onLoadMore: () => void;
  onRetryItems: () => void;
  footerTarget?: HTMLElement | null;
}) {
  const { tr, status } = props;
  const done = status.prepared + status.failed + status.skipped;
  const canContinue = ["completed", "completed-with-issues"].includes(status.state)
    && status.prepared > 0;
  const canCancel = status.state === "queued" || status.state === "preparing";
  const issues = props.items.filter((item) => item.state === "failed" || item.state === "skipped");
  const hasUnloadedIssues = status.failed + status.skipped > issues.length;
  const footerRef = useRef<HTMLDivElement>(null);
  const footerFocus = useRef<{ button: HTMLButtonElement; batchId: string } | null>(null);

  useLayoutEffect(() => {
    const previous = footerFocus.current;
    if (!previous) return;
    if (previous.batchId !== status.batchId) {
      footerFocus.current = null;
      return;
    }
    if (props.busy) return;
    const doc = previous.button.ownerDocument;
    const active = doc.activeElement;
    if (active && active !== doc.body && active !== doc.documentElement) {
      if (active !== previous.button) footerFocus.current = null;
      return;
    }
    const footer = footerRef.current;
    if (!footer || !isDialogFocusAllowed(footer)) {
      footerFocus.current = null;
      return;
    }
    // Last-page/result recovery removes the trigger; busy transitions can
    // also blur it. Repair only that loss, without activating another action.
    const target = previous.button.isConnected && !previous.button.disabled ? previous.button
      : footer.querySelector<HTMLButtonElement>("button:not(:disabled)");
    if (!target) return;
    footerFocus.current = null;
    target.focus();
  });

  return (
    <div data-import-prepare="1" className="session-import__prepare">
      <div className="session-import__status-card" data-state={status.state}>
        <p role="status">{tr(statusKey(status))}</p>
        <p role="status">
          {tr("sessionImport.prepare.progress", { done, total: status.total })}
        </p>
        <p>
          {tr("sessionImport.prepare.counts", {
            prepared: status.prepared, failed: status.failed, skipped: status.skipped,
          })}
        </p>
        <div
          className="session-import__progress"
          role="progressbar"
          aria-label={tr("sessionImport.step.prepare")}
          aria-valuemin={0}
          aria-valuemax={status.total}
          aria-valuenow={done}
          data-state={status.state}
        >
          <span style={{ width: `${status.total ? (done / status.total) * 100 : 0}%` }} />
        </div>
      </div>
      {issues.length ? (
        <ul className="session-import__list">
          {issues.map((item) => (
            <li key={item.candidateId}>
              <span>{item.title || item.provider}</span>
              {item.error ? (
                <p role="alert" className="session-import__notice" data-issue="unsupported">
                  {tr(importErrorKey(`${item.error.code}: ${item.error.message}`))}
                </p>
              ) : null}
            </li>
          ))}
        </ul>
      ) : null}
      {props.itemsError ? (
        <p role="alert" className="session-import__notice">
          {tr("sessionImport.prepare.resultsError")}
        </p>
      ) : null}
      <SessionImportActionPortal target={props.footerTarget}>
      <div
        ref={footerRef}
        className="session-import__toolbar session-import__footer"
        onFocusCapture={(event) => {
          footerFocus.current = event.target instanceof HTMLButtonElement
            ? { button: event.target, batchId: status.batchId } : null;
        }}
        onBlurCapture={(event) => {
          const previous = footerFocus.current;
          if (!previous || !previous.button.isSameNode(event.target) || !previous.button.isConnected) return;
          const doc = previous.button.ownerDocument;
          const movedToControl = event.relatedTarget && event.relatedTarget !== doc.body && event.relatedTarget !== doc.documentElement;
          if (!previous.button.disabled || movedToControl) footerFocus.current = null;
        }}
      >
        {props.itemsError ? (
          <button type="button" className="btn" disabled={props.busy} onClick={props.onRetryItems}>
            {tr("sessionImport.prepare.retryResults")}
          </button>
        ) : props.nextCursor && hasUnloadedIssues ? (
          <button type="button" className="btn" disabled={props.busy} onClick={props.onLoadMore}>
            {tr("sessionImport.loadMore")}
          </button>
        ) : null}
        {props.active ? (
          <button type="button" className="btn" disabled={props.busy || !canCancel} onClick={props.onCancel}>
            {tr("sessionImport.cancel")}
          </button>
        ) : null}
        {!props.active && status.state !== "completed" ? (
          <button type="button" className="btn" disabled={props.busy} onClick={props.onRetry}>
            {tr("sessionImport.retry")}
          </button>
        ) : null}
        {!props.active ? (
          <button type="button" className="btn" disabled={props.busy} onClick={props.onBack}>
            {tr("sessionImport.back")}
          </button>
        ) : null}
        {canContinue ? (
          <button type="button" className="btn btn--primary" disabled={props.busy} onClick={props.onContinue}>
            {tr("sessionImport.prepare.continue")}
          </button>
        ) : null}
      </div>
      </SessionImportActionPortal>
    </div>
  );
}
