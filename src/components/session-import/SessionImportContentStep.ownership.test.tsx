/** @vitest-environment jsdom */
import "@testing-library/jest-dom/vitest";
import { useRef, useState, type ComponentProps, type RefObject } from "react";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, expect, test, vi } from "vitest";
import { GlassModal } from "@/components/GlassModal";
import { createT } from "@/i18n";
import type { SourceCandidate } from "@/lib/api/sessionMigration";
import { peekSummaryFromAdapter } from "@/lib/sessionImport/peekSummary";
import { SessionImportContentStep } from "./SessionImportContentStep";

vi.mock("@/lib/nativeWebviewCover", () => ({ acquireNativeWebviewCover: () => () => {} }));

const tr = createT("en");
const candidate: SourceCandidate = {
  candidateId: "synthetic-c1",
  provider: "codex",
  nativeSessionId: "native",
  title: "Synthetic history",
  locator: "H:\\sessions\\chat.json",
  sourceRevision: "rev-1",
  completeness: "complete",
};

function Harness({
  layout,
  busy = false,
  candidates = [candidate],
  newer = false,
  withPreview = false,
  diagnosis,
  previewError,
  scanError,
}: {
  layout: "wide" | "narrow";
  busy?: boolean;
  candidates?: SourceCandidate[];
  newer?: boolean;
  withPreview?: boolean;
  diagnosis?: string;
  previewError?: string;
  scanError?: { code: string; message: string };
}) {
  const filterRef = useRef<HTMLInputElement>(null);
  const [listFilter, setListFilter] = useState("");
  const [onlyIncomplete, setOnlyIncomplete] = useState(false);
  const [onlyArchived, setOnlyArchived] = useState(false);
  type ContentProps = ComponentProps<typeof SessionImportContentStep>;
  const ui = {
    scanIssues: scanError ? [{ ...scanError, provider: "codex", path: candidate.locator }] : [],
    discoveredCount: candidates.length,
    selectedCount: 0,
    issueCount: 0,
    selected: new Set<string>(),
    scanCancelling: false,
    scanning: false,
    scanPaused: false,
    scanFailed: false,
    scanState: { state: "completed" },
    candidates,
    busy,
    nextCursor: "synthetic-next",
    pageIndex: 1,
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
    active: withPreview ? {
      candidateId: candidate.candidateId,
      title: "Synthetic history",
      busy: false,
      error: previewError ?? null,
      summary: diagnosis ? peekSummaryFromAdapter({ outcome: "partial", issues: [{ code: diagnosis, message: "raw backend diagnosis" }] }) : null,
      selection: {
        previewId: "preview-1",
        leaf: null,
        leaves: [],
        events: [],
        nextCursor: null,
        selectedEventIds: [],
        pinnedEventIds: [],
        assetIds: [],
        fields: {
          messageText: true,
          toolCallName: true,
          toolCallInput: true,
          toolResultContent: true,
          compactionSummary: true,
          visibleReasoning: false,
        },
        includeSubagents: false,
        pendingLeaf: null,
        generation: 1,
        archiveCount: 0,
        modelCount: 0,
        autoSelectPageDefaults: true,
      },
    } : null,
    byCandidate: {},
    dispatchSelection: vi.fn(),
    confirmLeaf: vi.fn(),
    loadMore: vi.fn(),
  } as unknown as ContentProps["contentStates"];
  return (
    <>
      <GlassModal open onClose={vi.fn()} title="Content" closeLabel="Close wizard" wrapBody initialFocus="none"
        className="session-import-modal session-import-wizard session-import-wizard--content">
        <SessionImportContentStep
          tr={tr}
          ui={ui}
          contentStates={contentStates}
          filteredCandidates={candidates}
          filteredExtras={[]}
          extraCandidates={[]}
          visibleIds={candidates.map((row) => row.candidateId)}
          listFilter={listFilter}
          setListFilter={setListFilter}
          onlyIncomplete={onlyIncomplete}
          setOnlyIncomplete={setOnlyIncomplete}
          onlyArchived={onlyArchived}
          setOnlyArchived={setOnlyArchived}
          filterRef={filterRef as RefObject<HTMLInputElement | null>}
          loadMoreScan={spies.loadMore}
          previousScanPage={spies.previous}
          toggleCandidate={spies.toggle}
          peekCandidate={spies.peek}
          copyVisibleImportError={vi.fn()}
          layout={layout}
        />
      </GlassModal>
      {newer ? (
        <GlassModal open onClose={spies.newerClose} title="Newer" closeLabel="Close newer" initialFocus="first">
          <button type="button">Newer action</button>
        </GlassModal>
      ) : null}
    </>
  );
}

const spies = {
  loadMore: vi.fn(),
  previous: vi.fn(),
  toggle: vi.fn(),
  peek: vi.fn(),
  newerClose: vi.fn(),
};

