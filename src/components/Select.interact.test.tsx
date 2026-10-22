/** @vitest-environment jsdom */
import "@testing-library/jest-dom/vitest";
import { useState } from "react";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { GlassModal } from "./GlassModal";
import { Select, type SelectOption } from "./Select";

vi.mock("@/lib/nativeWebviewCover", () => ({
  acquireNativeWebviewCover: () => () => {},
  rectOverlapsNativeWebviewHost: () => false,
}));
afterEach(cleanup);

const options: SelectOption[] = [
  { value: "a", label: "Alpha" },
  { value: "blocked", label: "Blocked", disabled: true },
  { value: "b", label: "Beta" },
  { value: "c", label: "Gamma" },
];

function Fixture({ modal = false, onClose = vi.fn(), onChange = vi.fn(), disabled = false,
  rows = options, value = "b", last = false }: {
  modal?: boolean; onClose?: () => void; onChange?: (value: string) => void;
  disabled?: boolean; rows?: SelectOption[]; value?: string; last?: boolean;
}) {
  const content = <>
    <input aria-label="Before" />
    <Select aria-label="Provider" value={value} options={rows} onChange={onChange} disabled={disabled} />
    {!last && <button type="button">After</button>}
  </>;
  return modal
    ? <GlassModal open title="Import" onClose={onClose} closeLabel="Close import">{content}</GlassModal>
    : content;
}

async function openMenu(user: ReturnType<typeof userEvent.setup>) {
  const trigger = screen.getByRole("button", { name: "Provider" });
  await user.click(trigger);
  const menu = await screen.findByRole("listbox");
  return { trigger, menu };
}

