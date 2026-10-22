import { GlassModal } from "@/components/GlassModal";
import { useSessionImportLocations } from "@/hooks/useSessionImportLocations";
import type { SourceRegistration } from "@/lib/api/sessionMigrationLocations";
import { importErrorKey } from "@/lib/sessionImport/errorCopy";
import { choiceFromLocations, type SourceChoice } from "@/lib/sessionImport/sourceChoice";
import { ownPickerTrigger } from "@/lib/sessionImport/pickerFocus";
import type { SessionImportTFn } from "./sessionImportHostLabels";

export function SessionImportSourceLocationsControl({ source, disabled, tr, onChoose, autoOpen = false, hideTrigger = false, onClosed, scopeKey }: {
  source: SourceRegistration; disabled: boolean; tr: SessionImportTFn; onChoose: (choice: SourceChoice) => void;
  autoOpen?: boolean; hideTrigger?: boolean; onClosed?: () => void;
  scopeKey?: string;
}) {
  const ui = useSessionImportLocations(source, undefined, JSON.stringify([scopeKey ?? "", disabled]));
  useEffect(() => { if (autoOpen && !disabled) void ui.start(); }, [autoOpen, disabled, ui.start]);
  useEffect(() => { if (disabled) ui.close(); }, [disabled, ui.close]);
  const close = () => { ui.close(); onClosed?.(); };
  return <>
    {!hideTrigger ? <button type="button" className="btn btn--sm" data-kind="locations" disabled={disabled}
      onClick={() => { if (!disabled) void ui.start(); }}>{tr("sessionImport.locations.title")}</button>
      : null}
    <GlassModal open={ui.open} onClose={close} closeLabel={tr("common.close")}
      title={tr("sessionImport.locations.title")} size="lg" wrapBody className="session-import-modal"
      footer={<>
        <button type="button" className="btn btn--ghost" onClick={close}>{tr("common.close")}</button>
        <button type="button" className="btn btn--solid" disabled={ui.busy || !!ui.error || !!ui.attempt || !ui.selected.length}
          onClick={() => {
            const selected = ui.takeSelection();
            if (selected) { onChoose(choiceFromLocations(selected.source, selected.locations)); onClosed?.(); }
          }}>{tr("sessionImport.locations.use", { n: ui.selected.length })}</button>
      </>}>
      <p className="settings-row__desc">{tr("sessionImport.locations.hint")}</p>
      <p className="session-import__locator">{ui.source?.rootPath}</p>
      {ui.busy ? <p role="status">{tr("sessionImport.locations.busy")}</p> : null}
      {ui.error ? <p role="alert">{tr(importErrorKey(ui.error))}</p> : null}
      {ui.attempt ? <p role="status">{tr("sessionImport.locations.pending")}</p> : null}
      {ui.saved ? <p role="status">{tr("sessionImport.locations.saved", { path: ui.saved.rootPath })}</p> : null}
      <div className="session-import__toolbar">
        <button type="button" className="btn" disabled={ui.busy || !!ui.attempt} onClick={(event) => {
          void ownPickerTrigger(event.currentTarget, ui.pick);
        }}>{tr("sessionImport.locations.pick")}</button>
        <button type="button" className="btn" disabled={ui.busy || !ui.path} onClick={() => void ui.save()}>
          {tr(ui.attempt ? "sessionImport.locations.check" : "sessionImport.locations.add")}</button>
        <button type="button" className="btn btn--ghost" disabled={ui.busy} onClick={() => void ui.refresh()}>{tr("sessionImport.refresh")}</button>
      </div>
      {ui.path ? <p className="session-import__locator">{ui.path}</p> : null}
      {ui.page && !ui.page.locations.length ? <p role="status">{tr("sessionImport.locations.empty")}</p> : null}
      <ul className="session-import__roots">
        {ui.page?.locations.map((location) => <li key={location.locationId}>
          <label className="session-import__source-location-choice">
            <input type="checkbox" disabled={ui.busy || !!ui.error || !!ui.attempt}
              checked={ui.selected.some((item) => item.locationId === location.locationId)} onChange={() => ui.toggle(location)} />
            <span className="session-import__badge">{tr(location.kind === "primary" ? "sessionImport.primaryRoot" : "sessionImport.locations.associated")}</span>
            <span className="session-import__locator">{location.rootPath}</span>
          </label>
        </li>)}
      </ul>
      <div className="session-import__toolbar">
        {ui.error ? <button type="button" className="btn" disabled={ui.busy} onClick={() => void ui.retry()}>{tr("sessionImport.locations.retry")}</button> : null}
        {ui.pageIndex > 0 ? <button type="button" className="btn" disabled={ui.busy} onClick={() => void ui.previous()}>{tr("sessionImport.previousPage")}</button> : null}
        {ui.page?.nextCursor ? <button type="button" className="btn" disabled={ui.busy} onClick={() => void ui.next()}>{tr("sessionImport.nextPage")}</button> : null}
      </div>
    </GlassModal>
  </>;
}
import { useEffect } from "react";
