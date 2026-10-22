/** @vitest-environment jsdom */
import { cleanup, fireEvent, render, renderHook } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { useTypeToFocusComposer } from "./useTypeToFocusComposer";

afterEach(() => { cleanup(); vi.restoreAllMocks(); });

it.each(["dialog", "alertdialog", "native"])(
  "does not redirect non-input descendants of a %s to the composer",
  (kind) => {
    const view = render(<>
      <div data-testid="composer" contentEditable suppressContentEditableWarning>Draft</div>
      {kind === "native"
        ? <dialog open><div tabIndex={0} data-testid="target"><span>Content</span></div></dialog>
        : <div role={kind}><div tabIndex={0} data-testid="target"><span>Content</span></div></div>}
    </>);
    const composer = view.getByTestId("composer") as HTMLDivElement;
    const target = view.getByTestId("target");
    const focus = vi.spyOn(composer, "focus");
    renderHook(() => useTypeToFocusComposer({
      getEditor: () => composer,
      getLive: () => ({ enabled: true, overlayOpen: false }),
    }));
    target.focus();
    for (const key of [" ", "/", "q", "Process"]) {
      fireEvent.keyDown(target.firstElementChild!, { key });
      expect(document.activeElement).toBe(target);
    }
    expect(focus).not.toHaveBeenCalled();
    expect(composer.textContent).toBe("Draft");
  },
);

it("still redirects ordinary chat chrome outside a dialog", () => {
  const view = render(<>
    <div data-testid="composer" contentEditable />
    <div tabIndex={0} data-testid="chrome" />
  </>);
  const composer = view.getByTestId("composer") as HTMLDivElement;
  const chrome = view.getByTestId("chrome");
  renderHook(() => useTypeToFocusComposer({
    getEditor: () => composer,
    getLive: () => ({ enabled: true, overlayOpen: false }),
  }));
  chrome.focus();
  fireEvent.keyDown(chrome, { key: "q" });
  expect(document.activeElement).toBe(composer);
});
