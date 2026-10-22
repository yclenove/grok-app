import { GlassModal } from "@/components/GlassModal";
import { useWorkspaceRelocation, type WorkspaceRelocationClient } from "@/hooks/useWorkspaceRelocation";
import type { MessageKey, Vars } from "@/i18n";
import type { SourceWorkspace, SourceWorkspaceRoot } from "@/lib/api/sessionMigration";
import { importErrorKey } from "@/lib/sessionImport/errorCopy";

const location = (workspace: SourceWorkspace) => workspace.workspaceFile ?? workspace.roots[0]?.originalPath;

export function WorkspaceRelocationControl({ workspace, disabled, onSaved, tr, projects = [], client }: {
  workspace: SourceWorkspace;
  disabled: boolean;
  onSaved: (workspace: SourceWorkspace) => void;
  tr: (key: MessageKey, vars?: Vars) => string;
  projects?: Array<{ id: string; name: string }>;
  client?: WorkspaceRelocationClient;
}) {
  const ui = useWorkspaceRelocation(workspace, onSaved, client);
  const p = ui.preview;
  const roots = (rows: SourceWorkspaceRoot[], title: MessageKey, mappings: SourceWorkspace) => (
    <section className="session-import__group">
      <h3 className="session-import__group-title">{tr(title)}</h3>
      {!rows.length ? <p>{tr("sessionImport.workspace.empty")}</p> : <ul className="session-import__roots">
        {rows.map((root) => {
          const choice = mappings.mappingChoices?.find((c) => c.rootId === root.rootId);
          const targetName = projects.find((project) => project.id === choice?.targetProjectId)?.name;
          return <li key={root.rootId}>
            {root.displayName ? <span>{root.displayName}</span> : null}
            <span className="session-import__locator">{root.sourceUri ?? root.originalPath}</span>
            <span className="session-import__locator">{choice?.explicitTarget ?? targetName ?? choice?.targetProjectId ?? tr("sessionImport.workspace.unassigned")}</span>
            {mappings.defaultRootId === root.rootId ? <span className="session-import__badge">{tr("sessionImport.primaryRoot")}</span> : null}
          </li>;
        })}
      </ul>}
    </section>
  );
  return <>
    <button type="button" className="btn" disabled={disabled} onClick={ui.start}>{tr("sessionImport.relocate.title")}</button>
    <GlassModal open={ui.open} onClose={ui.close} showClose={!ui.busy || !ui.attempt}
      closeLabel={tr("common.close")} title={tr("sessionImport.relocate.title")} size="lg" wrapBody className="session-import-modal"
      footer={<>
        <button type="button" className="btn btn--ghost" disabled={ui.busy && !!ui.attempt} onClick={ui.close}>{tr("common.close")}</button>
        <button type="button" className="btn btn--solid" disabled={ui.busy || (!p && !ui.attempt)} onClick={() => void ui.save()}>
          {tr(ui.attempt ? "sessionImport.relocate.retry" : "common.save")}
        </button>
      </>}
    >
      <p className="settings-row__desc">{tr("sessionImport.relocate.hint")}</p>
      {ui.error ? <p role="alert">{tr(importErrorKey(ui.error))}</p> : null}
      {ui.busy ? <p role="status">{tr("sessionImport.workspace.busy")}</p> : null}
      <p>{tr("sessionImport.relocate.before")}</p>
      <p className="session-import__locator">{location(p?.before ?? workspace)}</p>
      <button type="button" className="btn" disabled={ui.busy || !!ui.attempt} onClick={() => void ui.review()}>{tr("sessionImport.relocate.choose")}</button>
      {p || ui.attempt ? <>
        <p>{tr("sessionImport.relocate.after")}</p>
        <p className="session-import__locator">{ui.attempt?.preview.path ?? p?.request.path}</p>
      </> : null}
      {ui.attempt ? <p role="status">{tr("sessionImport.relocate.pending")}</p> : null}
      {p ? <>
        {roots(p.after.roots.filter((r) => p.retainedRootIds.includes(r.rootId)), "sessionImport.relocate.retained", p.after)}
        {roots(p.after.roots.filter((r) => p.addedRootIds.includes(r.rootId)), "sessionImport.relocate.added", p.after)}
        {roots(p.removedRoots, "sessionImport.relocate.removed", p.before)}
        <button type="button" className="btn" disabled={ui.busy || !!ui.attempt} onClick={() => void ui.review(p.request.path)}>{tr("sessionImport.refresh")}</button>
      </> : null}
    </GlassModal>
  </>;
}
