import { beforeEach, expect, test, vi } from "vitest";
import { sessionPendingPermission, sessionResolvePermission } from "./session";
const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("./host", () => ({ invoke }));
beforeEach(() => vi.resetAllMocks());

test("Tauri/mirror permission IPC echoes the opaque event identity", async () => {
  const args = { rpcId: 0, requestGeneration: "85c578b5-7582-4a49-b44c-91f547757d71", decision: "deny", sessionId: "background" };
  await sessionResolvePermission(args);
  expect(invoke).toHaveBeenCalledExactlyOnceWith("session_resolve_permission", {
    ...args, optionId: null, scopeKey: null, options: null, toolName: null,
  });
});
test("legacy missing identity stays missing for the Host compatibility check", async () => {
  await sessionResolvePermission({ rpcId: 81, decision: "deny" });
  expect(invoke.mock.calls[0][1].requestGeneration).toBeNull();
});
test("pending-card recovery preserves its original identity", async () => {
  const restored = { rpcId: 81, sessionId: "chat", requestGeneration: "original" };
  invoke.mockResolvedValue(restored);
  expect(await sessionPendingPermission("chat")).toBe(restored);
});
