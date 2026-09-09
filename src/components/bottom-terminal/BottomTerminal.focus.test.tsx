/**
 * @vitest-environment jsdom
 */
import "@/test/jsdomStubs";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import "@testing-library/jest-dom/vitest";
import { useState } from "react";
import userEvent from "@testing-library/user-event";
import { BottomTerminal } from "./BottomTerminal";
import { BottomTerminalToggle } from "./BottomTerminalToggle";
import {
  closeAllBottomTerminalTabs,
  closeBottomTerminal,
  closeBottomTerminalTab,
  toggleBottomTerminal,
  type BottomTerminalState,
} from "@/lib/bottomTerminal";

vi.mock("@/components/side-workbench/TerminalTab", () => ({
  TerminalTab: (props: { tabId: string }) => (
    <div data-testid="pty-host">{props.tabId}</div>
  ),
}));

afterEach(() => {
  cleanup();
});

const openState: BottomTerminalState = {
  open: true,
  height: 240,
  tabs: [{ id: "t1", sessionKey: "t1", name: "term" }],
  activeId: "t1",
};

function Harness(props: { initial?: BottomTerminalState }) {
  const [state, setState] = useState(props.initial ?? openState);
  return (
    <>
      <BottomTerminalToggle
        locale="en"
        open={state.open}
        onToggle={() => setState((s) => toggleBottomTerminal(s))}
      />
      <BottomTerminal
        locale="en"
        state={state}
        onAddTab={() => {}}
        onCloseTab={(id) => setState((s) => closeBottomTerminalTab(s, id))}
        onCloseAllTabs={() => setState((s) => closeAllBottomTerminalTabs(s))}
        onActivateTab={() => {}}
        onHeightChange={() => {}}
        onClosePanel={() => setState((s) => closeBottomTerminal(s))}
      />
    </>
  );
}

describe("BottomTerminal closed-panel focus", () => {
  it("marks the closed panel inert and returns focus to the toggle", () => {
    render(<Harness />);
    const close = screen.getByTestId("bottom-terminal-close");
    close.focus();
    expect(document.activeElement).toBe(close);
    fireEvent.click(close);
    const panel = screen.getByTestId("bottom-terminal");
    expect(panel).toHaveAttribute("data-open", "false");
    expect(panel).toHaveAttribute("inert");
    expect(document.activeElement).toBe(
      screen.getByTestId("bottom-terminal-toggle"),
    );
    expect(screen.getByTestId("pty-host")).toBeInTheDocument();
    expect(screen.getByTestId("pty-host")).toHaveTextContent("t1");
  });

  it("close-all drops hosts, closes the panel, and leaves it inert", () => {
    render(<Harness />);
    fireEvent.click(screen.getByTestId("bottom-terminal-close-all"));
    const panel = screen.getByTestId("bottom-terminal");
    expect(panel).toHaveAttribute("data-open", "false");
    expect(panel).toHaveAttribute("inert");
    expect(screen.queryByTestId("pty-host")).toBeNull();
  });

  it("starts closed with inert + aria-hidden while persist hosts stay mounted", () => {
    render(
      <Harness
        initial={{
          open: false,
          height: 240,
          tabs: [{ id: "t1", sessionKey: "t1", name: "term" }],
          activeId: "t1",
        }}
      />,
    );
    const panel = screen.getByTestId("bottom-terminal");
    expect(panel).toHaveAttribute("inert");
    expect(panel).toHaveAttribute("aria-hidden", "true");
    expect(screen.getByTestId("pty-host")).toHaveTextContent("t1");
  });

  it("toggle close keeps the persist host and returns focus to the toggle", () => {
    render(<Harness />);
    screen.getByTestId("bottom-terminal-close").focus();
    fireEvent.click(screen.getByTestId("bottom-terminal-toggle"));
    const panel = screen.getByTestId("bottom-terminal");
    expect(panel).toHaveAttribute("data-open", "false");
    expect(panel).toHaveAttribute("inert");
    expect(screen.getByTestId("pty-host")).toHaveTextContent("t1");
    expect(document.activeElement).toBe(
      screen.getByTestId("bottom-terminal-toggle"),
    );
  });

  it("closing the last tab closes the panel and restores the toggle", () => {
    render(<Harness />);
    const chip = document.querySelector(".rp-tab__x");
    expect(chip).toBeInstanceOf(HTMLElement);
    (chip as HTMLElement).focus();
    fireEvent.click(chip as HTMLElement);
    const panel = screen.getByTestId("bottom-terminal");
    expect(panel).toHaveAttribute("data-open", "false");
    expect(panel).toHaveAttribute("inert");
    expect(screen.queryByTestId("pty-host")).toBeNull();
    expect(document.activeElement).toBe(
      screen.getByTestId("bottom-terminal-toggle"),
    );
  });

  it("closing a non-last tab does not steal focus or close the panel", () => {
    render(
      <Harness
        initial={{
          open: true,
          height: 240,
          tabs: [
            { id: "t1", sessionKey: "t1", name: "term" },
            { id: "t2", sessionKey: "t2", name: "term" },
          ],
          activeId: "t1",
        }}
      />,
    );
    const chips = document.querySelectorAll(".rp-tab__x");
    expect(chips.length).toBe(2);
    fireEvent.click(chips[0] as HTMLElement);
    const panel = screen.getByTestId("bottom-terminal");
    expect(panel).toHaveAttribute("data-open", "true");
    expect(panel).not.toHaveAttribute("inert");
    expect(screen.getAllByTestId("pty-host")).toHaveLength(1);
    expect(document.activeElement).not.toBe(
      screen.getByTestId("bottom-terminal-toggle"),
    );
  });

  it("Tab from the toggle does not land inside a closed persist panel", async () => {
    const user = userEvent.setup();
    render(
      <Harness
        initial={{
          open: false,
          height: 240,
          tabs: [{ id: "t1", sessionKey: "t1", name: "term" }],
          activeId: "t1",
        }}
      />,
    );
    const panel = screen.getByTestId("bottom-terminal");
    const toggle = screen.getByTestId("bottom-terminal-toggle");
    expect(panel).toHaveAttribute("inert");
    expect(screen.getByTestId("bottom-terminal-close")).toHaveAttribute(
      "tabindex",
      "-1",
    );
    expect(screen.getByTestId("pty-host")).toHaveTextContent("t1");
    toggle.focus();
    await user.tab();
    expect(panel.contains(document.activeElement)).toBe(false);
  });
});
