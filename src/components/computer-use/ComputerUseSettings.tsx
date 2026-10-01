import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { createT, type Locale, type MessageKey } from "@/i18n";
import { GlassModal } from "@/components/GlassModal";
import { UiSwitch } from "@/components/settings/shared";
import { computerUseErrorKey } from "@/lib/computer-use/errors";
import * as api from "@/lib/api/computerUse";
import { ComputerRuntimeHealth } from "./ComputerRuntimeHealth";
import { ComputerGnomeHelper } from "./ComputerGnomeHelper";

type CleanupKind = "traces" | "staging" | "profiles";
// A read-only presentation deadline, never an action cancellation or release of a mutation lock.
const SETTINGS_READ_TIMEOUT_MS = 10_000;
const CLEANUP: Record<CleanupKind, { title: MessageKey; hint: MessageKey; run: () => Promise<void> }> = {
  traces: { title: "cu.settings.clearTraces", hint: "cu.settings.clearTracesHint", run: api.computerClearTraces },
  staging: { title: "cu.settings.clearStaging", hint: "cu.settings.clearStagingHint", run: api.computerClearStaging },
  profiles: { title: "cu.settings.clearProfiles", hint: "cu.settings.clearProfilesHint", run: api.computerClearManagedProfiles },
};

export function ComputerUseSettings({ locale }: { locale: Locale }) {
  const tr = useMemo(() => createT(locale), [locale]);
  const [on, setOn] = useState(false);
  const [loaded, setLoaded] = useState(false);
  const [busy, setBusy] = useState(false);
  const [runtimeBusy, setRuntimeBusy] = useState(false);
  const [loading, setLoading] = useState(true);
  const [loadFailed, setLoadFailed] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<{ path?: string } | null>(null);
  const [runtime, setRuntime] = useState<api.ComputerRuntimeStatus | null>(null);
  const [pendingCleanup, setPendingCleanup] = useState<CleanupKind | null>(null);
  const live = useRef(true);
  const inFlight = useRef(false);
  const readAttempt = useRef<{ id: symbol; timer: ReturnType<typeof setTimeout> } | null>(null);
  const load = useCallback(async (clearError = true) => {
    if (inFlight.current || readAttempt.current) return;
    const id = Symbol("settings read");
    setLoading(true);
    setLoaded(false);
    setRuntime(null);
    setLoadFailed(false);
    if (clearError) setError(null);
    const finishRead = () => {
      if (!live.current || readAttempt.current?.id !== id) return false;
      clearTimeout(readAttempt.current.timer);
      readAttempt.current = null;
      setLoading(false);
      return true;
    };
    readAttempt.current = { id, timer: setTimeout(() => {
      if (finishRead()) setLoadFailed(true);
    }, SETTINGS_READ_TIMEOUT_MS) };
    try {
      const [next, nextRuntime] = await Promise.all([
        api.computerStatus({ sessionId: null, runId: null }), api.computerRuntimeStatus(),
      ]);
      if (!finishRead()) return;
      setOn(next.featureEnabled);
      setLoaded(true);
      setRuntime(nextRuntime);
    } catch {
      if (finishRead()) setLoadFailed(true);
    }
  }, []);
  useEffect(() => {
    live.current = true;
    void load();
    return () => {
      live.current = false;
      if (readAttempt.current) clearTimeout(readAttempt.current.timer);
      readAttempt.current = null;
    };
  }, [load]);

  async function perform(action: () => Promise<void>, refreshAfter = false) {
    if (inFlight.current || readAttempt.current) return;
    inFlight.current = true;
    setBusy(true);
    setError(null);
    setNotice(null);
    if (refreshAfter) { setRuntime(null); setRuntimeBusy(true); }
    try {
      await action();
    } catch (cause) {
      if (live.current) setError(String(cause));
    } finally {
      inFlight.current = false;
      if (live.current) {
        setBusy(false);
        setRuntimeBusy(false);
        // Only the native mutation reply releases its latch. The subsequent
        // read has its own deadline/identity and can never replay the mutation.
        if (refreshAfter) void load(false);
      }
    }
  }
  const disabled = busy || loading;
  const cleanup = pendingCleanup ? CLEANUP[pendingCleanup] : null;
  return (
    <section id="settings-anchor-ext-computer" className="settings-card cu-settings">
      <div className="cu-settings__group">
        <h2>{tr("cu.settings.title")}</h2>
        <p>{tr("cu.settings.lead")}</p>
      </div>
      <div className="settings-row">
        <div className="cu-settings__label-block">
          <div className="settings-row__label">{tr("cu.settings.enable")}</div>
          <p className="settings-row__desc">{tr("cu.settings.enableDesc")}</p>
        </div>
        <UiSwitch checked={on} disabled={disabled || !loaded} label={tr("cu.settings.enable")}
          onChange={(next) => void perform(async () => {
            await api.computerSetEnabled(next);
            if (live.current) setOn(next);
          })} />
      </div>
      <div className="cu-settings__group cu-settings__runtime" aria-busy={loading || runtimeBusy}>
        <h3>{tr("cu.settings.runtimeTitle")}</h3>
        {runtimeBusy ? <p role="status">{tr("cu.panel.busy")}</p>
          : <ComputerRuntimeHealth runtime={runtime} loading={loading} tr={tr} />}
        <div className="cu-panel__actions">
          <button type="button" className="btn btn--solid" disabled={disabled || !runtime?.canRepair}
            onClick={() => void perform(api.computerRuntimeRepair, true)}>
            {tr("cu.settings.repair")}
          </button>
          <button type="button" className="btn btn--ghost" disabled={disabled || !runtime?.canRollback}
            onClick={() => void perform(api.computerRuntimeRollback, true)}>
            {tr("cu.settings.rollback")}
          </button>
        </div>
      </div>
      <div className="cu-settings__group">
        <details className="cu-settings__maintenance">
          <summary>{tr("cu.settings.maintenance")}</summary>
          <div>
            <p className="settings-desc">{tr("cu.settings.privacy")}</p>
            <p className="settings-desc">{tr("cu.settings.platforms")}</p>
            <p className="settings-desc">{tr("cu.settings.neverSystemNode")}</p>
            <button type="button" className="btn btn--ghost" disabled={disabled}
              onClick={() => void perform(async () => {
                const path = await api.computerExportBundle();
                if (live.current) setNotice({ path });
              })}>{tr("cu.settings.exportBundle")}</button>
            <div className="cu-panel__actions">
              {(Object.keys(CLEANUP) as CleanupKind[]).map((kind) => (
                <button key={kind} type="button" className="btn btn--ghost" disabled={disabled}
                  onClick={() => { setError(null); setPendingCleanup(kind); }}>{tr(CLEANUP[kind].title)}</button>
              ))}
            </div>
          </div>
        </details>
      </div>
      <ComputerGnomeHelper locale={locale} featureEnabled={on} />
      {(error || loadFailed) && !cleanup ? <div className="cu-settings__group">
        <p role="alert" className="cu-panel__error">{tr(error ? computerUseErrorKey(error) : "cu.settings.loadFailed")}</p>
        <button type="button" className="btn btn--ghost" disabled={disabled} onClick={() => void load()}>
          {tr("ui.errorBoundary.retry")}
        </button>
      </div> : null}
      {notice ? <p role="status" className="cu-settings__group cu-settings__notice">
        {notice.path ? tr("cu.settings.exported", { path: notice.path }) : tr("cu.settings.done")}
      </p> : null}
      <GlassModal open={!!cleanup} onClose={() => { if (!busy) setPendingCleanup(null); }}
        className="cu-settings__confirmation"
        title={cleanup ? tr(cleanup.title) : tr("cu.settings.maintenance")} size="sm" wrapBody
        closeLabel={tr("common.close")} closeOnOverlay={!busy} showClose={!busy}
        footer={<>
          <button type="button" className="btn btn--ghost" disabled={busy} onClick={() => setPendingCleanup(null)}>{tr("common.cancel")}</button>
          <button type="button" className="btn btn--danger" disabled={busy}
            onClick={() => { if (cleanup) void perform(async () => {
              await cleanup.run();
              if (live.current) { setPendingCleanup(null); setNotice({}); }
            }); }}>{tr(busy ? "cu.panel.busy" : "common.confirm")}</button>
        </>}>
        <p className="settings-desc">{cleanup ? tr(cleanup.hint) : ""}</p>
        {error ? <p role="alert" className="cu-panel__error">{tr(computerUseErrorKey(error))}</p> : null}
      </GlassModal>
    </section>
  );
}
