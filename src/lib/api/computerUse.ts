/** Desktop-only Computer Use approval bridge. Mirror clients cannot reach it. */

import { invoke, isDesktopHost } from "./host";
import type { Card, Claim } from "@/lib/computer-use/approvalDesk";

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

async function listenDesktop<T>(
  event: string,
  handler: (payload: T) => void,
): Promise<() => void> {
  if (!isDesktopHost()) return () => {};
  const { listen } = await import("@tauri-apps/api/event");
  const unlisten = await listen<T>(event, (emitted) => handler(emitted.payload));
  return unlisten;
}
