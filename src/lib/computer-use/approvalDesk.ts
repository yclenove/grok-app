/** Pending Computer Use approvals. A card exists only after a real request. */

export type Act = "turn" | "session" | "always" | "deny" | "timeout";

export type Fault = "absent" | "identity" | "replay" | "failed" | "stale";

/** Snake-case facts inside `proof`. They match the stored grant, not the card's camelCase. */
export type GrantProof = {
  cdhash?: string | null;
  certificate_sha256?: string | null;
  product?: string | null;
  build?: string | null;
  file_dev?: number | null;
  file_inode?: number | null;
};

export type ImageBinding = {
  platform: "macos" | "windows" | "linux";
  key: string;
  teamId: string | null;
  publisher: string | null;
  sha256Hex: string | null;
  signature: "valid" | "invalid" | "unsigned" | "unknown";
  imagePath: string;
  pid: number | null;
  /** Absent and empty are the same. A changed code hash is a different image. */
  proof?: GrantProof | null;
};

export type Card = {
  requestId: number;
  sessionId: string;
  enginePid: number;
  engineStarted: number;
  policyVersion: number;
  image: ImageBinding;
  name: string;
  reason: string;
  risk: "low" | "high";
  timeoutSecs: number;
  openedMs: number;
  hasIcon: boolean;
  alwaysEnabled: boolean;
};

/** What the user presented. Name and icon are not part of it. */
export type Claim = {
  requestId: number;
  sessionId: string;
  image: ImageBinding;
  enginePid: number;
  engineStarted: number;
  policyVersion: number;
  act: Act;
};

export type Desk = {
  cards: Card[];
  consumed: number[];
};

const LIMIT = 280;

/** Same plain-text rules as the Host desk. Identity fields are not passed here. */
export function sanitizePlain(input: string): string {
  let out = "";
  let count = 0;
  for (const ch of input) {
    if (count >= LIMIT) break;
    count += 1;
    if (ch === "\n" || ch === "\r" || ch === "\t") {
      out += " ";
      continue;
    }
    const code = ch.codePointAt(0) ?? 0;
    if (isHiddenControl(code)) {
      out += "\uFFFD";
      continue;
    }
    out += ch;
  }
  return out;
}

function isHiddenControl(code: number): boolean {
  return (
    code < 0x20 ||
    (code >= 0x7f && code <= 0x9f) ||
    code === 0x61c ||
    code === 0x200e ||
    code === 0x200f ||
    (code >= 0x202a && code <= 0x202e) ||
    (code >= 0x2066 && code <= 0x2069)
  );
}

export function emptyDesk(): Desk {
  return { cards: [], consumed: [] };
}

export function bindCard(desk: Desk, card: Card): Desk {
  if (desk.consumed.includes(card.requestId)) return desk;
  if (desk.cards.some((item) => item.requestId === card.requestId)) return desk;
  return { ...desk, cards: [...desk.cards, card] };
}

export function applySettled(desk: Desk, requestId: number): Desk {
  if (desk.consumed.includes(requestId) && !desk.cards.some((card) => card.requestId === requestId)) {
    return desk;
  }
  return {
    cards: desk.cards.filter((card) => card.requestId !== requestId),
    consumed: desk.consumed.includes(requestId) ? desk.consumed : [...desk.consumed, requestId],
  };
}

export function cardsForSession(desk: Desk, sessionId: string): Card[] {
  return desk.cards.filter((card) => card.sessionId === sessionId);
}

export function claimFor(card: Card, act: Act): Claim {
  return {
    requestId: card.requestId,
    sessionId: card.sessionId,
    image: card.image,
    enginePid: card.enginePid,
    engineStarted: card.engineStarted,
    policyVersion: card.policyVersion,
    act,
  };
}

export function assess(card: Card, claim: Claim): Fault | null {
  if (card.requestId !== claim.requestId) return "absent";
  if (
    card.sessionId !== claim.sessionId ||
    card.enginePid !== claim.enginePid ||
    card.engineStarted !== claim.engineStarted ||
    card.policyVersion !== claim.policyVersion ||
    !sameImage(card.image, claim.image)
  ) {
    return "identity";
  }
  return null;
}

export function faultFrom(error: unknown): Fault {
  const text = typeof error === "string" ? error : error instanceof Error ? error.message : "";
  if (
    text === "absent" ||
    text === "identity" ||
    text === "replay" ||
    text === "failed" ||
    text === "stale"
  ) {
    return text;
  }
  return "failed";
}

function sameImage(left: ImageBinding, right: ImageBinding): boolean {
  return (
    left.platform === right.platform &&
    left.key === right.key &&
    left.teamId === right.teamId &&
    left.publisher === right.publisher &&
    left.sha256Hex === right.sha256Hex &&
    left.signature === right.signature &&
    left.imagePath === right.imagePath &&
    left.pid === right.pid &&
    sameProof(left.proof, right.proof)
  );
}

function blank(value: string | null | undefined): string | null {
  if (value == null) return null;
  const text = value.trim();
  return text === "" ? null : text;
}

function sameProof(left: GrantProof | null | undefined, right: GrantProof | null | undefined): boolean {
  const a = left ?? {};
  const b = right ?? {};
  return (
    blank(a.cdhash) === blank(b.cdhash) &&
    blank(a.certificate_sha256) === blank(b.certificate_sha256) &&
    blank(a.product) === blank(b.product) &&
    blank(a.build) === blank(b.build) &&
    (a.file_dev ?? null) === (b.file_dev ?? null) &&
    (a.file_inode ?? null) === (b.file_inode ?? null)
  );
}
