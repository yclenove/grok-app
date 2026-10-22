/** @vitest-environment jsdom */
import "@testing-library/jest-dom/vitest";
import { StrictMode, type ReactNode } from "react";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { CopyIconButton } from "./CopyIconButton";

vi.mock("@/components/ui/tooltip", () => ({
  Tip: ({ children }: { children: ReactNode }) => children,
}));

beforeEach(() => {
  vi.useFakeTimers();
  Object.defineProperty(navigator, "clipboard", {
    configurable: true,
    value: { writeText: vi.fn().mockResolvedValue(undefined) },
  });
});
afterEach(() => {
  cleanup();
  vi.clearAllTimers();
  vi.useRealTimers();
  vi.restoreAllMocks();
});

const button = <CopyIconButton text="local fixture" copyLabel="Copy" copiedLabel="Copied" />;
async function copy() {
  await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Copy" })); });
}

describe("CopyIconButton lifecycle", () => {
  it("cancels the feedback timer when its row unmounts", async () => {
    const view = render(button);
    await copy();
    expect(vi.getTimerCount()).toBe(1);
    view.unmount();
    expect(vi.getTimerCount()).toBe(0);
  });

  it("does not schedule feedback for a clipboard reply arriving after unmount", async () => {
    let resolve!: () => void;
    vi.mocked(navigator.clipboard.writeText).mockReturnValue(new Promise<void>((done) => { resolve = done; }));
    const view = render(button);
    fireEvent.click(screen.getByRole("button", { name: "Copy" }));
    view.unmount();
    await act(async () => { resolve(); });
    expect(vi.getTimerCount()).toBe(0);
  });

  it("still shows and clears feedback after StrictMode effect replay", async () => {
    render(<StrictMode>{button}</StrictMode>);
    await copy();
    expect(screen.getByRole("button")).toHaveClass("is-copied");
    await act(async () => { vi.advanceTimersByTime(1200); });
    expect(screen.getByRole("button")).not.toHaveClass("is-copied");
    expect(vi.getTimerCount()).toBe(0);
  });

  it("restarts feedback without leaving the earlier timer alive", async () => {
    render(button);
    await copy();
    await act(async () => { vi.advanceTimersByTime(1000); });
    await copy();
    expect(vi.getTimerCount()).toBe(1);
    await act(async () => { vi.advanceTimersByTime(200); });
    expect(screen.getByRole("button")).toHaveClass("is-copied");
    await act(async () => { vi.advanceTimersByTime(1000); });
    expect(screen.getByRole("button")).not.toHaveClass("is-copied");
  });

  it("handles a clipboard failure without success feedback or a timer", async () => {
    vi.mocked(navigator.clipboard.writeText).mockRejectedValue(new Error("unavailable"));
    render(button);
    await copy();
    expect(screen.getByRole("button")).not.toHaveClass("is-copied");
    expect(vi.getTimerCount()).toBe(0);
  });

  it.each([{ text: "   ", disabled: false }, { text: "fixture", disabled: true }])(
    "does not copy an idle button: %j", async (props) => {
      render(<CopyIconButton {...props} copyLabel="Copy" copiedLabel="Copied" />);
      await copy();
      expect(navigator.clipboard.writeText).not.toHaveBeenCalled();
      expect(vi.getTimerCount()).toBe(0);
    },
  );

  it("copies the full text without triggering a parent row action", async () => {
    const rowClick = vi.fn();
    render(<div onClick={rowClick}>{button}</div>);
    await copy();
    expect(navigator.clipboard.writeText).toHaveBeenCalledExactlyOnceWith("local fixture");
    expect(rowClick).not.toHaveBeenCalled();
  });
});
