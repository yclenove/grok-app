import { beforeEach, describe, expect, it, vi } from "vitest";
import { closedSessionPlan, emptySessionPlan, mergePlanFromEvent } from "@/lib/planSession";
import { loadRestoredPlan } from "./restorePlan";

const api = vi.hoisted(() => ({
  sessionPlanChromeGet: vi.fn(),
  sessionAgentPlanSnapshot: vi.fn(),
  sessionPendingPlan: vi.fn(),
}));
vi.mock("@/lib/api", () => api);

beforeEach(() => {
  vi.clearAllMocks();
  api.sessionPlanChromeGet.mockResolvedValue({ body: "保存的计划", visible: true, rpcId: 70 });
  api.sessionAgentPlanSnapshot.mockResolvedValue({ found: false });
  api.sessionPendingPlan.mockResolvedValue(null);
});

describe("live plan restore identity", () => {
  it("never resurrects a reverse RPC from persisted chrome", async () => {
    const restored = await loadRestoredPlan("chat", "Plan");
    expect(restored.rpcId).toBeNull();
    expect(restored.requestGeneration).toBeFalsy();
    expect(restored.body).toBe("保存的计划");
  });

  it("keeps the conditional chrome identity for both persisted and agent-only read-only plans", async () => {
    for (const chrome of [{ body: "保存的计划", chromeRevision: "restored-revision" }, { chromeRevision: "agent-only-revision" }]) {
      api.sessionPlanChromeGet.mockResolvedValue(chrome);
      api.sessionAgentPlanSnapshot.mockResolvedValue({ found: true, planBody: "代理草稿", awaitingPlanApproval: true });
      expect(await loadRestoredPlan("chat", "计划")).toMatchObject({ rpcId: null, chromeRevision: chrome.chromeRevision });
    }
  });

  it("a lingering agent awaiting flag cannot reopen a user-closed plan without a new live gate", async () => {
    api.sessionPlanChromeGet.mockResolvedValue({ userClosed: true, closedToolCallId: "old-tool", chromeRevision: "closed-revision" });
    api.sessionAgentPlanSnapshot.mockResolvedValue({ found: true, awaitingPlanApproval: true, planBody: "尚未清除的代理草稿" });
    expect(await loadRestoredPlan("chat", "计划")).toMatchObject({ userClosed: true, visible: false,
      rpcId: null, chromeRevision: "closed-revision", awaitingAgentApproval: false });
  });

  it("marks a persisted RPC zero as stale instead of treating it as absent", async () => {
    api.sessionPlanChromeGet.mockResolvedValue({ body: "保存的计划", visible: true, rpcId: 0 });
    expect(await loadRestoredPlan("chat", "Plan")).toMatchObject({ rpcId: null, gateStale: true });
  });

  it("restores a current Host gate, including RPC zero", async () => {
    api.sessionPendingPlan.mockResolvedValue({ sessionId: "chat", rpcId: 0, requestGeneration: "new", body: "当前计划" });
    const restored = await loadRestoredPlan("chat", "Plan");
    expect(restored).toMatchObject({ rpcId: 0, requestGeneration: "new", body: "当前计划", gateStale: false });
  });

  it("keeps the live mirror gate when optional disk/snapshot queries are unavailable", async () => {
    api.sessionPlanChromeGet.mockRejectedValue(new Error("unsupported"));
    api.sessionAgentPlanSnapshot.mockRejectedValue(new Error("unsupported"));
    api.sessionPendingPlan.mockResolvedValue({ sessionId: "chat", rpcId: 70, requestGeneration: "new", body: "Live" });
    expect(await loadRestoredPlan("chat", "Plan")).toMatchObject({ rpcId: 70, requestGeneration: "new", body: "Live" });
  });

  it("keeps persisted read-only content when the optional live query is unavailable", async () => {
    api.sessionPendingPlan.mockRejectedValue(new Error("unsupported pending-plan query"));
    expect(await loadRestoredPlan("chat", "Plan")).toMatchObject({ rpcId: null, body: "保存的计划" });
  });

  it.each([
    { sessionId: "other", rpcId: 70, requestGeneration: "new" },
    { sessionId: "chat", rpcId: 70 },
    { sessionId: "chat", rpcId: -1, requestGeneration: "new" },
    { sessionId: "chat", rpcId: 0.5, requestGeneration: "new" },
    { sessionId: "chat", rpcId: 70, requestGeneration: " " },
    { sessionId: "chat", rpcId: 70, requestGeneration: 42 },
  ])("rejects a foreign or invalid live restore %j", async (card) => {
    api.sessionPendingPlan.mockResolvedValue(card);
    expect((await loadRestoredPlan("chat", "Plan")).rpcId).toBeNull();
  });

  it("reopens a new generation after closing the same RPC and tool", () => {
    const closed = closedSessionPlan("Plan", "tool", 70, "old");
    const next = mergePlanFromEvent(closed, { rpcId: 70, toolCallId: "tool", requestGeneration: "new" }, "Plan");
    expect(next).toMatchObject({ userClosed: false, rpcId: 70, requestGeneration: "new" });
    expect(mergePlanFromEvent(closed, { rpcId: 70, toolCallId: "tool", requestGeneration: "old" }, "Plan")).toBe(closed);
  });

  it("progress preserves the gate identity, while a legacy new gate cannot inherit it", () => {
    const first = mergePlanFromEvent(emptySessionPlan(), { rpcId: 70, requestGeneration: "first" }, "Plan");
    expect(mergePlanFromEvent(first, { body: "更新" }, "Plan").requestGeneration).toBe("first");
    expect(mergePlanFromEvent(first, { rpcId: 71, body: "Legacy" }, "Plan").requestGeneration).toBeNull();
  });
});
