import { beforeEach, expect, it, vi } from "vitest";
import { sessionQueueCancel } from "./sessionQueue";
const host = vi.hoisted(() => ({ has: true, invoke: vi.fn() }));
vi.mock("./host", () => ({ hasHost: () => host.has, invoke: host.invoke }));
const request = { sessionId: "s", itemId: "q", prompt: " exact prompt ", createdAt: 1 };
beforeEach(() => { host.has = true; host.invoke.mockReset().mockResolvedValue({ version: 1, status: "cancelled", sessionId: "s", itemId: "q" }); });
it("sends the complete immutable identity and accepts only its cancellation acknowledgement", async () => {
  await expect(sessionQueueCancel(request)).resolves.toEqual({ version: 1, status: "cancelled", sessionId: "s", itemId: "q" });
  expect(host.invoke.mock.calls).toEqual([["session_api_queue_cancel", { request }]]);
});
it.each([
  { sessionId: "__draft__" }, { sessionId: "imp:history" }, { sessionId: " s " }, { itemId: "" },
  { prompt: " " }, { createdAt: NaN }, { createdAt: 1.5 },
])("rejects invalid identity before IPC: %j", async patch => {
  await expect(sessionQueueCancel({ ...request, ...patch })).rejects.toThrow("SESSION_QUEUE_INVALID");
  expect(host.invoke).not.toHaveBeenCalled();
});
it.each([null, [], {}, { version: 1, status: "queued", sessionId: "s", itemId: "q" },
  { version: 2, status: "cancelled", sessionId: "s", itemId: "q" },
  { version: 1, status: "cancelled", sessionId: "other", itemId: "q" },
  { version: 1, status: "cancelled", sessionId: "s", itemId: "other" },
  { version: 1, status: "cancelled", sessionId: "s", itemId: "q", extra: true },
])("does not treat malformed or cross-owner acknowledgement as success: %j", async result => {
  host.invoke.mockResolvedValue(result);
  await expect(sessionQueueCancel(request)).rejects.toThrow("SESSION_QUEUE_UNAVAILABLE");
});
it("never fakes a cancellation without Host", async () => {
  host.has = false; await expect(sessionQueueCancel(request)).rejects.toThrow("SESSION_QUEUE_UNAVAILABLE");
  expect(host.invoke).not.toHaveBeenCalled();
});