describe("Select keyboard and portal ownership", () => {
  it("opens above a low trigger by default instead of clipping the final options", async () => {
    const user = userEvent.setup();
    render(<Fixture />);
    const trigger = screen.getByRole("button", { name: "Provider" });
    trigger.getBoundingClientRect = () => ({ top: window.innerHeight - 140, bottom: window.innerHeight - 108,
      left: 60, right: 180, width: 120, height: 32 }) as DOMRect;
    await user.click(trigger);
    const menu = await screen.findByRole("listbox");
    expect(Number.parseFloat(menu.style.top)).toBeLessThan(window.innerHeight - 140);
  });

  it("ArrowUp opens the last enabled option when there is no selected value", async () => {
    const user = userEvent.setup();
    render(<Fixture value="missing" />);
    screen.getByRole("button", { name: "Provider" }).focus();
    await user.keyboard("{ArrowUp}");
    await waitFor(() => expect(screen.getByRole("button", { name: "Gamma" })).toHaveFocus());
  });

  it.each([
    { name: "empty", rows: [] },
    { name: "disabled-only", rows: options.map((option) => ({ ...option, disabled: true })) },
  ])("$name options remain dismissible", async ({ rows }) => {
    const user = userEvent.setup();
    render(<Fixture rows={rows} />);
    const { menu, trigger } = await openMenu(user);
    await waitFor(() => expect(menu).toHaveFocus());
    await user.keyboard("{ArrowDown}{Home}{End}{Escape}");
    expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
    expect(trigger).toHaveFocus();
  });

  it("disabled selected option falls back to an enabled choice without committing", async () => {
    const user = userEvent.setup();
    const change = vi.fn();
    render(<Fixture value="blocked" onChange={change} />);
    const { menu } = await openMenu(user);
    await waitFor(() => expect(within(menu).getByRole("button", { name: "Alpha" })).toHaveFocus());
    expect(within(menu).getByRole("option", { name: "Blocked" })).toHaveAttribute("aria-disabled", "true");
    expect(change).not.toHaveBeenCalled();
  });

  it("recovers focus if refreshed options remove the active option, but does not reset surviving focus", async () => {
    const user = userEvent.setup();
    const view = render(<Fixture />);
    const { menu } = await openMenu(user);
    await waitFor(() => expect(within(menu).getByRole("button", { name: "Beta" })).toHaveFocus());
    await user.keyboard("{ArrowDown}");
    view.rerender(<Fixture rows={[...options]} />);
    expect(within(menu).getByRole("button", { name: "Gamma" })).toHaveFocus();
    view.rerender(<Fixture rows={options.filter((option) => option.value !== "c")} />);
    expect(within(menu).getByRole("button", { name: "Alpha" })).toHaveFocus();
  });

  it("an overlay opened above a still-mounted listbox owns Escape and restores its option", async () => {
    const user = userEvent.setup();
    const outerClose = vi.fn();
    const topClose = vi.fn();
    const layers = (top: boolean) => <>
      <Fixture modal onClose={outerClose} />
      <GlassModal open={top} title="New top" onClose={topClose}><button>Top action</button></GlassModal>
    </>;
    const view = render(layers(false));
    await waitFor(() => expect(screen.getByRole("button", { name: "Close import" })).toHaveFocus());
    const { menu } = await openMenu(user);
    const selected = within(menu).getByRole("button", { name: "Beta" });
    await waitFor(() => expect(selected).toHaveFocus());
    view.rerender(layers(true));
    await waitFor(() => expect(within(screen.getByRole("dialog", { name: "New top" })).getByRole("button", { name: "Close" })).toHaveFocus());
    await user.keyboard("{Escape}");
    expect(topClose).toHaveBeenCalledOnce();
    expect(outerClose).not.toHaveBeenCalled();
    expect(menu).toBeInTheDocument();
    view.rerender(layers(false));
    expect(selected).toHaveFocus();
  });

  it("disabled and composing triggers cannot open a menu", async () => {
    const user = userEvent.setup();
    const view = render(<Fixture disabled />);
    const trigger = screen.getByRole("button", { name: "Provider" });
    await user.click(trigger);
    fireEvent.keyDown(trigger, { key: "ArrowDown" });
    expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
    view.rerender(<Fixture />);
    for (const ime of [{ isComposing: true }, { keyCode: 229 }]) {
      fireEvent.keyDown(trigger, { key: "ArrowDown", ...ime });
      expect(fireEvent.keyDown(trigger, { key: "Enter", ...ime })).toBe(false);
    }
    expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
  });

  it("Escape closes only the listbox inside a modal and returns to its trigger", async () => {
    const user = userEvent.setup();
    const close = vi.fn();
    render(<Fixture modal onClose={close} />);
    await waitFor(() => expect(screen.getByRole("button", { name: "Close import" })).toHaveFocus());
    const { trigger } = await openMenu(user);
    await user.keyboard("{Escape}");
    expect(close).not.toHaveBeenCalled();
    expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
    expect(trigger).toHaveFocus();
    await user.keyboard("{Escape}");
    expect(close).toHaveBeenCalledOnce();
  });

  it("opens from arrows, focuses selected, skips disabled options and supports Home/End", async () => {
    const user = userEvent.setup();
    const change = vi.fn();
    render(<Fixture onChange={change} />);
    screen.getByRole("button", { name: "Provider" }).focus();
    await user.keyboard("{ArrowDown}");
    const menu = await screen.findByRole("listbox");
    await waitFor(() => expect(within(menu).getByRole("button", { name: "Beta" })).toHaveFocus());
    await user.keyboard("{ArrowUp}");
    expect(within(menu).getByRole("button", { name: "Alpha" })).toHaveFocus();
    await user.keyboard("{End}");
    expect(within(menu).getByRole("button", { name: "Gamma" })).toHaveFocus();
    await user.keyboard("{Home}{ArrowDown}{Enter}");
    expect(change).toHaveBeenCalledExactlyOnceWith("b");
    expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Provider" })).toHaveFocus();
  });

  it("Space opens and commits the keyboard option exactly once", async () => {
    const user = userEvent.setup();
    const change = vi.fn();
    render(<Fixture onChange={change} />);
    const trigger = screen.getByRole("button", { name: "Provider" });
    trigger.focus();
    await user.keyboard(" ");
    await waitFor(() => expect(screen.getByRole("button", { name: "Beta" })).toHaveFocus());
    await user.keyboard("{ArrowDown} ");
    expect(change).toHaveBeenCalledExactlyOnceWith("c");
    expect(trigger).toHaveFocus();
  });

  it.each([false, true])("Tab closes without committing and continues from trigger, shift=%s", async (shift) => {
    const user = userEvent.setup();
    const change = vi.fn();
    render(<Fixture modal onChange={change} />);
    await waitFor(() => expect(screen.getByRole("button", { name: "Close import" })).toHaveFocus());
    await openMenu(user);
    await user.tab({ shift });
    expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
    expect(shift ? screen.getByRole("textbox", { name: "Before" }) : screen.getByRole("button", { name: "After" })).toHaveFocus();
    expect(change).not.toHaveBeenCalled();
  });

  it("Tab from the last dialog control wraps within the parent after dismissing the listbox", async () => {
    const user = userEvent.setup();
    render(<Fixture modal last />);
    const close = screen.getByRole("button", { name: "Close import" });
    await waitFor(() => expect(close).toHaveFocus());
    await openMenu(user);
    await user.tab();
    expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
    expect(close).toHaveFocus();
  });

  it("outside click dismisses without stealing focus from the clicked input", async () => {
    const user = userEvent.setup();
    render(<Fixture />);
    await openMenu(user);
    const input = screen.getByRole("textbox", { name: "Before" });
    await user.click(input);
    expect(input).toHaveFocus();
    expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
  });

  it.each([{ isComposing: true }, { keyCode: 229 }])("does not dismiss or select during IME: %j", async (ime) => {
    const user = userEvent.setup();
    const change = vi.fn();
    const close = vi.fn();
    render(<Fixture modal onClose={close} onChange={change} />);
    await waitFor(() => expect(screen.getByRole("button", { name: "Close import" })).toHaveFocus());
    const { menu } = await openMenu(user);
    const option = within(menu).getByRole("button", { name: "Beta" });
    option.focus();
    fireEvent.keyDown(option, { key: "Escape", ...ime });
    fireEvent.keyDown(option, { key: "Enter", ...ime });
    fireEvent.keyDown(option, { key: "ArrowDown", ...ime });
    expect(menu).toBeInTheDocument();
    expect(option).toHaveFocus();
    expect(change).not.toHaveBeenCalled();
    expect(close).not.toHaveBeenCalled();
  });

  it("disabling an open Select removes the menu and cannot leave a focus scope behind", async () => {
    const user = userEvent.setup();
    const close = vi.fn();
    const view = render(<Fixture modal onClose={close} />);
    await waitFor(() => expect(screen.getByRole("button", { name: "Close import" })).toHaveFocus());
    await openMenu(user);
    view.rerender(<Fixture modal disabled onClose={close} />);
    expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
    await user.keyboard("{Escape}");
    expect(close).toHaveBeenCalledOnce();
  });

  it("keeps the original modal trigger when navigation unmounts an open menu", async () => {
    const user = userEvent.setup();
    function Harness() {
      const [open, setOpen] = useState(false);
      return <>
        <button onClick={() => setOpen(true)}>Open import</button>
        <GlassModal open={open} onClose={() => setOpen(false)} title="Import">
          <Select aria-label="Provider" value="b" options={options} onChange={() => setOpen(false)} />
        </GlassModal>
      </>;
    }
    render(<Harness />);
    const original = screen.getByRole("button", { name: "Open import" });
    await user.click(original);
    await waitFor(() => expect(screen.getByRole("button", { name: "Close" })).toHaveFocus());
    const { menu } = await openMenu(user);
    await user.click(within(menu).getByRole("button", { name: "Gamma" }));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(original).toHaveFocus();
  });
});
