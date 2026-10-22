import { beforeEach, expect, it, vi } from "vitest";
import { sessionQueueSnapshot } from "./sessionQueue";
const host = vi.hoisted(() => ({ has: true, invoke: vi.fn() }));
vi.mock("./host", () => ({ hasHost: () => host.has, invoke: host.invoke }));
beforeEach(() => { host.has = true; host.invoke.mockReset().mockResolvedValue({ version: 1, items: [] }); });
it("invokes only the read-only Host projection", async () => {
  await expect(sessionQueueSnapshot()).resolves.toEqual({ version: 1, items: [] });
  expect(host.invoke.mock.calls).toEqual([["session_api_queue_snapshot"]]);
});
it("does not fake an empty queue in an ordinary browser", async () => {
  host.has = false; await expect(sessionQueueSnapshot()).rejects.toThrow("SESSION_QUEUE_UNAVAILABLE");
  expect(host.invoke).not.toHaveBeenCalled();
});
it("rejects malformed and failed Host reads instead of clearing the list", async () => {
  host.invoke.mockResolvedValueOnce({ version: 99, items: [] });
  await expect(sessionQueueSnapshot()).rejects.toThrow("SESSION_QUEUE_UNAVAILABLE");
  host.invoke.mockRejectedValueOnce(new Error("synthetic"));
  await expect(sessionQueueSnapshot()).rejects.toThrow("synthetic");
});
