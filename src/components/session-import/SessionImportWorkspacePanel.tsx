import { useLayoutEffect, useRef } from "react";
import { SessionImportMappingSelect } from "./SessionImportMappingSelect";
import type { MessageKey, Vars } from "@/i18n";
import type { WorkspaceImportUi } from "@/hooks/useWorkspaceImport";
import type { WorkspaceRelocationClient } from "@/hooks/useWorkspaceRelocation";
import { importErrorKey } from "@/lib/sessionImport/errorCopy";
import { MAX_EXTRA_WORKSPACE_ROOTS } from "@/lib/multiRootWorkspace";
import type { SourceRegistration } from "@/lib/api/sessionMigration";
import { WorkspaceSourceIdentityControl, workspaceSourceLabel } from "./WorkspaceSourceIdentityControl";
import { WorkspaceRelocationControl } from "./WorkspaceRelocationControl";
import { SessionImportActionPortal } from "./SessionImportActionPortal";
import { isDialogFocusAllowed } from "@/lib/a11yFocus";
import { hasPickerTriggerWithin, ownPickerTrigger, releaseDisconnectedPickerTrigger, restorePickerTrigger } from "@/lib/sessionImport/pickerFocus";

export function SessionImportWorkspacePanel({ ui, tr, projects, registrations = [], footerTarget, relocationClient }: {
  ui: WorkspaceImportUi;
  tr: (key: MessageKey, vars?: Vars) => string;
  projects: Array<{ id: string; name: string }>;
  registrations?: SourceRegistration[];
  footerTarget?: HTMLElement | null;
  relocationClient?: WorkspaceRelocationClient;
}) {
  const panelRef = useRef<HTMLElement>(null);
  useLayoutEffect(() => {
    restorePickerTrigger();
    return releaseDisconnectedPickerTrigger;
  });
  useLayoutEffect(() => {
    const panel = panelRef.current;
    const frame = panel?.closest<HTMLElement>("[role='dialog']");
    // Picker completion, a portaled footer or Save can replace the focused
    // control. Restore only lost focus in the active dialog. A newer dialog
    // or another focused control always keeps its keyboard ownership.
    if (!panel || !frame || hasPickerTriggerWithin(panel)
      || document.activeElement !== document.body || !isDialogFocusAllowed(frame)) return;
    const actions = footerTarget ?? panelRef.current;
    (actions?.querySelector<HTMLButtonElement>(".btn--solid:not(:disabled)")
      ?? actions?.querySelector<HTMLButtonElement>("button:not(:disabled)"))?.focus();
  }, [footerTarget, ui.busy, ui.mode]);
  const workspace = ui.workspace;
  const preview = ui.preview;
  const projectOptions = [...projects, ...ui.createdProjects.filter((created) => !projects.some((project) => project.id === created.id))];
  const canCreateNative = !!workspace?.roots.length && workspace.roots.length <= MAX_EXTRA_WORKSPACE_ROOTS + 1
    && ui.choices.length === workspace.roots.length
    && ui.choices.every((choice) => choice.primary ? !!choice.targetProjectId : !!(choice.targetProjectId || choice.explicitTarget));
  return (
    <section ref={panelRef}
      className="session-import__group" aria-busy={ui.busy} data-import-workspace={ui.mode}>
      <h3 className="session-import__group-title">{workspace?.displayName ?? tr("sessionImport.workspace.savedList")}</h3>
      <p className="settings-row__desc">{tr("sessionImport.workspace.metadataOnly")}</p>
      {ui.error ? <p role="alert">{tr((ui.mode === "edit" || ui.mode === "saved") && ui.error.includes("PLAN_STALE")
        ? "sessionImport.workspace.mappingChanged" : importErrorKey(ui.error))}</p> : null}
      {ui.busy ? <p role="status">{tr("sessionImport.workspace.busy")}</p> : null}
      {ui.mode === "list" ? (
        <>
          {!ui.busy && !ui.error && !ui.items.length ? <p>{tr("sessionImport.workspace.noSaved")}</p> : null}
          <ul className="session-import__roots">
            {ui.items.map((item) => (
              <li key={item.sourceWorkspaceId}>
                <button type="button" className="btn" disabled={ui.busy} onClick={() => ui.showSaved(item)}>
                  {item.displayName}
                  <span className="session-import__locator">{item.workspaceFile ?? item.roots[0]?.originalPath}</span>
                  <span className="session-import__locator">{workspaceSourceLabel(item, registrations) ?? tr("sessionImport.workspace.sourceUnknown")}</span>
                </button>
              </li>
            ))}
          </ul>
          {ui.cursor ? <button type="button" className="btn" disabled={ui.busy} onClick={() => void ui.list(true)}>{tr("sessionImport.loadMore")}</button> : null}
          {ui.error ? <button type="button" className="btn" disabled={ui.busy} onClick={() => void ui.list(!!ui.cursor)}>{tr("sessionImport.refresh")}</button> : null}
        </>
      ) : null}
      {workspace ? (
        <>
          <p className="session-import__locator">{workspace.workspaceFile ?? workspace.roots[0]?.originalPath}</p>
          <WorkspaceSourceIdentityControl ui={ui} registrations={registrations} tr={tr} />
          {ui.mode === "saved" ? <p role="status">{tr("sessionImport.workspace.saved")}</p> : null}
          {!workspace.roots.length ? <p>{tr("sessionImport.workspace.empty")}</p> : null}
          <ul className="session-import__list session-import__workspace-roots">
            {workspace.roots.map((root) => {
              const choice = ui.choices.find((row) => row.rootId === root.rootId);
              const target = choice?.targetProjectId;
              const sourceLabel = root.sourceUri ?? root.originalPath;
              return (
                <li key={root.rootId}>
                  <div className="session-import__workspace-root-heading">
                    {root.displayName ? <strong>{root.displayName}</strong> : null}
                    <span className="session-import__locator" title={root.originalPath}>{sourceLabel}</span>
                    <span className="session-import__badge">{tr(root.unsupportedUri
                      ? "sessionImport.workspace.unsupportedUri" : `sessionImport.workspace.${root.availability}`)}</span>
                  </div>
                  <SessionImportMappingSelect
                    tr={tr} sourceLabel={sourceLabel}
                    disabled={ui.busy || !ui.editable}
                    value={choice?.explicitTarget ? "__directory__" : target ?? ""}
                    onChange={(value) => { if (value !== "__directory__") ui.setProject(root.rootId, value || null); }}
                    options={[
                      { value: "", label: tr("sessionImport.workspace.unassigned") },
                      ...(choice?.explicitTarget ? [{ value: "__directory__", label: choice.explicitTarget }] : []),
                      ...projectOptions.map((project) => ({ value: project.id, label: project.name })),
                      ...(target && !projectOptions.some((project) => project.id === target)
                        ? [{ value: target, label: `${tr("sessionImport.err.mappingConflict")}: ${target}` }]
                        : []),
                    ]}
                  />
                  <div className="session-import__workspace-root-actions">
                  <button
                    type="button" className="btn btn--sm"
                    aria-pressed={choice?.primary ?? workspace.defaultRootId === root.rootId}
                    disabled={ui.busy || !ui.editable}
                    onClick={() => ui.setPrimary(root.rootId)}
                  >{tr("sessionImport.primaryRoot")}</button>
                  {ui.editable ? <button type="button" className="btn btn--sm" disabled={ui.busy}
                    aria-label={`${tr("sessionImport.workspace.chooseTarget")}: ${sourceLabel}`}
                    onClick={(event) => {
                      void ownPickerTrigger(event.currentTarget, () => ui.chooseTarget(root.rootId));
                    }}>{tr("sessionImport.workspace.chooseTarget")}</button> : null}
                  {ui.editable && choice?.explicitTarget ? (
                    <button type="button" className="btn btn--sm" disabled={ui.busy}
                      aria-label={`${tr("sessionImport.workspace.createProject")}: ${sourceLabel}`}
                      onClick={() => void ui.createProject(root.rootId)}>{tr("sessionImport.workspace.createProject")}</button>
                  ) : null}
                  </div>
                  {ui.editable && choice?.explicitTarget ? <p className="settings-row__desc">{tr("sessionImport.workspace.projectHint")}</p> : null}
                  {ui.editable && ui.projectNotice?.rootId === root.rootId
                    ? <p role="status">{tr("sessionImport.workspace.projectReady", { name: ui.projectNotice.name })}</p> : null}
                </li>
              );
            })}
          </ul>
        </>
      ) : null}
      {ui.mode === "saved" && workspace ? <>
        <p className="settings-row__desc">{tr("sessionImport.workspace.nativeHint")}</p>
        {ui.nativeWorkspace ? <p role="status">{tr("sessionImport.workspace.nativeCreated", { name: ui.nativeWorkspace.name })}</p> : null}
      </> : null}
      <SessionImportActionPortal target={footerTarget}>
      <div className="session-import__toolbar session-import__footer">
        <button type="button" className="btn" onClick={ui.reset}>{tr("sessionImport.back")}</button>
        {ui.mode === "saved" ? <button type="button" className="btn" disabled={ui.busy} onClick={() => void ui.edit()}>{tr("sessionImport.workspace.editMappings")}</button> : null}
        {ui.mode === "saved" && workspace ? <WorkspaceRelocationControl key={workspace.sourceWorkspaceId}
          workspace={workspace} disabled={ui.busy} onSaved={ui.showSaved} tr={tr} projects={projectOptions} client={relocationClient} /> : null}
        {ui.mode === "saved" ? <button type="button" className="btn btn--solid" disabled={ui.busy || !canCreateNative || !!ui.nativeWorkspace}
          onClick={() => void ui.createNative()}>{tr("sessionImport.workspace.createNative")}</button> : null}
        {ui.mode === "edit" ? <>
          <button type="button" className="btn" disabled={ui.busy} onClick={ui.cancelEdit}>{tr("common.cancel")}</button>
          <button type="button" className="btn" disabled={ui.busy} onClick={() => void ui.edit()}>{tr("sessionImport.refresh")}</button>
          <button type="button" className="btn btn--solid" disabled={ui.busy || !ui.editable} onClick={() => void ui.save()}>{tr("common.save")}</button>
        </> : null}
        {ui.mode === "preview" && preview ? (
          <>
            <button type="button" className="btn" disabled={ui.busy} onClick={() => void ui.load(preview.source)}>{tr("sessionImport.refresh")}</button>
            <button type="button" className="btn btn--solid" disabled={ui.busy} onClick={() => void ui.save()}>{tr("common.save")}</button>
          </>
        ) : null}
      </div>
      </SessionImportActionPortal>
    </section>
  );
}
