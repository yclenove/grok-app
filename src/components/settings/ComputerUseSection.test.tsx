/** @vitest-environment jsdom */
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import "@testing-library/jest-dom/vitest";
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { ComputerUseSection } from "./ComputerUseSection";
import * as api from "@/lib/api/computerUse";
import { prepareForAppUpdate } from "@/lib/api/system";
import { relaunch } from "@tauri-apps/plugin-process";
import type { Approval, Doctor, Settings } from "@/lib/computer-use/settings";
import { judgeRelease, type ReleaseEvidence } from "@/lib/computer-use/releaseGate";
import { createT, LOCALES } from "@/i18n";
import { buildSettingsHash, parseSettingsHash, searchSettingsEntries } from "@/lib/settingsCatalog";

vi.mock("@/lib/api/host", () => ({ isDesktopHost: () => true }));
vi.mock("@/lib/api/computerUse", () => ({
  cuSettingsGet: vi.fn(), cuSettingsEnabled: vi.fn(), cuSettingsPolicy: vi.fn(),
  cuApprovalsList: vi.fn(), cuApprovalsRevoke: vi.fn(), cuDoctor: vi.fn(), cuOpenSystemSettings: vi.fn(),
}));
vi.mock("@tauri-apps/plugin-process", () => ({ relaunch: vi.fn() }));
vi.mock("@/lib/api/system", () => ({ prepareForAppUpdate: vi.fn() }));
const defaults: Settings = { enabled: true, confirmation: "standard", foreground: "ask", hotkey: "default" };
const row: Approval = { id: "exact-image", name: "Editor", imagePath: "/fixture/editor", grantedMs: 1234, icon: { mime: "image/png", width: 1, height: 1, bytes: [137,80,78,71,13,10,26,10] } };
const doctor: Doctor = { ready: true, issues: [], capabilities: { platform: "macos", version: "15", observe: { tree: true, screenshot: true, events: false }, delivery: { accessibility: true, targeted_pointer: false, targeted_keys: false, foreground: "policy" }, background_operation: "partial", user_activity_guard: true } };
function deferred<T>() { let resolve!: (v: T) => void; let reject!: (e: Error) => void; const promise = new Promise<T>((a,b) => {resolve=a;reject=b;}); return { promise, resolve, reject }; }
beforeEach(() => {
  vi.resetAllMocks();
  vi.mocked(api.cuSettingsGet).mockResolvedValue(defaults);
  vi.mocked(api.cuApprovalsList).mockResolvedValue([]);
  vi.mocked(api.cuDoctor).mockResolvedValue(doctor);
  vi.mocked(api.cuSettingsEnabled).mockResolvedValue();
  vi.mocked(api.cuSettingsPolicy).mockResolvedValue();
  vi.mocked(api.cuApprovalsRevoke).mockResolvedValue();
  vi.mocked(api.cuOpenSystemSettings).mockResolvedValue();
});
afterEach(cleanup);

