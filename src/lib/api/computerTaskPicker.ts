/** Host retains the native identities; these IDs only address one picker. */
import { invoke, isDesktopHost } from "./host";
import type { ComputerTaskKey } from "./computerTask";
import type { ComputerPairingChallenge } from "./computerUse";

export type ComputerTaskPickerSurface = "desktop" | "existing-tabs" | "app-webview";

export type ComputerTaskPickerStarted = {
  sessionId: string;
  pickerId: string;
  key: ComputerTaskKey;
  maxSelection: number;
  surface: ComputerTaskPickerSurface;
};

export type ComputerTaskCandidate = {
  candidateId: string;
  title: string;
  appName: string;
  kind: string;
  surface: string;
};

export type ComputerTaskPickerView = ComputerTaskPickerStarted & {
  candidates: ComputerTaskCandidate[];
  truncated: boolean;
};

/** Task-bound metadata only; no bearer or native sharing proof enters the UI. */
export type ComputerTaskPickerPreparation = ComputerTaskPickerStarted & {
  pairing: ComputerPairingChallenge | null;
  pairingConfirmed: boolean;
  ready: boolean;
  availableTabs: number;
  incompatibleTabs: number;
  borrowedTabs: number;
};

export type ComputerTaskApproval = {
  requestId: string;
  revision: number;
  runId: string;
  decisionClass: string;
};

function requireHost() {
  if (!isDesktopHost()) throw new Error("Computer Use requires the local desktop host");
}

export async function computerTaskPickerBegin(
  sessionId: string,
  key: ComputerTaskKey,
  surface: ComputerTaskPickerSurface,
): Promise<ComputerTaskPickerStarted> {
  requireHost();
  return invoke("computer_use_task_picker_begin", { sessionId, key, surface });
}

export async function computerTaskPickerLoad(picker: ComputerTaskPickerStarted): Promise<ComputerTaskPickerView> {
  requireHost();
  return invoke("computer_use_task_picker_load", { sessionId: picker.sessionId, key: picker.key, pickerId: picker.pickerId });
}

export async function computerTaskPickerCancel(picker: ComputerTaskPickerStarted): Promise<boolean> {
  requireHost();
  return invoke("computer_use_task_picker_cancel", { sessionId: picker.sessionId, key: picker.key, pickerId: picker.pickerId });
}

export async function computerTaskPickerPreparation(picker: ComputerTaskPickerStarted): Promise<ComputerTaskPickerPreparation> {
  requireHost();
  return invoke("computer_use_task_picker_preparation", { sessionId: picker.sessionId, key: picker.key, pickerId: picker.pickerId });
}

export async function computerTaskPickerPairingConfirm(picker: ComputerTaskPickerStarted, nonce: string): Promise<void> {
  requireHost();
  await invoke("computer_use_task_picker_pairing_confirm", { sessionId: picker.sessionId, key: picker.key, pickerId: picker.pickerId, nonce });
}

export async function computerTaskPickerApprove(
  picker: ComputerTaskPickerStarted,
  candidateIds: string[],
): Promise<ComputerTaskApproval> {
  requireHost();
  return invoke("computer_use_task_picker_approve", { sessionId: picker.sessionId, key: picker.key, pickerId: picker.pickerId, candidateIds });
}
