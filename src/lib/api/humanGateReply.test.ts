import { beforeEach, describe, expect, it, vi } from "vitest";
import { sessionPendingPlan, sessionResolveAskUser, sessionResolvePlan, sessionPlanChromeSet } from "./session";
import { settleAskUserDecision } from "@/lib/askUser/askUserSettle";

const host = vi.hoisted(() => ({ invoke: vi.fn(), isTauri: () => true, isMirrorClient: () => false }));
vi.mock("./host", () => host);
beforeEach(() => host.invoke.mockReset().mockResolvedValue({}));

describe("human gate IPC identity", () => {
  it("echoes the displayed plan identity without changing the decision", async () => {
    await sessionResolvePlan({ decision: "cancelled", feedback: "请修改", sessionId: "chat", rpcId: 0, requestGeneration: "plan-generation" });
    expect(host.invoke).toHaveBeenCalledWith("session_resolve_plan", {
      decision: "cancelled", feedback: "请修改", sessionId: "chat", rpcId: 0, requestGeneration: "plan-generation",
    });
  });

  it("passes the questionnaire identity through the production settle consumer", async () => {
    await settleAskUserDecision({
      payload: { sessionId: "chat", rpcId: 70, requestGeneration: "ask-generation" },
      decision: "accepted", answers: { "方案": "甲" },
      resolve: sessionResolveAskUser,
    });
    expect(host.invoke).toHaveBeenCalledWith("session_resolve_ask_user", {
      decision: "accepted", answers: { "方案": "甲" }, sessionId: "chat", rpcId: 70, requestGeneration: "ask-generation",
    });
  });

  it("requests a live identity only for the requested chat", async () => {
    await sessionPendingPlan("chat");
    expect(host.invoke).toHaveBeenCalledWith("session_pending_plan", { sessionId: "chat" });
  });

  it("requires and echoes a conditional chrome revision without adding it to stored chrome", async () => {
    await expect(sessionPlanChromeSet("chat", { userClosed: true })).rejects.toMatchObject({ code: "PLAN_CHROME_CHANGED" });
    expect(host.invoke).not.toHaveBeenCalled();
    await sessionPlanChromeSet("chat", { userClosed: true }, "captured-revision");
    expect(host.invoke).toHaveBeenCalledExactlyOnceWith("session_plan_chrome_set", {
      sessionId: "chat", chrome: { userClosed: true }, expectedRevision: "captured-revision",
    });
  });
});
