/** @vitest-environment jsdom */
import "@testing-library/jest-dom/vitest";
import { useState } from "react";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, expect, it, vi } from "vitest";
import { createT } from "@/i18n";
import { installDialogFocus } from "@/lib/a11yFocus";
import { ImportedReaderSearch } from "./ImportedReaderSearch";

afterEach(cleanup);
const tr = createT("en");
const searchName = tr("sessionImport.search");
const clearName = tr("sessionImport.clearSearch");

function setup(initial: { busy?: boolean; hasHits?: boolean; query?: string } = {}) {
  const onSearch = vi.fn();
  const onClear = vi.fn();
  function Harness({ busy = initial.busy ?? false, identity = "first" }) {
    const [query, setQuery] = useState(initial.query ?? "");
    return <ImportedReaderSearch key={identity} tr={tr} busy={busy}
      query={query} hasHits={initial.hasHits ?? false} onQueryChange={setQuery}
      onSearch={() => onSearch(query)} onClear={() => { onClear(); setQuery(""); }} />;
  }
  const view = render(<Harness />);
  const input = screen.getByRole("searchbox") as HTMLInputElement;
  return { ...view, Harness, input, form: input.form!, onSearch, onClear };
}

it("preserves mixed text and submits by explicit button or plain Enter", async () => {
  const { input, onSearch } = setup();
  const text = "中文 English 🧪 /path/with space";
  fireEvent.change(input, { target: { value: text } });
  expect(input).toHaveValue(text);
  await userEvent.click(screen.getByRole("button", { name: searchName }));
  expect(onSearch).toHaveBeenLastCalledWith(text);
  await userEvent.click(input);
  await userEvent.keyboard("{Enter}");
  expect(onSearch).toHaveBeenCalledTimes(2);
});

it("blocks submit during composition without consuming candidate keys or changing text", () => {
  const { input, form, onSearch } = setup();
  fireEvent.compositionStart(input);
  fireEvent.change(input, { target: { value: "ni" } });
  const key = new KeyboardEvent("keydown", { key: "Enter", isComposing: true, bubbles: true, cancelable: true });
  fireEvent(input, key);
  expect(key.defaultPrevented).toBe(false);
  expect(fireEvent.submit(form)).toBe(false);
  expect(onSearch).not.toHaveBeenCalled();
  expect(input).toHaveValue("ni");
  fireEvent.change(input, { target: { value: "你" } });
  fireEvent.compositionEnd(input, { data: "你" });
  fireEvent.keyUp(input, { key: "Enter" });
  fireEvent.submit(form);
  expect(onSearch).toHaveBeenCalledExactlyOnceWith("你");
});

it.each([
  { key: "Enter", isComposing: true },
  { key: "Enter", isComposing: false, keyCode: 229 },
  { key: "Enter", ctrlKey: true },
  { key: "Enter", altKey: true },
  { key: "Enter", metaKey: true },
  { key: "Enter", shiftKey: true },
])("rejects an implicit submit for owned or modified Enter: %j", (init) => {
  const { input, form, onSearch } = setup({ query: "ready" });
  const key = new KeyboardEvent("keydown", { ...init, bubbles: true, cancelable: true });
  fireEvent(input, key);
  expect(key.defaultPrevented).toBe(false);
  fireEvent.compositionEnd(input);
  fireEvent.submit(form);
  expect(onSearch).not.toHaveBeenCalled();
  fireEvent.keyUp(input, { key: "Enter" });
  fireEvent.keyDown(input, { key: "Enter" });
  fireEvent.submit(form);
  expect(onSearch).toHaveBeenCalledExactlyOnceWith("ready");
});

it("does not revive an already cancelled submit", () => {
  const { form, onSearch } = setup();
  const event = new Event("submit", { bubbles: true, cancelable: true });
  event.preventDefault();
  fireEvent(form, event);
  expect(onSearch).not.toHaveBeenCalled();
});

it("does not revive an already consumed Enter", () => {
  const { input, form, onSearch } = setup();
  const event = new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true });
  event.preventDefault();
  fireEvent(input, event);
  fireEvent.submit(form);
  expect(onSearch).not.toHaveBeenCalled();
});

it("enforces busy on the form, leaves typing available and recovers when ready", () => {
  const { input, form, onSearch, rerender, Harness } = setup({ busy: true });
  fireEvent.change(input, { target: { value: "next query" } });
  expect(input).toHaveValue("next query");
  expect(screen.getByRole("button", { name: searchName })).toBeDisabled();
  fireEvent.submit(form);
  expect(onSearch).not.toHaveBeenCalled();
  rerender(<Harness busy={false} />);
  fireEvent.submit(form);
  expect(onSearch).toHaveBeenCalledExactlyOnceWith("next query");
});

it("blocks a covered reader and restores search when the newer scope closes", () => {
  const { form, onSearch } = setup({ query: "unchanged" });
  const overlay = document.createElement("div");
  overlay.setAttribute("role", "dialog");
  const input = document.createElement("input");
  overlay.append(input);
  document.body.append(overlay);
  const stop = installDialogFocus(() => overlay, { initialFocus: "none" });
  try {
    input.focus();
    fireEvent.submit(form);
    expect(onSearch).not.toHaveBeenCalled();
    expect(document.activeElement).toBe(input);
  } finally { stop(); overlay.remove(); }
  fireEvent.submit(form);
  expect(onSearch).toHaveBeenCalledExactlyOnceWith("unchanged");
});

it("clears a cancelled composition gate on blur without erasing input", () => {
  const { input, form, onSearch } = setup({ query: "draft" });
  fireEvent.compositionStart(input);
  fireEvent.keyDown(input, { key: "Enter", keyCode: 229 });
  fireEvent.blur(input);
  fireEvent.submit(form);
  expect(onSearch).toHaveBeenCalledExactlyOnceWith("draft");
  expect(input).toHaveValue("draft");
});

it("does not carry an unfinished composition gate to another reader identity", () => {
  const { input, onSearch, rerender, Harness } = setup({ query: "next" });
  fireEvent.compositionStart(input);
  fireEvent.keyDown(input, { key: "Enter", isComposing: true });
  rerender(<Harness identity="second" />);
  fireEvent.submit((screen.getByRole("searchbox") as HTMLInputElement).form!);
  expect(onSearch).toHaveBeenCalledExactlyOnceWith("next");
});

it("keeps clear a non-submit action, including empty queries with retained hits", () => {
  const { input, onClear, onSearch } = setup({ hasHits: true });
  fireEvent.click(screen.getByRole("button", { name: clearName }));
  expect(onClear).toHaveBeenCalledOnce();
  expect(onSearch).not.toHaveBeenCalled();
  expect(input).toHaveValue("");
});

it("keeps long query edits local until a deliberate search", () => {
  const { input, onSearch } = setup();
  const text = "长文本🙂 /mixed/path ".repeat(400);
  fireEvent.change(input, { target: { value: text } });
  expect(input).toHaveValue(text);
  expect(onSearch).not.toHaveBeenCalled();
  expect(screen.getByRole("button", { name: clearName })).toBeEnabled();
});
