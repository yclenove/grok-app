/** @vitest-environment jsdom */
import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, expect, test, vi } from "vitest";
import type { MessageKey, Vars } from "@/i18n";
import { ImportedEventList } from "./ImportedEventList";

afterEach(() => cleanup());

const tr = (key: MessageKey, vars?: Vars) =>
  vars ? `${key}:${JSON.stringify(vars)}` : key;

test("assistant messages use inert Markdown and user/system messages stay literal", () => {
  render(
    <ImportedEventList tr={tr} events={[
      { id: "a", kind: "message", role: "assistant", text: "## 回答\n\n**重点** ![tracking](https://example.invalid/img)" },
      { id: "u", kind: "message", role: "user", text: "## 用户原文" },
      { id: "s", kind: "message", role: "system", text: "**系统原文** [run](grok://run)" },
    ]} />,
  );
  expect(screen.getByRole("heading", { name: "回答" })).toBeTruthy();
  expect(screen.getByText("重点").tagName).toBe("STRONG");
  expect(screen.getByText("## 用户原文").className).toBe("session-import__user-body");
  expect(screen.getByText("**系统原文** [run](grok://run)").tagName).toBe("PRE");
  expect(screen.queryByText("assistant")).toBeNull();
  expect(screen.queryByText("user")).toBeNull();
  expect(document.querySelectorAll("a, img, video, iframe")).toHaveLength(0);
});

test("anchors nested base.id and shows three issue classes plus system notice", () => {
  render(
    <ImportedEventList
      tr={tr}
      highlightId="u1"
      images={{ states: { "blob-9": { state: "missing" } }, retry: vi.fn() }}
      events={[
        {
          kind: "unsupported",
          sourceType: "weird-block",
          base: { id: "u1" },
          role: "system",
          complete: false,
          blocks: [
            { kind: "asset", assetId: "blob-9", alt: "shot.png" },
            { kind: "text", text: "[[chat:other]] keep as text" },
          ],
        },
      ]}
    />,
  );
  const row = document.getElementById("import-ev-u1");
  expect(row).toBeTruthy();
  expect(row?.getAttribute("data-highlight")).toBe("1");
  expect(screen.getByText(/unsupportedBlock/)).toBeTruthy();
  expect(screen.getByText(/mediaMissing/)).toBeTruthy();
  expect(screen.getByText(/mediaIncomplete/)).toBeTruthy();
  expect(screen.queryByText(/truncatedOutput/)).toBeNull();
  expect(screen.getByText(/systemNotice/)).toBeTruthy();
  expect(screen.getByText(/\[\[chat:other\]\] keep as text/)).toBeTruthy();
  expect(document.querySelectorAll("a").length).toBe(0);
});

test("copy text copies only the visible message body", () => {
  Object.assign(navigator, { clipboard: { writeText: vi.fn().mockResolvedValue(undefined) } });
  render(
    <ImportedEventList
      tr={tr}
      events={[
        {
          id: "m1",
          kind: "message",
          role: "user",
          blocks: [{ kind: "text", text: "hello" }],
        },
      ]}
    />,
  );
  fireEvent.click(screen.getByText("sessionImport.copyText"));
  expect(navigator.clipboard.writeText).toHaveBeenCalledWith("hello");
});

test("continue from here is offered on message events and skipped for system", () => {
  const onContinueFrom = vi.fn();
  render(
    <ImportedEventList
      tr={tr}
      onContinueFrom={onContinueFrom}
      events={[
        {
          id: "m1",
          kind: "message",
          role: "user",
          blocks: [{ kind: "text", text: "hello" }],
        },
        {
          id: "s1",
          kind: "message",
          role: "system",
          blocks: [{ kind: "text", text: "[[chat:x]]" }],
        },
      ]}
    />,
  );
  const buttons = screen.getAllByText("sessionImport.continueFromHere");
  expect(buttons).toHaveLength(1);
  fireEvent.click(buttons[0]);
  expect(onContinueFrom).toHaveBeenCalledWith("m1");
});

