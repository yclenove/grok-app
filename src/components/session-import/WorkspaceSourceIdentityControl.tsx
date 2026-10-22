import { Select } from "@/components/Select";
import type { MessageKey, Vars } from "@/i18n";
import type { WorkspaceImportUi } from "@/hooks/useWorkspaceImport";
import type { SourceRegistration, SourceWorkspace } from "@/lib/api/sessionMigration";
import { registrationLabel, registrationShortLabel, sourceIdentityLabel } from "@/lib/sessionImport/sourceLabel";

export function workspaceSourceLabel(workspace: SourceWorkspace, registrations: SourceRegistration[]) {
  return sourceIdentityLabel(workspace.sourceIdentity, registrations);
}

export function WorkspaceSourceIdentityControl({ ui, registrations, tr }: {
  ui: WorkspaceImportUi;
  registrations: SourceRegistration[];
  tr: (key: MessageKey, vars?: Vars) => string;
}) {
  if (!ui.workspace) return null;
  const editable = ui.editable && (ui.mode === "preview" || !ui.workspace.sourceIdentity);
  const savedLabel = workspaceSourceLabel(ui.workspace, registrations);
  const selected = registrations.find((source) => source.sourceId === ui.sourceId);
  return <div className="session-import__group">
    {editable ? <>
      <Select aria-label={tr("sessionImport.workspace.sourceIdentity")}
        className="session-import__source-identity" menuClassName="session-import__source-identity-menu"
        value={ui.sourceId ?? ""} disabled={ui.busy}
        onChange={(value) => void ui.chooseSource(value || null)}
        options={[
          { value: "", label: tr("sessionImport.workspace.sourceUnknown") },
          ...registrations.map((source) => ({ value: source.sourceId, label: registrationShortLabel(source) })),
          ...(ui.sourceId && !registrations.some((source) => source.sourceId === ui.sourceId)
            ? [{ value: ui.sourceId, label: savedLabel ?? ui.sourceId }] : []),
        ]} />
      {selected ? <p className="session-import__locator session-import__source-identity-details">{registrationLabel(selected)}</p> : null}
      <p className="settings-row__desc">{tr("sessionImport.workspace.sourceHint")}</p>
    </> : <p className="session-import__locator session-import__source-identity-details">
      {tr("sessionImport.workspace.sourceIdentity")}: {savedLabel ?? tr("sessionImport.workspace.sourceUnknown")}
    </p>}
  </div>;
}
