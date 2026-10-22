import { Select } from "@/components/Select";
import { IconRefresh } from "@/components/icons";
import { ImportIconButton } from "./ImportIconButton";
import { useImportedWorkspaceGroups } from "@/hooks/useImportedWorkspaceGroups";
import { IMPORTED_LIST_WINDOW } from "@/lib/sessionImport/catalogView";
import { importErrorKey } from "@/lib/sessionImport/errorCopy";
import type { ImportedWorkspaceFilter as Filter, ImportedWorkspaceGroup } from "@/lib/api/sessionWorkspaceAssociation";
import type { MessageKey, Vars } from "@/i18n";

export function ImportedWorkspaceFilter({ value, onChange, tr }: {
  value: Filter | null;
  onChange: (value: Filter | null) => void;
  tr: (key: MessageKey, vars?: Vars) => string;
}) {
  const id = value?.kind === "workspace" ? value.sourceWorkspaceId : null;
  const groups = useImportedWorkspaceGroups(id);
  return <ImportedWorkspaceFilterContent value={value} onChange={onChange} tr={tr} groups={groups} />;
}

export function ImportedWorkspaceFilterContent({ value, onChange, tr, groups }: {
  value: Filter | null;
  onChange: (value: Filter | null) => void;
  tr: (key: MessageKey, vars?: Vars) => string;
  groups: ReturnType<typeof useImportedWorkspaceGroups>;
}) {
  const id = value?.kind === "workspace" ? value.sourceWorkspaceId : null;
  const options = [...groups.items];
  if (id && !options.some((item) => item.sourceWorkspaceId === id)) {
    options.push(groups.selected?.sourceWorkspaceId === id ? groups.selected : {
      sourceWorkspaceId: id, displayName: tr(groups.busy ? "sessionImport.association.loading" : "sessionImport.workspaceFilter.unavailable"),
      sourceId: null, provider: null, sourceLocator: null, missing: false,
    });
  }
  const label = (group: ImportedWorkspaceGroup) => [group.displayName, group.provider,
    group.sourceId?.slice(0, 8), group.sourceLocator, group.missing ? tr("sessionImport.workspaceFilter.unavailable") : null].filter(Boolean).join(" · ");
  return <div className="tree-imported__workspace" data-import-workspace-filter="1">
    <div className="tree-imported__workspace-controls">
    <Select aria-label={tr("sessionImport.workspaceFilter.title")}
      value={id ? `workspace:${id}` : value?.kind === "unassigned" ? "unassigned" : "all"}
      onChange={(next) => onChange(next === "all" ? null : next === "unassigned" ? { kind: "unassigned" }
        : { kind: "workspace", sourceWorkspaceId: next.slice("workspace:".length) })}
      options={[
        { value: "all", label: tr("sessionImport.workspaceFilter.all") },
        { value: "unassigned", label: tr("sessionImport.association.none") },
        ...options.map((group) => ({ value: `workspace:${group.sourceWorkspaceId}`, label: label(group) })),
      ]} />
      <ImportIconButton label={tr("sessionImport.workspaceFilter.refresh")} disabled={groups.busy}
        onClick={() => void groups.refresh()}><IconRefresh size={14} /></ImportIconButton>
    </div>
    {groups.busy ? <p role="status" className="settings-row__desc">{tr("sessionImport.association.loading")}</p> : null}
    {groups.error ? <p role="alert" className="settings-row__desc">{tr(importErrorKey(groups.error))}</p> : null}
    {groups.windowed ? <p className="settings-row__desc">{tr("sessionImport.listWindow", { n: IMPORTED_LIST_WINDOW })}</p> : null}
    {groups.cursor ? <button type="button" className="btn btn--ghost btn--sm" disabled={groups.busy} onClick={() => void groups.loadMore()}>{tr("sessionImport.workspaceFilter.more")}</button> : null}
  </div>;
}
