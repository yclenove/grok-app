/** @vitest-environment jsdom */
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import type { MessageKey, Vars } from "@/i18n";
import { SessionImportSourceActions, SessionImportSourceStep } from "./SessionImportSourceStep";
import { installDialogFocus } from "@/lib/a11yFocus";
import {
  choiceFromCustomFile,
  choiceFromDefaultRoot,
  type SourceChoice,
} from "@/lib/sessionImport/sourceChoice";

afterEach(() => cleanup());

const tr = (key: MessageKey, vars?: Vars) =>
  vars ? `${key}:${JSON.stringify(vars)}` : key;

const roots = [
  {
    provider: "codex" as const,
    kind: "codex-home",
    path: "C:\\Users\\me\\.codex",
    exists: true,
    readable: true,
  },
];

const registrations = [
  {
    sourceId: "src-1",
    provider: "cursor" as const,
    rootPath: "D:\\cursor-copy",
    profile: "default",
  },
];

function renderStep(extra: { choice?: SourceChoice | null; busy?: boolean; scanStarting?: boolean } = {}) {
  const onChoose = vi.fn();
  const onPickFolder = vi.fn();
  const onPickFile = vi.fn();
  const onRelocate = vi.fn();
  const onScan = vi.fn();
  const state = { choice: null, busy: false, scanStarting: false, ...extra };
  render(
    <>
    <SessionImportSourceStep
      tr={tr}
      roots={roots}
      registrations={registrations}
      choice={state.choice}
      busy={state.busy}
      onChoose={onChoose}
      onPickFolder={onPickFolder}
      onPickFile={onPickFile}
      onRelocate={onRelocate}
      onScan={onScan}
    />
    <SessionImportSourceActions tr={tr} {...state} onChoose={onChoose} onScan={onScan} />
    </>,
  );
  return { onChoose, onPickFolder, onPickFile, onRelocate, onScan };
}

test("default roots, registered sources, folder and file stay separate", () => {
  const { onPickFolder } = renderStep();
  expect(screen.getByText("sessionImport.defaults")).toBeTruthy();
  expect(screen.getByText("sessionImport.registered")).toBeTruthy();
  expect(screen.getByText("sessionImport.customRoot")).toBeTruthy();
  expect(document.querySelector('[data-kind="default"]')).toBeTruthy();
  expect(document.querySelector('[data-kind="registered"]')).toBeTruthy();
  expect(screen.getAllByText("sessionImport.entryHint")).toHaveLength(1);
  expect(screen.queryByText("sessionImport.keyboardHint")).toBeNull();
  expect(screen.getByText("sessionImport.scanAll")).toBeTruthy();
  fireEvent.click(screen.getByText("sessionImport.chooseRoot"));
  expect(onPickFolder).toHaveBeenCalledWith("codex");
});

test("custom folder provider can be selected before opening the picker", () => {
  const { onPickFolder } = renderStep();
  fireEvent.click(screen.getByRole("button", { name: "sessionImport.customRoot" }));
  fireEvent.click(screen.getByRole("button", { name: "sessionImport.source.claude" }));
  fireEvent.click(screen.getByText("sessionImport.chooseRoot"));
  expect(onPickFolder).toHaveBeenCalledWith("claude-code");
});

test("changing provider also updates an already chosen folder", () => {
  const choice: SourceChoice = { kind: "custom-folder", provider: "generic-file", path: "D:/copy" };
  const { onChoose, onScan } = renderStep({ choice });
  fireEvent.click(screen.getByRole("button", { name: "sessionImport.customRoot" }));
  fireEvent.click(screen.getByRole("button", { name: /^sessionImport\.source\.codex$/ }));
  expect(onChoose).toHaveBeenCalledWith({ ...choice, provider: "codex" });
  expect(onScan).not.toHaveBeenCalled();
});

test("picking a default root is not a custom folder", () => {
  const { onChoose } = renderStep();
  fireEvent.click(document.querySelector('[data-kind="default"]') as HTMLButtonElement);
  expect(onChoose).toHaveBeenCalledWith(
    choiceFromDefaultRoot("codex", "C:\\Users\\me\\.codex"),
  );
});

test("relocate is a distinct action on a registered source", () => {
  const { onRelocate, onChoose } = renderStep();
  fireEvent.click(document.querySelector('[data-kind="relocate"]') as HTMLButtonElement);
  expect(onRelocate).toHaveBeenCalledWith(registrations[0]);
  expect(onChoose).not.toHaveBeenCalled();
});

test("a chosen file shows the file kind, not the folder kind", () => {
  renderStep({ choice: choiceFromCustomFile("D:\\one.json") });
  expect(document.querySelector('[data-kind="custom-file"]')).toBeTruthy();
  expect(screen.getByText("sessionImport.kind.file")).toBeTruthy();
  expect(screen.getByText("sessionImport.scan")).toBeTruthy();
});

test("Enter does not scan while IME is composing", () => {
  const { onScan } = renderStep();
  const root = document.querySelector(".session-import__source") as HTMLElement;
  fireEvent.keyDown(root, { key: "Enter", isComposing: true });
  expect(onScan).not.toHaveBeenCalled();
  fireEvent.keyDown(root, { key: "Enter", isComposing: false });
  expect(onScan).toHaveBeenCalledTimes(1);
});

test("Enter on a picker button does not bubble into scan-all", () => {
  const { onScan } = renderStep();
  fireEvent.keyDown(screen.getByText("sessionImport.fileMode"), { key: "Enter" });
  expect(onScan).not.toHaveBeenCalled();
});


