/**
 * @vitest-environment jsdom
 */
import { afterEach, describe, expect, it } from "vitest";
import {
  restoreFocusFromHiddenPanel,
  visibleBottomTerminalToggle,
} from "./hiddenPanelFocus";

afterEach(() => {
  document.body.innerHTML = "";
});

describe("restoreFocusFromHiddenPanel", () => {
  it("returns false when focus is outside the panel", () => {
    document.body.innerHTML = `
      <button id="outside">out</button>
      <div id="panel"><button id="inside">in</button></div>
    `;
    const outside = document.getElementById("outside")!;
    outside.focus();
    expect(
      restoreFocusFromHiddenPanel(
        document.getElementById("panel"),
        document.getElementById("outside"),
      ),
    ).toBe(false);
    expect(document.activeElement).toBe(outside);
  });

  it("moves focus to the restore target with preventScroll when it is inside", () => {
    document.body.innerHTML = `
      <button id="toggle">term</button>
      <div id="panel"><button id="inside">in</button></div>
    `;
    const inside = document.getElementById("inside")!;
    const toggle = document.getElementById("toggle")!;
    const calls: unknown[] = [];
    const orig = toggle.focus.bind(toggle);
    toggle.focus = ((opts?: FocusOptions) => {
      calls.push(opts);
      orig(opts);
    }) as typeof toggle.focus;
    inside.focus();
    expect(
      restoreFocusFromHiddenPanel(
        document.getElementById("panel"),
        toggle,
      ),
    ).toBe(true);
    expect(document.activeElement).toBe(toggle);
    expect(calls).toEqual([{ preventScroll: true }]);
  });

  it("blurs the hidden control when no restore target exists", () => {
    document.body.innerHTML = `<div id="panel"><button id="inside">in</button></div>`;
    const inside = document.getElementById("inside")!;
    inside.focus();
    expect(
      restoreFocusFromHiddenPanel(document.getElementById("panel"), null),
    ).toBe(true);
    expect(document.activeElement === inside).toBe(false);
  });

  it("blurs when a hidden restore target cannot accept focus", () => {
    document.body.innerHTML = `
      <div id="panel"><button id="inside">in</button></div>
    `;
    const inside = document.getElementById("inside")!;
    inside.focus();
    expect(
      restoreFocusFromHiddenPanel(document.getElementById("panel"), {
        focus: () => {},
      }),
    ).toBe(true);
    expect(document.activeElement === inside).toBe(false);
  });
});

describe("visibleBottomTerminalToggle", () => {
  it("prefers a toggle that has a client rect", () => {
    document.body.innerHTML = `
      <button data-testid="bottom-terminal-toggle" id="a"></button>
      <button data-testid="bottom-terminal-toggle" id="b"></button>
    `;
    const a = document.getElementById("a")!;
    const b = document.getElementById("b")!;
    a.getClientRects = () => [] as unknown as DOMRectList;
    b.getClientRects = () =>
      [{ width: 16, height: 16 }] as unknown as DOMRectList;
    expect(visibleBottomTerminalToggle()?.id).toBe("b");
  });
});