test.each([
  ["trailing-incomplete", "trailingIncomplete"],
  ["broken-record", "brokenRecord"],
  ["invalid-utf8", "invalidUtf8"],
  ["binary-record", "binary"],
  ["unknown-record", "unknownRecord"],
] as const)("preview explains %s even when it has no visible events", (diagnosis, suffix) => {
  render(<Harness layout="wide" withPreview diagnosis={diagnosis} />);
  expect(screen.getByText(`${tr("sessionImport.issues")}: ${tr(`sessionImport.diagnostic.${suffix}`)}`)).toBeVisible();
  expect(screen.queryByText(/raw backend diagnosis/)).toBeNull();
  expect(screen.getByText(/Partial/)).toBeVisible();
});

test("a failed preview shows a localized diagnosis without claiming the conversation is empty", () => {
  render(<Harness layout="wide" withPreview previewError="FORMAT_UNSUPPORTED: binary-record" />);
  expect(screen.getByRole("alert")).toHaveTextContent(tr("sessionImport.diagnostic.binary"));
  expect(screen.queryByText(tr("sessionImport.peekEmpty"))).toBeNull();
});

test("a zero-byte source is explained on the scan page", () => {
  render(<Harness layout="wide" candidates={[]} scanError={{ code: "FORMAT_UNSUPPORTED", message: "empty-jsonl" }} />);
  expect(screen.getByText(`${tr("sessionImport.diagnostic.empty")} — ${candidate.locator}`)).toBeVisible();
});

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

async function openNarrowPreview() {
  const user = userEvent.setup();
  const view = render(<Harness layout="narrow" />);
  await user.click(screen.getByRole("button", { name: "Preview" }));
  const preview = document.querySelector(".session-import__preview-pane") as HTMLElement;
  await waitFor(() => expect(preview.hidden).toBe(false));
  preview.focus();
  return { user, view, preview };
}

test("narrow preview Space, arrows, and modifiers do not operate the hidden list", async () => {
  const { user, preview } = await openNarrowPreview();
  const list = document.querySelector(".session-import__candidates-pane") as HTMLElement;
  expect(list.hidden).toBe(true);
  await user.keyboard(" ");
  await user.keyboard("{ArrowDown}");
  await user.keyboard("{ArrowRight}");
  await user.keyboard("{ArrowLeft}");
  expect(spies.toggle).not.toHaveBeenCalled();
  expect(spies.loadMore).not.toHaveBeenCalled();
  expect(spies.previous).not.toHaveBeenCalled();
  expect(list.contains(document.activeElement)).toBe(false);
  expect(preview.contains(document.activeElement) || document.activeElement === preview).toBe(true);

  const editor = document.createElement("textarea");
  preview.append(editor);
  editor.focus();
  await user.keyboard("{ArrowRight} ");
  expect(spies.loadMore).not.toHaveBeenCalled();
  expect(spies.toggle).not.toHaveBeenCalled();
  expect(document.activeElement).toBe(editor);

  preview.focus();
  const compose = new KeyboardEvent("keydown", { key: " ", bubbles: true, cancelable: true, isComposing: true });
  const ime = new KeyboardEvent("keydown", { key: "ArrowRight", bubbles: true, cancelable: true });
  Object.defineProperty(ime, "keyCode", { get: () => 229 });
  preview.dispatchEvent(compose);
  preview.dispatchEvent(ime);
  preview.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true, cancelable: true, ctrlKey: true }));
  preview.dispatchEvent(new KeyboardEvent("keydown", { key: " ", bubbles: true, cancelable: true, altKey: true }));
  preview.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowRight", bubbles: true, cancelable: true, metaKey: true }));
  expect(spies.toggle).not.toHaveBeenCalled();
  expect(spies.loadMore).not.toHaveBeenCalled();
  expect(list.contains(document.activeElement)).toBe(false);
});

test("a preview control keeps activation and a visible list still toggles", async () => {
  const user = userEvent.setup();
  const view = render(<Harness layout="narrow" withPreview />);
  await user.click(screen.getByRole("button", { name: "Preview" }));
  const selectPage = await screen.findByRole("button", { name: "Select this page" });
  selectPage.focus();
  await user.keyboard(" ");
  expect(spies.toggle).not.toHaveBeenCalled();
  expect(selectPage).toHaveFocus();
  view.unmount();
  render(<Harness layout="wide" />);
  const row = document.querySelector("[data-import-candidate]") as HTMLElement;
  row.focus();
  await user.keyboard(" ");
  expect(spies.toggle).toHaveBeenCalledWith("synthetic-c1");
  await user.keyboard("{ArrowRight}");
  expect(spies.loadMore).toHaveBeenCalledTimes(1);
});

