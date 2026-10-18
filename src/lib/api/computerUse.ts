/** Desktop-only Computer Use approval bridge. Mirror clients cannot reach it. */

import { invoke, isDesktopHost } from "./host";
import type { Card, Claim } from "@/lib/computer-use/approvalDesk";
import type {
  ConfirmCard,
  ConfirmClaim,
  ConfirmEnd,
  HandoffCard,
  HandoffClaim,
  HandoffEnd,
} from "@/lib/computer-use/confirmState";

type PendingNotice = { kind: "pending"; card: Card };
type SettledNotice = {
  kind: "settled";
  requestId: number;
  sessionId: string;
  outcome: string;
};

export async function cuApprovalDecide(claim: Claim): Promise<void> {
  if (!isDesktopHost()) throw new Error("failed");
  await invoke<void>("cu_approval_decide", { body: claim });
}

export function listenComputerUseApproval(
  handler: (card: Card) => void,
): Promise<() => void> {
  return listenDesktop<PendingNotice>("cu://approval", (payload) => {
    if (payload.kind === "pending") handler(payload.card);
  });
}

export function listenComputerUseSettled(
  handler: (requestId: number) => void,
): Promise<() => void> {
  return listenDesktop<SettledNotice>("cu://approval-settled", (payload) => {
    if (payload.kind === "settled") handler(payload.requestId);
  });
}

type ConfirmPendingNotice = { kind: "confirmPending"; card: ConfirmCard };
type ConfirmSettledNotice = {
  kind: "confirmSettled";
  requestId: number;
  sessionId: string;
  end: ConfirmEnd;
};
type HandoffPendingNotice = { kind: "handoffPending"; card: HandoffCard };
type HandoffSettledNotice = {
  kind: "handoffSettled";
  requestId: number;
  sessionId: string;
  end: HandoffEnd;
};

export async function cuConfirmDecide(claim: ConfirmClaim): Promise<ConfirmEnd> {
  if (!isDesktopHost()) throw new Error("failed");
  return invoke<ConfirmEnd>("cu_confirm_decide", { body: claim });
}

export async function cuHandoffDecide(claim: HandoffClaim): Promise<HandoffEnd> {
  if (!isDesktopHost()) throw new Error("failed");
  return invoke<HandoffEnd>("cu_handoff_decide", { body: claim });
}

export function listenComputerUseConfirm(
  handler: (card: ConfirmCard) => void,
): Promise<() => void> {
  return listenDesktop<ConfirmPendingNotice>("cu://confirm", (payload) => {
    if (payload.kind === "confirmPending") handler(payload.card);
  });
}

export function listenComputerUseConfirmSettled(
  handler: (requestId: number, end: ConfirmEnd) => void,
): Promise<() => void> {
  return listenDesktop<ConfirmSettledNotice>("cu://confirm-settled", (payload) => {
    if (payload.kind === "confirmSettled") handler(payload.requestId, payload.end);
  });
}

export function listenComputerUseHandoff(
  handler: (card: HandoffCard) => void,
): Promise<() => void> {
  return listenDesktop<HandoffPendingNotice>("cu://handoff", (payload) => {
    if (payload.kind === "handoffPending") handler(payload.card);
  });
}

export function listenComputerUseHandoffSettled(
  handler: (requestId: number, end: HandoffEnd) => void,
): Promise<() => void> {
  return listenDesktop<HandoffSettledNotice>("cu://handoff-settled", (payload) => {
    if (payload.kind === "handoffSettled") handler(payload.requestId, payload.end);
  });
}

async function listenDesktop<T>(
  event: string,
  handler: (payload: T) => void,
): Promise<() => void> {
  if (!isDesktopHost()) return () => {};
  const { listen } = await import("@tauri-apps/api/event");
  const unlisten = await listen<T>(event, (emitted) => handler(emitted.payload));
  return unlisten;
}

// Trusted desktop settings. Mirror clients never receive these commands.
import type { Approval, Confirmation, Doctor, Foreground, PermissionKind, Settings } from "@/lib/computer-use/settings";
function settingsInvoke<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (!isDesktopHost()) return Promise.reject(new Error("desktop-only"));
  return invoke<T>(command, args);
}
export const cuSettingsGet = () => settingsInvoke<Settings>("cu_settings_get");
export const cuSettingsEnabled = (enabled: boolean) => settingsInvoke<void>("cu_settings_enabled", { enabled });
export const cuBrowserClear = () => settingsInvoke<void>("cu_browser_clear");
export const cuSettingsPolicy = (confirmation: Confirmation, foreground: Foreground) => settingsInvoke<void>("cu_settings_policy", { confirmation, foreground });
export const cuApprovalsList = () => settingsInvoke<Approval[]>("cu_approvals_list");
export const cuApprovalsRevoke = (id: string) => settingsInvoke<void>("cu_approvals_revoke", { id });
export const cuDoctor = () => settingsInvoke<Doctor>("cu_doctor");
export const cuOpenSystemSettings = (kind: PermissionKind) => settingsInvoke<void>("cu_open_system_settings", { kind });
