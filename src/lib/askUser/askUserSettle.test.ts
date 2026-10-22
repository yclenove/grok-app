import { describe, expect, it, vi } from "vitest";
import { askUserDismissLocked, settleAskUserDecision } from "./askUserSettle";

describe("questionnaire transport", () => {
  it("keeps Dismiss clickable during an active accept", () => {
    expect(askUserDismissLocked(false)).toBe(false);
    expect(askUserDismissLocked(true)).toBe(false);
  });

  it.each(["accepted", "cancelled"] as const)("echoes the request identity: %s", async (decision) => {
    const resolve = vi.fn().mockResolvedValue(undefined);
    expect(await settleAskUserDecision({
      payload: { rpcId: 0, sessionId: "chat", requestGeneration: "generation" },
      decision,
      answers: decision === "accepted" ? { q: "甲" } : undefined,
      resolve,
    })).toEqual({ kind: "delivered" });
    expect(resolve).toHaveBeenCalledExactlyOnceWith({
      rpcId: 0,
      sessionId: "chat",
      requestGeneration: "generation",
      decision,
      answers: decision === "accepted" ? { q: "甲" } : null,
    });
  });

  it.each(["accepted", "cancelled"] as const)("reports a failed write without declaring it dismissed: %s", async (decision) => {
    const error = new Error("synthetic stdin timeout");
    expect(await settleAskUserDecision({
      payload: { rpcId: 8, sessionId: "chat" },
      decision,
      resolve: vi.fn().mockRejectedValue(error),
    })).toEqual({ kind: "failed", error });
  });

  it("keeps the legacy wire without inventing a generation", async () => {
    const resolve = vi.fn().mockResolvedValue(undefined);
    await settleAskUserDecision({
      payload: { rpcId: 7, sessionId: "legacy" },
      decision: "cancelled",
      resolve,
    });
    expect(resolve).toHaveBeenCalledExactlyOnceWith({
      rpcId: 7,
      sessionId: "legacy",
      decision: "cancelled",
      answers: null,
    });
  });
});
