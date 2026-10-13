import { useEffect, useId, useMemo, useRef, useState } from "react";
import { createT, intlLocale, type Locale } from "@/i18n";
import { GlassModal } from "@/components/GlassModal";
import { Select } from "@/components/Select";
import { isComputerSurface, surfaceLabelKey } from "@/lib/computer-use/surface";
import type { ComputerTaskSnapshot } from "@/lib/computer-use/taskSource";
import { taskActionErrorKey, taskCanPause, taskCanResume, taskIsCompleted, taskIsStopped } from "@/lib/computer-use/taskPresentation";
import { useComputerTaskActions } from "./useComputerTaskActions";
import { ComputerTaskExistingPreparation } from "./ComputerTaskExistingPreparation";
import { ComputerTaskUpload } from "./ComputerTaskUpload";

type Props = { locale: Locale; sessionId: string | null; host: ComputerTaskSnapshot; openWorkspace?: () => void; showDetails?: boolean };

export function ComputerTaskRequestDetails({ locale, host }: Pick<Props, "locale" | "host">) {
  const tr = useMemo(() => createT(locale), [locale]);
  const request = host.view?.request;
  if (!request || !["pending", "preparing"].includes(request.phase)) return null;
  return <>
    <p className="cu-task-card__scope">{request.scopeIntent}</p>
    <p className="cu-panel__hint">{tr("cu.consent.taskOnly", {
      seconds: new Intl.NumberFormat(intlLocale(locale)).format(Math.ceil(request.remainingMs / 1000)),
    })}</p>
    <details className="cu-panel__diag">
      <summary>{tr("cu.consent.permissions")}</summary>
      <ul className="cu-consent__permissions">
        {request.capabilities.map((cap) => <li key={cap}>{tr(cap === "observe" ? "cu.consent.observe"
          : cap === "input" ? "cu.consent.input" : "cu.consent.otherPermission")}</li>)}
        {request.surfaces.filter(isComputerSurface).map((value) => <li key={value}>{tr(surfaceLabelKey(value))}</li>)}
      </ul>
    </details>
  </>;
}

