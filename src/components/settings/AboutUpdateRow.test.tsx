// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createT } from "@/i18n";
import type { UpdateStatus } from "@/hooks/useUpdater";

const updater = vi.hoisted(() => ({
  status: { state: "error", message: "cleanup pending", version: "9.9.9", restartPending: true } as UpdateStatus,
  channelInfo: { channel: "silent", pluginEnabled: true, platformSupported: true, endpoint: "" },
  checkForUpdate: vi.fn(),
  installAndRelaunch: vi.fn(),
  applyAvailableUpdate: vi.fn(),
  githubReleasesUrl: "https://example.invalid/releases",
}));
vi.mock("@/hooks/UpdaterProvider", () => ({ useUpdaterContext: () => updater }));
vi.mock("@/lib/api", () => ({ isDesktopHost: () => true, openExternalUrl: vi.fn() }));
vi.mock("@/components/UpdateInstallConfirmModal", () => ({ UpdateInstallConfirmModal: () => null }));
vi.mock("@/components/ui/tooltip", () => ({ Tip: ({ children }: { children: React.ReactNode }) => children }));
vi.mock("@/lib/updateSim", () => ({ isUpdateSimActive: () => false }));

import { AboutUpdateRow } from "./AboutUpdateRow";
import { SidebarUpdateButton } from "../SidebarUpdateButton";

beforeEach(() => {
  vi.clearAllMocks();
  updater.status = { state: "error", message: "cleanup pending", version: "9.9.9", restartPending: true };
  updater.applyAvailableUpdate.mockResolvedValue({ kind: "installing" });
});
afterEach(cleanup);

describe("installed update cleanup recovery UI", () => {
  it("disables About and sidebar replay when the native outcome cannot be verified", () => {
    updater.status = { state: "error", version: "9.9.9", message: "launch unknown", installPending: true, installBlocked: true };
    render(<><AboutUpdateRow t={createT("en")} /><SidebarUpdateButton t={createT("en")} /></>);
    expect(screen.getByRole("alert").textContent).toContain("Repeating installation is blocked");
    expect(screen.getByRole("alert").textContent).not.toContain("Retry using the same");
    for (const button of screen.getAllByRole("button")) {
      expect((button as HTMLButtonElement).disabled).toBe(true);
      fireEvent.click(button);
    }
    expect(updater.installAndRelaunch).not.toHaveBeenCalled();
    expect(updater.applyAvailableUpdate).not.toHaveBeenCalled();
    expect(updater.checkForUpdate).not.toHaveBeenCalled();
  });
  it("distinguishes a retained Windows package from an installed update", () => {
    updater.status = { state: "error", message: "native cleanup refused", version: "9.9.9", installPending: true };
    render(<AboutUpdateRow t={createT("en")} />);
    const alert = screen.getByRole("alert").textContent;
    expect(alert).toContain("9.9.9");
    expect(alert).toContain("installation is not confirmed");
    expect(alert).toContain("native cleanup refused");
    expect(alert).not.toContain("is installed");
    expect(screen.queryByRole("button", { name: "Retry cleanup and restart" })).toBeNull();
    expect((screen.getByRole("button", { name: "Check for updates" }) as HTMLButtonElement).disabled).toBe(true);
    fireEvent.click(screen.getByRole("button", { name: "Retry installation" }));
    expect(updater.installAndRelaunch).toHaveBeenCalledTimes(1);
    expect(updater.checkForUpdate).not.toHaveBeenCalled();
  });

  it("keeps the sidebar retry bound to the pending native installation", () => {
    updater.status = { state: "error", message: "launch refused", version: "9.9.9", installPending: true };
    render(<SidebarUpdateButton t={createT("en")} />);
    fireEvent.click(screen.getByRole("button", { name: "Retry installation" }));
    expect(updater.applyAvailableUpdate).toHaveBeenCalledTimes(1);
    expect(updater.checkForUpdate).not.toHaveBeenCalled();
  });

  it("uses Chinese installation recovery copy without claiming installation succeeded", () => {
    updater.status = { state: "error", message: "cleanup refused", version: "9.9.9", installPending: true };
    render(<AboutUpdateRow t={createT("zh")} />);
    expect(screen.getByRole("button", { name: "重试安装" })).toBeTruthy();
    expect(screen.getByRole("alert").textContent).toContain("尚未确认安装完成");
  });

  it("shows the installed version, cleanup error, and a real About retry action", () => {
    render(<AboutUpdateRow t={createT("en")} />);
    expect(screen.getByRole("alert").textContent).toContain("cleanup pending");
    expect(screen.getByRole("alert").textContent).toContain("9.9.9");
    expect(screen.getByRole("alert").textContent).toContain("will not be installed again");
    expect(screen.queryByRole("button", { name: "Install and restart" })).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Retry cleanup and restart" }));
    expect(updater.installAndRelaunch).toHaveBeenCalledTimes(1);
    expect(updater.checkForUpdate).not.toHaveBeenCalled();
    expect((screen.getByRole("button", { name: "Check for updates" }) as HTMLButtonElement).disabled).toBe(true);
  });

  it("retains the sidebar action after failed cleanup instead of hiding recovery", () => {
    render(<SidebarUpdateButton t={createT("en")} />);
    fireEvent.click(screen.getByRole("button", { name: "Retry cleanup and restart" }));
    expect(updater.applyAvailableUpdate).toHaveBeenCalledTimes(1);
  });

  it("does not offer restart-only recovery for an update that failed to install", () => {
    updater.status = { state: "error", message: "install failed" };
    render(<AboutUpdateRow t={createT("en")} />);
    expect(screen.queryByRole("button", { name: "Retry cleanup and restart" })).toBeNull();
    expect((screen.getByRole("button", { name: "Check for updates" }) as HTMLButtonElement).disabled).toBe(false);
  });

  it("uses localized recovery copy in Chinese", () => {
    render(<AboutUpdateRow t={createT("zh")} />);
    expect(screen.getByRole("button", { name: "重试清理并重启" })).toBeTruthy();
    expect(screen.getByRole("alert").textContent).toContain("不会再次安装更新");
  });
  it("disables both action surfaces while native cleanup is still pending", () => {
    updater.status = { state: "preparing-restart", version: "9.9.9" };
    render(<><AboutUpdateRow t={createT("en")} /><SidebarUpdateButton t={createT("en")} /></>);
    const buttons = screen.getAllByRole("button", { name: "Preparing restart…" });
    expect(buttons).toHaveLength(2);
    for (const button of buttons) {
      expect((button as HTMLButtonElement).disabled).toBe(true);
      fireEvent.click(button);
    }
    expect(updater.installAndRelaunch).not.toHaveBeenCalled();
    expect(updater.applyAvailableUpdate).not.toHaveBeenCalled();
  });
});
