import { invoke } from "./host";
import type { SavedWorkspace, SourceWorkspace, SourceWorkspaceRoot } from "./sessionMigration";

export type WorkspaceRelocationInput = {
  sourceWorkspaceId: string;
  expectedRevision: string;
  path: string;
};

export type WorkspaceRelocationPreview = {
  request: WorkspaceRelocationInput;
  before: SourceWorkspace;
  after: SourceWorkspace;
  retainedRootIds: string[];
  addedRootIds: string[];
  removedRoots: SourceWorkspaceRoot[];
  fingerprint: string;
  proposalHash: string;
};

export type WorkspaceRelocationSave = {
  requestId: string;
  preview: WorkspaceRelocationInput;
  fingerprint: string;
  proposalHash: string;
};

export function workspaceRelocationPreview(request: WorkspaceRelocationInput) {
  return invoke<WorkspaceRelocationPreview>("session_import_workspace_relocation_preview", { request });
}

export function workspaceRelocationSave(request: WorkspaceRelocationSave) {
  return invoke<SavedWorkspace>("session_import_workspace_relocation_save", { request });
}
