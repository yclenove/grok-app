// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { AsideResizeHandle } from "./AsideResizeHandle";

afterEach(cleanup);
const control = () => ({ value: 458, min: 458, max: 574, begin: vi.fn(), change: vi.fn() });

describe("AsideResizeHandle", () => {
  it("does not steal application shortcuts or Tab navigation", () => {
    const resize = control();
    render(<AsideResizeHandle control={resize} label="Resize side pane" busy={false} />);
    const handle = screen.getByRole("separator");
    for (const options of [
      { key: "Tab" }, { key: "ArrowLeft", ctrlKey: true },
      { key: "Home", altKey: true }, { key: "End", metaKey: true },
    ]) expect(fireEvent.keyDown(handle, options)).toBe(true);
    expect(resize.change).not.toHaveBeenCalled();
    expect(fireEvent.keyDown(handle, { key: "ArrowLeft" })).toBe(false);
    expect(resize.change).toHaveBeenCalledOnce();
  });

  it("does not change keyboard geometry during a pointer drag", () => {
    const resize = control();
    render(<AsideResizeHandle control={resize} label="Resize side pane" busy />);
    fireEvent.keyDown(screen.getByRole("separator"), { key: "End" });
    expect(resize.change).not.toHaveBeenCalled();
  });

  it("only starts a primary-button drag and preserves keyboard focus", () => {
    const resize = control();
    render(<AsideResizeHandle control={resize} label="Resize side pane" busy={false} />);
    const handle = screen.getByRole("separator");
    // jsdom has no native PointerEvent; the handler only needs the button field.
    fireEvent(handle, new MouseEvent("pointerdown", { bubbles: true, button: 2 }));
    expect(resize.begin).not.toHaveBeenCalled();
    fireEvent(handle, new MouseEvent("pointerdown", { bubbles: true, button: 0 }));
    expect(resize.begin).toHaveBeenCalledOnce();
    expect(document.activeElement).toBe(handle);
  });
});
