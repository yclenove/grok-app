/** @vitest-environment jsdom */
import "@testing-library/jest-dom/vitest";
import { StrictMode } from "react";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { GlassModal } from "./GlassModal";

vi.mock("@/lib/nativeWebviewCover", () => ({
  acquireNativeWebviewCover: () => () => {},
}));

afterEach(cleanup);

function panels(outer: boolean, inner: boolean, onOuter = () => {}, onInner = () => {}) {
  return <>
    <button type="button">Open reader</button>
    <GlassModal open={outer} onClose={onOuter} title="Reader" closeLabel="Close reader">
      <button type="button">Open confirmation</button>
      <input aria-label="Reader query" />
    </GlassModal>
    <GlassModal open={inner} onClose={onInner} title="Confirmation" closeLabel="Close confirmation">
      <button type="button">Back to reader</button>
    </GlassModal>
  </>;
}

describe("GlassModal real portal focus lifecycle", () => {
  it("only dismisses the top dialog and restores each keyboard trigger", async () => {
    const user = userEvent.setup();
    const closeOuter = vi.fn();
    const closeInner = vi.fn();
    const view = render(panels(false, false, closeOuter, closeInner));
    const originalTrigger = screen.getByRole("button", { name: "Open reader" });
    originalTrigger.focus();
    view.rerender(panels(true, false, closeOuter, closeInner));
    await waitFor(() => expect(screen.getByRole("button", { name: "Close reader" })).toHaveFocus());
    const nestedTrigger = screen.getByRole("button", { name: "Open confirmation" });
    await user.tab();
    expect(nestedTrigger).toHaveFocus();
    view.rerender(panels(true, true, closeOuter, closeInner));
    await waitFor(() => expect(screen.getByRole("button", { name: "Close confirmation" })).toHaveFocus());

    await user.tab({ shift: true });
    expect(screen.getByRole("button", { name: "Back to reader" })).toHaveFocus();
    await user.tab();
    expect(screen.getByRole("button", { name: "Close confirmation" })).toHaveFocus();
    await user.keyboard("{Escape}");
    expect(closeInner).toHaveBeenCalledOnce();
    expect(closeOuter).not.toHaveBeenCalled();
    view.rerender(panels(true, false, closeOuter, closeInner));
    expect(nestedTrigger).toHaveFocus();
    await user.keyboard("{Escape}");
    expect(closeOuter).toHaveBeenCalledOnce();
    view.rerender(panels(false, false, closeOuter, closeInner));
    expect(originalTrigger).toHaveFocus();
  });

  it("preserves the original trigger when a covered portal detaches before effect cleanup", async () => {
    const view = render(panels(false, false));
    const originalTrigger = screen.getByRole("button", { name: "Open reader" });
    originalTrigger.focus();
    view.rerender(panels(true, false));
    await waitFor(() => expect(screen.getByRole("button", { name: "Close reader" })).toHaveFocus());
    screen.getByRole("button", { name: "Open confirmation" }).focus();
    view.rerender(panels(true, true));
    const topClose = screen.getByRole("button", { name: "Close confirmation" });
    await waitFor(() => expect(topClose).toHaveFocus());

    view.rerender(panels(false, true));
    expect(topClose).toHaveFocus();
    view.rerender(panels(false, false));
    expect(originalTrigger).toHaveFocus();
  });

  it("returns to the keyboard trigger after async preparation temporarily disables it", async () => {
    const view = render(panels(true, false));
    await waitFor(() => expect(screen.getByRole("button", { name: "Close reader" })).toHaveFocus());
    const trigger = screen.getByRole("button", { name: "Open confirmation" });
    trigger.focus();
    // Reproduce Chromium's disabled-button blur, absent in jsdom.
    trigger.blur();
    trigger.setAttribute("disabled", "");
    expect(document.body).toHaveFocus();
    trigger.removeAttribute("disabled");
    view.rerender(panels(true, true));
    await waitFor(() => expect(screen.getByRole("button", { name: "Close confirmation" })).toHaveFocus());
    view.rerender(panels(true, false));
    expect(trigger).toHaveFocus();
  });

  it("keeps custom input focus on parent rerenders under StrictMode", async () => {
    const close = vi.fn();
    const initial = vi.fn(() => document.querySelector<HTMLInputElement>("input"));
    const content = (label: string) => <StrictMode>
      <GlassModal open onClose={() => close()} title={label} initialFocus={initial}>
        <input aria-label="Query" />
      </GlassModal>
    </StrictMode>;
    const view = render(content("Reader"));
    const input = screen.getByRole("textbox", { name: "Query" });
    await waitFor(() => expect(input).toHaveFocus());
    const calls = initial.mock.calls.length;
    fireEvent.change(input, { target: { value: "synthetic query" } });
    view.rerender(content("Updated reader"));
    expect(input).toHaveFocus();
    expect(input).toHaveValue("synthetic query");
    expect(initial).toHaveBeenCalledTimes(calls);
    fireEvent.keyDown(input, { key: "Escape", isComposing: true });
    expect(close).not.toHaveBeenCalled();
    fireEvent.keyDown(input, { key: "Escape" });
    expect(close).toHaveBeenCalledOnce();
  });

  it("returns inside the reader when the nested trigger becomes disabled", async () => {
    const view = render(panels(true, false));
    const reader = screen.getByRole("dialog", { name: "Reader" });
    const trigger = within(reader).getByRole("button", { name: "Open confirmation" });
    await waitFor(() => expect(within(reader).getByRole("button", { name: "Close reader" })).toHaveFocus());
    trigger.focus();
    view.rerender(panels(true, true));
    await waitFor(() => expect(screen.getByRole("button", { name: "Close confirmation" })).toHaveFocus());
    trigger.setAttribute("disabled", "");
    view.rerender(panels(true, false));
    expect(within(reader).getByRole("button", { name: "Close reader" })).toHaveFocus();
  });
});
