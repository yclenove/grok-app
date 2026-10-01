import { beforeEach, expect, it, vi } from "vitest";
import { invoke, isDesktopHost } from "./host";
import { computerHelperAction, computerHelperStatus, type HelperStatus } from "./computerGnomeHelper";
vi.mock("./host", () => ({ invoke: vi.fn(), isDesktopHost: vi.fn() }));
beforeEach(() => { vi.resetAllMocks(); vi.mocked(isDesktopHost).mockReturnValue(true); });
const status: HelperStatus = { available: true, busy: false, featureEnabled: false, state: "disabled",
  installation: "current", shellVersion: "46.0", actions: ["enable"] };
it("never invokes the desktop helper from a browser or mirror client", async () => {
  vi.mocked(isDesktopHost).mockReturnValue(false);
  expect((await computerHelperStatus()).available).toBe(false);
  await expect(computerHelperAction("install")).rejects.toThrow("unavailable");
  expect(invoke).not.toHaveBeenCalled();
});
it("retains one mutation across callers and clears eligibility until its original reply", async () => {
  let resolve!: (value: HelperStatus) => void;
  const original = new Promise<HelperStatus>((r) => { resolve = r; });
  vi.mocked(invoke).mockReturnValueOnce(original).mockResolvedValue(status);
  const first = computerHelperAction("enable");
  await expect(computerHelperAction("disable")).rejects.toThrow("busy");
  expect(await computerHelperStatus()).toEqual({ ...status, busy: true, actions: [] });
  expect(invoke).toHaveBeenCalledTimes(2);
  resolve(status); await first;
  expect(await computerHelperStatus()).toEqual(status);
  expect(invoke).toHaveBeenCalledWith("computer_use_helper_action", { action: "enable" });
});
it("native errors release the latch without automatically replaying the action", async () => {
  vi.mocked(invoke).mockRejectedValueOnce(new Error("unknown outcome")).mockResolvedValue(status);
  await expect(computerHelperAction("repair")).rejects.toThrow("unknown outcome");
  expect(invoke).toHaveBeenCalledTimes(1);
  expect(await computerHelperStatus()).toEqual(status);
});
