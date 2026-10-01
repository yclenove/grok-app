/** Typed Host invokes for Computer Use. */

import { invoke, isTauri } from "./host";

export type ComputerTarget = {
  targetId: string;
  title: string;
  appName: string;
  kind: string;
  surface?: string;
};

export type ComputerTrace = {
  kind: string;
  runId: string;
  detail: string;
  ms: number;
  audience: "model" | "ui";
};

export type ComputerTimings = {
  observeMs: number;
  actMs: number;
  verifyMs: number;
};

export type ComputerMcpCatalogStatus = {
  desiredGeneration: number;
  appliedGeneration: number | null;
  desiredPresent: boolean;
  pending: boolean;
  cleanupPending: boolean;
  lastError: string | null;
};

export type ComputerStatus = {
  runId: string | null;
  targetId: string | null;
  targetName: string | null;
  paused: boolean;
  featureEnabled: boolean;
  enabled: boolean;
  stopState: "running" | "stop_requested" | "stopped";
  backend: string;
  /** Host-selected consent mode; not an authorized target or capability. */
  desktopSelection?: "targets" | "portal";
  notes: string[];
  traces: ComputerTrace[];
  recovery: "user" | "system" | null;
  timings: ComputerTimings | null;
  targetAlive: boolean;
  mcpCatalog: ComputerMcpCatalogStatus | null;
};

export type ComputerObservation = {
  snapshotId: string;
  geometryRevision: number;
  previewDataUrl: string | null;
  truncated?: boolean;
};

const emptyStatus = (): ComputerStatus => ({
  runId: null,
  targetId: null,
  targetName: null,
  paused: false,
  featureEnabled: false,
  enabled: false,
  stopState: "stopped",
  backend: "none",
  desktopSelection: "targets",
  notes: [],
  traces: [],
  recovery: null,
  timings: null,
  targetAlive: false,
  mcpCatalog: null,
});

export async function computerStatus(opts: {
  sessionId: string | null;
  runId: string | null;
}): Promise<ComputerStatus> {
  if (!isTauri()) return emptyStatus();
  return invoke<ComputerStatus>("computer_use_status", opts);
}

export async function computerListTargets(opts: {
  sessionId: string | null;
  runId: string | null;
  surface: string;
}): Promise<ComputerTarget[]> {
  if (!isTauri()) return [];
  return invoke<ComputerTarget[]>("computer_use_list_targets", opts);
}

export type ComputerAuthorization = {
  attemptId: string;
  selectorRevision: number;
  runId: string;
  target: ComputerTarget;
};

export type ComputerAuthorizationAttempt = {
  sessionId: string | null;
  attemptId: string;
  selectorRevision: number;
};

export async function computerAuthorizeSurface(opts: {
  sessionId: string | null;
  runId: string | null;
  attemptId: string;
  selectorRevision: number;
  surface: string;
  targetId: string;
}): Promise<ComputerAuthorization> {
  if (!isTauri()) throw new Error("Computer Use requires the desktop app");
  return invoke<ComputerAuthorization>("computer_use_authorize_surface", { request: opts });
}

export async function computerCancelAuthorization(
  opts: ComputerAuthorizationAttempt,
): Promise<boolean> {
  if (!isTauri()) return false;
  return invoke<boolean>("computer_use_cancel_authorization", opts);
}

export async function computerObserve(opts: {
  sessionId: string | null;
  runId: string | null;
}): Promise<ComputerObservation | null> {
  if (!isTauri()) return { snapshotId: "", geometryRevision: 0, previewDataUrl: null };
  // null means the run is busy; no new frame was captured.
  return invoke<ComputerObservation | null>("computer_use_observe", opts);
}

export async function computerPause(opts: {
  sessionId: string | null;
  runId: string | null;
}): Promise<void> {
  if (!isTauri()) return;
  await invoke("computer_use_pause", opts);
}

