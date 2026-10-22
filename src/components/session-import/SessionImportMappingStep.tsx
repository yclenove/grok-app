import type { useSessionImportUi } from "@/hooks/useSessionImport";
import { isDialogFocusAllowed } from "@/lib/a11yFocus";
import { activationOwningTarget, mappingKeyCommand } from "@/lib/sessionImport/hostControls";
import { isPrimaryMapping, mappingRows } from "@/lib/sessionImport/mappingView";
import { SessionImportMappingSelect } from "./SessionImportMappingSelect";
import { SessionImportCommitStatus } from "./SessionImportCommitStatus";
import type { SessionImportTFn } from "./sessionImportHostLabels";
import { SessionImportActionPortal } from "./SessionImportActionPortal";
import { SessionImportAssociationDialog } from "./SessionImportAssociationDialog";
import { SessionImportPreparedAssociations } from "./SessionImportPreparedAssociations";
import { importErrorKey } from "@/lib/sessionImport/errorCopy";

type MappingUi = Pick<ReturnType<typeof useSessionImportUi>,
  "busy" | "prepareBatch" | "mappings" | "registrations" | "pendingPlan" |
  "projectId" | "projects" | "setMappingProject" | "commitPhase" |
  "associations" | "prepareItems" | "prepareNextCursor" | "prepareItemsError" |
  "loadMorePrepare" | "retryPrepareItems">;

/** The production mapping composition; the review gallery supplies local actions. */
export function SessionImportMappingStep({ tr, ui, mappingSessions, mappingSessionCount, commitImport, onBack, footerTarget }: {
  tr: SessionImportTFn;
  ui: MappingUi;
  mappingSessions: { id: string; title: string }[];
  mappingSessionCount: number;
  commitImport: () => void;
  onBack: () => void;
  footerTarget?: HTMLElement | null;
}) {
  return (
  <div
    data-import-mapping="1"
    onKeyDown={(e) => {
      // Only an unclaimed, plain Enter in the active dialog may publish.
      // Some IMEs clear isComposing before their final keyCode 229 event.
      if (e.defaultPrevented || e.nativeEvent.keyCode === 229 ||
          e.ctrlKey || e.altKey || e.metaKey || e.shiftKey ||
          !isDialogFocusAllowed(e.currentTarget)) return;
      const cmd = mappingKeyCommand(e.key, {
        busy: ui.busy,
        composing: e.nativeEvent.isComposing,
        onControl: activationOwningTarget(e.target),
      });
      if (cmd === "none") return;
      e.preventDefault();
      if (cmd === "commit") commitImport();
    }}
  >
    {ui.prepareBatch || mappingSessions.length > 0 ? (
      <div>
        <p className="settings-row__desc">{tr("sessionImport.selectedSessions")}</p>
        {ui.prepareBatch ? (
          <>
            <p className="settings-row__desc">
              {tr("sessionImport.prepare.counts", {
                prepared: ui.prepareBatch.prepared, failed: ui.prepareBatch.failed, skipped: ui.prepareBatch.skipped,
              })}
            </p>
            <p className="settings-row__desc">{tr("sessionImport.prepare.onlyPrepared")}</p>
          </>
        ) : null}
        <ul className="session-import__list session-import__selected-summary">
          {mappingSessions.map(({ id, title }) => (
            <li key={id}>{title}</li>
          ))}
        </ul>
        {mappingSessionCount > mappingSessions.length ? (
          <p className="settings-row__desc">
            {tr("sessionImport.moreSelected", { n: mappingSessionCount - mappingSessions.length })}
          </p>
        ) : null}
      </div>
    ) : null}
    <ul className="session-import__list session-import__mappings">
      {mappingRows(ui.mappings, ui.registrations, tr("sessionImport.sourceFolderUnknown")).map((m, i) => (
        <li key={m.rootId}>
          <span>
            {m.label}
            {isPrimaryMapping(m, i, ui.mappings) ? (
              <span className="session-import__badge" data-kind="ok">
                {tr("sessionImport.primaryRoot")}
              </span>
            ) : null}
            {m.suggestedTarget && m.suggestedTarget !== m.label
              ? ` · ${m.suggestedTarget}`
              : ""}
          </span>
          {m.sourceLabel ? <p className="session-import__locator">
            {tr("sessionImport.workspace.sourceIdentity")}: {m.sourceLabel}
          </p> : null}
          <SessionImportMappingSelect
            tr={tr} sourceLabel={`${m.label}${m.sourceLabel ? ` · ${m.sourceLabel}` : ""}`}
            disabled={ui.busy || !!ui.pendingPlan}
            value={m.unassigned ? "" : (m.targetProjectId ?? "")}
            onChange={(v) => {
              ui.setMappingProject(m.rootId, v ? v : null);
            }}
            options={[
              { value: "", label: tr("sessionImport.mapUnassigned") },
              ...(ui.projectId ? [{ value: ui.projectId, label: tr("sessionImport.mapExisting") }] : []),
              ...ui.projects
              .filter((p) => p.id !== ui.projectId)
              .map((p) => ({ value: p.id, label: p.name })),
            ]}
          />
        </li>
      ))}
    </ul>
    {ui.prepareBatch && !ui.pendingPlan ? <SessionImportPreparedAssociations
      key={JSON.stringify([ui.prepareBatch.batchId, ui.prepareBatch.stateVersion])} tr={tr} ui={ui} /> : null}
    {ui.pendingPlan?.associationTargets?.length ? <section className="session-import__history-links">
      <p>{tr("sessionImport.historyLink.saved", { n: ui.pendingPlan.associationTargets.length })}</p>
      <p className="settings-row__desc">{tr("sessionImport.historyLink.notice")}</p>
      <ul className="session-import__list">{ui.pendingPlan.associationTargets.map((link) => <li key={link.previewId}>
        <code className="session-import__locator">{link.previewId} → {link.importId}@{link.expectedRevision}</code>
      </li>)}</ul>
    </section> : null}
    {ui.associations.error && !ui.associations.dialog ? <p role="alert">{tr(importErrorKey(ui.associations.error))}</p> : null}
    <SessionImportAssociationDialog tr={tr} ui={ui.associations} />
    <SessionImportActionPortal target={footerTarget}>
    <div className="session-import__toolbar session-import__footer">
      <SessionImportCommitStatus phase={ui.commitPhase} tr={tr} />
      <button
        type="button"
        className="btn"
        data-import-back="mapping"
        disabled={ui.busy}
        onClick={onBack}
      >
        {tr("sessionImport.back")}
      </button>
      <button
        type="button"
        className="btn btn--primary"
        data-import-commit="1"
        disabled={ui.busy}
        onClick={commitImport}
      >
        {tr("sessionImport.importSelected")}
      </button>
    </div>
    </SessionImportActionPortal>
  </div>
  );
}

