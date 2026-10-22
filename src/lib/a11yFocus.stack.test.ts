/** @vitest-environment jsdom */
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { installDialogFocus, isDialogFocusAllowed, type InstallDialogFocusOptions } from "./a11yFocus";

const cleanups: Array<() => void> = [];

function panel() {
  const root = document.createElement("section");
  root.setAttribute("role", "dialog");
  const first = document.createElement("button");
  const last = document.createElement("button");
  root.append(first, last);
  document.body.append(root);
  return { root, first, last };
}

function install(root: HTMLElement, options: InstallDialogFocusOptions = {}) {
  const cleanup = installDialogFocus(() => root, options);
  cleanups.push(cleanup);
  return cleanup;
}

function key(key: string, init: KeyboardEventInit = {}) {
  const event = new KeyboardEvent("keydown", {
    key,
    bubbles: true,
    cancelable: true,
    ...init,
  });
  (document.activeElement ?? document.body).dispatchEvent(event);
  return event;
}

beforeEach(() => vi.useFakeTimers());
afterEach(() => {
  cleanups.splice(0).reverse().forEach((cleanup) => cleanup());
  vi.useRealTimers();
  vi.restoreAllMocks();
  document.body.replaceChildren();
});

describe("stacked dialog keyboard and focus ownership", () => {
  it("only the top scope can override Tab, and IME events do not invoke the override", () => {
    const outer = panel();
    const inner = panel();
    const outerTab = vi.fn();
    const innerTab = vi.fn((event: KeyboardEvent) => event.preventDefault());
    install(outer.root, { onTab: outerTab });
    const closeInner = install(inner.root, { onTab: innerTab });
    vi.runAllTimers();
    expect(key("Tab").defaultPrevented).toBe(true);
    expect(innerTab).toHaveBeenCalledOnce();
    expect(outerTab).not.toHaveBeenCalled();
    key("Tab", { isComposing: true });
    key("Tab", { keyCode: 229 });
    expect(innerTab).toHaveBeenCalledOnce();
    closeInner();
    key("Tab");
    expect(outerTab).toHaveBeenCalledOnce();
  });

  it.each([false, true])("reads the restoration policy at cleanup, returning focus=%s", (restore) => {
    const outer = panel();
    const inner = panel();
    install(outer.root);
    vi.runAllTimers();
    let restoreNow = !restore;
    const closeInner = install(inner.root, { restoreFocus: () => restoreNow });
    vi.runAllTimers();
    outer.last.focus();
    restoreNow = restore;
    closeInner();
    expect(document.activeElement).toBe(restore ? outer.first : outer.last);
  });

  it("delayed work may only focus the current layer, and ownership resumes on close", () => {
    const outer = panel();
    const inner = panel();
    const closeOuter = install(outer.root);
    expect(isDialogFocusAllowed(outer.first)).toBe(true);
    expect(isDialogFocusAllowed(inner.first)).toBe(false);
    const closeInner = install(inner.root);
    expect(isDialogFocusAllowed(outer.first)).toBe(false);
    expect(isDialogFocusAllowed(inner.first)).toBe(true);
    expect(isDialogFocusAllowed(document.body)).toBe(false);
    closeInner();
    expect(isDialogFocusAllowed(outer.first)).toBe(true);
    closeOuter();
    expect(isDialogFocusAllowed(document.body)).toBe(true);
  });

  it("a pending panel never gives its focus to a background target", () => {
    const close = installDialogFocus(() => null, { initialFocus: "none" });
    cleanups.push(close);
    expect(isDialogFocusAllowed(document.body)).toBe(false);
    close();
    expect(isDialogFocusAllowed(document.body)).toBe(true);
  });

  it.each([
    [false, false], [true, true], [true, false], [false, true],
  ])("Escape closes only the top layer (outer capture=%s, inner capture=%s)", (outerCapture, innerCapture) => {
    const outer = panel();
    const inner = panel();
    const closeOuter = vi.fn();
    const closeInner = vi.fn();
    install(outer.root, { onEscape: closeOuter, capture: outerCapture });
    install(inner.root, { onEscape: closeInner, capture: innerCapture });
    vi.runAllTimers();

    expect(key("Escape").defaultPrevented).toBe(true);
    expect(closeInner).toHaveBeenCalledOnce();
    expect(closeOuter).not.toHaveBeenCalled();
  });

  it("does not let an underlying trap redirect Tab or Shift+Tab", () => {
    const outer = panel();
    const inner = panel();
    install(outer.root);
    install(inner.root, { capture: false });
    vi.runAllTimers();
    const outerFocus = vi.spyOn(outer.first, "focus");

    inner.first.focus();
    expect(key("Tab").defaultPrevented).toBe(false);
    expect(outerFocus).not.toHaveBeenCalled();
    inner.last.focus();
    expect(key("Tab").defaultPrevented).toBe(true);
    expect(document.activeElement).toBe(inner.first);
    key("Tab", { shiftKey: true });
    expect(document.activeElement).toBe(inner.last);
    expect(outerFocus).not.toHaveBeenCalled();
  });

  it("a non-dismissible top layer shields the underlying Escape handler", () => {
    const outer = panel();
    const inner = panel();
    const closeOuter = vi.fn();
    install(outer.root, { onEscape: closeOuter });
    install(inner.root);
    vi.runAllTimers();
    key("Escape");
    expect(closeOuter).not.toHaveBeenCalled();
  });

  it("honors Escape already handled by a nested widget", () => {
    const dialog = panel();
    const close = vi.fn();
    install(dialog.root, { onEscape: close, capture: false });
    vi.runAllTimers();
    dialog.first.addEventListener("keydown", (event) => event.preventDefault());
    key("Escape");
    expect(close).not.toHaveBeenCalled();
  });

  it.each([{ isComposing: true }, { keyCode: 229 }])("does not dismiss while an IME consumes Escape (%j)", (init) => {
    const dialog = panel();
    const close = vi.fn();
    install(dialog.root, { onEscape: close });
    vi.runAllTimers();
    expect(key("Escape", init).defaultPrevented).toBe(false);
    expect(close).not.toHaveBeenCalled();
  });

  it("does not run an underlying delayed initial-focus callback", () => {
    const outer = panel();
    const inner = panel();
    const outerFocus = vi.spyOn(outer.first, "focus");
    const initialFocus = vi.fn(() => outer.first);
    install(outer.root, { initialFocus });
    install(inner.root);
    vi.runAllTimers();
    expect(document.activeElement).toBe(inner.first);
    expect(initialFocus).not.toHaveBeenCalled();
    expect(outerFocus).not.toHaveBeenCalled();
  });

  it("restores the nested trigger, then the original trigger", () => {
    const trigger = document.createElement("button");
    document.body.append(trigger);
    trigger.focus();
    const outer = panel();
    const closeOuter = install(outer.root);
    vi.runAllTimers();
    outer.last.focus();
    const inner = panel();
    const closeInner = install(inner.root);
    vi.runAllTimers();
    closeInner();
    inner.root.remove();
    expect(document.activeElement).toBe(outer.last);
    closeOuter();
    outer.root.remove();
    expect(document.activeElement).toBe(trigger);
  });

  it("closing an underlying layer preserves top focus and its restoration chain", () => {
    const trigger = document.createElement("button");
    document.body.append(trigger);
    trigger.focus();
    const outer = panel();
    const closeOuter = install(outer.root);
    vi.runAllTimers();
    const inner = panel();
    const closeInner = install(inner.root);
    vi.runAllTimers();

    closeOuter();
    expect(document.activeElement).toBe(inner.first);
    outer.root.remove();
    closeInner();
    inner.root.remove();
    expect(document.activeElement).toBe(trigger);
  });

  it("remembers the nested trigger across a temporary busy-state blur", () => {
    const outer = panel();
    install(outer.root);
    vi.runAllTimers();
    outer.last.focus();
    // Chromium blurs disabled buttons; jsdom needs that browser step explicit.
    outer.last.blur();
    outer.last.disabled = true;
    expect(document.activeElement).toBe(document.body);
    outer.last.disabled = false;
    const inner = panel();
    const closeInner = install(inner.root);
    vi.runAllTimers();
    closeInner();
    inner.root.remove();
    expect(document.activeElement).toBe(outer.last);
  });

  it.each(["disabled", "removed"])("does not revive an unavailable remembered trigger (%s)", (state) => {
    const outer = panel();
    install(outer.root);
    vi.runAllTimers();
    outer.last.focus();
    outer.last.blur();
    const inner = panel();
    const closeInner = install(inner.root);
    vi.runAllTimers();
    if (state === "disabled") outer.last.disabled = true;
    else outer.last.remove();
    closeInner();
    inner.root.remove();
    expect(document.activeElement).toBe(outer.first);
  });

  it("prefers a newly focused control over the remembered busy trigger", () => {
    const outer = panel();
    install(outer.root);
    vi.runAllTimers();
    outer.last.focus();
    outer.last.blur();
    outer.first.focus();
    const inner = panel();
    const closeInner = install(inner.root);
    vi.runAllTimers();
    closeInner();
    inner.root.remove();
    expect(document.activeElement).toBe(outer.first);
  });

  it("keeps the original return chain if an async child's parent disappears", () => {
    const trigger = document.createElement("button");
    document.body.append(trigger);
    trigger.focus();
    const outer = panel();
    const closeOuter = install(outer.root);
    vi.runAllTimers();
    outer.last.focus();
    outer.last.blur();
    const inner = panel();
    const closeInner = install(inner.root);
    vi.runAllTimers();
    closeOuter();
    outer.root.remove();
    expect(document.activeElement).toBe(inner.first);
    closeInner();
    inner.root.remove();
    expect(document.activeElement).toBe(trigger);
  });

  it("restores inside the surviving layer when the previous control was removed", () => {
    const outer = panel();
    install(outer.root);
    vi.runAllTimers();
    outer.last.focus();
    const inner = panel();
    const closeInner = install(inner.root);
    vi.runAllTimers();
    outer.last.remove();
    closeInner();
    inner.root.remove();
    expect(document.activeElement).toBe(outer.first);
  });

  it("cleanup is idempotent and cannot pull focus from a later dialog", () => {
    const trigger = document.createElement("button");
    document.body.append(trigger);
    trigger.focus();
    const first = panel();
    const closeFirst = install(first.root);
    vi.runAllTimers();
    closeFirst();
    first.root.remove();
    const second = panel();
    install(second.root);
    vi.runAllTimers();
    closeFirst();
    expect(document.activeElement).toBe(second.first);
  });

  it("synchronous top cleanup cannot deliver the same Escape to the layer beneath", () => {
    const outer = panel();
    const closeOuter = vi.fn();
    install(outer.root, { onEscape: closeOuter, capture: false });
    const inner = panel();
    const closeInner = install(inner.root, { onEscape: () => closeInner(), capture: true });
    vi.runAllTimers();
    key("Escape");
    expect(closeOuter).not.toHaveBeenCalled();
    inner.root.remove();
    key("Escape");
    expect(closeOuter).toHaveBeenCalledOnce();
  });
});
