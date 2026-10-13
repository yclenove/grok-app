import { useMemo } from "react";
import { createT, type Locale } from "@/i18n";
import type { ComputerTaskSnapshot } from "@/lib/computer-use/taskSource";
import { useComputerTaskUpload } from "./useComputerTaskUpload";

/** Same task-local consent in chat and workspace; never a model-provided path. */
export function ComputerTaskUpload({ locale, sessionId, host }: {
  locale: Locale; sessionId: string | null; host: ComputerTaskSnapshot;
}) {
  const tr = useMemo(() => createT(locale), [locale]);
  const { state, actions } = useComputerTaskUpload(sessionId);
  const pending = host.view?.executions.filter(run => run.upload) ?? [];
  const live = host.state === "live" && !!host.view?.featureEnabled;
  const resultKey = state.result === "assigned" ? "cu.upload.assigned" : state.result === "cancelled"
    ? "cu.upload.cancelled" : state.result === "unknown" ? "cu.consent.outcomeUnknown" : "cu.upload.failed";
  if (!pending.length && !state.pending && !state.result) return null;
  return <div className="cu-upload">
    {pending.map(run => {
      const upload = run.upload!;
      const awaiting = upload.phase === "awaiting_file";
      const title = tr("cu.upload.dialog", { name: upload.targetName.replace(/[\p{Cc}]/gu, " ").slice(0, 100) });
      return <div key={`${run.runId}:${upload.requestId}`} className="cu-upload__request">
        <p className="cu-task-card__scope">{tr("cu.upload.destination", { name: upload.targetName })}</p>
        {upload.fieldName ? <p className="cu-panel__hint">{tr("cu.upload.field", { name: upload.fieldName })}</p> : null}
        <p className="cu-panel__hint" role="status">{tr(awaiting ? "cu.upload.limit"
          : upload.phase === "selecting" ? "cu.upload.selecting" : "cu.upload.assigning")}</p>
        <div className="cu-task-card__actions">
          {awaiting ? <button type="button" className="btn btn--solid" disabled={!live || state.pending || state.cancelling}
            onClick={() => void actions()?.choose(run, title)}>{tr("cu.upload.choose")}</button> : null}
          {upload.phase !== "assigning" ? <button type="button" className="btn btn--ghost"
            disabled={!live || state.cancelling} onClick={() => void actions()?.cancel(run)}>{tr("common.cancel")}</button> : null}
        </div>
      </div>;
    })}
    {state.result && (!pending.length || state.result === "failed" || state.result === "unknown") ? <p className={state.result === "failed" || state.result === "unknown" ? "cu-panel__error" : "cu-panel__hint"}
      role={state.result === "failed" || state.result === "unknown" ? "alert" : "status"}>{tr(resultKey)}</p> : null}
    {state.pending && !pending.some(run => run.upload?.phase === "assigning")
      ? <p className="cu-panel__hint" role="status">{tr("cu.upload.closePicker")}</p> : null}
  </div>;
}
