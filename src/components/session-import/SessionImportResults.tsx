import { useEffect, useId, useRef, useState } from "react";
import type { JobItem, JobRecord } from "@/lib/api/sessionMigration";
import { isDialogFocusAllowed } from "@/lib/a11yFocus";
import { importErrorKey } from "@/lib/sessionImport/errorCopy";
import { groupJobItems, jobCanRetry, jobProgress, jobStatusKey, jobUiStatus, undoConflictReasonKey } from "@/lib/sessionImport/jobView";
import { outcomeLabel, type SessionImportTFn } from "./sessionImportHostLabels";
import { SessionImportActionPortal } from "./SessionImportActionPortal";

type Props = {
  tr: SessionImportTFn;
  job: JobRecord;
  titleByCandidate: ReadonlyMap<string, string>;
  active: boolean;
  busy: boolean;
  itemsStale: boolean;
  hasPrevious: boolean;
  onPrevious: () => void;
  onNext: () => void;
  onRefresh: () => void;
  onCancel: () => void;
  onRetry: () => void;
  onUndo: () => void;
  onDone: () => void;
  onOpen: (id: string) => void;
  onContinue: (id: string) => void;
  onExport: (id: string) => Promise<void>;
  footerTarget?: HTMLElement | null;
};

function itemStatus(item: JobItem, tr: SessionImportTFn): string {
  switch (item.phase) {
    case "cancelled": return tr("sessionImport.item.cancelled");
    case "undone": return tr("sessionImport.item.undone");
    case "conflict": return tr("sessionImport.item.notUndone");
    case "recovery_needed": return tr("sessionImport.item.recovery");
    case "failed": return tr("sessionImport.job.failed");
    case "queued": return tr("sessionImport.job.queued");
    case "committed":
      return item.outcome === "deduplicated" ? tr("sessionImport.item.deduplicated")
        : !item.outcome || ["complete", "partial", "unsupported"].includes(item.outcome)
          ? outcomeLabel(item.outcome ?? "complete", tr) : tr("sessionImport.item.unknown");
    case "running":
    case "prepared":
    case "publishing":
    case "published": return tr("sessionImport.job.running");
    default: return tr("sessionImport.item.unknown");
  }
}

