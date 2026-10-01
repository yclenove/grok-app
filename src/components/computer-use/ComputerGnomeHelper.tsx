import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { createT, type Locale, type MessageKey } from "@/i18n";
import { GlassModal } from "@/components/GlassModal";
import { computerHelperAction, computerHelperStatus, type HelperAction, type HelperState, type HelperStatus } from "@/lib/api/computerGnomeHelper";

const ACTIONS: Record<HelperAction, MessageKey> = {
  install: "cu.helper.install", repair: "cu.helper.repair", enable: "cu.helper.enable", disable: "cu.helper.disable",
};
const STATES: Record<HelperState, MessageKey> = {
  unavailable: "cu.helper.unavailable", unsupported: "cu.helper.unsupported", conflict: "cu.helper.conflict",
  missing: "cu.helper.missing", repair_required: "cu.helper.repairRequired", restart_required: "cu.helper.restartRequired",
  global_disabled: "cu.helper.globalDisabled", ready: "cu.helper.ready", disabled: "cu.helper.disabled",
  blocked: "cu.helper.blocked", unconfirmed: "cu.helper.unconfirmed",
};

export function ComputerGnomeHelper({ locale, featureEnabled = false }: { locale: Locale; featureEnabled?: boolean }) {
  const tr = useMemo(() => createT(locale), [locale]);
  const [status, setStatus] = useState<HelperStatus | null>(null);
  const [loading, setLoading] = useState(false);
  const [busy, setBusy] = useState(false);
  const [failed, setFailed] = useState(false);
  const [action, setAction] = useState<HelperAction | null>(null);
  const live = useRef(true);
  const mutating = useRef(false);
  const reading = useRef<{ id: symbol; timer: ReturnType<typeof setTimeout> } | null>(null);
  const load = useCallback(async (clearError = true) => {
    if (reading.current || mutating.current) return;
    const id = Symbol("helper read");
    setLoading(true); setStatus(null);
    if (clearError) setFailed(false);
    const finish = () => {
      if (!live.current || reading.current?.id !== id) return false;
      clearTimeout(reading.current.timer); reading.current = null; setLoading(false); return true;
    };
    reading.current = { id, timer: setTimeout(() => { if (finish()) setFailed(true); }, 10_000) };
    try {
      const result = await computerHelperStatus();
      if (finish()) setStatus(result);
    } catch { if (finish()) setFailed(true); }
  }, []);
  useEffect(() => {
    live.current = true;
    if (!mutating.current) setAction(null);
    void load();
    return () => {
      live.current = false;
      if (reading.current) clearTimeout(reading.current.timer);
      reading.current = null;
    };
  }, [load, featureEnabled]);
  async function confirm() {
    if (!action || mutating.current || reading.current || !status?.actions.includes(action)) return;
    const selected = action;
    mutating.current = true; setBusy(true); setFailed(false); setStatus(null);
    try { await computerHelperAction(selected); }
    catch { if (live.current) setFailed(true); }
    finally {
      mutating.current = false;
      if (live.current) { setBusy(false); setAction(null); void load(false); }
    }
  }
  const disabled = busy || loading || !!status?.busy || !status?.available;
  return <div id="settings-anchor-ext-computer-helper" className="cu-settings__group" aria-busy={busy || loading}>
    <h3>{tr("cu.helper.title")}</h3>
    <p className="settings-desc">{tr("cu.helper.description")}</p>
    {busy || loading || status?.busy ? <p role="status">{tr("cu.panel.busy")}</p> : null}
    {status ? <p role="status">{tr(STATES[status.state] ?? "cu.helper.unconfirmed")}</p> : null}
    {status?.available && status.featureEnabled ? <p className="settings-desc">{tr("cu.helper.turnOff")}</p> : null}
    {failed ? <p role="alert" className="cu-panel__error">{tr("cu.helper.failed")}</p> : null}
    <div className="cu-panel__actions">
      {(Object.keys(ACTIONS) as HelperAction[]).map((kind) => <button key={kind} type="button" className="btn btn--ghost"
        disabled={disabled || !status?.actions.includes(kind)} onClick={() => { setFailed(false); setAction(kind); }}>
        {tr(ACTIONS[kind])}
      </button>)}
      <button type="button" className="btn btn--ghost" disabled={busy || loading}
        onClick={() => void load()}>{tr("cu.helper.refresh")}</button>
    </div>
    <GlassModal open={action !== null} onClose={() => { if (!busy) setAction(null); }}
      title={action ? tr(ACTIONS[action]) : tr("cu.helper.title")} size="sm" wrapBody
      className="cu-settings__confirmation" closeLabel={tr("common.close")} closeOnOverlay={!busy} showClose={!busy}
      footer={<>
        <button type="button" className="btn btn--ghost" disabled={busy} onClick={() => setAction(null)}>{tr("common.cancel")}</button>
        <button type="button" className="btn btn--solid" disabled={busy || loading || !!status?.busy || !action || !status?.actions.includes(action)} onClick={() => void confirm()}>
          {tr(busy ? "cu.panel.busy" : "common.confirm")}
        </button>
      </>}>
      <p className="settings-desc">{tr(action === "enable" ? "cu.helper.enableConfirm"
        : action === "disable" ? "cu.helper.disableConfirm" : "cu.helper.installConfirm")}</p>
    </GlassModal>
  </div>;
}
