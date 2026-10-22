import { invoke } from "./host";

export type ImportedWorkspaceChoice = {
  sourceWorkspaceId: string;
  revision: string;
  displayName: string;
  sourceLocator?: string;
};

export type ImportedWorkspaceAssociation = {
  importId: string;
  version: number;
  sourceId: string;
  provider: string;
  sourceReady: boolean;
  choice: ImportedWorkspaceChoice | null;
  status: "unassigned" | "assigned" | "stale" | "missing";
};

export type ImportedWorkspaceUpdate = {
  importId: string;
  expectedVersion: number;
  requestId: string;
  choice: ImportedWorkspaceChoice | null;
};

export type ImportedWorkspaceOptions = { items: ImportedWorkspaceChoice[]; nextCursor: string | null };

export type ImportedWorkspaceFilter = { kind: "unassigned" } | { kind: "workspace"; sourceWorkspaceId: string };
export type ImportedWorkspaceGroup = {
  sourceWorkspaceId: string;
  displayName: string;
  sourceId: string | null;
  provider: string | null;
  sourceLocator: string | null;
  missing: boolean;
};
export type ImportedWorkspaceGroupsPage = {
  items: ImportedWorkspaceGroup[];
  nextCursor: string | null;
  selected: ImportedWorkspaceGroup | null;
};

export function importedWorkspaceGroups(cursor: string | null = null, selectedId: string | null = null) {
  return invoke<ImportedWorkspaceGroupsPage>("session_import_workspace_groups", { cursor, limit: 50, selectedId });
}

export function importedWorkspaceGet(importId: string) {
  return invoke<ImportedWorkspaceAssociation>("session_import_workspace_association_get", { importId });
}

export function importedWorkspaceUpdate(request: ImportedWorkspaceUpdate) {
  return invoke<ImportedWorkspaceAssociation>("session_import_workspace_association_update", { request });
}

export function importedWorkspaceOptions(importId: string, cursor: string | null = null) {
  return invoke<ImportedWorkspaceOptions>("session_import_workspace_association_options", { importId, cursor, limit: 50 });
}
