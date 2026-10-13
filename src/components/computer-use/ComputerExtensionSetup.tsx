import { useEffect, useRef, useState } from "react";
import { createT, type Locale } from "@/i18n";
import * as api from "@/lib/api/computerExtension";

/** A user gesture publishes files, never installs into or launches a browser. */
export function ComputerExtensionSetup({ locale, disabled = false }: { locale: Locale; disabled?: boolean }) {
  const tr = createT(locale);
  const live = useRef(false); const open = useRef(false); const revision = useRef(0); const inFlight = useRef(false);
  const [info, setInfo] = useState<api.ComputerExtensionInfo | null>(null);
  const [exported, setExported] = useState<api.ComputerExtensionExport | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<"info" | "export" | "open" | "copy" | null>(null);
  const [copied, setCopied] = useState(false);
  const pathField = useRef<HTMLInputElement>(null);
  const trigger = useRef<HTMLElement | null>(null);
  useEffect(() => { live.current = true; return () => { live.current = false; revision.current++; }; }, []);
  async function perform(action: "info" | "export" | "open" | "copy", from?: HTMLElement) {
    if (!open.current || disabled || inFlight.current || (action === "export" && !info)
      || (["open", "copy"].includes(action) && !exported)) return;
    const epoch = ++revision.current;
    inFlight.current = true; setBusy(action); setError(null); setCopied(false); trigger.current = from ?? null;
    const current = () => live.current && open.current && epoch === revision.current;
    try {
      if (action === "info") { const next = await api.computerExtensionInfo(); if (current()) setInfo(next); }
      if (action === "export") {
        const next = await api.computerExtensionExport();
        if (next.sha256 !== info?.sha256 || next.extensionId !== info.extensionId || next.version !== info.version) throw new Error("Bundled extension changed");
        if (current()) setExported(next);
      }
      if (action === "open") await api.computerExtensionOpen();
      if (action === "copy") { await navigator.clipboard.writeText(exported!.path); if (current()) setCopied(true); }
    } catch { if (current()) setError(action); }
    finally { inFlight.current = false; if (live.current) setBusy(null); }
  }
  useEffect(() => {
    if (exported && trigger.current && (document.activeElement === trigger.current
      || (!trigger.current.isConnected && document.activeElement === document.body))) pathField.current?.focus();
  }, [exported]);
  const inactive = disabled || !!busy;
  return <details className="cu-panel__diag cu-extension-setup" onToggle={event => {
    if (open.current === event.currentTarget.open) return;
    open.current = event.currentTarget.open;
    if (!open.current) revision.current++;
    else if (!info) void perform("info");
  }}>
    <summary>{tr("cu.consent.extensionSetup")}</summary>
    <div className="cu-extension-setup__body" aria-busy={!!busy}>
      {info ? <>
        <p className="cu-panel__hint">{tr("cu.extensionSetup.version", { version: info.version, minimum: info.minimumBrowserVersion })}</p>
        <p className="cu-panel__hint cu-extension-setup__identity">{tr("cu.panel.pairingIdentity", { id: info.extensionId })}</p>
        {!exported ? <button type="button" className="btn btn--ghost" aria-disabled={inactive}
          onClick={event => { if (!inactive) void perform("export", event.currentTarget); }}>{tr("cu.extensionSetup.get")}</button> : <>
          <label className="cu-extension-setup__path">
            <span>{tr("cu.extensionSetup.path")}</span>
            <input ref={pathField} className="input" readOnly spellCheck={false} value={exported.path} onFocus={event => event.currentTarget.select()} />
          </label>
          <div className="cu-panel__actions">
            <button type="button" className="btn btn--ghost btn--sm" aria-disabled={inactive}
              onClick={() => { if (!inactive) void perform("copy"); }}>{tr("cu.extensionSetup.copyPath")}</button>
            <button type="button" className="btn btn--ghost btn--sm" aria-disabled={inactive}
              onClick={() => { if (!inactive) void perform("open"); }}>{tr("cu.extensionSetup.openFolder")}</button>
          </div>
          <ol className="cu-extension-setup__steps">
            <li>{tr("cu.extensionSetup.stepBrowser")}</li>
            <li>{tr("cu.extensionSetup.stepLoad")}</li>
          </ol>
        </>}
      </> : !busy ? <button type="button" className="btn btn--ghost" disabled={disabled} onClick={() => void perform("info")}>{tr("cu.extensionSetup.retry")}</button> : null}
      {busy === "info" || busy === "export" ? <p role="status">{tr(busy === "info" ? "cu.status.checking" : "cu.extensionSetup.exporting")}</p> : null}
      {error ? <p role="alert" className="cu-panel__error">{tr(error === "info" ? "cu.extensionSetup.infoFailed" : error === "export" ? "cu.extensionSetup.exportFailed" : error === "open" ? "cu.extensionSetup.openFailed" : "cu.pairing.copyFailed")}</p> : null}
      {copied ? <p role="status">{tr("cu.pairing.copied")}</p> : null}
    </div>
  </details>;
}
