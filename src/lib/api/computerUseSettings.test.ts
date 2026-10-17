import { beforeEach, describe, expect, it, vi } from "vitest";
import { cuApprovalsList, cuApprovalsRevoke, cuDoctor, cuOpenSystemSettings, cuSettingsEnabled, cuSettingsGet, cuSettingsPolicy } from "./computerUse";
import { invoke, isDesktopHost } from "./host";
vi.mock("./host", () => ({ invoke: vi.fn(), isDesktopHost: vi.fn() }));
beforeEach(() => { vi.resetAllMocks(); vi.mocked(isDesktopHost).mockReturnValue(false); });

describe("Computer Use trusted settings bridge", () => {
  it("mirror_or_browser_cannot_invoke_any_settings_command", async () => {
    const calls = [() => cuSettingsGet(), () => cuSettingsEnabled(true), () => cuSettingsPolicy("strict", "never"),
      () => cuApprovalsList(), () => cuApprovalsRevoke("image"), () => cuDoctor(), () => cuOpenSystemSettings("accessibility")];
    for (const call of calls) await expect(call()).rejects.toThrow("desktop-only");
    expect(invoke).not.toHaveBeenCalled();
  });
  it("desktop_forwards_exact_revoke_identity_and_policy_values", async () => {
    vi.mocked(isDesktopHost).mockReturnValue(true);
    await cuApprovalsRevoke("exact-build");
    await cuSettingsPolicy("strict", "never");
    expect(invoke).toHaveBeenNthCalledWith(1, "cu_approvals_revoke", { id: "exact-build" });
    expect(invoke).toHaveBeenNthCalledWith(2, "cu_settings_policy", { confirmation: "strict", foreground: "never" });
  });
});
