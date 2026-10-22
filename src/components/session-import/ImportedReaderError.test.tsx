/** @vitest-environment jsdom */
import { useLayoutEffect, useState } from "react";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, expect, test, vi } from "vitest";
import { createT } from "@/i18n";
import { installDialogFocus } from "@/lib/a11yFocus";
import { ImportedReaderError } from "./ImportedReaderError";

const tr = createT("en");
afterEach(cleanup);
function setup() {
  const onReload = vi.fn(); const onCopy = vi.fn();
  let props = { error: "READ_FAILED" as string | null, busy: false, copyBusy: false, identity: "a" };
  const view = () => <div role="dialog"><ImportedReaderError key={props.identity} {...props} tr={tr} onReload={onReload} onCopy={onCopy} /><form className="session-import__reader-search"><input aria-label="search" /></form><button type="button">elsewhere</button></div>;
  const rendered = render(view());
  return { ...rendered, onReload, onCopy, update: (next: Partial<typeof props>) => { props = { ...props, ...next }; rendered.rerender(view()); } };
}
function start() {
  const refresh = screen.getByRole("button", { name: "Refresh" }); refresh.focus(); fireEvent.click(refresh); return refresh;
}

test.each(["{Enter}", " "])("ordinary %s activates a non-submit recovery button", async (key) => {
  const user = userEvent.setup(); const h = setup(); const refresh = screen.getByRole("button", { name: "Refresh" });
  expect((refresh as HTMLButtonElement).type).toBe("button"); refresh.focus(); await user.keyboard(key);
  expect(h.onReload).toHaveBeenCalledTimes(1); expect(h.onCopy).not.toHaveBeenCalled();
});

test("successful recovery restores focus after the removed control and pending work settle", () => {
  const h = setup(); const refresh = start(); h.update({ busy: true });
  expect((refresh as HTMLButtonElement).disabled).toBe(true);
  expect(screen.getByRole("alert").getAttribute("aria-busy")).toBe("true");
  h.update({ error: null }); expect(screen.getByLabelText("search")).not.toBe(document.activeElement);
  h.update({ busy: false }); expect(screen.queryByRole("alert")).toBeNull(); expect(screen.getByLabelText("search")).toBe(document.activeElement);
});

test("deliberate focus movement wins over recovery completion", () => {
  const h = setup(); start(); h.update({ busy: true }); const elsewhere = screen.getByRole("button", { name: "elsewhere" }); elsewhere.focus();
  h.update({ error: null, busy: false }); expect(document.activeElement).toBe(elsewhere);
});

test("programmatic activation does not manufacture a focus restoration claim", () => {
  const h = setup(); fireEvent.click(screen.getByRole("button", { name: "Refresh" })); h.update({ busy: true });
  h.update({ error: null, busy: false }); expect(screen.getByLabelText("search")).not.toBe(document.activeElement);
});

test("a failed refresh stays visible and returns its original control to usable state", () => {
  const h = setup(); const refresh = start(); h.update({ busy: true }); h.update({ busy: false, error: "STILL_OFFLINE" });
  expect(screen.getByRole("alert")).toBeTruthy(); expect((refresh as HTMLButtonElement).disabled).toBe(false); expect(document.activeElement).toBe(refresh);
  refresh.blur(); h.update({ error: null }); expect(screen.getByLabelText("search")).not.toBe(document.activeElement);
});

test("copy and reader busy gates remain independent", () => {
  const h = setup(); h.update({ busy: true }); fireEvent.click(screen.getByRole("button", { name: "Refresh" })); fireEvent.click(screen.getByRole("button", { name: "Copy text" }));
  expect(h.onReload).not.toHaveBeenCalled(); expect(h.onCopy).toHaveBeenCalledTimes(1);
  h.update({ busy: false, copyBusy: true }); fireEvent.click(screen.getByRole("button", { name: "Copy text" })); start();
  expect(h.onCopy).toHaveBeenCalledTimes(1); expect(h.onReload).toHaveBeenCalledTimes(1);
});

test("a newer focus scope prevents covered refresh and late focus restoration", () => {
  const h = setup(); start(); h.update({ busy: true });
  const overlay = document.createElement("div"); overlay.innerHTML = '<button type="button">upper</button>'; document.body.append(overlay);
  const dispose = installDialogFocus(() => overlay, { initialFocus: "none", restoreFocus: false });
  try {
    h.update({ busy: false }); fireEvent.click(screen.getByRole("button", { name: "Refresh" })); expect(h.onReload).toHaveBeenCalledTimes(1);
    h.update({ error: null }); expect(screen.getByLabelText("search")).not.toBe(document.activeElement);
  } finally { dispose(); overlay.remove(); }
});

test("a new reader identity cannot inherit an old recovery focus claim", () => {
  const h = setup(); start(); h.update({ busy: true }); h.update({ identity: "b", error: null, busy: false });
  expect(screen.getByLabelText("search")).not.toBe(document.activeElement);
});

test("an immediate recovery activation cannot be consumed by the previous error commit", async () => {
  function ImmediateRecovery() {
    const [state, setState] = useState({ error: "READ_FAILED" as string | null, busy: false });
    useLayoutEffect(() => {
      // Exercise activation before the previous commit's passive effects run.
      const refresh = screen.getByRole("button", { name: "Refresh" });
      refresh.focus(); (refresh as HTMLButtonElement).click();
    }, []);
    useLayoutEffect(() => {
      if (state.busy) setState({ error: null, busy: false });
    }, [state.busy]);
    return <div role="dialog"><ImportedReaderError tr={tr} {...state} copyBusy={false}
      onReload={() => setState({ error: "READ_FAILED", busy: true })} onCopy={() => {}} />
      <form className="session-import__reader-search"><input aria-label="immediate search" /></form></div>;
  }
  await act(async () => { render(<ImmediateRecovery />); });
  expect(screen.queryByRole("alert")).toBeNull();
  expect(document.activeElement).toBe(screen.getByLabelText("immediate search"));
});
