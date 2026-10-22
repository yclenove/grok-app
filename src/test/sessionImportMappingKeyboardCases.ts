import { fireEvent } from "@testing-library/react";
import { expect, it } from "vitest";
import { installDialogFocus } from "@/lib/a11yFocus";

/** Mapping shortcut contracts run against the Host integration fixture.
 * Keeping this focused suite separate avoids duplicating its provider mocks. */
export function mappingKeyboardCases(renderMapping: () => HTMLElement, commit: () => unknown) {
  it.each([
    { key: "Enter", isComposing: true },
    { key: "Enter", keyCode: 229, isComposing: false },
    { key: "Enter", ctrlKey: true },
    { key: "Enter", altKey: true },
    { key: "Enter", metaKey: true },
    { key: "Enter", shiftKey: true },
  ])("mapping does not publish for composition or a modified Enter: %j", (init) => {
    const pane = renderMapping();
    const event = new KeyboardEvent("keydown", { bubbles: true, cancelable: true, ...init });
    fireEvent(pane, event);
    expect(commit).not.toHaveBeenCalled();
    expect(event.defaultPrevented).toBe(false);
  });

  it("mapping respects an Enter already consumed by a descendant", () => {
    const pane = renderMapping();
    const label = pane.querySelector("li span") as HTMLElement;
    const consume = (event: Event) => event.preventDefault();
    label.addEventListener("keydown", consume);
    try {
      const event = new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true });
      fireEvent(label, event);
      expect(event.defaultPrevented).toBe(true);
      expect(commit).not.toHaveBeenCalled();
    } finally {
      label.removeEventListener("keydown", consume);
    }
  });

  it("mapping cannot publish beneath an active dialog and regains Enter after it closes", () => {
    const pane = renderMapping();
    const overlay = document.createElement("div");
    overlay.setAttribute("role", "dialog");
    const input = document.createElement("input");
    overlay.append(input);
    document.body.append(overlay);
    const stop = installDialogFocus(() => overlay, { initialFocus: "none" });
    try {
      input.focus();
      const event = new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true });
      fireEvent(pane, event);
      expect(commit).not.toHaveBeenCalled();
      expect(event.defaultPrevented).toBe(false);
      expect(input).toHaveFocus();
    } finally {
      stop();
      overlay.remove();
    }
    fireEvent.keyDown(pane, { key: "Enter" });
    expect(commit).toHaveBeenCalledTimes(1);
  });

}
