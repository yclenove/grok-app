import { beforeEach, expect, it, vi } from "vitest";

const { invoke, isDesktopHost } = vi.hoisted(() => ({ invoke: vi.fn(), isDesktopHost: vi.fn() }));
vi.mock("./host", () => ({ invoke, isDesktopHost }));
import { computerTaskDecide, computerTaskPause, computerTaskResume, computerTaskPreview, computerTaskReadback, computerTaskChooseUpload, computerTaskCancelUpload } from "./computerTask";

beforeEach(() => {
  invoke.mockReset();
  isDesktopHost.mockReturnValue(true);
});

it("upload commands carry the original opaque request, not local paths or presentation", async () => {
  const key = { connectionId: "original", taskId: "task", requestId: "scope", revision: 2 };
  const address = { key, runId: "run", generation: "18446744073709551614" };
  const row = { ...address, targetName: "private page", upload: { requestId: "opaque" }, path: "never sent" };
  await computerTaskChooseUpload("chat", row, "opaque", "Choose a file");
  expect(invoke).toHaveBeenLastCalledWith("computer_use_task_choose_upload", {
    sessionId: "chat", address, uploadRequestId: "opaque", dialogTitle: "Choose a file",
  });
  await computerTaskCancelUpload("chat", row, "opaque");
  expect(invoke).toHaveBeenLastCalledWith("computer_use_task_cancel_upload", { sessionId: "chat", address, uploadRequestId: "opaque" });
});

it("mirror and web clients cannot choose or cancel Host uploads", async () => {
  isDesktopHost.mockReturnValue(false);
  const address = { key: { connectionId: "1", taskId: "task", requestId: "scope", revision: 1 }, runId: "run", generation: "1" };
  await expect(computerTaskChooseUpload("chat", address, "opaque", "Choose")).rejects.toThrow("local desktop host");
  await expect(computerTaskCancelUpload("chat", address, "opaque")).rejects.toThrow("local desktop host");
  expect(invoke).not.toHaveBeenCalled();
});

it("reads no-run requests through the dedicated Host metadata command", async () => {
  const view = { sessionId: "synthetic-local-app", request: { phase: "pending" }, executions: [] };
  invoke.mockResolvedValueOnce(view);
  expect(await computerTaskReadback("synthetic-local-app")).toBe(view);
  expect(invoke).toHaveBeenCalledExactlyOnceWith("computer_use_task_readback", { sessionId: "synthetic-local-app" });
});

it.each(["deny", "cancel", "stop"] as const)("binds %s to the original request, task and connection", async (decision) => {
  const key = { connectionId: "1:1:1", taskId: "synthetic-task", requestId: "synthetic-request", revision: 2 };
  const receipt = { sessionId: "synthetic-local-app", key, decision };
  invoke.mockResolvedValueOnce(receipt);
  expect(await computerTaskDecide("synthetic-local-app", key, decision)).toBe(receipt);
  expect(invoke).toHaveBeenCalledExactlyOnceWith("computer_use_task_decide", { sessionId: "synthetic-local-app", key, decision });
});

it("does not grant mirror or browser clients local readback/decision access", async () => {
  isDesktopHost.mockReturnValue(false);
  expect(await computerTaskReadback("synthetic-app")).toBeNull();
  await expect(computerTaskDecide("synthetic-app", { connectionId: "1:1:1", taskId: "task", requestId: "request", revision: 1 }, "stop")).rejects.toThrow("local desktop host");
  expect(invoke).not.toHaveBeenCalled();
});

it("propagates Host retirement instead of inventing a successful decision", async () => {
  invoke.mockRejectedValueOnce(new Error("request retired"));
  await expect(computerTaskDecide("synthetic-app", { connectionId: "1:1:1", taskId: "task", requestId: "request", revision: 1 }, "deny")).rejects.toThrow("request retired");
});

it("task pause and preview use full original execution identity without legacy run lookup", async () => {
  const address = { key: { connectionId: "synthetic-connection", taskId: "task", requestId: "request", revision: 1 }, runId: "synthetic-run", generation: "18446744073709551614" };
  invoke.mockResolvedValueOnce(undefined);
  await computerTaskPause("synthetic-app", address);
  expect(invoke).toHaveBeenLastCalledWith("computer_use_task_pause", { sessionId: "synthetic-app", address });
  const frame = { address, previewDataUrl: null, truncated: false };
  invoke.mockResolvedValueOnce(frame);
  expect(await computerTaskPreview("synthetic-app", address)).toBe(frame);
  expect(invoke).toHaveBeenLastCalledWith("computer_use_task_preview", { sessionId: "synthetic-app", address });
});

it("mirror and browser cannot pause or capture through task commands", async () => {
  isDesktopHost.mockReturnValue(false);
  const address = { key: { connectionId: "1", taskId: "task", requestId: "request", revision: 1 }, runId: "run", generation: "1" };
  await expect(computerTaskPause("synthetic-app", address)).rejects.toThrow("local desktop host");
  await expect(computerTaskResume("synthetic-app", address)).rejects.toThrow("local desktop host");
  await expect(computerTaskPreview("synthetic-app", address)).rejects.toThrow("local desktop host");
  expect(invoke).not.toHaveBeenCalled();
});

it("resumes only the original execution and preserves the full decimal generation", async () => {
  const address = { key: { connectionId: "synthetic-connection", taskId: "task", requestId: "request", revision: 1 }, runId: "synthetic-run", generation: "18446744073709551614" };
  const receipt = { phase: "resumed", address: { ...address, generation: "18446744073709551615" } };
  invoke.mockResolvedValueOnce(receipt);
  expect(await computerTaskResume("synthetic-app", { ...address, targetName: "never sent" } as typeof address)).toBe(receipt);
  expect(invoke).toHaveBeenCalledExactlyOnceWith("computer_use_task_resume", { sessionId: "synthetic-app", address });
});

it("passes only correlation fields when the caller supplies a full execution presentation row", async () => {
  const key = { connectionId: "synthetic-connection", taskId: "task", requestId: "request", revision: 1 };
  const row = { key, runId: "synthetic-run", generation: "2", targetName: "Fixture title", phase: "idle", cleanupPending: false };
  invoke.mockResolvedValueOnce(undefined);
  await computerTaskPause("synthetic-app", row);
  expect(invoke).toHaveBeenCalledExactlyOnceWith("computer_use_task_pause", {
    sessionId: "synthetic-app", address: { key, runId: row.runId, generation: row.generation },
  });
});