export async function computerResume(opts: {
  sessionId: string | null;
  runId: string | null;
}): Promise<void> {
  if (!isTauri()) return;
  await invoke("computer_use_resume", opts);
}

export async function computerTakeover(opts: {
  sessionId: string | null;
  runId: string | null;
}): Promise<void> {
  if (!isTauri()) return;
  await invoke("computer_use_takeover", opts);
}

export async function computerStop(opts: {
  sessionId: string | null;
  runId: string | null;
}): Promise<void> {
  if (!isTauri()) return;
  await invoke("computer_use_stop", opts);
}

export async function computerRetryCleanup(opts: {
  sessionId: string | null;
}): Promise<ComputerMcpCatalogStatus> {
  if (!isTauri()) throw new Error("Computer Use requires the desktop app");
  return invoke<ComputerMcpCatalogStatus>("computer_use_retry_cleanup", opts);
}

export async function computerSetPreview(opts: {
  sessionId: string | null;
  runId: string | null;
  visible: boolean;
}): Promise<void> {
  if (!isTauri()) return;
  await invoke("computer_use_set_preview", opts);
}

export async function computerSetEnabled(enabled: boolean): Promise<void> {
  if (!isTauri()) return;
  await invoke("computer_use_set_feature", { enabled });
}

export type ComputerPairingChallenge = {
  nonce: string;
  instanceId: string;
  verificationCode: string;
  endpoint: string;
  installedExtensionId: string;
  expiresAtMs: number;
};

export async function computerBeginPairing(): Promise<ComputerPairingChallenge | null> {
  if (!isTauri()) return null;
  return invoke<ComputerPairingChallenge>("computer_use_begin_pairing");
}

export async function computerConfirmPairingApp(nonce: string): Promise<void> {
  if (!isTauri()) return;
  await invoke("computer_use_confirm_pairing_app", { nonce });
}

export async function computerRevokePairing(): Promise<void> {
  if (!isTauri()) return;
  await invoke("computer_use_revoke_pairing");
}

export async function computerListSharedTabs(): Promise<ComputerTarget[]> {
  if (!isTauri()) return [];
  return invoke<ComputerTarget[]>("computer_use_list_shared_tabs");
}

export async function computerUnbindWebview(opts: {
  sessionId: string | null;
  runId: string | null;
}): Promise<void> {
  if (!isTauri()) return;
  await invoke("computer_use_unbind_webview", opts);
}

export type ComputerRuntimeIssue = {
  code: string;
  component: string;
  action: string;
};

export type ComputerRuntimeStatus = {
  issues: ComputerRuntimeIssue[];
  canRepair: boolean;
  canRollback: boolean;
};

export async function computerRuntimeStatus(): Promise<ComputerRuntimeStatus> {
  if (!isTauri()) {
    return { issues: [], canRepair: false, canRollback: false };
  }
  return invoke<ComputerRuntimeStatus>("computer_use_runtime_status");
}

export async function computerRuntimeRepair(): Promise<void> {
  if (!isTauri()) throw new Error("Computer Use requires the desktop app");
  await invoke("computer_use_runtime_repair");
}

export async function computerRuntimeRollback(): Promise<void> {
  if (!isTauri()) throw new Error("Computer Use requires the desktop app");
  await invoke("computer_use_runtime_rollback");
}

export async function computerExportBundle(): Promise<string> {
  if (!isTauri()) throw new Error("Computer Use requires the desktop app");
  return invoke<string>("computer_use_export_bundle");
}

export async function computerClearTraces(): Promise<void> {
  if (!isTauri()) return;
  await invoke("computer_use_clear_traces");
}

export async function computerClearStaging(): Promise<void> {
  if (!isTauri()) return;
  await invoke("computer_use_clear_staging");
}

export async function computerClearManagedProfiles(): Promise<void> {
  if (!isTauri()) return;
  await invoke("computer_use_clear_managed_profiles");
}
