/** @vitest-environment jsdom */
import { describe, expect, it } from "vitest";
import {
  activationOwningTarget,
  contentKeyCommand,
  mappingKeyCommand,
  visibleImportErrorText,
} from "./hostControls";

describe("visibleImportErrorText", () => {
  it("returns the same error string, never a generic empty stand-in", () => {
    expect(visibleImportErrorText("SOURCE_BUSY: locked")).toBe(
      "SOURCE_BUSY: locked",
    );
    expect(visibleImportErrorText("")).toBeNull();
    expect(visibleImportErrorText(null)).toBeNull();
    expect(visibleImportErrorText(undefined)).toBeNull();
  });
});

describe("contentKeyCommand", () => {
  const idle = {
    busy: false,
    composing: false,
    inTextField: false,
    onControl: false,
    hasNextPage: true,
    canPreviousPage: true,
    hasFocusedCandidate: true,
  };

  it("maps list keys to the same commands the buttons run", () => {
    expect(contentKeyCommand("ArrowRight", idle)).toBe("loadMore");
    expect(contentKeyCommand("ArrowLeft", idle)).toBe("previousPage");
    expect(contentKeyCommand(" ", idle)).toBe("toggleFocused");
    expect(contentKeyCommand("Enter", idle)).toBe("peekFocused");
  });

  it("disables those commands while busy", () => {
    const busy = { ...idle, busy: true };
    expect(contentKeyCommand("ArrowRight", busy)).toBe("none");
    expect(contentKeyCommand(" ", busy)).toBe("none");
    expect(contentKeyCommand("Enter", busy)).toBe("none");
  });

  it("does not steal Enter/Space from buttons or comboboxes", () => {
    const onControl = { ...idle, onControl: true };
    expect(contentKeyCommand("Enter", onControl)).toBe("none");
    expect(contentKeyCommand(" ", onControl)).toBe("none");
    expect(contentKeyCommand("ArrowRight", onControl)).toBe("none");
  });
});

describe("mappingKeyCommand", () => {
  it("commits on Enter only when idle and not on a control", () => {
    expect(
      mappingKeyCommand("Enter", {
        busy: false,
        composing: false,
        onControl: false,
      }),
    ).toBe("commit");
    expect(
      mappingKeyCommand("Enter", {
        busy: true,
        composing: false,
        onControl: false,
      }),
    ).toBe("none");
    expect(
      mappingKeyCommand("Enter", {
        busy: false,
        composing: false,
        onControl: true,
      }),
    ).toBe("none");
  });
});

describe("activationOwningTarget", () => {
  it("leaves keys inside editable descendants to the editor", () => {
    const editor = document.createElement("div");
    editor.setAttribute("contenteditable", "true");
    const span = document.createElement("span");
    editor.append(span);
    expect(activationOwningTarget(span)).toBe(true);
    editor.setAttribute("contenteditable", "plaintext-only");
    expect(activationOwningTarget(span)).toBe(true);
    editor.setAttribute("contenteditable", "false");
    expect(activationOwningTarget(span)).toBe(false);
  });

  it("treats buttons and combobox triggers as owning Enter", () => {
    const button = document.createElement("button");
    const combo = document.createElement("div");
    combo.setAttribute("role", "combobox");
    const pane = document.createElement("div");
    pane.append(button, combo);
    expect(activationOwningTarget(button)).toBe(true);
    expect(activationOwningTarget(combo)).toBe(true);
    expect(activationOwningTarget(pane)).toBe(false);
  });

  it("leaves disclosure keyboard activation to the summary and its descendants", () => {
    const summary = document.createElement("summary");
    const label = document.createElement("span");
    summary.append(label);
    expect(activationOwningTarget(summary)).toBe(true);
    expect(activationOwningTarget(label)).toBe(true);
  });
});