test.each([
  { key: "Enter", isComposing: true },
  { key: "Enter", keyCode: 229, isComposing: false },
  { key: "Enter", ctrlKey: true },
  { key: "Enter", altKey: true },
  { key: "Enter", metaKey: true },
  { key: "Enter", shiftKey: true },
])("source leaves composing or modified Enter untouched: %j", (init) => {
  const { onScan } = renderStep();
  const root = document.querySelector(".session-import__source") as HTMLElement;
  const event = new KeyboardEvent("keydown", { bubbles: true, cancelable: true, ...init });
  fireEvent(root, event);
  expect(onScan).not.toHaveBeenCalled();
  expect(event.defaultPrevented).toBe(false);
});

test("source does not scan for an already consumed Enter", () => {
  const { onScan } = renderStep();
  const root = document.querySelector(".session-import__source") as HTMLElement;
  const event = new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true });
  event.preventDefault();
  fireEvent(root, event);
  expect(onScan).not.toHaveBeenCalled();
});

test("source cannot scan underneath a newer dialog but recovers after close", () => {
  const { onScan } = renderStep();
  const root = document.querySelector(".session-import__source") as HTMLElement;
  const overlay = document.createElement("div");
  overlay.setAttribute("role", "dialog");
  const input = document.createElement("input");
  overlay.append(input);
  document.body.append(overlay);
  const stop = installDialogFocus(() => overlay, { initialFocus: "none" });
  try {
    input.focus();
    const event = new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true });
    fireEvent(root, event);
    expect(onScan).not.toHaveBeenCalled();
    expect(event.defaultPrevented).toBe(false);
    expect(document.activeElement).toBe(input);
  } finally {
    stop();
    overlay.remove();
  }
  expect(fireEvent.keyDown(root, { key: "Enter" })).toBe(false);
  expect(onScan).toHaveBeenCalledOnce();
});

test("source leaves Enter alone while busy and preserves explicit scan button activation", () => {
  const busy = renderStep({ busy: true });
  const root = document.querySelector(".session-import__source") as HTMLElement;
  expect(fireEvent.keyDown(root, { key: "Enter" })).toBe(true);
  fireEvent.click(screen.getByRole("button", { name: "sessionImport.scanAll" }));
  expect(busy.onScan).not.toHaveBeenCalled();
  cleanup();
  const ready = renderStep();
  fireEvent.click(screen.getByRole("button", { name: "sessionImport.scanAll" }));
  expect(ready.onScan).toHaveBeenCalledOnce();
});

test.each([
  [null, "sessionImport.scanAll"],
  [choiceFromCustomFile("D:/selected.json"), "sessionImport.scan"],
] as const)("V117 unrelated busy preserves the scan action for %j", (choice, label) => {
  const { onScan, onPickFile, onPickFolder } = renderStep({ choice, busy: true });
  const scan = screen.getByRole("button", { name: label }) as HTMLButtonElement;
  expect(scan.disabled).toBe(true);
  expect(screen.queryByText("sessionImport.scanning")).toBeNull();
  fireEvent.click(scan);
  fireEvent.click(screen.getByText("sessionImport.fileMode"));
  fireEvent.click(screen.getByText("sessionImport.chooseRoot"));
  expect(onScan).not.toHaveBeenCalled();
  expect(onPickFile).not.toHaveBeenCalled();
  expect(onPickFolder).not.toHaveBeenCalled();
});

test("V117 an actual pending scan displays its own progress label", () => {
  const { onScan } = renderStep({ busy: true, scanStarting: true });
  const scan = screen.getByRole("button", { name: "sessionImport.scanning" }) as HTMLButtonElement;
  expect(scan.disabled).toBe(true);
  fireEvent.click(scan);
  expect(onScan).not.toHaveBeenCalled();
});

test("manual source controls precede both location lists in DOM order", () => {
  renderStep();
  const manual = screen.getByText("sessionImport.chooseRoot");
  for (const key of ["sessionImport.defaults", "sessionImport.registered"]) {
    expect(manual.compareDocumentPosition(screen.getByRole("list", { name: key })) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  }
  expect(document.querySelector(".session-import__source .session-import__footer")).toBeNull();
});

test("long shared-prefix paths retain distinct tails and exact source identity", () => {
  const prefix = "C:\\Users\\me\\AppData\\Local\\VeryLongSyntheticImportRoot\\";
  const paths = [`${prefix}codex-one`, `${prefix}codex-two`];
  const onChoose = vi.fn();
  render(<SessionImportSourceStep tr={tr} roots={[]}
    registrations={paths.map((rootPath, i) => ({ ...registrations[0], sourceId: `source-${i}`, rootPath }))}
    choice={null} busy={false} onChoose={onChoose} onPickFolder={vi.fn()} onPickFile={vi.fn()}
    onRelocate={vi.fn()} onScan={vi.fn()} />);
  expect(screen.getByText("sessionImport.noSources")).toBeTruthy();
  paths.forEach((path, i) => {
    const location = screen.getByTitle(path);
    expect(location.querySelector('[aria-hidden="true"]')?.textContent).toContain(`codex-${i === 0 ? "one" : "two"}`);
    expect(location.querySelector(".sr-only")?.textContent).toBe(path);
    fireEvent.click(location.closest("button")!);
    expect(onChoose).toHaveBeenLastCalledWith({ kind: "registered", provider: "cursor", sourceId: `source-${i}`, path });
  });
});

test.each(["default", "registered", "relocate"] as const)("selected %s source exposes its full path outside compact cards", (kind) => {
  const choice: SourceChoice = { kind, provider: "codex", path: "D:/long-prefix/imports/a-complete-path", sourceId: "known" };
  const { onChoose } = renderStep({ choice });
  expect(document.querySelector(".session-import__source-choice .session-import__locator")?.textContent).toBe(choice.path);
  fireEvent.click(screen.getByText("sessionImport.clearSource"));
  expect(onChoose).toHaveBeenCalledWith(null);
});
