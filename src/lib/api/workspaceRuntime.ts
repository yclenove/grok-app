import { invoke } from "./host";
import type { WorkspaceRoot } from "@/lib/multiRootWorkspace";

export type WorkspaceRuntimeReview = {
  sessionId: string;
  cwd: string;
  sshAlias?: string | null;
  roots: WorkspaceRoot[];
  requestHash: string;
};

export function previewWorkspaceRuntime(id: string) {
  return invoke<WorkspaceRuntimeReview>("session_workspace_runtime_preview", { id });
}

export function confirmWorkspaceRuntime(review: WorkspaceRuntimeReview) {
  return invoke<void>("session_workspace_runtime_confirm", { id: review.sessionId, requestHash: review.requestHash });
}
