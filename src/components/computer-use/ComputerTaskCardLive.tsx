import { useMemo } from "react";
import { createT, type Locale } from "@/i18n";
import { IconDeviceDesktop } from "@/components/icons";
import { requestComputerPanel } from "@/lib/computer-use/panelStore";
import { taskDataState, taskStatusKey } from "@/lib/computer-use/taskPresentation";
import { useComputerTask } from "./useComputerTask";
import { useComputerTaskActions } from "./useComputerTaskActions";
import { ComputerTaskConsent } from "./ComputerTaskConsent";
import { ComputerTaskMetadata } from "./ComputerTaskMetadata";

export function ComputerTaskCardLive(props: { locale: Locale; sessionId: string | null }) {
  return <SessionTaskCard key={props.sessionId} {...props} />;
}

function SessionTaskCard({ locale, sessionId }: { locale: Locale; sessionId: string | null }) {
  const tr = useMemo(() => createT(locale), [locale]);
  const host = useComputerTask(sessionId);
  const { state } = useComputerTaskActions(sessionId);
  const view = host.view;
  if (!view || (!view.request && !view.executions.length && !view.grants.length)) return null;
  const status = taskStatusKey(host, state);
  const title = view.request?.purpose || view.executions.find((run) => run.targetName)?.targetName || tr("cu.title");
  const failure = view.executions.find((run) => run.lastFailure)?.lastFailure;
  return <section className="cu-task-card" data-state={taskDataState(status)}
    aria-label={tr("cu.title")}>
    <div className="cu-task-card__head">
      <span className="cu-panel__emblem cu-panel__emblem--small" aria-hidden><IconDeviceDesktop size={20} /></span>
      <div>
        <p className="cu-task-card__purpose">{title}</p>
        <p className="cu-task-card__state" role="status" aria-live="polite">{tr(status)}</p>
      </div>
    </div>
    <ComputerTaskConsent locale={locale} sessionId={sessionId} host={host} openWorkspace={() => requestComputerPanel()} />
    <ComputerTaskMetadata locale={locale} view={view} />
    {failure ? <p className="cu-panel__error" role="alert">{tr(failure === "outcome_unknown" ? "cu.consent.outcomeUnknown" : "cu.panel.error")}</p> : null}
  </section>;
}
