import { Select } from "@/components/Select";
import { createT, type Locale } from "@/i18n";
import { useImportedWorkspaceAssociation } from "@/hooks/useImportedWorkspaceAssociation";
import { importErrorCode, importErrorKey } from "@/lib/sessionImport/errorCopy";
import type { ImportedWorkspaceChoice } from "@/lib/api/sessionWorkspaceAssociation";
import { IMPORTED_LIST_WINDOW } from "@/lib/sessionImport/catalogView";
import { shortPathLabel } from "@/lib/extensionsUi";

function optionLabel(choice: ImportedWorkspaceChoice, choices: ImportedWorkspaceChoice[]) {
  const duplicates = choices.filter((item) => item.displayName === choice.displayName);
  if (duplicates.length < 2) return choice.displayName;
  const location = shortPathLabel(choice.sourceLocator, 48);
  const ambiguous = !location || duplicates.some((item) => item.sourceWorkspaceId !== choice.sourceWorkspaceId
    && shortPathLabel(item.sourceLocator, 48) === location);
  return [choice.displayName, location, ambiguous ? choice.sourceWorkspaceId : ""].filter(Boolean).join(" · ");
}

export function ImportedWorkspaceAssociation({ importId, locale }: { importId: string; locale: Locale }) {
  const tr = createT(locale);
  const ui = useImportedWorkspaceAssociation(importId);
  return <ImportedWorkspaceAssociationContent ui={ui} tr={tr} />;
}

export function ImportedWorkspaceAssociationContent({ ui, tr }: {
  ui: ReturnType<typeof useImportedWorkspaceAssociation>;
  tr: ReturnType<typeof createT>;
}) {
  const current = ui.association;
  const errorKey = ui.error ? (["PLAN_STALE", "MAPPING_CONFLICT"].includes(importErrorCode(ui.error))
    ? "sessionImport.association.stale" : importErrorKey(ui.error)) : null;
  const listed = ui.items.some((item) => item.sourceWorkspaceId === ui.draft?.sourceWorkspaceId);
  const choices = !listed && ui.draft ? [...ui.items, ui.draft] : ui.items;
  return <details className="session-import__group" data-import-workspace-association="1" onKeyDown={(event) => {
    if (event.key === "n" || event.key === "p") event.stopPropagation();
  }}>
    <summary>{tr("sessionImport.association.title")}{current?.choice ? ` · ${current.choice.displayName}` : ""}</summary>
    <p className="settings-row__desc">{tr("sessionImport.association.hint")}</p>
    {ui.busy ? <p role="status">{tr("sessionImport.association.loading")}</p> : null}
    {errorKey ? <p role="alert">{tr(errorKey)}</p> : null}
    {ui.windowed ? <p className="settings-row__desc">{tr("sessionImport.listWindow", { n: IMPORTED_LIST_WINDOW })}</p> : null}
    {current ? <>
      {!current.sourceReady ? <p className="settings-row__desc">{tr("sessionImport.association.sourceUnknown")}</p> : null}
      {current.status === "stale" || current.status === "missing"
        ? <p role="status">{tr(current.status === "stale" ? "sessionImport.association.stale" : "sessionImport.association.missing")}</p>
        : null}
      {current.sourceReady ? <>
        <Select aria-label={tr("sessionImport.association.title")} value={ui.draft?.sourceWorkspaceId ?? ""}
          title={ui.draft?.sourceLocator} disabled={ui.busy} onChange={ui.choose} options={[
            { value: "", label: tr("sessionImport.association.none") },
            ...choices.map((item) => ({ value: item.sourceWorkspaceId, label: optionLabel(item, choices) })),
          ]} />
        {ui.draft?.sourceLocator ? <p className="session-import__source-path">{ui.draft.sourceLocator}</p> : null}
        {!ui.items.length && !ui.busy ? <p className="settings-row__desc">{tr("sessionImport.association.empty")}</p> : null}
      </> : null}
      {ui.saved ? <p role="status">{tr("sessionImport.association.saved")}</p> : null}
    </> : null}
    <div className="session-import__toolbar">
      <button type="button" className="btn btn--sm" disabled={ui.busy} onClick={() => void ui.refresh()}>{tr("sessionImport.refresh")}</button>
      {ui.cursor ? <button type="button" className="btn btn--sm" disabled={ui.busy} onClick={() => void ui.loadMore()}>{tr("sessionImport.loadMore")}</button> : null}
      {current?.sourceReady ? <button type="button" className="btn btn--sm" disabled={ui.busy} onClick={() => void ui.save()}>{tr("common.save")}</button> : null}
      {current?.choice ? <button type="button" className="btn btn--sm" disabled={ui.busy} onClick={() => void ui.save(true)}>{tr("sessionImport.association.clear")}</button> : null}
    </div>
  </details>;
}
