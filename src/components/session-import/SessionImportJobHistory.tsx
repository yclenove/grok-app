import { useMemo } from "react";
import { createT, intlLocale, type Locale } from "@/i18n";
import { GlassModal } from "@/components/GlassModal";
import { useSessionImportJobHistory } from "@/hooks/useSessionImportJobHistory";
import { importErrorKey } from "@/lib/sessionImport/errorCopy";
import { jobStatusKey, jobUiStatus } from "@/lib/sessionImport/jobView";

export function SessionImportJobHistory({ locale, enabled, onSelect }: {
  locale: Locale;
  enabled: boolean;
  onSelect: (id: string, isCurrent: () => boolean) => Promise<boolean>;
}) {
  const tr = useMemo(() => createT(locale), [locale]);
  const history = useSessionImportJobHistory(enabled, onSelect);
  const dateFormat = useMemo(() => new Intl.DateTimeFormat(intlLocale(locale), {
    dateStyle: "medium", timeStyle: "short",
  }), [locale]);
  return <>
    <div className="session-import__jobs-entry">
      <button type="button" className="btn btn--sm btn--ghost" disabled={!enabled} onClick={history.show}>
        {tr("sessionImport.jobs.title")}
      </button>
    </div>
    <GlassModal open={history.open} onClose={history.close} title={tr("sessionImport.jobs.title")}
      closeLabel={tr("common.close")} size="md" wrapBody className="session-import-jobs"
      footer={<>
        <button type="button" className="btn btn--ghost" disabled={history.busy} onClick={history.refresh}>{tr("sessionImport.refresh")}</button>
        <button type="button" className="btn btn--ghost" disabled={history.busy || history.page === 0} onClick={history.previous}>{tr("sessionImport.previousPage")}</button>
        <button type="button" className="btn btn--ghost" disabled={history.busy || !history.nextCursor} onClick={history.next}>{tr("sessionImport.nextPage")}</button>
        <button type="button" className="btn btn--ghost" onClick={history.close}>{tr("common.close")}</button>
      </>}>
      {history.busy ? <p role="status">{tr("sessionImport.jobs.loading")}</p> : null}
      {history.error ? <div role="alert">
        {tr(importErrorKey(history.error))}
        {history.canRetry ? <button type="button" className="btn btn--sm" disabled={history.busy} onClick={history.retry}>{tr("error.action.retry")}</button> : null}
      </div> : null}
      {!history.busy && !history.error && !history.items.length ? <p>{tr("sessionImport.jobs.empty")}</p> : null}
      <ul className="session-import__job-list" aria-busy={history.busy}>
        {history.items.map((job) => {
          const date = job.createdAt ? new Date(job.createdAt) : null;
          const when = date && Number.isFinite(date.getTime()) ? dateFormat.format(date) : tr("sessionImport.jobs.unknownDate");
          const status = tr(jobStatusKey(jobUiStatus(job.state ?? undefined)));
          return <li key={job.jobId}>
            <button type="button" className="session-import__job-row" disabled={history.busy}
              onClick={() => void history.select(job.jobId)}>
              <span>{when}</span>
              <span className="settings-row__desc">{status} · {tr("sessionImport.jobs.count", { n: job.itemCount })}</span>
            </button>
          </li>;
        })}
      </ul>
    </GlassModal>
  </>;
}
