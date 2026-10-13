import { beforeEach, expect, it, vi } from "vitest";

const { invoke, isDesktopHost } = vi.hoisted(() => ({ invoke: vi.fn(), isDesktopHost: vi.fn() }));
vi.mock("./host", () => ({ invoke, isDesktopHost }));
import { computerTaskPickerApprove, computerTaskPickerBegin, computerTaskPickerCancel, computerTaskPickerLoad, computerTaskPickerPreparation, computerTaskPickerPairingConfirm } from "./computerTaskPicker";

const key = { connectionId: "synthetic:1", taskId: "task", requestId: "request", revision: 2 };
const picker = { sessionId: "synthetic-app", key, pickerId: "picker", maxSelection: 8, surface: "desktop" as const };

beforeEach(() => {
  invoke.mockReset();
  isDesktopHost.mockReturnValue(true);
});

it("only begins discovery after the explicit request/surface gesture", async () => {
  invoke.mockResolvedValueOnce(picker);
  expect(await computerTaskPickerBegin(picker.sessionId, key, "desktop")).toBe(picker);
  expect(invoke).toHaveBeenCalledExactlyOnceWith("computer_use_task_picker_begin", { sessionId: picker.sessionId, key, surface: "desktop" });
});

it.each([
  ["load", computerTaskPickerLoad],
  ["cancel", computerTaskPickerCancel],
  ["preparation", computerTaskPickerPreparation],
] as const)("binds %s to the original picker while it is loading", async (operation, call) => {
  await call(picker);
  expect(invoke).toHaveBeenCalledExactlyOnceWith(`computer_use_task_picker_${operation}`, { sessionId: picker.sessionId, key, pickerId: picker.pickerId });
});

it("approves only picker-local references without requesting legacy authorization", async () => {
  await computerTaskPickerApprove(picker, ["candidate-a", "candidate-b"]);
  expect(invoke).toHaveBeenCalledExactlyOnceWith("computer_use_task_picker_approve", { sessionId: picker.sessionId, key, pickerId: picker.pickerId, candidateIds: ["candidate-a", "candidate-b"] });
});

it("refuses all picker mutations and discovery from mirrors or browser clients", async () => {
  isDesktopHost.mockReturnValue(false);
  for (const call of [() => computerTaskPickerBegin(picker.sessionId, key, "desktop"), () => computerTaskPickerLoad(picker), () => computerTaskPickerCancel(picker), () => computerTaskPickerApprove(picker, ["candidate"]), () => computerTaskPickerPreparation(picker), () => computerTaskPickerPairingConfirm(picker, "nonce")]) {
    await expect(call()).rejects.toThrow("local desktop host");
  }
  expect(invoke).not.toHaveBeenCalled();
});

it("task pairing sends only the original full address and nonce, never an endpoint or legacy pairing request", async () => {
  const existing = { ...picker, surface: "existing-tabs" as const };
  await computerTaskPickerPairingConfirm(existing, "original-nonce");
  expect(invoke).toHaveBeenCalledExactlyOnceWith("computer_use_task_picker_pairing_confirm", { sessionId: picker.sessionId, key, pickerId: picker.pickerId, nonce: "original-nonce" });
});

it("keeps retirement errors visible instead of reporting a successful approval", async () => {
  invoke.mockRejectedValueOnce(new Error("picker retired"));
  await expect(computerTaskPickerApprove(picker, ["candidate"])).rejects.toThrow("picker retired");
});
