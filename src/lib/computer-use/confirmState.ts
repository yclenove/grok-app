/**
 * Sensitive confirmation and handoff state.
 * Deny is the default. A timer never grants. Handoff does not replay or clear Stop.
 */

import type { ImageBinding } from "@/lib/computer-use/approvalDesk";

export type ConfirmPhase =
  | "pending"
  | "submitting"
  | "allowed"
  | "denied"
  | "timed_out"
  | "stale"
  | "closed";

export type ConfirmAct = "allow" | "deny" | "timeout";

export type ConfirmEnd = "allowed" | "denied" | "timed_out" | "stale" | "closed";

export type HandoffPhase = "pending" | "submitting" | "handled" | "closed";

export type HandoffEnd = "handled" | "closed";

export type CropView =
  | { kind: "redacted"; mime: string; width: number; height: number; bytes: number[] }
  | { kind: "missing" }
  | { kind: "unlocated" };

export const DEFAULT_CONFIRM_ACT: ConfirmAct = "deny";

export type ConfirmEvent =
  | { type: "submit" }
  | { type: "resolve"; end: ConfirmEnd }
  | { type: "fault" }
  | { type: "settled"; end: ConfirmEnd };

export type HandoffEvent =
  | { type: "submit" }
  | { type: "resolve"; end: HandoffEnd }
  | { type: "fault" }
  | { type: "settled"; end: HandoffEnd };

export type ConfirmCard = {
  requestId: number;
  sessionId: string;
  enginePid: number;
  engineStarted: number;
  policyVersion: number;
  image: ImageBinding;
  name: string;
  summary: string;
  reason: string;
  snapshotId: string;
  actionDigest: string;
  crop: CropView;
  timeoutSecs: number;
  openedMs: number;
  hasIcon: boolean;
};

export type HandoffCard = {
  requestId: number;
  sessionId: string;
  enginePid: number;
  engineStarted: number;
  policyVersion: number;
  image: ImageBinding;
  name: string;
  code: string;
  detail: string;
  openedMs: number;
  hasIcon: boolean;
};

export type ConfirmClaim = {
  requestId: number;
  sessionId: string;
  image: ImageBinding;
  enginePid: number;
  engineStarted: number;
  policyVersion: number;
  snapshotId: string;
  actionDigest: string;
  act: ConfirmAct;
};

export type HandoffClaim = {
  requestId: number;
  sessionId: string;
  image: ImageBinding;
  enginePid: number;
  engineStarted: number;
  policyVersion: number;
  act: "handled";
};

/** A handled handoff may send a new message. It never replays or clears Stop. */
export type HandoffFollow = {
  text: string;
  clearStop: false;
  replay: false;
};

const PNG = [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a];
const MAX_CROP_BYTES = 256 * 1024;
const MAX_CROP_EDGE = 4096;

export function isTerminalConfirm(phase: ConfirmPhase): boolean {
  return phase !== "pending" && phase !== "submitting";
}

export function reduceConfirm(phase: ConfirmPhase, event: ConfirmEvent): ConfirmPhase {
  if (isTerminalConfirm(phase)) return phase;
  if (event.type === "settled") return event.end;
  if (phase === "pending" && event.type === "submit") return "submitting";
  if (phase === "submitting" && event.type === "resolve") return event.end;
  if (phase === "submitting" && event.type === "fault") return "pending";
  return phase;
}

export function reduceHandoff(phase: HandoffPhase, event: HandoffEvent): HandoffPhase {
  if (phase === "handled" || phase === "closed") return phase;
  if (event.type === "settled") return event.end;
  if (phase === "pending" && event.type === "submit") return "submitting";
  if (phase === "submitting" && event.type === "resolve") return event.end;
  if (phase === "submitting" && event.type === "fault") return "pending";
  return phase;
}

/** Data URL for a redacted PNG. Any other crop returns null and must not be shown. */
export function redactedCropSrc(crop: CropView): string | null {
  if (crop.kind !== "redacted") return null;
  if (crop.mime !== "image/png") return null;
  if (
    crop.width < 1 ||
    crop.height < 1 ||
    crop.width > MAX_CROP_EDGE ||
    crop.height > MAX_CROP_EDGE
  ) {
    return null;
  }
  if (crop.bytes.length < PNG.length || crop.bytes.length > MAX_CROP_BYTES) return null;
  if (!PNG.every((byte, index) => crop.bytes[index] === byte)) return null;
  let binary = "";
  for (const byte of crop.bytes) binary += String.fromCharCode(byte & 0xff);
  return `data:image/png;base64,${btoa(binary)}`;
}

export function handoffContinuation(text: string): HandoffFollow {
  return { text, clearStop: false, replay: false };
}

export function confirmClaimFor(card: ConfirmCard, act: ConfirmAct = DEFAULT_CONFIRM_ACT): ConfirmClaim {
  return {
    requestId: card.requestId,
    sessionId: card.sessionId,
    image: card.image,
    enginePid: card.enginePid,
    engineStarted: card.engineStarted,
    policyVersion: card.policyVersion,
    snapshotId: card.snapshotId,
    actionDigest: card.actionDigest,
    act,
  };
}

export function handoffClaimFor(card: HandoffCard): HandoffClaim {
  return {
    requestId: card.requestId,
    sessionId: card.sessionId,
    image: card.image,
    enginePid: card.enginePid,
    engineStarted: card.engineStarted,
    policyVersion: card.policyVersion,
    act: "handled",
  };
}

export function bindSensitive<T extends { requestId: number }>(cards: T[], card: T): T[] {
  if (cards.some((item) => item.requestId === card.requestId)) return cards;
  return [...cards, card];
}

/** Drop sensitive cards for one session, or every card when `all` is set. */
export function evictSensitive<T extends { sessionId: string }>(
  cards: T[],
  scope: { sessionId?: string; all?: boolean },
): T[] {
  if (scope.all) return [];
  if (!scope.sessionId) return cards;
  return cards.filter((card) => card.sessionId !== scope.sessionId);
}

export function cardsForSession<T extends { sessionId: string }>(cards: T[], sessionId: string): T[] {
  return cards.filter((card) => card.sessionId === sessionId);
}
