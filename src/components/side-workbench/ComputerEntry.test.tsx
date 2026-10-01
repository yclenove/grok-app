/** @vitest-environment jsdom */
import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import "@testing-library/jest-dom/vitest";
import "@/test/jsdomStubs";
import { SidePicker } from "./SidePicker";
import { SideTabBar } from "./SideTabBar";

vi.mock("@/components/ui/tooltip", () => ({
  Tip: ({ children }: { children: React.ReactNode }) => children,
}));

afterEach(cleanup);

describe("Computer Use workbench identity", () => {
  it("distinguishes the computer picker from the browser without auto-selecting", async () => {
    const onPick = vi.fn();
    render(<SidePicker locale="en" isGitProject={false} onPick={onPick} />);
    const computer = screen.getByTestId("side-picker-computer");
    expect(computer.querySelector(".tabler-icon-device-desktop")).not.toBeNull();
    expect(screen.getByTestId("side-picker-browser").querySelector(".tabler-icon-world")).not.toBeNull();
    expect(onPick).not.toHaveBeenCalled();
    await userEvent.click(computer);
    expect(onPick).toHaveBeenCalledExactlyOnceWith("computer");
  });

  it("keeps the computer tab identifiable when it is inactive and icon-only", async () => {
    const onActivate = vi.fn();
    const onCloseTab = vi.fn();
    render(<SideTabBar
      locale="en"
      tabs={[
        { id: "computer", kind: "computer", name: "Computer", surface: "desktop" },
        { id: "browser", kind: "browser", name: "Browser" },
      ]}
      activeId="browser"
      isGitProject={false}
      expanded={false}
      onActivate={onActivate}
      onCloseTab={onCloseTab}
      onCloseOtherTabs={vi.fn()}
      onCloseAllTabs={vi.fn()}
      onCloseTabsToLeft={vi.fn()}
      onCloseTabsToRight={vi.fn()}
      onPickNew={vi.fn()}
      onToggleExpand={vi.fn()}
      onToggleSide={vi.fn()}
    />);
    const computer = screen.getByTestId("side-tab-computer");
    expect(computer).toHaveAttribute("aria-selected", "false");
    expect(computer.querySelector(".rp-tab__name")).toBeNull();
    expect(computer.querySelector(".tabler-icon-device-desktop")).not.toBeNull();
    expect(screen.getByTestId("side-tab-browser").querySelector(".tabler-icon-world")).not.toBeNull();
    expect(onActivate).not.toHaveBeenCalled();
    await userEvent.click(computer);
    expect(onActivate).toHaveBeenCalledExactlyOnceWith("computer");
    expect(onCloseTab).not.toHaveBeenCalled();
  });
});