test("wide-narrow-wide keeps list shortcuts on the visible list only", async () => {
  const user = userEvent.setup();
  const view = render(<Harness layout="wide" />);
  const row = document.querySelector("[data-import-candidate]") as HTMLElement;
  row.focus();
  await user.keyboard(" ");
  expect(spies.toggle).toHaveBeenCalledTimes(1);
  view.rerender(<Harness layout="narrow" />);
  const list = document.querySelector(".session-import__candidates-pane") as HTMLElement;
  expect(list.hidden).toBe(false);
  await user.click(screen.getAllByRole("button", { name: "Preview" }).at(-1)!);
  const preview = document.querySelector(".session-import__preview-pane") as HTMLElement;
  await waitFor(() => expect(preview.hidden).toBe(false));
  preview.focus();
  await user.keyboard(" {ArrowRight}");
  expect(spies.toggle).toHaveBeenCalledTimes(1);
  expect(spies.loadMore).not.toHaveBeenCalled();
  view.rerender(<Harness layout="wide" />);
  const visible = document.querySelector("[data-import-candidate]") as HTMLElement;
  visible.focus();
  await user.keyboard(" ");
  expect(spies.toggle).toHaveBeenCalledTimes(2);
});

test("busy rows ignore list shortcuts", async () => {
  const user = userEvent.setup();
  render(<Harness layout="wide" busy />);
  const row = document.querySelector("[data-import-candidate]") as HTMLElement;
  row.focus();
  await user.keyboard(" {ArrowRight}{ArrowDown}");
  expect(spies.toggle).not.toHaveBeenCalled();
  expect(spies.loadMore).not.toHaveBeenCalled();
  expect(document.activeElement).toBe(row);
});

test("removing the focused row does not steal a newer dialog", async () => {
  const user = userEvent.setup();
  const view = render(<Harness layout="wide" />);
  const row = document.querySelector("[data-import-candidate]") as HTMLElement;
  act(() => row.focus());
  view.rerender(<Harness layout="wide" newer />);
  const foreground = screen.getByRole("button", { name: "Newer action" });
  await user.click(foreground);
  view.rerender(<Harness layout="wide" newer candidates={[]} />);
  expect(foreground).toHaveFocus();
  expect(screen.getByRole("searchbox")).not.toHaveFocus();
  expect(screen.getByRole("button", { name: "Close newer" })).toBeTruthy();
  expect(spies.toggle).not.toHaveBeenCalled();
});

test("a newer dialog owns keyboard commands until it closes", async () => {
  const user = userEvent.setup();
  const view = render(<Harness layout="wide" />);
  const row = document.querySelector("[data-import-candidate]") as HTMLElement;
  act(() => row.focus());
  view.rerender(<Harness layout="wide" newer />);
  const foreground = screen.getByRole("button", { name: "Newer action" });
  await user.click(foreground);

  for (const key of [" ", "Enter", "ArrowRight", "ArrowLeft", "ArrowDown", "/"]) {
    fireEvent.keyDown(row, { key });
  }
  expect(spies.toggle).not.toHaveBeenCalled();
  expect(spies.peek).not.toHaveBeenCalled();
  expect(spies.loadMore).not.toHaveBeenCalled();
  expect(spies.previous).not.toHaveBeenCalled();
  expect(foreground).toHaveFocus();

  view.rerender(<Harness layout="wide" />);
  act(() => row.focus());
  await user.keyboard(" {ArrowRight}");
  expect(spies.toggle).toHaveBeenCalledOnce();
  expect(spies.loadMore).toHaveBeenCalledOnce();
});

test("wide preview navigation does not change list selection or paging", async () => {
  const user = userEvent.setup();
  render(<Harness layout="wide" />);
  const preview = document.querySelector(".session-import__preview-pane") as HTMLElement;
  act(() => preview.focus());
  await user.keyboard(" {ArrowRight}{ArrowLeft}{ArrowDown}{Home}{End}/");
  expect(spies.toggle).not.toHaveBeenCalled();
  expect(spies.loadMore).not.toHaveBeenCalled();
  expect(spies.previous).not.toHaveBeenCalled();
  expect(preview).toHaveFocus();
});

test("nested editable content owns its typing and arrow keys", () => {
  render(<Harness layout="wide" />);
  const row = document.querySelector("[data-import-candidate]") as HTMLElement;
  const editor = document.createElement("div");
  editor.setAttribute("contenteditable", "true");
  const nested = document.createElement("span");
  editor.append(nested);
  row.append(editor);
  for (const key of [" ", "Enter", "ArrowRight", "ArrowLeft", "ArrowDown", "/"]) {
    fireEvent.keyDown(nested, { key });
  }
  expect(spies.toggle).not.toHaveBeenCalled();
  expect(spies.peek).not.toHaveBeenCalled();
  expect(spies.loadMore).not.toHaveBeenCalled();
  expect(spies.previous).not.toHaveBeenCalled();
});

test("a removed focused row restores the filter when the list stays visible", async () => {
  const view = render(<Harness layout="wide" />);
  const row = document.querySelector("[data-import-candidate]") as HTMLElement;
  act(() => row.focus());
  view.rerender(<Harness layout="wide" candidates={[]} />);
  expect(document.activeElement).toBe(screen.getByRole("searchbox"));
});
