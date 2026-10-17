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