test("renders actual Rust tool arguments and snake_case diagnostic identities as inert text", () => {
  render(<ImportedEventList tr={tr} events={[
    { id: "tool", kind: "tool-call", name: "exec_command", input: { encoding: "json", value: { command: "<script>do not run</script>" } } },
    { id: "unsupported", kind: "unsupported", source_type: "rust-custom", blocks: [{ kind: "asset", asset_id: "asset-from-rust" }] },
  ]} />);
  expect(screen.getByText(/"command": "<script>do not run<\/script>"/).tagName).toBe("PRE");
  expect(screen.getByText('sessionImport.unsupportedBlock:{"type":"rust-custom"}')).toBeTruthy();
  expect(screen.getByText('sessionImport.peekLoading')).toBeTruthy();
  expect(screen.queryByText(/asset-from-rust/)).toBeNull();
  expect(document.querySelectorAll("script, iframe, a, img, video")).toHaveLength(0);
});

test("opens complete long text in a bounded reader, finds its tail and copies only its visible page", async () => {
  const copy = vi.fn().mockResolvedValue(undefined);
  Object.assign(navigator, { clipboard: { writeText: copy } });
  const content = "a".repeat(4200) + "TAIL_SENTINEL" + "b".repeat(4200);
  const onContinueFrom = vi.fn();
  render(<ImportedEventList tr={tr} onContinueFrom={onContinueFrom} events={[
    { id: "long", kind: "message", role: "user", complete: true, text: content },
  ]} />);
  expect(screen.queryByText("sessionImport.truncatedOutput")).toBeNull();
  const open = screen.getByRole("button", { name: "sessionImport.readFullText" });
  open.focus();
  await userEvent.keyboard("{Enter}");
  const dialog = screen.getByRole("dialog", { name: "sessionImport.readFullText" });
  expect(dialog.querySelector("pre")?.textContent).toHaveLength(4000);
  const input = within(dialog).getByRole("searchbox");
  fireEvent.change(input, { target: { value: "tail_sentinel" } });
  fireEvent.click(within(dialog).getByRole("button", { name: "sessionImport.search" }));
  expect(dialog.querySelector("pre")?.textContent).toContain("TAIL_SENTINEL");
  expect(dialog.querySelector("pre")!.textContent!.length).toBeLessThanOrEqual(4000);
  fireEvent.click(within(dialog).getByRole("button", { name: "sessionImport.copyVisible" }));
  expect(copy).toHaveBeenLastCalledWith(dialog.querySelector("pre")?.textContent);
  fireEvent.click(within(dialog).getByRole("button", { name: "sessionImport.close" }));
  expect(screen.queryByRole("dialog")).toBeNull();
  expect(document.activeElement).toBe(open);
  expect(onContinueFrom).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "sessionImport.copyText" }));
  expect(copy).toHaveBeenLastCalledWith(content.slice(0, 4000));
});

test("full-text actions isolate ancestor activation and reader shortcuts, and removed events cannot reopen a dialog", async () => {
  const activate = vi.fn();
  const navigate = vi.fn();
  const event = { id: "long", kind: "message", role: "assistant", text: "x".repeat(4500) };
  function Harness({ visible = true }) {
    return <div onClick={activate} onKeyDown={(key) => { if (key.key === "n") navigate(); }}>
      <ImportedEventList tr={tr} events={visible ? [event] : []} />
    </div>;
  }
  const view = render(<Harness />);
  fireEvent.click(screen.getByRole("button", { name: "sessionImport.readFullText" }));
  expect(activate).not.toHaveBeenCalled();
  const close = screen.getByRole("button", { name: "sessionImport.close" });
  close.focus();
  await userEvent.keyboard("n");
  expect(navigate).not.toHaveBeenCalled();
  fireEvent.click(close);
  expect(activate).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "sessionImport.readFullText" }));
  view.rerender(<Harness visible={false} />);
  expect(screen.queryByRole("dialog")).toBeNull();
  view.rerender(<Harness />);
  expect(screen.queryByRole("dialog")).toBeNull();
});
