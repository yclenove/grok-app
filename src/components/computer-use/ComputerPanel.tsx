import { useMemo, useState } from "react";
import { createT, type Locale } from "@/i18n";
import { IconDeviceDesktop } from "@/components/icons";
import { reconnectTaskSession } from "@/lib/computer-use/taskSource";
import { taskCanPause, taskDataState, taskStatusKey } from "@/lib/computer-use/taskPresentation";
import { isComputerSurface, surfaceLabelKey, type ComputerSurface } from "@/lib/computer-use/surface";
import { sameComputerTaskKey } from "@/lib/computer-use/taskActions";
import { useComputerTask } from "./useComputerTask";
import { useComputerTaskActions } from "./useComputerTaskActions";
import { useComputerTaskPreview } from "./useComputerTaskPreview";
import { ComputerTaskConsent, ComputerTaskRequestDetails } from "./ComputerTaskConsent";
import { ComputerTaskMetadata } from "./ComputerTaskMetadata";

export type ComputerPanelProps = { locale: Locale; sessionId: string | null; runId: string | null; surface?: ComputerSurface };

export function ComputerPanel(props: ComputerPanelProps) {
  return <TaskPanel key={props.sessionId} {...props} />;
}

function TaskPanel({ locale, sessionId, runId }: ComputerPanelProps) {
  const tr = useMemo(() => createT(locale), [locale]);
  const host = useComputerTask(sessionId);
  const { state } = useComputerTaskActions(sessionId);
  const [previewHidden, setPreviewHidden] = useState(false);
  const view = host.view;
  const run = view?.executions.find((execution) => runId ? execution.runId === runId : execution.phase !== "stopped") ?? null;
  const previewActive = !!run && taskCanPause(host, run) && !previewHidden && !state.pending
    && !!view?.grants.some((grant) => grant.executable && grant.runId === run.runId && sameComputerTaskKey(grant.key, run.key) && grant.capabilities.includes("observe"));
  const { frame, error, stale } = useComputerTaskPreview(sessionId, run, host.owner?.hostInstanceId ?? null, previewActive);
  const status = taskStatusKey(host, state);
  const hasTask = !!(view?.request || view?.executions.length || view?.grants.length);
  const failure = view?.executions.find((execution) => execution.lastFailure)?.lastFailure;
  return <section className="cu-panel" id="computer-panel" aria-label={tr("cu.title")}
    data-state={taskDataState(status)}>
    <header className="cu-panel__head">
      <span className="cu-panel__emblem cu-panel__emblem--small" aria-hidden><IconDeviceDesktop size={20} /></span>
      <div className="cu-panel__heading"><h2>{tr("cu.panel.title")}</h2>
        {view?.request ? <p className="cu-task-card__purpose">{view.request.purpose}</p> : null}
      </div>
    </header>
    <p className="cu-panel__status" role="status" aria-live="polite">
      {tr(!sessionId ? "cu.panel.noSession" : hasTask ? status : host.state === "connecting" ? "cu.status.checking"
        : host.state === "live" ? "cu.consent.idle" : "cu.task.unavailable")}
    </p>
    <div className="cu-panel__body">
      {hasTask ? <>
        {view?.request && ["pending", "preparing"].includes(view.request.phase) ? <section className="cu-panel__card">
          <ComputerTaskRequestDetails locale={locale} host={host} />
        </section> : null}
        {view?.grants.length ? <section className="cu-panel__card">
          <h3>{tr(view?.grants.some((grant) => grant.executable) ? "cu.consent.allowedScope" : "cu.panel.target")}</h3>
          <ul className="cu-consent__permissions">
            {view.grants.flatMap((grant) => grant.members.map((member) => <li key={JSON.stringify([grant.key, member.targetId])}>
              {view.executions.find((execution) => execution.target?.targetId === member.targetId && sameComputerTaskKey(execution.key, grant.key))?.targetName
                || member.displayName
                || tr(isComputerSurface(member.surface) ? surfaceLabelKey(member.surface) : "cu.panel.target")}
            </li>))}
          </ul>
        </section> : null}
        {run?.target ? <section className="cu-panel__preview-section" aria-label={tr("cu.workspace.preview")}>
          <div className="cu-panel__section-head"><h3>{tr("cu.workspace.preview")}</h3>
            <button type="button" className="btn btn--ghost btn--sm" onClick={() => setPreviewHidden((value) => !value)}>
              {tr(previewHidden ? "cu.panel.showPreview" : "cu.panel.hidePreview")}
            </button>
          </div>
          <div className="cu-panel__preview" aria-busy={previewActive && !frame}>
            {frame?.previewDataUrl ? <div className="cu-panel__frame"><img src={frame.previewDataUrl} alt={tr("cu.panel.previewLabel")} /></div>
              : <p className="cu-panel__empty">{tr(previewHidden ? "cu.panel.previewHidden" : previewActive ? "cu.workspace.waitingPreview" : "cu.workspace.previewInactive")}</p>}
          </div>
          {frame && stale ? <p className="cu-panel__warn">{tr("cu.panel.staleFrame")}</p> : null}
          {frame && !frame.previewDataUrl ? <p className="cu-panel__warn">{tr("cu.panel.noVision")}</p> : null}
          {frame?.truncated ? <p className="cu-panel__warn">{tr("cu.panel.truncated")}</p> : null}
          {error ? <p className="cu-panel__error" role="alert">{tr("cu.panel.error")}</p> : null}
        </section> : null}
        {failure ? <p className="cu-panel__error" role="alert">{tr(failure === "outcome_unknown" ? "cu.consent.outcomeUnknown" : "cu.panel.error")}</p> : null}
        {view ? <ComputerTaskMetadata locale={locale} view={view} /> : null}
      </> : null}
      {sessionId && host.state !== "live" ? <button type="button" className="btn btn--ghost"
        onClick={() => reconnectTaskSession(sessionId)}>{tr("cu.consent.refreshStatus")}</button> : null}
    </div>
    {hasTask ? <footer className="cu-panel__footer">
      <ComputerTaskConsent locale={locale} sessionId={sessionId} host={host} showDetails={false} />
    </footer> : null}
  </section>;
}
