// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import "@testing-library/jest-dom/vitest";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ComputerUseOverlay } from "./ComputerUseOverlay";
import { overlayControl, sampleCursor, type OverlayFrame } from "@/lib/computer-use/overlay";

const frame: OverlayFrame = {
  phase: "running", message: "<b>fixture</b>", connected: true,
  cursor: { x: 100, y: 80, kind: "pointer" }, revision: 1, response_ms: 350,
};
afterEach(() => { cleanup(); vi.restoreAllMocks(); });

describe("ComputerUseOverlay", () => {
  it("synthetic gestures cannot invoke trusted Resume", async () => {
    await expect(overlayControl("resume", { isTrusted: false })).rejects.toThrow("untrusted");
  });
  it("draws cursor states and treats engine status as plain text", () => {
    const { rerender } = render(<ComputerUseOverlay frame={frame} locale="en" controls onControl={vi.fn()} />);
    expect(screen.getByRole("status").textContent).toContain("<b>fixture</b>");
    expect(document.querySelector("b")).toBeNull();
    rerender(<ComputerUseOverlay frame={{ ...frame, cursor: { x: 5, y: 7, kind: "drag" } }} locale="en" onControl={vi.fn()} />);
    expect(screen.getByTestId("cu-cursor").getAttribute("data-kind")).toBe("drag");
  });

  it("Stop sends once while busy and explicit Resume is a separate action", async () => {
    let finish!: () => void;
    const control = vi.fn(() => new Promise<void>((resolve) => { finish = resolve; }));
    const { rerender } = render(<ComputerUseOverlay frame={frame} locale="en" controls onControl={control} />);
    fireEvent.click(screen.getByRole("button", { name: "Stop" }));
    fireEvent.click(screen.getByRole("button", { name: "Stop" }));
    expect(control).toHaveBeenCalledTimes(1);
    expect(control).toHaveBeenCalledWith("stop", expect.any(Object));
    await act(async () => finish());
    rerender(<ComputerUseOverlay frame={{ ...frame, phase: "stopped", cursor: null }} locale="en" controls onControl={control} />);
    expect(screen.queryByTestId("cu-cursor")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Resume" }));
    expect(control).toHaveBeenLastCalledWith("resume", expect.any(Object));
    await act(async () => finish());
  });

  it("show, updates and buttons never focus the overlay or steal chat Enter", async () => {
    const focus = vi.spyOn(HTMLElement.prototype, "focus");
    const control = vi.fn().mockResolvedValue(undefined);
    const { rerender } = render(<ComputerUseOverlay frame={frame} locale="en" controls onControl={control} />);
    fireEvent.keyDown(document, { key: "Enter" });
    expect(control).not.toHaveBeenCalled();
    rerender(<ComputerUseOverlay frame={{ ...frame, phase: "stopped" }} locale="en" controls onControl={control} />);
    expect(focus).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Resume" }));
    await waitFor(() => expect(control).toHaveBeenCalledTimes(1));
    expect(focus).not.toHaveBeenCalled();
  });

  it("failed control stays actionable with localized error and no automatic resume", async () => {
    const control = vi.fn().mockRejectedValue(new Error("secret diagnostic"));
    render(<ComputerUseOverlay frame={{ ...frame, phase: "stopped" }} locale="en" controls onControl={control} />);
    fireEvent.click(screen.getByRole("button", { name: "Resume" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Could not update Computer Use.");
    expect(screen.queryByText("secret diagnostic")).toBeNull();
    expect(control).toHaveBeenCalledTimes(1);
  });

  it("disconnected Stop latch keeps Resume disabled and hidden tasks render nothing", () => {
    const { rerender } = render(<ComputerUseOverlay frame={{ ...frame, phase: "stopped", connected: false }} locale="en" controls onControl={vi.fn()} />);
    expect(screen.getByRole("button", { name: "Resume" })).toBeDisabled();
    rerender(<ComputerUseOverlay frame={{ ...frame, phase: "hidden" }} locale="en" controls onControl={vi.fn()} />);
    expect(screen.queryByRole("status")).toBeNull();
  });

  it("analytic cursor converges with missed frames and reduced motion snaps", () => {
    const start = { x: -50, y: 0 };
    const end = { x: 100, y: 60 };
    expect(sampleCursor(start, end, 0)).toEqual(start);
    expect(sampleCursor(start, end, 100).x).toBeGreaterThan(start.x);
    expect(sampleCursor(start, end, 3000).x).toBeCloseTo(end.x);
    expect(sampleCursor(start, end, 1, true)).toEqual(end);
  });
});
