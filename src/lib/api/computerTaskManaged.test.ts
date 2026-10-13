import { beforeEach, expect, it, vi } from "vitest";

const { invoke, isDesktopHost } = vi.hoisted(() => ({ invoke: vi.fn(), isDesktopHost: vi.fn() }));
vi.mock("./host", () => ({ invoke, isDesktopHost }));
import { computerTaskManagedApprove } from "./computerTaskManaged";

const key = { connectionId: "synthetic:1", taskId: "task", requestId: "request", revision: 2 };
beforeEach(() => { invoke.mockReset(); isDesktopHost.mockReturnValue(true); });

it("sends only the original full request address after explicit managed consent", async () => {
  const receipt = { requestId: key.requestId, revision: key.revision, runId: "owned-run", decisionClass: "synthetic" };
  invoke.mockResolvedValueOnce(receipt);
  const supplied = { ...key, runId: "untrusted-run", profile: "untrusted-profile", nativeProof: "untrusted" };
  expect(await computerTaskManagedApprove("chat", supplied)).toBe(receipt);
  expect(invoke).toHaveBeenCalledExactlyOnceWith("computer_use_task_managed_approve", { sessionId: "chat", key });
});

it("does not create an environment from a browser or mirror", async () => {
  isDesktopHost.mockReturnValue(false);
  await expect(computerTaskManagedApprove("chat", key)).rejects.toThrow("local desktop host");
  expect(invoke).not.toHaveBeenCalled();
});

it("preserves preparation failures rather than reporting an approval", async () => {
  invoke.mockRejectedValueOnce(new Error("original request retired"));
  await expect(computerTaskManagedApprove("chat", key)).rejects.toThrow("original request retired");
});
