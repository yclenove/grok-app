/** @vitest-environment jsdom */
import { describe, expect, it } from "vitest";
import { nextReaderHit, readerShortcutDirection } from "./readerNavigation";

describe("nextReaderHit", () => {
  it("starts at the correct end and wraps in both directions", () => {
    const ids = ["a", "b", "c"];
    expect(nextReaderHit(ids, null, 1)).toBe("a");
    expect(nextReaderHit(ids, null, -1)).toBe("c");
    expect(nextReaderHit(ids, "c", 1)).toBe("a");
    expect(nextReaderHit(ids, "a", -1)).toBe("c");
    expect(nextReaderHit(ids, "b", 1)).toBe("c");
    expect(nextReaderHit(ids, "b", -1)).toBe("a");
  });

  it("handles stale anchors, empty results, and a single result", () => {
    expect(nextReaderHit(["a", "b"], "previous-search", -1)).toBe("b");
    expect(nextReaderHit(["a", "b"], "previous-search", 1)).toBe("a");
    for (const direction of [-1, 1] as const) {
      expect(nextReaderHit([], null, direction)).toBeNull();
      expect(nextReaderHit(["only"], "only", direction)).toBe("only");
    }
  });
});

describe("readerShortcutDirection", () => {
  it("handles only plain lowercase n and p", () => {
    expect(readerShortcutDirection(new KeyboardEvent("keydown", { key: "n" }))).toBe(1);
    expect(readerShortcutDirection(new KeyboardEvent("keydown", { key: "p" }))).toBe(-1);
    for (const key of ["N", "P", "Enter", "Escape", "Tab", "Process"]) {
      expect(readerShortcutDirection(new KeyboardEvent("keydown", { key }))).toBeNull();
    }
  });

  it("leaves modified, composing and already-handled events untouched", () => {
    for (const key of ["n", "p"]) {
      for (const flag of ["ctrlKey", "altKey", "metaKey", "shiftKey", "isComposing"]) {
        expect(readerShortcutDirection(new KeyboardEvent("keydown", { key, [flag]: true }))).toBeNull();
      }
      const handled = new KeyboardEvent("keydown", { key, cancelable: true });
      handled.preventDefault();
      expect(readerShortcutDirection(handled)).toBeNull();
    }
  });

  it("ignores IME compatibility keycode even after the composing flag clears", () => {
    for (const key of ["n", "p"]) {
      const event = new KeyboardEvent("keydown", { key, keyCode: 229, isComposing: false });
      expect(readerShortcutDirection(event)).toBeNull();
    }
  });

  it("does not steal keys from text editors or custom typeahead controls", () => {
    const targets: HTMLElement[] = [document.createElement("input"), document.createElement("textarea"), document.createElement("select")];
    for (const mode of ["true", "", "plaintext-only"]) {
      const editor = document.createElement("div");
      editor.setAttribute("contenteditable", mode);
      const child = document.createElement("span");
      editor.append(child);
      targets.push(child);
    }
    for (const role of ["textbox", "combobox", "listbox", "option"]) {
      const control = document.createElement("div");
      control.setAttribute("role", role);
      const child = document.createElement("span");
      control.append(child);
      targets.push(child);
    }
    for (const target of targets) {
      const event = new KeyboardEvent("keydown", { key: "n" });
      target.dispatchEvent(event);
      expect(readerShortcutDirection(event)).toBeNull();
    }
  });

  it("keeps shortcuts usable from ordinary buttons and read-only text", () => {
    for (const tag of ["button", "p"]) {
      const target = document.createElement(tag);
      target.setAttribute("contenteditable", "false");
      const event = new KeyboardEvent("keydown", { key: "p" });
      target.dispatchEvent(event);
      expect(readerShortcutDirection(event)).toBe(-1);
    }
  });
});
