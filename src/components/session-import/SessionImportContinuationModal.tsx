import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { createT, type Locale } from "@/i18n";
import { GlassModal } from "@/components/GlassModal";
import { Select } from "@/components/Select";
import { workspacesList } from "@/lib/api/workspace";
import { isImportedHistoryId, type sessionImportEventContext } from "@/lib/api/sessionMigration";
import type { WorkspaceRecord } from "@/lib/multiRootWorkspace";
import type { useSessionImportUi } from "@/hooks/useSessionImport";
import { importErrorKey } from "@/lib/sessionImport/errorCopy";
import { coverageLabel } from "./sessionImportHostLabels";
import { ContinuationScopeEditor } from "./ContinuationScopeEditor";

export type ContinuationModalClient = {
  workspacesList: typeof workspacesList;
  eventContext: typeof sessionImportEventContext;
};

export function SessionImportContinuationModal({ ui, locale, onOpenNativeSession, client, className, dataTheme }: {
  ui: Pick<ReturnType<typeof useSessionImportUi>,
    "draft" | "projects" | "busy" | "continuationVisible" | "continuationBusy" | "continuationLocked" |
    "continuationReviewingScope" | "continuationScopePending" | "continuationError" | "setDraft" |
    "hideContinuation" | "discardContinuation" | "createContinue" | "selectContinuationWorkspace" |
    "selectContinuationScope" | "cancelContinuationScopeReview" | "beginContinuationScopeReview">;
  locale: Locale;
  onOpenNativeSession?: (id: string) => void;
  client?: ContinuationModalClient;
  className?: string;
  dataTheme?: string;
}) {
  const tr = useMemo(() => createT(locale), [locale]);
  const [workspaces, setWorkspaces] = useState<WorkspaceRecord[]>([]);
  const [listError, setListError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [refresh, setRefresh] = useState(0);
  const [query, setQuery] = useState("");
  const reviewTrigger = useRef<HTMLButtonElement>(null);
  const wasReviewing = useRef(false);
  useLayoutEffect(() => {
    if (wasReviewing.current && !ui.continuationReviewingScope && document.activeElement === document.body) {
      reviewTrigger.current?.focus();
    }
    wasReviewing.current = !!ui.continuationReviewingScope;
  }, [ui.continuationReviewingScope]);
  const draft = ui.draft;
  const open = !!draft && ui.continuationVisible !== false;
  const loadWorkspaces = client?.workspacesList ?? workspacesList;
  useEffect(() => {
    if (!open) return;
    let cancelled = false;
    setLoading(true);
    setListError(null);
    void loadWorkspaces().then((rows) => {
      if (!cancelled) setWorkspaces(rows);
    }).catch((cause) => {
      if (!cancelled) setListError(String(cause));
    }).finally(() => {
      if (!cancelled) setLoading(false);
    });
    return () => { cancelled = true; };
  }, [loadWorkspaces, open, refresh]);
  useEffect(() => { if (!open) setQuery(""); }, [open]);
  const disabled = ui.continuationBusy || ui.continuationLocked || ui.continuationReviewingScope;
  const selected = draft?.workspace;
  const constraint = draft?.creationVersion ? draft.workspaceProjectConstraint : draft?.projectId;
  const candidates = workspaces.filter((row) =>
    (!constraint || row.primaryProjectId === constraint)
    && `${row.name} ${row.roots.map((root) => root.path).join(" ")}`.toLowerCase().includes(query.toLowerCase()),
  );
  const visible = candidates.slice(0, 200);
  if (selected && !visible.some((row) => row.id === selected.id)) visible.unshift(selected);
  const cancel = () => { if (!ui.continuationBusy) void ui.discardContinuation(); };
  const close = () => { if (!ui.continuationBusy) ui.hideContinuation(); };
  const project = ui.projects.find((row) => row.id === draft?.projectId)?.name ?? draft?.projectId;
  return (
    <GlassModal open={open} onClose={close} title={tr("sessionImport.handoffTitle")}
      size="lg" wrapBody className={`session-import-modal session-import-continuation ${className ?? ""}`}
      dataTheme={dataTheme} closeLabel={tr("sessionImport.close")}
      showClose={!ui.continuationBusy}
      footer={<>
        <button type="button" className="btn btn--ghost" disabled={ui.continuationBusy || ui.continuationScopePending} onClick={cancel}>
          {tr("common.cancel")}
        </button>
        <button type="button" className="btn btn--primary" disabled={ui.busy || ui.continuationReviewingScope || !draft?.text.trim()} onClick={() => {
          void ui.createContinue().then((id) => {
            if (id && !isImportedHistoryId(id)) onOpenNativeSession?.(id);
          });
        }}>{tr("sessionImport.createDraft")}</button>
      </>}
    >
      <p className="session-import__read-only">{tr("sessionImport.handoffSendHint")}</p>
      {ui.continuationError ? <p role="alert" className="settings-row__desc">
        {tr(importErrorKey(ui.continuationError))}
      </p> : null}
      <div className="session-import__handoff-grid" data-reviewing={ui.continuationReviewingScope ? "1" : "0"}>
      <section className="session-import__handoff-source" aria-label={tr("sessionImport.handoffSource")}>
      <h3 className="session-import__pane-title">{tr("sessionImport.handoffSource")}</h3>
      {draft?.selection ? (ui.continuationReviewingScope
        ? <ContinuationScopeEditor key={draft.draftId} draft={draft} tr={tr}
            busy={ui.continuationBusy} pending={ui.continuationScopePending}
            onApply={ui.selectContinuationScope} onCancel={ui.cancelContinuationScopeReview} loadContext={client?.eventContext} />
        : <button ref={reviewTrigger} type="button" className="btn btn--ghost" disabled={disabled}
            onClick={ui.beginContinuationScopeReview}>{tr("sessionImport.scopeReview")}</button>) : null}
      {draft ? <>
        <p className="settings-row__desc">{tr("sessionImport.handoffMeta", {
          coverage: coverageLabel(draft.coverage, tr), n: draft.estimatedTokens, leaf: draft.leaf,
        })}</p>
        <p className="settings-row__desc">{tr("sessionImport.handoffScope", {
          fragment: draft.fragmentEventIds?.length ?? 0,
          readable: draft.readableEventIds?.length ?? draft.selection?.eventIds.length ?? 0,
        })}</p>
      </> : null}
      </section>
      <section className="session-import__handoff-workspace" aria-label={tr("sessionImport.handoffWorkspace")}>
      <h3 className="session-import__pane-title">{tr("sessionImport.handoffWorkspace")}</h3>
      <input className="input" aria-label={tr("sessionImport.handoffWorkspaceSearch")}
        placeholder={tr("sessionImport.handoffWorkspaceSearch")} value={query}
        disabled={disabled} onChange={(event) => setQuery(event.target.value)} />
      <Select value={selected?.id ?? ""} disabled={disabled || loading || !!listError}
        aria-label={tr("sessionImport.handoffWorkspace")}
        options={[{ value: "", label: tr("sessionImport.handoffWithoutWorkspace") },
          ...visible.map((row) => ({ value: row.id, label: row.name, disabled: row.capability === "blocked" }))]}
        onChange={(id) => { void ui.selectContinuationWorkspace(id || null); }} />
      <p className="settings-row__desc">{project
        ? tr("sessionImport.handoffProject", { project })
        : tr("sessionImport.handoffDefaultProject")}</p>
      {candidates.length > 200 ? <p className="settings-row__desc">{tr("sessionImport.handoffWorkspaceSearchHint")}</p> : null}
      {listError ? <p role="alert" className="settings-row__desc">{tr(importErrorKey(listError))}</p> : null}
      <button type="button" className="btn btn--ghost" disabled={disabled || loading} onClick={() => {
        setRefresh((value) => value + 1);
        if (selected) void ui.selectContinuationWorkspace(selected.id);
      }}>{tr("sessionImport.handoffReviewWorkspace")}</button>
      {selected ? <>
        {selected.capability !== "none" ? <p className="settings-row__desc">{tr("workspace.multiRoot.capabilityLabel")}: {tr(
          selected.capability === "blocked" ? "workspace.multiRoot.capBlocked"
            : selected.capability === "extraWriteActive" ? "workspace.multiRoot.capWriteActive"
              : selected.capability === "enforcedRead" ? "workspace.multiRoot.capEnforcedRead"
                : "workspace.multiRoot.capContextOnly",
        )}</p> : null}
        <ul className="session-import__target-roots">{selected.roots.map((root) => <li key={root.path}>
          <code>{root.path}</code>{" · "}{tr(root.role === "primary" ? "workspace.multiRoot.rolePrimary" : "workspace.multiRoot.roleExtra")}
          {" · "}{tr(root.access === "write" ? "workspace.multiRoot.accessWrite" : "workspace.multiRoot.accessRead")}
        </li>)}</ul>
      </> : null}
      {ui.continuationLocked ? <p role="status" className="settings-row__desc">{tr("sessionImport.handoffCreationLocked")}</p> : null}
      </section>
      {draft ? (
        <section className="session-import__handoff-target" aria-label={tr("sessionImport.handoffTarget")}>
        <h3 className="session-import__pane-title">{tr("sessionImport.handoffTarget")}</h3>
        <textarea className="session-import__draft" aria-label={tr("sessionImport.handoffEdit")}
          value={draft.text} disabled={disabled} onChange={(event) => ui.setDraft({ ...draft, text: event.target.value })} rows={10} />
        </section>
      ) : null}
      </div>
    </GlassModal>
  );
}
