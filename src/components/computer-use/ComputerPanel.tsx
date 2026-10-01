import { useEffect, useMemo, useRef, useState } from "react";
import { createT, type Locale } from "@/i18n";
import { Select } from "@/components/Select";
import { IconDeviceDesktop, IconRefresh, IconShieldCheck } from "@/components/icons";
import { computerUseErrorKey } from "@/lib/computer-use/errors";
import { requestComputerPanel } from "@/lib/computer-use/panelStore";
import { buildSettingsHash } from "@/lib/settingsCatalog";
import { computerStateLabel, computerTaskState } from "@/lib/computer-use/taskCard";
import { COMPUTER_SURFACES, surfaceLabelKey, type ComputerSurface } from "@/lib/computer-use/surface";
import { useComputerController } from "./useComputerController";
import { useComputerPreview } from "./useComputerPreview";
import { ComputerPairing } from "./ComputerPairing";
import { ComputerDiagnostics } from "./ComputerDiagnostics";

export type ComputerPanelProps = {
  locale: Locale;
  sessionId: string | null;
  runId: string | null;
  surface?: ComputerSurface;
};

export function ComputerPanel(props: ComputerPanelProps) {
  const surface = props.surface ?? "desktop";
  return <ComputerPanelSession key={JSON.stringify([props.sessionId, props.runId, surface])} {...props} surface={surface} />;
}

