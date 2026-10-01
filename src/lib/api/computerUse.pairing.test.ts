import { beforeEach, describe, expect, it, vi } from "vitest";

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("./host", () => ({ isTauri: () => true, invoke }));
import { computerBeginPairing, computerConfirmPairingApp, computerRevokePairing } from "./computerUse";

beforeEach(() => invoke.mockReset());
describe("Computer Use pairing IPC", () => {
  it("returns the App-only code and public endpoint for the user to transfer", async () => {
    const challenge = { nonce: "challenge", instanceId: "instance", verificationCode: "ABCDE-ABCDE-ABCDE-ABCDE",
      endpoint: "http://127.0.0.1:32100", installedExtensionId: "extension", expiresAtMs: 12345 };
    invoke.mockResolvedValueOnce(challenge);
    await expect(computerBeginPairing()).resolves.toEqual(challenge);
    expect(invoke).toHaveBeenCalledExactlyOnceWith("computer_use_begin_pairing");
  });
  it("binds App confirmation to the challenge actually displayed", async () => {
    await computerConfirmPairingApp("challenge-a");
    expect(invoke).toHaveBeenCalledExactlyOnceWith("computer_use_confirm_pairing_app", { nonce: "challenge-a" });
  });
  it("propagates a stale confirmation rejection", async () => {
    invoke.mockRejectedValueOnce(new Error("pairing challenge changed"));
    await expect(computerConfirmPairingApp("old")).rejects.toThrow("pairing challenge changed");
  });
  it("revokes the extension through the Host", async () => {
    await computerRevokePairing();
    expect(invoke).toHaveBeenCalledExactlyOnceWith("computer_use_revoke_pairing");
  });
});