/** One bounded Host page; group counts never masquerade as whole-job totals. */
export function SessionImportResults(props: Props) {
  const { tr, job } = props;
  const id = useId();
  const groups = groupJobItems(job.items);
  const progress = jobProgress(job);
  const status = jobUiStatus(job.state);
  const [confirmation, setConfirmation] = useState<{ version: number; action: "retry" | "undo" } | null>(null);
  const [exporting, setExporting] = useState<string | null>(null);
  const [notice, setNotice] = useState<"copied" | "failed" | null>(null);
  const exportFlight = useRef(false);
  const confirmButton = useRef<HTMLButtonElement>(null);
  const retryButton = useRef<HTMLButtonElement>(null);
  const undoButton = useRef<HTMLButtonElement>(null);
  const doneButton = useRef<HTMLButtonElement>(null);
  const previousArmed = useRef<"retry" | "undo" | null>(null);
  const armed = confirmation?.version === job.stateVersion ? confirmation.action : null;
  useEffect(() => {
    if (armed) {
      const target = confirmButton.current;
      if (target && isDialogFocusAllowed(target)) target.focus();
    }
    else if (previousArmed.current) {
      const source = previousArmed.current === "retry" ? retryButton.current : undoButton.current;
      const target = source && !source.disabled ? source : doneButton.current;
      const doc = target?.ownerDocument;
      const active = doc?.activeElement;
      // A job update can disarm while the user is reading or refreshing. Only
      // recover focus lost with the removed confirmation, never another layer.
      const focusLost = !active || active === doc?.body || active === doc?.documentElement;
      if (target && !target.disabled && focusLost && isDialogFocusAllowed(target)) target.focus();
    }
    previousArmed.current = armed;
  }, [armed]);
  const busy = props.busy || exporting !== null;
  const rowBusy = busy || props.itemsStale;
  const exportItem = async (importId: string) => {
    if (exportFlight.current || rowBusy) return;
    exportFlight.current = true;
    setExporting(importId);
    setNotice(null);
    try {
      await props.onExport(importId);
      setNotice("copied");
    } catch {
      // Clipboard/Host errors may contain paths; never leak them into result rows.
      setNotice("failed");
    } finally {
      exportFlight.current = false;
      setExporting(null);
    }
  };

  return (
    <div className="session-import__results">
      <div className="session-import__status-card" data-state={status}>
        <p role="status">{tr(jobStatusKey(status))}</p>
        <p>{progress.total == null
          ? tr("sessionImport.jobUnknownTotal", { done: progress.done })
          : tr("sessionImport.jobProgress", { done: progress.done, total: progress.total })}</p>
        {progress.total != null ? (
          <div className="session-import__progress" role="progressbar" aria-label={tr("sessionImport.step.import")}
            aria-valuemin={0} aria-valuemax={progress.total} aria-valuenow={Math.min(progress.done, progress.total)} data-state={status}>
            <span style={{ width: `${Math.min(100, progress.done / progress.total * 100)}%` }} />
          </div>
        ) : null}
      </div>
      {notice ? <p role={notice === "failed" ? "alert" : "status"} className="session-import__notice">
        {tr(notice === "failed" ? "sessionImport.results.copyFailed" : "sessionImport.copiedExport")}
      </p> : null}
      <p className="session-import__result-page-note">{tr("sessionImport.results.pageSummary", { n: job.items.length })}</p>
      {props.itemsStale ? <p role="status" className="session-import__notice">{tr("sessionImport.results.stale")}</p> : null}
      <div className="session-import__result-groups" aria-busy={props.itemsStale}>
        {(["failed", "partial", "pending", "stopped", "complete"] as const).map((group) => {
          const rows = groups[group];
          if (!rows.length) return null;
          const headingId = `${id}-${group}`;
          return (
            <section key={group} className="session-import__result-group" aria-labelledby={headingId} data-result-group={group}>
              <header className="session-import__result-group-head">
                <h3 id={headingId}>{tr(`sessionImport.outcomeGroup.${group}`)}</h3>
                <span className="session-import__result-count">{rows.length}</span>
              </header>
              <ul className="session-import__list session-import__result-list">
                {rows.map((item) => (
                  <li key={item.candidateId} className="tree-imported__row" data-group={group}>
                    <div className="session-import__result-heading">
                      <strong>{item.title || props.titleByCandidate.get(item.candidateId) || item.importId || item.candidateId}</strong>
                      <span className="session-import__result-badge">{itemStatus(item, tr)}</span>
                    </div>
                    {item.error ? <p className="session-import__notice" data-issue="unsupported">
                      {tr((item.phase === "conflict" && undoConflictReasonKey(item.error)) || importErrorKey(item.error))}</p> : null}
                    {item.importId && item.phase === "committed" ? (
                      <div className="tree-imported__actions">
                        <button type="button" className="btn btn--sm btn--primary" disabled={rowBusy} onClick={() => props.onOpen(item.importId!)}>{tr("sessionImport.openReader")}</button>
                        <button type="button" className="btn btn--sm" disabled={rowBusy} onClick={() => props.onContinue(item.importId!)}>{tr("sessionImport.continueInGrok")}</button>
                        <button type="button" className="btn btn--sm btn--ghost" disabled={rowBusy} aria-busy={exporting === item.importId} onClick={() => void exportItem(item.importId!)}>{tr("sessionImport.exportMarkdown")}</button>
                      </div>
                    ) : null}
                  </li>
                ))}
              </ul>
            </section>
          );
        })}
        {!job.items.length ? <p className="session-import__empty">{tr("sessionImport.results.empty")}</p> : null}
      </div>
      <nav className="session-import__toolbar session-import__result-pager" aria-label={tr("sessionImport.step.result")}>
        <button type="button" className="btn" disabled={busy || !props.hasPrevious} onClick={props.onPrevious}>{tr("sessionImport.previousPage")}</button>
        <button type="button" className="btn" disabled={busy || !job.nextItemCursor} onClick={props.onNext}>{tr("sessionImport.nextPage")}</button>
        <button type="button" className="btn btn--ghost" disabled={busy} onClick={props.onRefresh}>{tr("sessionImport.refresh")}</button>
      </nav>
      <SessionImportActionPortal target={props.footerTarget}>
      <div className="session-import__toolbar session-import__footer session-import__result-footer">
        {props.active ? (
          <button type="button" className="btn" disabled={busy || status === "cancelling"} onClick={props.onCancel}>{tr("sessionImport.cancel")}</button>
        ) : (
          <>
            {armed ? (
              <div className="session-import__result-confirm" role="group" aria-label={tr(armed === "retry" ? "sessionImport.confirmRetry" : "sessionImport.confirmUndo")} onKeyDown={(event) => {
                if (event.key === "Escape" && !event.defaultPrevented &&
                    !event.nativeEvent.isComposing && event.nativeEvent.keyCode !== 229) {
                  event.preventDefault(); event.stopPropagation(); setConfirmation(null);
                }
              }}>
                <button ref={confirmButton} type="button" className={`btn ${armed === "undo" ? "btn--danger" : "btn--primary"}`} disabled={busy || props.itemsStale} onClick={() => {
                  setConfirmation(null);
                  if (armed === "retry") props.onRetry(); else props.onUndo();
                }}>{tr(armed === "retry" ? "sessionImport.confirmRetry" : "sessionImport.confirmUndo")}</button>
                <button type="button" className="btn btn--ghost" disabled={busy} onClick={() => setConfirmation(null)}>{tr("common.cancel")}</button>
              </div>
            ) : (
              <>
                {jobCanRetry(job) ? <button ref={retryButton} type="button" className="btn" disabled={busy || props.itemsStale} onClick={() => setConfirmation({ version: job.stateVersion, action: "retry" })}>{tr("sessionImport.retry")}</button> : null}
                <button ref={undoButton} type="button" className="btn btn--ghost" disabled={busy || props.itemsStale} onClick={() => setConfirmation({ version: job.stateVersion, action: "undo" })}>{tr("sessionImport.undo")}</button>
              </>
            )}
            <button ref={doneButton} type="button" className="btn btn--primary session-import__result-done" disabled={busy} onClick={props.onDone}>{tr("sessionImport.done")}</button>
          </>
        )}
      </div>
      </SessionImportActionPortal>
    </div>
  );
}