function ComputerPanelSession({ locale, sessionId, runId, surface = "desktop" }: ComputerPanelProps) {
  const tr = useMemo(() => createT(locale), [locale]);
  const c = useComputerController(sessionId, runId, surface);
  const { status } = c;
  const [previewHidden, setPreviewHidden] = useState(false);
  const [choosing, setChoosing] = useState(false);
  const [focusAfterConsent, setFocusAfterConsent] = useState<HTMLElement | null>(null);
  const previewRef = useRef<HTMLElement>(null);
  const settling = c.stopping || c.cleanupPending || status?.stopState === "stop_requested";
  const showSetup = !settling && (!status?.runId || !status.enabled || choosing || c.authorizing);
  useEffect(() => {
    if (showSetup || !focusAfterConsent) return;
    if (document.activeElement === focusAfterConsent || (!focusAfterConsent.isConnected && document.activeElement === document.body)) {
      previewRef.current?.focus();
    }
    setFocusAfterConsent(null);
  }, [showSetup, focusAfterConsent]);
  const previewActive = c.fresh && !!status?.enabled && status.stopState === "running"
    && !status.paused && !previewHidden && !c.busy && !settling;
  // A candidate is never used as a preview identity, even during authorization.
  const { frame, error: previewError, stale } = useComputerPreview(
    sessionId, c.activeRun, status?.targetId ?? "", previewActive,
  );
  const candidate = c.targets.find((target) => target.targetId === c.candidateId);
  const options = c.targets.map((target) => ({
    value: target.targetId, label: target.title || target.appName || target.targetId,
  }));
  const phase = c.stopping ? "stop_requested" : c.authorizing ? "authorizing"
    : status ? computerTaskState(status, c.fresh) : "unknown";
  const stateLabel = c.retryingCleanup ? tr("cu.panel.retryingCleanup")
    : phase === "authorizing" ? tr("cu.panel.authorizing")
    : c.cleanupPending && status?.stopState === "stopped" && phase !== "stop_requested" && c.fresh ? tr("cu.panel.cleanupPending")
    : !status?.runId && c.fresh ? tr("cu.status.selecting")
    : tr(computerStateLabel[phase]);
  const targetName = status?.targetName || status?.targetId;
  // An idle status reports the Desktop adapter. It cannot establish whether
  // an independently selected browser or WebView surface is available.
  const desktopUnavailable = surface === "desktop" && status && !status.runId
    && (!status.backend.trim() || ["none", "unavailable"].includes(status.backend.toLowerCase()));
  const initialMessage = !sessionId ? "cu.panel.noSession" : !status
    ? c.fresh ? "cu.status.checking" : "cu.workspace.unknown"
    : !status.featureEnabled && !status.runId ? "cu.flag.off" : null;

  if (initialMessage) {
    return (
      <section className="cu-panel cu-panel--setup" aria-label={tr("cu.title")}
        data-state={!status && c.fresh ? "checking" : phase}>
        <div className="cu-panel__body">
          <div className="cu-panel__welcome">
            <span className="cu-panel__emblem" aria-hidden><IconDeviceDesktop size={28} /></span>
            <h2>{tr("cu.title")}</h2>
            <p role="status">{tr(initialMessage)}</p>
            {status && !status.featureEnabled ? <p className="muted">{tr("cu.flag.offHint")}</p> : null}
            {initialMessage === "cu.flag.off" ? (
              <a className="btn btn--solid" href={buildSettingsHash({ section: "extensions", tab: "computer" })}>
                {tr("sidebar.settings")}
              </a>
            ) : null}
            {c.error ? <p role="alert" className="cu-panel__error">{tr(computerUseErrorKey(c.error))}</p> : null}
          </div>
        </div>
        {sessionId && !status && (c.activeRun || !c.fresh) ? <footer className="cu-panel__footer">
          <div className="cu-panel__actions" role="toolbar" aria-label={tr("cu.panel.controls")}>
            <button type="button" className="btn btn--danger cu-panel__stop" disabled={c.stopping}
              onClick={() => void c.stop()}>{tr(c.stopping ? "cu.panel.stopping" : "cu.panel.stop")}</button>
          </div>
        </footer> : null}
      </section>
    );
  }
  if (!status) return null;
  const canControl = c.fresh && status.enabled && status.targetAlive
    && status.stopState === "running" && !c.controlsLocked;
  const canStop = !c.stopping && (!c.fresh || status.stopState !== "stop_requested")
    && (c.authorizing || c.busy || status.stopState === "running" || (!c.fresh && !!c.activeRun));

  return (
    <section className="cu-panel" id="computer-panel" aria-label={tr("cu.title")} data-state={phase}>
      <header className="cu-panel__head">
        <span className="cu-panel__emblem cu-panel__emblem--small" aria-hidden><IconDeviceDesktop size={20} /></span>
        <div className="cu-panel__heading">
          <h2>{tr("cu.panel.title")}</h2>
          {targetName ? <p className="cu-panel__current" title={targetName}>{targetName}</p> : null}
        </div>
        <span className="cu-panel__state-dot" data-state={phase} aria-hidden />
      </header>
      <p className="cu-panel__status" data-state={phase} role="status" aria-live="polite">{stateLabel}</p>

      <div className="cu-panel__body">
        {showSetup ? <section className="cu-panel__card" aria-label={tr("cu.workspace.chooseTarget")}>
          <div className="cu-panel__section-head">
            <h3>{tr("cu.workspace.chooseTarget")}</h3>
            {choosing && status.enabled ? <button type="button" className="btn btn--ghost btn--sm"
              disabled={c.controlsLocked} onClick={() => setChoosing(false)}>{tr("common.cancel")}</button> : null}
            {!c.usesSystemPicker ? <button type="button" className="btn btn--ghost btn--sm cu-panel__refresh"
              aria-label={tr("cu.panel.refreshTargets")} title={tr("cu.panel.refreshTargets")}
              disabled={c.controlsLocked || c.loadingTargets} onClick={c.refreshTargets}>
              <IconRefresh size={16} />
            </button> : null}
          </div>
          <div className="cu-panel__fields" aria-busy={c.loadingTargets}>
            <label className="cu-panel__label">
              {tr("cu.surface.label")}
              <Select aria-label={tr("cu.surface.label")} value={surface}
                options={COMPUTER_SURFACES.map((item) => ({ value: item, label: tr(surfaceLabelKey(item)) }))}
                onChange={(value) => { if (value && value !== surface) requestComputerPanel(value as ComputerSurface); }}
                disabled={c.controlsLocked} />
            </label>
            {!c.usesSystemPicker ? <label className="cu-panel__label">
              {tr("cu.panel.target")}
              <Select aria-label={tr(surface === "app-webview" ? "cu.panel.selectWebview" : "cu.panel.selectTarget")}
                value={c.candidateId}
                options={[{ value: "", label: tr("cu.panel.selectTarget"), disabled: true }, ...options]}
                onChange={c.setCandidateId} disabled={c.controlsLocked || c.loadingTargets || !!c.targetError} />
            </label> : null}
          </div>
          {desktopUnavailable ? <div className="cu-panel__availability">
            <p className="cu-panel__warn">{tr("cu.panel.backendUnavailable")}</p>
            <a className="btn btn--ghost btn--sm" href={buildSettingsHash({ section: "extensions", tab: "computer" })}>
              {tr("sidebar.settings")}
            </a>
          </div> : null}
          {c.targetError ? <p role="alert" className="cu-panel__error">{tr("cu.panel.targetsUnavailable")}</p>
            : c.loadingTargets ? <p role="status" className="cu-panel__hint">{tr("cu.status.checking")}</p>
            : !c.usesSystemPicker && c.targets.length === 0 ? <p className="cu-panel__empty">{tr("cu.workspace.noTargets")}</p> : null}
          {c.usesSystemPicker ? <div className="cu-panel__consent">
            <IconShieldCheck size={18} aria-hidden />
            <div>
              <p>{tr("cu.workspace.systemScope")}</p>
              <p className="cu-panel__hint">{tr("cu.workspace.systemPickerHint")}</p>
            </div>
          </div> : candidate ? (
            <div className="cu-panel__consent">
              <IconShieldCheck size={18} aria-hidden />
              <div>
                <strong title={candidate.title || candidate.targetId}>{candidate.title || candidate.targetId}</strong>
                <p>{tr("cu.workspace.scope")}</p>
              </div>
            </div>
          ) : !c.loadingTargets && !c.targetError ? <p className="cu-panel__hint">{tr("cu.workspace.selectHint")}</p> : null}
          <button type="button" className="btn btn--solid cu-panel__authorize"
            disabled={c.controlsLocked || c.loadingTargets || !!c.targetError || (!c.usesSystemPicker && !candidate)}
            onClick={(event) => {
              const trigger = event.currentTarget;
              void c.authorize().then((authorized) => {
                if (authorized) { setChoosing(false); setFocusAfterConsent(trigger); }
              });
            }}>
            {tr(c.authorizing ? "cu.panel.authorizing" : c.usesSystemPicker ? "cu.workspace.systemPicker" : "cu.workspace.authorize")}
          </button>
          {surface === "app-webview" && status.targetId ? (
            <button type="button" className="btn btn--ghost btn--sm" disabled={c.controlsLocked}
              onClick={() => void c.runCommand("unbind")}>{tr("cu.panel.unbindWebview")}</button>
          ) : null}
        </section> : !settling ? <button type="button" className="btn btn--ghost cu-panel__change-target"
          disabled={c.controlsLocked} onClick={() => setChoosing(true)}>{tr("cu.workspace.chooseTarget")}</button> : null}

        {surface === "existing-tabs" ? (
          <ComputerPairing locale={locale} disabled={c.controlsLocked} onChanged={c.refreshTargets} />
        ) : null}

        {status.runId && status.targetId ? (
          <section ref={previewRef} tabIndex={-1} className="cu-panel__preview-section" aria-label={tr("cu.workspace.preview")}>
            <div className="cu-panel__section-head">
              <h3>{tr("cu.workspace.preview")}</h3>
              <button type="button" className="btn btn--ghost btn--sm" onClick={() => setPreviewHidden((value) => !value)}>
                {tr(previewHidden ? "cu.panel.showPreview" : "cu.panel.hidePreview")}
              </button>
            </div>
            <div className="cu-panel__preview" aria-busy={previewActive && !frame?.previewDataUrl}>
              {frame?.previewDataUrl && !previewHidden ? (
                <div className="cu-panel__frame"><img src={frame.previewDataUrl} alt={tr("cu.panel.previewLabel")} /></div>
              ) : (
                <p className="cu-panel__empty">{tr(previewHidden ? "cu.panel.previewHidden"
                  : previewActive ? "cu.workspace.waitingPreview" : "cu.workspace.previewInactive")}</p>
              )}
            </div>
            {frame?.previewDataUrl && stale && !previewHidden ? <p className="cu-panel__warn">{tr("cu.panel.staleFrame")}</p> : null}
            {frame?.snapshotId && !frame.previewDataUrl && !previewHidden ? <p className="cu-panel__warn">{tr("cu.panel.noVision")}</p> : null}
            {frame?.truncated && !previewHidden ? <p className="cu-panel__warn">{tr("cu.panel.truncated")}</p> : null}
          </section>
        ) : null}

        {c.cleanupPending && (c.cleanupRetryError || status.mcpCatalog?.lastError) ? (
          <p role="alert" className="cu-panel__error">{tr("cu.panel.cleanupError", {
            error: c.cleanupRetryError || status.mcpCatalog?.lastError || "",
          })}</p>
        ) : null}
        {(c.error || previewError) && !c.cleanupPending ? (
          <p role="alert" className="cu-panel__error">{tr(computerUseErrorKey(c.error || previewError || ""))}</p>
        ) : null}
        <ComputerDiagnostics locale={locale} status={status} />
      </div>

      <footer className="cu-panel__footer">
        <div className="cu-panel__actions" role="toolbar" aria-label={tr("cu.panel.controls")}>
          {c.cleanupPending ? (
            <button type="button" className="btn btn--ghost" disabled={c.retryingCleanup || c.stopping || c.authorizing || c.busy}
              onClick={() => void c.retryCleanup()}>{tr(c.retryingCleanup ? "cu.panel.retryingCleanup" : "cu.panel.retryCleanup")}</button>
          ) : (
            <>
              <button type="button" className="btn btn--ghost"
                disabled={c.controlsLocked || status.stopState !== "running" || (!status.paused && !canControl)}
                onClick={() => void c.runCommand(status.paused ? "resume" : "pause")}>
                {tr(status.paused ? "cu.panel.resume" : "cu.panel.pause")}
              </button>
              <button type="button" className="btn btn--ghost" disabled={!canControl || status.paused}
                onClick={() => void c.runCommand("takeover")}>{tr("cu.panel.takeover")}</button>
            </>
          )}
          <button type="button" className="btn btn--danger cu-panel__stop" disabled={!canStop}
            onClick={() => void c.stop()}>{tr(c.stopping || status.stopState === "stop_requested" ? "cu.panel.stopping" : "cu.panel.stop")}</button>
        </div>
      </footer>
    </section>
  );
}