/** The same controls in chat and workspace use a single session transaction. */
export function ComputerTaskConsent({ locale, sessionId, host, openWorkspace, showDetails = true }: Props) {
  const tr = useMemo(() => createT(locale), [locale]);
  const owner = useId();
  const { state, actions } = useComputerTaskActions(sessionId);
  const [surface, setSurface] = useState("");
  const request = host.view?.request;
  const live = host.state === "live";
  const pending = !!request && ["pending", "preparing"].includes(request.phase);
  const preparing = request?.phase === "preparing" || state.pending === "approve";
  const surfaces = request?.surfaces.filter((value) => isComputerSurface(value) && host.view?.pickerSurfaces.includes(value)) ?? [];
  const selectedSurface = surfaces.includes(surface) ? surface : surfaces[0] ?? "";
  const picker = state.picker && "candidates" in state.picker ? state.picker : null;
  const ownPicker = state.choosing && state.pickerOwner === owner;
  const existingPicker = state.picker?.surface === "existing-tabs";
  const appWebviewPicker = state.picker?.surface === "app-webview";
  const requestActionKey = selectedSurface === "managed-browser"
    ? host.view?.featureEnabled ? "cu.consent.allow" : "cu.consent.enableAllow"
    : selectedSurface === "existing-tabs" ? "cu.consent.selectTabs"
      : selectedSurface === "app-webview" ? "cu.consent.selectAppPages"
        : selectedSurface === "desktop" ? "cu.consent.selectWindows" : "cu.panel.selectTarget";
  const pickerTrigger = useRef<HTMLButtonElement | null>(null);
  const pickerWasOpen = useRef(false);
  const stopped = taskIsStopped(host);
  const completed = taskIsCompleted(host);
  const terminal = !!request && ["denied", "cancelled", "expired", "retired", "completed"].includes(request.phase)
    && !host.view?.grants.some((grant) => grant.executable)
    && (host.view?.executions.every((run) => run.phase === "stopped") ?? true);
  const stopping = state.pending === "stop" || (live && host.view?.executions.some((run) => run.phase === "stop_requested"));
  const run = host.view?.executions.find((execution) => taskCanPause(host, execution));
  const resume = host.view?.executions.find((execution) => taskCanResume(host, execution));
  const showResume = state.pending === "resume" || host.view?.executions.some((execution) =>
    execution.phase === "resuming" || (execution.phase === "paused" && execution.resumeAvailable === true));
  const canStop = !!(request || host.view?.executions.length || host.view?.grants.length)
    && !stopped && !(live && terminal) && !stopping;
  useEffect(() => () => {
    const current = actions();
    if (current?.getSnapshot().pickerOwner === owner) current.closePicker();
  }, [actions, owner]);
  useEffect(() => {
    if (ownPicker) { pickerWasOpen.current = true; return; }
    if (!pickerWasOpen.current) return;
    pickerWasOpen.current = false;
    // Chromium blurs a button when it becomes disabled. Remember the actual
    // trigger before that render; the shared modal's captured body isn't it.
    const trigger = pickerTrigger.current;
    const timer = setTimeout(() => {
      if (trigger?.isConnected && !trigger.disabled && (document.activeElement === document.body || !document.activeElement?.isConnected)) trigger.focus();
    }, 0);
    return () => clearTimeout(timer);
  }, [ownPicker]);

  return <>
    {pending ? <>
      {showDetails ? <ComputerTaskRequestDetails locale={locale} host={host} /> : null}
      {surfaces.length > 1 ? <Select aria-label={tr("cu.surface.label")} value={selectedSurface}
        options={surfaces.filter(isComputerSurface).map((value) => ({ value, label: tr(surfaceLabelKey(value)) }))}
        disabled={!live || !!state.pending || state.choosing} onChange={setSurface} /> : null}
      {selectedSurface === "managed-browser" ? <p className="cu-panel__hint">{tr("cu.consent.managedScope")}</p> : null}
      {selectedSurface === "app-webview" ? <p className="cu-panel__hint">{tr("cu.consent.appWebviewScope")}</p> : null}
      {!selectedSurface ? <p className="cu-panel__warn">{tr("cu.consent.scopeUnavailable")}</p> : null}
    </> : null}
    <div className="cu-task-card__actions" role="toolbar" aria-label={tr("cu.panel.controls")}>
      {pending ? <>
        <button type="button" className="btn btn--solid"
          aria-haspopup={selectedSurface !== "managed-browser" && selectedSurface ? "dialog" : undefined}
          disabled={!live || request.phase !== "pending" || !!state.pending || state.choosing || !selectedSurface}
          onClick={(event) => { pickerTrigger.current = event.currentTarget; void actions()?.choose(selectedSurface, owner); }}>
          {tr(requestActionKey)}
        </button>
        <button type="button" className="btn btn--ghost" disabled={!live || ["deny", "cancel", "stop"].includes(state.pending ?? "")}
          onClick={() => void actions()?.decide(preparing ? "cancel" : "deny")}>
          {tr(preparing ? "common.cancel" : "cu.consent.deny")}
        </button>
      </> : completed ? null : showResume ? <button type="button" className="btn btn--solid" disabled={!resume || !!state.pending}
        onClick={() => { if (resume) void actions()?.resume(resume); }}>{tr("cu.panel.resume")}</button>
        : <button type="button" className="btn btn--ghost" disabled={!run || !!state.pending}
        onClick={() => { if (run) void actions()?.pause(run); }}>{tr("cu.panel.pause")}</button>}
      {openWorkspace ? <button type="button" className="btn btn--ghost btn--sm" onClick={openWorkspace}>{tr("cu.workspace.open")}</button> : null}
      {!completed ? <button type="button" className="btn btn--danger cu-panel__stop" disabled={!canStop}
        onClick={() => void actions()?.decide("stop")}>
        {tr(stopping ? "cu.panel.stopping" : "cu.panel.stop")}
      </button> : null}
    </div>
    <ComputerTaskUpload locale={locale} sessionId={sessionId} host={host} />
    {state.error && !state.choosing ? <p role="alert" className="cu-panel__error">{tr(taskActionErrorKey(state.error))}</p> : null}
    <GlassModal open={ownPicker} title={tr(existingPicker ? "cu.consent.selectTabs"
      : appWebviewPicker ? "cu.consent.selectAppPages" : "cu.consent.selectWindows")} closeLabel={tr("common.cancel")}
      onClose={() => actions()?.closePicker()} className="cu-consent__modal" wrapBody
      footer={<>
        <button type="button" className="btn btn--ghost" onClick={() => void actions()?.decide("cancel")}>{tr("common.cancel")}</button>
        <button type="button" className="btn btn--solid" disabled={!live || !!state.pending || state.preparationBusy || !state.selected.length}
          onClick={() => void actions()?.approve()}>{tr(host.view?.featureEnabled ? "cu.consent.allow" : "cu.consent.enableAllow")}</button>
        <button type="button" className="btn btn--danger" disabled={!!stopping}
          onClick={() => void actions()?.decide("stop")}>{tr(stopping ? "cu.panel.stopping" : "cu.panel.stop")}</button>
      </>}>
      {existingPicker && ownPicker ? <ComputerTaskExistingPreparation locale={locale} owner={owner} state={state} actions={actions} /> : null}
      {!existingPicker || picker?.candidates.length ? <p className="cu-panel__hint">{tr("cu.consent.selection", { count: state.selected.length, max: picker?.maxSelection ?? state.picker?.maxSelection ?? 8 })}</p> : null}
      {state.pending === "choose" && !existingPicker ? <p role="status">{tr("cu.status.checking")}</p> : null}
      {state.pending === "approve" ? <p role="status">{tr("cu.consent.preparing")}</p> : null}
      {picker ? <div className="cu-consent__candidates">
        {picker.candidates.map((candidate) => <button type="button" key={candidate.candidateId}
          className="cu-consent__candidate" aria-pressed={state.selected.includes(candidate.candidateId)}
          disabled={!!state.pending || state.preparationBusy || (!state.selected.includes(candidate.candidateId) && state.selected.length >= picker.maxSelection)}
          onClick={() => actions()?.toggleCandidate(candidate.candidateId)}>
          <span>{candidate.title || candidate.appName || tr("cu.panel.target")}</span>
          {candidate.appName && candidate.appName !== candidate.title ? <small>{candidate.appName}</small> : null}
        </button>)}
        {!picker.candidates.length ? <p className="cu-panel__empty">{tr(existingPicker ? "cu.consent.existingEmpty"
          : appWebviewPicker ? "cu.consent.appWebviewEmpty" : "cu.workspace.noTargets")}</p> : null}
      </div> : null}
      {picker?.truncated ? <p className="cu-panel__warn">{tr("cu.panel.truncated")}</p> : null}
      {state.error ? <p role="alert" className="cu-panel__error">{tr(state.error === "preparation" ? "cu.consent.existingFailed" : state.error === "picker" ? "cu.consent.pickerFailed" : "cu.panel.error")}</p> : null}
    </GlassModal>
  </>;
}
