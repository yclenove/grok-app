import { describe, expect, it } from "vitest";
import {
  DEFAULT_CONFIRM_ACT,
  bindSensitive,
  cardsForSession,
  confirmClaimFor,
  handoffContinuation,
  redactedCropSrc,
  reduceConfirm,
  reduceHandoff,
  type ConfirmCard,
  type CropView,
} from "./confirmState";

const png: CropView = {
  kind: "redacted",
  mime: "image/png",
  width: 8,
  height: 8,
  bytes: [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 1, 2],
};

function card(over: Partial<ConfirmCard> = {}): ConfirmCard {
  return {
    requestId: 7,
    sessionId: "sess",
    enginePid: 41,
    engineStarted: 9001,
    policyVersion: 1,
    image: {
      platform: "linux",
      key: "linux:editor",
      teamId: null,
      publisher: "Desk",
      sha256Hex: "ab".repeat(32),
      signature: "valid",
      imagePath: "/usr/bin/editor",
      pid: 77,
    },
    name: "Editor",
    summary: "Click Run",
    reason: "",
    snapshotId: "snap-1",
    actionDigest: "ab".repeat(32),
    crop: { kind: "missing" },
    timeoutSecs: 120,
    openedMs: 1_500,
    hasIcon: false,
    ...over,
  };
}

describe("confirmState", () => {
  it("moves pending to submitting once, then to the server end", () => {
    expect(reduceConfirm("pending", { type: "submit" })).toBe("submitting");
    expect(reduceConfirm("submitting", { type: "submit" })).toBe("submitting");
    expect(reduceConfirm("submitting", { type: "resolve", end: "allowed" })).toBe("allowed");
    expect(reduceConfirm("allowed", { type: "submit" })).toBe("allowed");
    expect(reduceConfirm("denied", { type: "resolve", end: "allowed" })).toBe("denied");
  });

  it("defaults to deny and keeps a terminal settled state", () => {
    expect(DEFAULT_CONFIRM_ACT).toBe("deny");
    expect(confirmClaimFor(card()).act).toBe("deny");
    expect(reduceConfirm("pending", { type: "settled", end: "closed" })).toBe("closed");
    expect(reduceConfirm("submitting", { type: "settled", end: "timed_out" })).toBe("timed_out");
    expect(reduceConfirm("stale", { type: "settled", end: "allowed" })).toBe("stale");
    expect(reduceConfirm("submitting", { type: "fault" })).toBe("pending");
  });

  it("refuses a crop that is missing, unlocated, or not a PNG", () => {
    expect(redactedCropSrc({ kind: "missing" })).toBeNull();
    expect(redactedCropSrc({ kind: "unlocated" })).toBeNull();
    expect(redactedCropSrc({ ...png, mime: "image/jpeg" })).toBeNull();
    expect(redactedCropSrc({ ...png, bytes: [0x47, 0x49, 0x46, 0x38, 0x39, 0x61, 0, 0] })).toBeNull();
    const src = redactedCropSrc(png);
    expect(src?.startsWith("data:image/png;base64,")).toBe(true);
    expect(src).not.toContain("GIF");
  });

  it("keeps cards across sessions and does not replay a handoff", () => {
    const cards = bindSensitive(
      bindSensitive([], card({ sessionId: "alpha" })),
      card({ requestId: 8, sessionId: "beta" }),
    );
    expect(cardsForSession(cards, "beta").map((item) => item.requestId)).toEqual([8]);
    expect(cardsForSession(cards, "alpha")).toHaveLength(1);
    expect(bindSensitive(cards, card({ sessionId: "alpha" }))).toHaveLength(2);
    const follow = handoffContinuation("look again");
    expect(follow).toEqual({ text: "look again", clearStop: false, replay: false });
    expect(reduceHandoff("pending", { type: "submit" })).toBe("submitting");
    expect(reduceHandoff("submitting", { type: "resolve", end: "handled" })).toBe("handled");
    expect(reduceHandoff("handled", { type: "submit" })).toBe("handled");
    expect(reduceHandoff("pending", { type: "settled", end: "closed" })).toBe("closed");
  });
});