describe("ComputerUseSection", () => {
  it("lists_loading_empty_and_error_with_retry", async () => {
    const request = deferred<Approval[]>();
    vi.mocked(api.cuApprovalsList).mockReturnValueOnce(request.promise);
    render(<ComputerUseSection locale="en" />);
    const card = screen.getByRole("region", { name: "Always allowed apps" });
    expect(within(card).getByText("Loading…")).toBeInTheDocument();
    await act(async () => request.reject(new Error("private/path")));
    expect(within(card).getByRole("alert")).toHaveTextContent("Could not load");
    expect(screen.queryByText("private/path")).not.toBeInTheDocument();
    fireEvent.click(within(card).getByRole("button", { name: "Try again" }));
    expect(await within(card).findByText("No persistent approvals.")).toBeInTheDocument();
  });

  it("revoke_requires_dialog_confirmation_disables_duplicates_and_refreshes_list", async () => {
    vi.mocked(api.cuApprovalsList).mockResolvedValueOnce([row]).mockResolvedValue([]);
    const mutation = deferred<void>();
    vi.mocked(api.cuApprovalsRevoke).mockReturnValue(mutation.promise);
    render(<ComputerUseSection locale="en" />);
    expect(await screen.findByText("Editor")).toBeInTheDocument();
    expect(screen.getByRole("img")).toHaveAttribute("src", expect.stringContaining("data:image/png"));
    expect(screen.getByText(/Granted/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Revoke" }));
    expect(api.cuApprovalsRevoke).not.toHaveBeenCalled();
    const dialog = screen.getByRole("dialog");
    await waitFor(() => expect(within(dialog).getByRole("button", { name: "Cancel" })).toHaveFocus());
    const confirm = within(dialog).getByRole("button", { name: "Revoke" });
    fireEvent.click(confirm); fireEvent.click(confirm);
    expect(confirm).toBeDisabled();
    expect(api.cuApprovalsRevoke).toHaveBeenCalledExactlyOnceWith("exact-image");
    await act(async () => mutation.resolve());
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    expect(await screen.findByText("No persistent approvals.")).toBeInTheDocument();
  });

  it("revoke_failure_preserves_dialog_and_allows_retry_or_escape", async () => {
    vi.mocked(api.cuApprovalsList).mockResolvedValue([row]);
    vi.mocked(api.cuApprovalsRevoke).mockRejectedValueOnce(new Error("failed"));
    render(<ComputerUseSection locale="en" />);
    fireEvent.click(await screen.findByRole("button", { name: "Revoke" }));
    fireEvent.click(within(screen.getByRole("dialog")).getByRole("button", { name: "Revoke" }));
    await waitFor(() => expect(within(screen.getByRole("dialog")).getByRole("alert")).toHaveTextContent("Could not save"));
    fireEvent.keyDown(document, { key: "Escape" });
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("policy_selects_persist_real_values_and_switch_disables_while_saving", async () => {
    render(<ComputerUseSection locale="en" />);
    const confirmation = await screen.findByRole("button", { name: "Sensitive action confirmation" });
    await waitFor(() => expect(confirmation).toBeEnabled());
    fireEvent.click(confirmation);
    fireEvent.click(await screen.findByRole("button", { name: "Strict — all changes" }));
    await waitFor(() => expect(api.cuSettingsPolicy).toHaveBeenCalledWith("strict", "ask"));
    await waitFor(() => expect(screen.getByRole("button", { name: "Foreground input" })).toBeEnabled());
    fireEvent.click(screen.getByRole("button", { name: "Foreground input" }));
    fireEvent.click(await screen.findByRole("button", { name: "Never" }));
    await waitFor(() => expect(api.cuSettingsPolicy).toHaveBeenCalledWith("standard", "never"));
    const mutation = deferred<void>();
    vi.mocked(api.cuSettingsEnabled).mockReturnValue(mutation.promise);
    await waitFor(() => expect(screen.getByRole("switch")).toBeEnabled());
    fireEvent.click(screen.getByRole("switch")); fireEvent.click(screen.getByRole("switch"));
    expect(api.cuSettingsEnabled).toHaveBeenCalledExactlyOnceWith(false);
    expect(screen.getByRole("switch")).toBeDisabled();
    await act(async () => mutation.resolve());
  });

  it.each(["accessibility", "screen_recording"] as const)("mac_missing_%s_opens_only_its_system_panel", async (kind) => {
    vi.mocked(api.cuDoctor).mockResolvedValue({ ...doctor, ready: false, issues: [`permission missing: ${kind}`] });
    render(<ComputerUseSection locale="en" />);
    const title = kind === "accessibility" ? "Accessibility" : "Screen Recording";
    const region = await screen.findByRole("region", { name: title });
    expect(within(region).getByText("Not allowed")).toBeInTheDocument();
    fireEvent.click(within(region).getByRole("button", { name: "Open system settings" }));
    await waitFor(() => expect(api.cuOpenSystemSettings).toHaveBeenCalledWith(kind));
  });

  it("doctor_failure_unknown_permission_and_retry_never_claim_ready", async () => {
    vi.mocked(api.cuDoctor).mockRejectedValueOnce(new Error("failed"));
    render(<ComputerUseSection locale="en" />);
    const region = screen.getByRole("region", { name: "System permissions" });
    fireEvent.click(await within(region).findByRole("button", { name: "Try again" }));
    expect(await within(region).findByText("Ready")).toBeInTheDocument();
  });

  it.each(["windows", "linux"] as const)("%s_displays_platform_guidance_without_mac_settings_controls", async (platform) => {
    vi.mocked(api.cuDoctor).mockResolvedValue({ ...doctor, issues: platform === "linux" ? ["session_type: wayland"] : [], capabilities: { ...doctor.capabilities, platform } });
    render(<ComputerUseSection locale="en" />);
    expect(await screen.findByText(platform === "windows" ? /No additional system permission/ : /Wayland asks/)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Open system settings" })).not.toBeInTheDocument();
    expect(screen.getByText("Ctrl+Alt+Shift+Backspace")).toBeInTheDocument();
  });

  it("feature_off_does_not_probe_and_displays_enable_guidance", async () => {
    vi.mocked(api.cuSettingsGet).mockResolvedValue({ ...defaults, enabled: false });
    render(<ComputerUseSection locale="en" />);
    expect(await screen.findByText("Enable Computer Use to check system permissions.")).toBeInTheDocument();
    expect(api.cuDoctor).not.toHaveBeenCalled();
  });
  it("unrelated_doctor_failure_keeps_permissions_unknown_and_input_guard_honest", async () => {
    vi.mocked(api.cuDoctor).mockResolvedValue({ ...doctor, ready: false, issues: ["probe timed out"], capabilities: { ...doctor.capabilities, user_activity_guard: false } });
    render(<ComputerUseSection locale="en" />);
    expect(await screen.findByText("Some capabilities are unavailable")).toBeInTheDocument();
    for (const name of ["Accessibility", "Screen Recording", "Input Monitoring"]) {
      expect(within(screen.getByRole("region", { name })).getByText("Could not verify")).toBeInTheDocument();
    }
    expect(screen.queryByText("Ready")).not.toBeInTheDocument();
  });

  it("settings_load_failure_disables_controls_until_retry", async () => {
    vi.mocked(api.cuSettingsGet).mockRejectedValueOnce(new Error("private/path"));
    render(<ComputerUseSection locale="en" />);
    const region = screen.getByRole("region", { name: "Computer Use" });
    const retry = await within(region).findByRole("button", { name: "Try again" });
    expect(screen.getByRole("switch")).toBeDisabled();
    expect(api.cuDoctor).not.toHaveBeenCalled();
    fireEvent.click(retry);
    await waitFor(() => expect(screen.getByRole("switch")).toBeEnabled());
  });

  it("screen_recording_grant_keeps_restart_entry_after_permission_refresh", async () => {
    vi.mocked(api.cuDoctor).mockResolvedValueOnce({ ...doctor, ready: false, issues: ["permission missing: screen_recording"] }).mockResolvedValue(doctor);
    render(<ComputerUseSection locale="en" />);
    const region = await screen.findByRole("region", { name: "Screen Recording" });
    fireEvent.click(within(region).getByRole("button", { name: "Open system settings" }));
    await waitFor(() => expect(screen.getByRole("switch")).toBeEnabled());
    fireEvent.click(within(screen.getByRole("region", { name: "System permissions" })).getByRole("button", { name: "Refresh" }));
    expect(await screen.findByText("Ready")).toBeInTheDocument();
    const restart = screen.getByRole("button", { name: "Restart Grok" });
    await waitFor(() => expect(restart).toBeEnabled());
    fireEvent.click(restart);
    await waitFor(() => expect(prepareForAppUpdate).toHaveBeenCalledOnce());
    await waitFor(() => expect(relaunch).toHaveBeenCalledOnce());
    expect(prepareForAppUpdate).toHaveBeenCalledOnce();
  });

  it("legacy_grant_has_unknown_time_and_bad_icon_falls_back_without_remote_sources", async () => {
    vi.mocked(api.cuApprovalsList).mockResolvedValue([{ ...row, grantedMs: null, icon: { ...row.icon!, mime: "image/svg+xml" } }]);
    render(<ComputerUseSection locale="en" />);
    expect(await screen.findByText("Grant time unavailable")).toBeInTheDocument();
    expect(screen.queryByRole("img")).not.toBeInTheDocument();
    expect(screen.getByText("Editor")).toBeInTheDocument();
  });

  it("system_settings_busy_disables_repeated_clicks_and_failure_stays_visible", async () => {
    vi.mocked(api.cuDoctor).mockResolvedValue({ ...doctor, ready: false, issues: ["permission missing: accessibility"] });
    const opening = deferred<void>();
    vi.mocked(api.cuOpenSystemSettings).mockReturnValue(opening.promise);
    render(<ComputerUseSection locale="en" />);
    const region = await screen.findByRole("region", { name: "Accessibility" });
    const button = within(region).getByRole("button", { name: "Open system settings" });
    fireEvent.click(button); fireEvent.click(button);
    expect(button).toBeDisabled();
    expect(api.cuOpenSystemSettings).toHaveBeenCalledExactlyOnceWith("accessibility");
    await act(async () => opening.reject(new Error("private/path")));
    expect(screen.getByRole("alert")).toHaveTextContent("Could not save");
    expect(screen.queryByText("private/path")).not.toBeInTheDocument();
  });

  it("returning_from_system_settings_refreshes_permission_status", async () => {
    vi.mocked(api.cuDoctor).mockResolvedValueOnce({ ...doctor, ready: false, issues: ["permission missing: accessibility"] }).mockResolvedValue(doctor);
    render(<ComputerUseSection locale="en" />);
    expect(await screen.findByText("Some capabilities are unavailable")).toBeInTheDocument();
    fireEvent(window, new Event("focus"));
    expect(await screen.findByText("Ready")).toBeInTheDocument();
    expect(within(screen.getByRole("region", { name: "Accessibility" })).getByText("Allowed")).toBeInTheDocument();
  });

});

describe("Computer Use release channel", () => {
  function passed(): ReleaseEvidence {
    const layers: ReleaseEvidence["layers"] = {};
    const l6: ReleaseEvidence["l6"] = {};
    for (const platform of ["macos", "windows", "linux"]) {
      layers[platform] = {};
      for (let index = 1; index <= 7; index += 1) layers[platform][`L${index}`] = "pass";
      l6[platform] = { native_bp: 9000, text_bp: 9000, overall_bp: 9000 };
    }
    return {
      source: "runner", layers, l6, security_signed: Array(10).fill(true),
      screenshot_chain_redacted: true, unreliable_screenshot_refused: true,
      ide_unknown_stop_regression: true, performance_budget_met: true,
      compatibility_apps: 30, signed_and_notarized: true, red_team_violations: 0,
    };
  }

  it("stays experimental and does not add a promotion control", async () => {
    render(<ComputerUseSection locale="en" />);
    const region = screen.getByRole("region", { name: "Release" });
    expect(within(region).getByRole("status")).toHaveTextContent("Experimental");
    expect(within(region).getByText("Stays experimental until the evaluation gate passes.")).toBeInTheDocument();
    expect(within(region).queryByRole("switch")).not.toBeInTheDocument();
    expect(within(region).queryByRole("button")).not.toBeInTheDocument();
    expect(screen.getByText("Control local apps with your approval. Experimental.")).toBeInTheDocument();
    expect(api.cuSettingsEnabled).not.toHaveBeenCalled();
  });

  it("shows the formal label only for a passing runner report", async () => {
    const evidence = passed();
    expect(judgeRelease(evidence).defaultFormal).toBe(true);
    render(<ComputerUseSection locale="en" evidence={evidence} />);
    const region = screen.getByRole("region", { name: "Release" });
    expect(within(region).getByRole("status")).toHaveTextContent("Standard");
    expect(within(region).queryByRole("switch")).not.toBeInTheDocument();
    expect(screen.getByText("Control local apps with your approval.")).toBeInTheDocument();
    expect(screen.queryByText("Control local apps with your approval. Experimental.")).not.toBeInTheDocument();
    expect(api.cuSettingsEnabled).not.toHaveBeenCalled();
  });

  it("search_hits_the_release_row", () => {
    expect(searchSettingsEntries("release gate", createT("en")).some(hit => hit.entry.id === "ext.computer.release")).toBe(true);
    expect(searchSettingsEntries("实验", createT("zh")).some(hit => hit.entry.id === "ext.computer.release")).toBe(true);
    expect(searchSettingsEntries(createT("zh")("cu.settings.release"), createT("zh")).some(hit => hit.entry.id === "ext.computer.release")).toBe(true);
  });
});

describe("Computer Use settings navigation", () => {
  it("deep_link_and_search_land_on_live_computer_tab", () => {
    expect(parseSettingsHash("#/settings/extensions/computer")).toEqual({ section: "extensions", tab: "computer" });
    expect(buildSettingsHash({ section: "extensions", tab: "computer" })).toBe("#/settings/extensions/computer");
    for (const query of ["computer use", "电脑操作", "桌面控制", "撤销", "foreground", "hotkey"]) {
      expect(searchSettingsEntries(query, createT("en")).some(h => h.entry.tab === "computer")).toBe(true);
    }
  });
  it("searches_the_translated_title_in_every_shipped_locale", () => {
    for (const locale of LOCALES) {
      const t = createT(locale);
      expect(searchSettingsEntries(t("cu.settings.confirmation"), t).some(h => h.entry.id === "ext.computer.confirmation")).toBe(true);
    }
  });
});
