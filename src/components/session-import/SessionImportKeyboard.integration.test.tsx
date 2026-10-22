/** @vitest-environment jsdom */
import "@testing-library/jest-dom/vitest";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { useRef, useState, type ComponentProps, type RefObject } from "react";
import { act, cleanup, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { GlassModal } from "@/components/GlassModal";
import { createT } from "@/i18n";
import type { ImportStep } from "@/hooks/useSessionImport";
import { listFocusable } from "@/lib/a11yFocus";
import type { SourceCandidate } from "@/lib/api/sessionMigration";
import { SessionImportContentStep } from "./SessionImportContentStep";
import { SessionImportStepNav } from "./SessionImportStepNav";

const stylesDir = join(dirname(fileURLToPath(import.meta.url)), "../../styles");
const modalStyles = readFileSync(join(stylesDir, "chat.part4.css"), "utf8");
const importStyles = readFileSync(join(stylesDir, "session-import.css"), "utf8");
const workbenchStyles = readFileSync(join(stylesDir, "session-import-workbench.css"), "utf8");
const readerStyles = readFileSync(join(stylesDir, "session-import-reader.css"), "utf8");

vi.mock("@/lib/nativeWebviewCover", () => ({ acquireNativeWebviewCover: () => () => {} }));

let style: HTMLStyleElement;
beforeEach(() => {
  style = document.createElement("style");
  const modalRules = ["modal", "modal-body", "modal-actions"].map((name) => {
    const rule = modalStyles.match(new RegExp(`^\\.${name}\\s*\\{[^}]+\\}`, "m"))?.[0];
    expect(rule).toBeTruthy();
    return rule;
  });
  style.textContent = [
    ...modalRules,
    workbenchStyles,
    readerStyles,
    importStyles.replace(/^@import[^\n]+\n/gm, ""),
  ].join("\n");
  document.head.append(style);
});
afterEach(() => {
  cleanup();
  style.remove();
});

const tr = createT("en");
const openable = (step: string) => step === "source" || step === "content";

function nativeKey(target: EventTarget, key: string, extra?: { composing?: boolean; keyCode?: number }) {
  const event = new KeyboardEvent("keydown", {
    key,
    bubbles: true,
    cancelable: true,
    isComposing: extra?.composing ?? false,
  });
  if (extra?.keyCode != null) {
    Object.defineProperty(event, "keyCode", { configurable: true, value: extra.keyCode });
  }
  target.dispatchEvent(event);
}

function WizardMenu({
  busy = false,
  layout = "narrow",
  onGo = vi.fn(),
  onClose = vi.fn(),
  allow = openable,
}: {
  busy?: boolean;
  layout?: "wide" | "narrow";
  onGo?: (step: string) => void;
  onClose?: () => void;
  allow?: (step: string) => boolean;
}) {
  return (
    <GlassModal open onClose={onClose} title="Wizard" closeLabel="Close wizard" wrapBody
      className="session-import-modal session-import-wizard">
      <SessionImportStepNav
        tr={tr}
        step="content"
        busy={busy}
        layout={layout}
        stepOpenable={allow as (step: ImportStep) => boolean}
        onGo={onGo as (step: ImportStep) => void}
      />
    </GlassModal>
  );
}

test("narrow step menu owns keyboard inside the parent modal", async () => {
  const user = userEvent.setup();
  const onGo = vi.fn();
  const onClose = vi.fn();
  const view = render(<WizardMenu onGo={onGo} onClose={onClose} />);
  const trigger = screen.getByRole("button", { name: "Import steps" });
  await waitFor(() => expect(screen.getByRole("button", { name: "Close wizard" })).toHaveFocus());
  nativeKey(trigger, "ArrowDown", { composing: true });
  expect(screen.queryByRole("menu")).toBeNull();
  nativeKey(trigger, "ArrowDown", { keyCode: 229 });
  expect(screen.queryByRole("menu")).toBeNull();

  trigger.focus();
  await user.keyboard("{ArrowDown}");
  const source = await screen.findByRole("menuitem", { name: "1. Source" });
  const prepare = screen.getByRole("menuitem", { name: "3. Prepare" });
  expect(prepare).toBeDisabled();
  await waitFor(() => expect(source).toHaveFocus());
  await user.keyboard("{ArrowDown}");
  const sessions = screen.getByRole("menuitem", { name: "2. Sessions" });
  expect(sessions).toHaveFocus();
  await user.keyboard("{Tab}");
  expect(source).toHaveFocus();
  await user.keyboard("{Shift>}{Tab}{/Shift}");
  expect(sessions).toHaveFocus();
  await user.keyboard("{Enter}");
  expect(onGo).toHaveBeenCalledExactlyOnceWith("content");
  expect(screen.queryByRole("menu")).toBeNull();
  expect(onClose).not.toHaveBeenCalled();
  expect(trigger).toHaveFocus();

  await user.keyboard("{ArrowDown}");
  await screen.findByRole("menu");
  nativeKey(document, "Escape", { composing: true });
  nativeKey(document, "Escape", { keyCode: 229 });
  expect(screen.getByRole("menu")).toBeTruthy();
  await user.keyboard("{Escape}");
  expect(screen.queryByRole("menu")).toBeNull();
  expect(onClose).not.toHaveBeenCalled();
  expect(trigger).toHaveFocus();
  await user.keyboard("{Escape}");
  expect(onClose).toHaveBeenCalledOnce();

  onClose.mockClear();
  view.rerender(<WizardMenu onGo={onGo} onClose={onClose} />);
  await user.click(screen.getByRole("button", { name: "Import steps" }));
  await screen.findByRole("menu");
  view.rerender(<WizardMenu onGo={onGo} onClose={onClose} layout="wide" />);
  expect(screen.queryByRole("menu")).toBeNull();
  expect(onClose).not.toHaveBeenCalled();
});

test("busy menu items do not navigate, and a newer modal owns Escape", async () => {
  const user = userEvent.setup();
  const onGo = vi.fn();
  const wizardClose = vi.fn();
  const newerClose = vi.fn();
  const controls = {
    setBusy: (_busy: boolean) => {},
    setNewer: (_open: boolean) => {},
  };
  function Stack() {
    const [busy, setBusy] = useState(true);
    const [newer, setNewer] = useState(false);
    controls.setBusy = setBusy;
    controls.setNewer = setNewer;
    return (
      <>
        <button type="button">Outside</button>
        <WizardMenu busy={busy} onGo={onGo} onClose={wizardClose} allow={() => true} />
        {newer ? (
          <GlassModal open onClose={() => { newerClose(); setNewer(false); }} title="Newer" closeLabel="Close newer">
            <button type="button">Newer action</button>
          </GlassModal>
        ) : null}
      </>
    );
  }
  render(<Stack />);
  await user.click(screen.getByRole("button", { name: "Import steps" }));
  const item = await screen.findByRole("menuitem", { name: "1. Source" });
  expect(item).toBeDisabled();
  await user.click(item);
  expect(onGo).not.toHaveBeenCalled();
  await user.keyboard("{Escape}");
  expect(screen.queryByRole("menu")).toBeNull();
  expect(wizardClose).not.toHaveBeenCalled();

  await user.click(screen.getByRole("button", { name: "Import steps" }));
  await screen.findByRole("menu");
  await user.click(screen.getByRole("button", { name: "Outside" }));
  expect(screen.queryByRole("menu")).toBeNull();
  expect(wizardClose).not.toHaveBeenCalled();

  await act(async () => { controls.setBusy(false); });
  await user.click(screen.getByRole("button", { name: "Import steps" }));
  await screen.findByRole("menu");
  await act(async () => { controls.setNewer(true); });
  expect(await screen.findByRole("button", { name: "Close newer" })).toBeTruthy();
  expect(screen.getByRole("menu")).toBeTruthy();
  await user.keyboard("{Escape}");
  expect(newerClose).toHaveBeenCalledOnce();
  expect(wizardClose).not.toHaveBeenCalled();
  expect(screen.queryByRole("button", { name: "Close newer" })).toBeNull();
  expect(screen.getByRole("menu")).toBeTruthy();
  await user.keyboard("{Escape}");
  expect(screen.queryByRole("menu")).toBeNull();
  expect(wizardClose).not.toHaveBeenCalled();
});

const candidate: SourceCandidate = {
  candidateId: "synthetic-1",
  provider: "codex",
  nativeSessionId: "native",
  title: "Synthetic session",
  locator: "H:\\sessions\\chat.json",
  sourceRevision: "rev-1",
  completeness: "complete",
};

function ContentDialog({ layout, busy = false }: { layout: "wide" | "narrow"; busy?: boolean }) {
  const filterRef = useRef<HTMLInputElement>(null);
  const [listFilter, setListFilter] = useState("");
  const [onlyIncomplete, setOnlyIncomplete] = useState(false);
  const [onlyArchived, setOnlyArchived] = useState(false);
  type ContentProps = ComponentProps<typeof SessionImportContentStep>;
  const ui = {
    scanIssues: [],
    discoveredCount: 1,
    selectedCount: 1,
    issueCount: 0,
    selected: new Set([candidate.candidateId]),
    scanCancelling: false,
    scanning: false,
    scanPaused: false,
    scanFailed: false,
    scanState: { state: "completed" },
    candidates: [candidate],
    busy,
    nextCursor: null,
    pageIndex: 0,
    extraCount: 0,
    cancelScan: vi.fn(),
    continueScan: vi.fn(),
    selectCurrent: vi.fn(),
    clearSelected: vi.fn(),
    selectThese: vi.fn(),
    invertThese: vi.fn(),
    refreshScan: vi.fn(),
    previewSelected: vi.fn(),
  } as unknown as ContentProps["ui"];
  const contentStates = {
    active: null,
    byCandidate: {},
    dispatchSelection: vi.fn(),
    confirmLeaf: vi.fn(),
    loadMore: vi.fn(),
  } as unknown as ContentProps["contentStates"];
  return (
    <GlassModal open onClose={vi.fn()} title="Content" closeLabel="Close wizard" wrapBody
      className="session-import-modal session-import-wizard session-import-wizard--content">
      <SessionImportContentStep
        tr={tr}
        ui={ui}
        contentStates={contentStates}
        filteredCandidates={[candidate]}
        filteredExtras={[]}
        extraCandidates={[]}
        visibleIds={[candidate.candidateId]}
        listFilter={listFilter}
        setListFilter={setListFilter}
        onlyIncomplete={onlyIncomplete}
        setOnlyIncomplete={setOnlyIncomplete}
        onlyArchived={onlyArchived}
        setOnlyArchived={setOnlyArchived}
        filterRef={filterRef as RefObject<HTMLInputElement | null>}
        loadMoreScan={vi.fn()}
        previousScanPage={vi.fn()}
        toggleCandidate={vi.fn()}
        peekCandidate={vi.fn()}
        copyVisibleImportError={vi.fn()}
        layout={layout}
      />
    </GlassModal>
  );
}

test("hidden content panes are excluded from focus and recover inside the modal", async () => {
  const user = userEvent.setup();
  expect(style.textContent).toContain(".session-import__candidates-pane[hidden]");
  const view = render(<ContentDialog layout="narrow" />);
  const preview = document.querySelector(".session-import__preview-pane") as HTMLElement;
  const list = document.querySelector(".session-import__candidates-pane") as HTMLElement;
  expect(preview.hidden).toBe(true);
  expect(preview).toHaveAttribute("inert");
  expect(getComputedStyle(preview).display).toBe("none");
  const dialog = screen.getByRole("dialog");
  for (const node of listFocusable(dialog)) expect(preview.contains(node)).toBe(false);

  await user.click(screen.getByRole("button", { name: "Preview" }));
  await waitFor(() => expect(preview.hidden).toBe(false));
  expect(list.hidden).toBe(true);
  expect(getComputedStyle(list).display).toBe("none");
  expect(preview).toHaveFocus();
  for (const node of listFocusable(dialog)) expect(list.contains(node)).toBe(false);

  await user.click(screen.getByRole("button", { name: "Session list" }));
  expect(list.hidden).toBe(false);
  expect(preview.hidden).toBe(true);
  expect(screen.getByRole("button", { name: "Session list" })).toHaveFocus();
  for (const node of listFocusable(dialog)) expect(preview.contains(node)).toBe(false);

  view.rerender(<ContentDialog layout="wide" />);
  expect(list.hidden).toBe(false);
  expect(preview.hidden).toBe(false);
  list.querySelector("button")?.focus();
  view.rerender(<ContentDialog layout="narrow" />);
  await waitFor(() => expect(list.hidden).toBe(false));
  expect(preview.hidden).toBe(true);
});
