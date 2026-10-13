/** Host task consent readback. Correlation keys are never execution grants. */
import { invoke, isDesktopHost } from "./host";

export type ComputerTaskKey = {
  connectionId: string;
  taskId: string;
  requestId: string;
  revision: number;
};

export type ComputerTaskRequest = {
  key: ComputerTaskKey;
  phase: "pending" | "preparing" | "approved" | "denied" | "cancelled" | "expired" | "retired" | "completed";
  purpose: string;
  scopeIntent: string;
  surfaces: string[];
  capabilities: string[];
  remainingMs: number;
};

export type ComputerTaskMember = { targetId: string; surface: string; displayName?: string | null };

export type ComputerTaskGrant = {
  key: ComputerTaskKey;
  runId: string | null;
  members: ComputerTaskMember[];
  capabilities: string[];
  suspended: boolean;
  executable: boolean;
};

export type ComputerTaskExecution = {
  key: ComputerTaskKey;
  runId: string;
  generation: string;
  phase: "preparing" | "awaiting_observation" | "idle" | "busy" | "paused" | "resuming" | "switching" | "unknown" | "target_closed" | "stop_requested" | "stopped";
  target: ComputerTaskMember | null;
  targetName: string | null;
  backend?: string | null;
  cleanupPending: boolean;
  resumeAvailable: boolean;
  hasFreshObservation: boolean;
  lastFailure: "action_rejected" | "outcome_unknown" | null;
  upload?: ComputerTaskUpload | null;
};

export type ComputerTaskUpload = {
  requestId: string;
  actionId: string;
  phase: "awaiting_file" | "selecting" | "assigning";
  targetName: string;
  fieldName: string;
};

export type ComputerTaskUploadCompletion = {
  outcome: { actionId: string; runId: string; kind: "applied" | "verified" | "rejected" | "unknown"; executed: boolean; reason?: string | null };
  bytes: number | null;
};

export type ComputerTaskView = {
  sessionId: string;
  connectionId: string;
  taskId: string | null;
  featureEnabled: boolean;
  request: ComputerTaskRequest | null;
  grants: ComputerTaskGrant[];
  executions: ComputerTaskExecution[];
  /** Compiled Host selection producers, including managed creation; not a grant
   * or a promise that the local runtime is ready. The wire name is retained. */
  pickerSurfaces: string[];
};

export type ComputerTaskExecutionAddress = Pick<ComputerTaskExecution, "key" | "runId" | "generation">;
export type ComputerTaskPreview = {
  address: ComputerTaskExecutionAddress;
  previewDataUrl: string | null;
  truncated: boolean;
};

function executionAddress(address: ComputerTaskExecutionAddress): ComputerTaskExecutionAddress {
  const { connectionId, taskId, requestId, revision } = address.key;
  return { key: { connectionId, taskId, requestId, revision }, runId: address.runId, generation: address.generation };
}

export async function computerTaskChooseUpload(
  sessionId: string, address: ComputerTaskExecutionAddress, uploadRequestId: string, dialogTitle: string,
): Promise<ComputerTaskUploadCompletion> {
  if (!isDesktopHost()) throw new Error("Computer Use requires the local desktop host");
  return invoke("computer_use_task_choose_upload", { sessionId, address: executionAddress(address), uploadRequestId, dialogTitle });
}

export async function computerTaskCancelUpload(
  sessionId: string, address: ComputerTaskExecutionAddress, uploadRequestId: string,
): Promise<ComputerTaskUploadCompletion> {
  if (!isDesktopHost()) throw new Error("Computer Use requires the local desktop host");
  return invoke("computer_use_task_cancel_upload", { sessionId, address: executionAddress(address), uploadRequestId });
}

export async function computerTaskPause(sessionId: string, address: ComputerTaskExecutionAddress): Promise<void> {
  if (!isDesktopHost()) throw new Error("Computer Use requires the local desktop host");
  return invoke("computer_use_task_pause", { sessionId, address: executionAddress(address) });
}

export type ComputerTaskResumeOutcome = { phase: "in_progress" }
  | { phase: "resumed"; address: ComputerTaskExecutionAddress };

export async function computerTaskResume(sessionId: string, address: ComputerTaskExecutionAddress): Promise<ComputerTaskResumeOutcome> {
  if (!isDesktopHost()) throw new Error("Computer Use requires the local desktop host");
  return invoke("computer_use_task_resume", { sessionId, address: executionAddress(address) });
}

export async function computerTaskPreview(sessionId: string, address: ComputerTaskExecutionAddress): Promise<ComputerTaskPreview> {
  if (!isDesktopHost()) throw new Error("Computer Use requires the local desktop host");
  return invoke("computer_use_task_preview", { sessionId, address: executionAddress(address) });
}

export type ComputerTaskDecisionReceipt = {
  sessionId: string;
  key: ComputerTaskKey;
  decision: "deny" | "cancel" | "stop";
};

export async function computerTaskReadback(sessionId: string): Promise<ComputerTaskView | null> {
  if (!isDesktopHost()) return null;
  return invoke<ComputerTaskView>("computer_use_task_readback", { sessionId });
}

export async function computerTaskDecide(
  sessionId: string,
  key: ComputerTaskKey,
  decision: "deny" | "cancel" | "stop",
): Promise<ComputerTaskDecisionReceipt> {
  if (!isDesktopHost()) throw new Error("Computer Use requires the local desktop host");
  return invoke<ComputerTaskDecisionReceipt>("computer_use_task_decide", { sessionId, key, decision });
}
