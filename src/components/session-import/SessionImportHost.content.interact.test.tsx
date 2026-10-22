/** @vitest-environment jsdom */
import { act, cleanup, fireEvent, render, renderHook, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { SourceCandidate } from "@/lib/api/sessionMigration";
import { useTypeToFocusComposer } from "@/hooks/useTypeToFocusComposer";
import { installDialogFocus } from "@/lib/a11yFocus";

const mocks = vi.hoisted(() => {
  const candidate: SourceCandidate = {
    candidateId: "cand-1",
    provider: "generic-file",
    nativeSessionId: "n1",
    title: "One",
    locator: "chat.json",
    sourceRevision: "rev-1",
    completeness: "complete",
  };
  const candidate2: SourceCandidate = {
    candidateId: "cand-2",
    provider: "generic-file",
    nativeSessionId: "n2",
    title: "Two",
    locator: "chat-2.json",
    sourceRevision: "rev-2",
    completeness: "complete",
  };
  return {
    candidate,
    candidate2,
    candidates: [candidate, candidate2],
    extraCandidates: [] as SourceCandidate[],
    selected: new Set(["cand-1"]),
    scanGeneration: { current: 1 },
    preview: vi.fn(),
    previewSelected: vi.fn(),
    events: vi.fn(),
    loadMoreScan: vi.fn(),
    previousScanPage: vi.fn(),
    toggle: vi.fn(),
    commit: vi.fn(),
    setStep: vi.fn(),
    writeText: vi.fn().mockResolvedValue(undefined),
    scrollIntoView: vi.fn(),
    busy: false,
    error: null as string | null,
    nextCursor: "cursor-2" as string | null,
    pageIndex: 0,
  };
});

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (err: unknown) => void;
  const promise = new Promise<T>((ok, fail) => {
    resolve = ok;
    reject = fail;
  });
  return { promise, resolve, reject };
}

vi.mock("@/i18n", () => ({
  createT: () => (key: string) => key,
  intlLocale: () => "en-US",
}));

vi.mock("@/components/GlassModal", () => ({
  GlassModal: ({
    open,
    children,
    title,
    footer,
  }: {
    open: boolean;
    children: React.ReactNode;
    title: React.ReactNode;
    footer?: React.ReactNode;
  }) => (open ? <div role="dialog">{title}<div className="modal-body">{children}</div>{footer ? <div className="modal-actions">{footer}</div> : null}</div> : null),
}));

vi.mock("@/components/Select", () => ({
  Select: ({ "aria-label": label, value, options, onChange }: {
    "aria-label"?: string;
    value?: string;
    options?: { value: string }[];
    onChange?: (value: string) => void;
  }) => (
    <button type="button" role="combobox" aria-label={label ?? "combo"} onKeyDown={(event) => {
      if (event.key !== "ArrowDown" || !options?.length) return;
      const next = options[(options.findIndex((option) => option.value === value) + 1) % options.length];
      onChange?.(next.value);
    }}>
      combo
    </button>
  ),
}));

vi.mock("@/hooks/useImportedCatalog", () => ({
  useImportedCatalog: () => ({ entries: [] }),
}));

vi.mock("@/hooks/useImportedReader", () => ({
  useImportedReader: () => ({
    events: [],
    hits: [],
    error: null,
    busy: false,
    query: "",
    setQuery: vi.fn(),
    search: vi.fn(),
    ensureEvent: vi.fn(),
    notice: null,
  }),
}));

vi.mock("@/lib/api/sessionMigration", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/api/sessionMigration")>();
  return {
    ...actual,
    sessionImportPreview: mocks.preview,
    sessionImportPreviewEvents: mocks.events,
    sessionImportExportMarkdown: vi.fn().mockResolvedValue("# md"),
  };
});

vi.mock("@/hooks/useSessionImport", () => ({
  useSessionImportUi: () => ({
    open: true,
    setOpen: vi.fn(),
    step: "content" as const,
    setStep: mocks.setStep,
    error: mocks.error,
    busy: mocks.busy,
    scanState: { state: "completed", extraCandidates: mocks.extraCandidates },
    candidates: mocks.candidates,
    extraCount: 0,
    scanIssues: [],
    discoveredCount: 1,
    selected: mocks.selected,
    selectedCount: mocks.selected.size,
    issueCount: 0,
    scanning: false,
    scanCancelling: false,
    scanPaused: false,
    scanFailed: false,
    scanGeneration: mocks.scanGeneration,
    cancelScan: vi.fn(),
    continueScan: vi.fn(),
    selectCurrent: vi.fn(),
    clearSelected: vi.fn(),
    selectThese: vi.fn(),
    invertThese: vi.fn(),
    refreshScan: vi.fn(),
    toggle: mocks.toggle,
    pageIndex: mocks.pageIndex,
    nextCursor: mocks.nextCursor,
    loadMore: mocks.loadMoreScan,
    previousPage: mocks.previousScanPage,
    previewSelected: mocks.previewSelected,
    prepareBatch: null,
    prepareItems: [],
    prepareNextCursor: null,
    prepareActive: false,
    cancelPrepare: vi.fn(),
    retryPrepare: vi.fn(),
    loadMorePrepare: vi.fn(),
    finishPrepare: vi.fn(),
    abandonPrepare: vi.fn(),
    mappings: [],
    setMappingProject: vi.fn(),
    projectId: null,
    projects: [],
    commit: mocks.commit,
    setDraft: vi.fn(),
    job: null,
    readerId: null,
    readerRevision: null,
    readerLeaf: null,
    setReaderId: vi.fn(),
    createContinue: vi.fn(),
    draft: null,
    roots: [],
    registrations: [],
    choice: null,
    setChoice: vi.fn(),
    pickCustomRoot: vi.fn(),
    pickCustomFile: vi.fn(),
    relocateRegistered: vi.fn(),
    scan: vi.fn(),
  }),
}));

import { SessionImportHost } from "./SessionImportHost";
const originalScrollIntoView = Object.getOwnPropertyDescriptor(HTMLElement.prototype, "scrollIntoView");

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
  vi.unstubAllGlobals();
  mocks.busy = false;
  mocks.error = null;
  mocks.selected = new Set(["cand-1"]);
  mocks.nextCursor = "cursor-2";
  mocks.pageIndex = 0;
  mocks.candidates = [mocks.candidate, mocks.candidate2];
  mocks.extraCandidates = [];
  if (originalScrollIntoView) {
    Object.defineProperty(HTMLElement.prototype, "scrollIntoView", originalScrollIntoView);
  } else {
    Reflect.deleteProperty(HTMLElement.prototype, "scrollIntoView");
  }
});

beforeEach(() => {
  Object.defineProperty(HTMLElement.prototype, "scrollIntoView", {
    configurable: true,
    value: mocks.scrollIntoView,
  });
  Object.defineProperty(navigator, "clipboard", {
    configurable: true,
    value: { writeText: mocks.writeText },
  });
  mocks.preview.mockImplementation(async (id: string) => ({
    previewId: `preview-${id}`,
    adapter: {
      outcome: "complete",
      activeLeaf: `leaf-${id}`,
      graph: { events: [{}, {}] },
      inventory: { imported: { messages: 2, toolCalls: 0, attachments: 0 } },
    },
  }));
  mocks.events.mockImplementation(
    async (previewId: string, cursor: string | null, _limit: number, leaf?: string | null) => ({
      events: [
        {
          eventId: `${previewId}:${leaf ?? "default"}:${cursor ?? "0"}`,
          kind: "message",
          role: "user",
          text: `body-${previewId}`,
          selectable: true,
          defaultSelected: true,
        },
      ],
      nextCursor: cursor ? null : "c1",
    }),
  );
});

function renderHost(container?: HTMLElement) {
  return render(
    <SessionImportHost locale="en" />,
    container ? { container } : undefined,
  );
}

function candidateRows() {
  return Array.from(document.querySelectorAll<HTMLLIElement>(".session-import__list > li"));
}

it("keeps the preview summary on the confirmed alternate branch through preparation", async () => {
  const user = userEvent.setup();
  mocks.preview.mockImplementation(async (id: string) => ({
    previewId: `preview-${id}`,
    adapter: {
      outcome: "complete", activeLeaf: "branch-b", leaves: ["branch-b", "branch-a"],
      eventCount: 2,
    },
  }));
  renderHost();
  await user.click(document.querySelector("[data-import-peek]") as HTMLButtonElement);
  const summary = () => document.querySelector(".session-import__peek > .settings-row__desc")!;
  expect(summary().textContent).toContain("branch-b");
  const picker = screen.getByRole("combobox", { name: "sessionImport.selectLeaf" });
  act(() => picker.focus());
  await user.keyboard("{ArrowDown}");
  expect(summary().textContent).toContain("branch-b");
  await user.click(screen.getByRole("button", { name: "sessionImport.applySwitch" }));
  expect(summary().textContent).toContain("branch-a");
  expect(summary().textContent).not.toContain("branch-b");
  await user.click(screen.getByRole("button", { name: "sessionImport.next" }));
  expect(mocks.previewSelected).toHaveBeenCalledWith([expect.objectContaining({ leafId: "branch-a" })]);
});

it("places candidates and the preview in separate named regions with a dedicated footer", async () => {
  const user = userEvent.setup();
  mocks.selected = new Set();
  renderHost();
  const list = screen.getByRole("region", { name: "sessionImport.step.content" });
  const preview = screen.getByRole("region", { name: "sessionImport.preview" });
  expect(list.querySelectorAll("[data-import-candidate]")).toHaveLength(2);
  expect(preview.textContent).toContain("sessionImport.previewHint");
  expect(screen.getByText("sessionImport.next").closest(".session-import__footer")).not.toBeNull();
  expect(screen.getByText("sessionImport.next").closest(".modal-body")).toBeNull();
  expect(screen.getByText("sessionImport.next").closest(".modal-actions")).not.toBeNull();
  expect(list.closest(".modal-body")).not.toBeNull();
  await user.click(candidateRows()[1].querySelector<HTMLButtonElement>("[data-import-peek]")!);
  expect(await screen.findByText("body-preview-cand-2")).toBeTruthy();
  expect(preview.querySelector(".session-import__detail")).not.toBeNull();
  expect(screen.queryByText("sessionImport.previewHint")).toBeNull();
  expect(mocks.toggle).not.toHaveBeenCalled();
});

it("keeps footer Back and Next keyboard actions independent of candidate shortcuts", async () => {
  const user = userEvent.setup();
  renderHost();
  await user.click(candidateRows()[0].querySelector<HTMLButtonElement>("[data-import-peek]")!);
  expect(await screen.findByText("body-preview-cand-1")).toBeTruthy();
  const back = screen.getByRole("button", { name: "sessionImport.back" });
  const next = screen.getByRole("button", { name: "sessionImport.next" });
  act(() => back.focus());
  await user.keyboard("{Enter}");
  expect(mocks.setStep).toHaveBeenCalledWith("source");
  await user.tab();
  expect(document.activeElement).toBe(next);
  await user.keyboard(" ");
  expect(mocks.previewSelected).toHaveBeenCalledTimes(1);
  expect(mocks.previewSelected).toHaveBeenCalledWith([expect.objectContaining({
    candidateId: "cand-1", previewId: "preview-cand-1", leafId: "leaf-cand-1",
    selection: expect.any(Object),
  })]);
  expect(mocks.toggle).not.toHaveBeenCalled();
  expect(mocks.loadMoreScan).not.toHaveBeenCalled();
  expect(mocks.commit).not.toHaveBeenCalled();
});

it("disables footer Next for empty selection and busy work without duplicating the footer", async () => {
  const user = userEvent.setup();
  mocks.selected = new Set();
  const view = renderHost();
  let next = screen.getByRole("button", { name: "sessionImport.next" });
  expect((next as HTMLButtonElement).disabled).toBe(true);
  await user.click(next);
  expect(mocks.previewSelected).not.toHaveBeenCalled();
  mocks.selected = new Set(["cand-1"]);
  mocks.busy = true;
  view.rerender(<SessionImportHost locale="en" />);
  next = screen.getByRole("button", { name: "sessionImport.next" });
  expect((next as HTMLButtonElement).disabled).toBe(true);
  await user.click(next);
  expect(mocks.previewSelected).not.toHaveBeenCalled();
  mocks.busy = false;
  view.rerender(<SessionImportHost locale="en" />);
  expect((screen.getByRole("button", { name: "sessionImport.next" }) as HTMLButtonElement).disabled).toBe(false);
  expect(document.querySelectorAll(".session-import__footer")).toHaveLength(1);
  expect(document.querySelector("[data-import-content] .session-import__footer")).toBeNull();
});

it("opens content options with the keyboard without invoking candidate or paging shortcuts", async () => {
  const user = userEvent.setup();
  mocks.selected = new Set();
  renderHost();
  await user.click(candidateRows()[1].querySelector<HTMLButtonElement>("[data-import-peek]")!);
  expect(await screen.findByText("body-preview-cand-2")).toBeTruthy();
  const summary = screen.getByRole("button", { name: "sessionImport.contentOptions" });
  expect(summary.getAttribute("aria-expanded")).toBe("false");
  act(() => summary.focus());
  await user.keyboard("{Enter}");
  expect(summary.getAttribute("aria-expanded")).toBe("true");
  expect(document.activeElement).toBe(summary);
  expect(mocks.preview).toHaveBeenCalledTimes(1);
  expect(mocks.toggle).not.toHaveBeenCalled();
  expect(mocks.loadMoreScan).not.toHaveBeenCalled();
  await user.click(screen.getByLabelText("sessionImport.includeReasoning"));
  expect((screen.getByLabelText("sessionImport.includeReasoning") as HTMLInputElement).checked).toBe(true);
  await user.click(summary);
  await user.click(summary);
  expect((screen.getByLabelText("sessionImport.includeReasoning") as HTMLInputElement).checked).toBe(true);
});

it("shows an empty-filter state when checkboxes alone exclude every session", async () => {
  const user = userEvent.setup();
  mocks.selected = new Set();
  renderHost();
  await user.click(screen.getByLabelText("sessionImport.onlyArchived"));
  expect(candidateRows()).toHaveLength(0);
  expect(screen.getByText("sessionImport.filterEmpty")).toBeTruthy();
  await user.click(screen.getByLabelText("sessionImport.onlyArchived"));
  expect(candidateRows()).toHaveLength(2);
});

it("keeps candidate shortcuts in the wizard when the global composer redirect is enabled", async () => {
  const user = userEvent.setup();
  const composer = render(<div contentEditable suppressContentEditableWarning>Existing draft</div>)
    .container.firstElementChild as HTMLDivElement;
  const composerFocus = vi.spyOn(composer, "focus");
  renderHook(() => useTypeToFocusComposer({
    getEditor: () => composer,
    // The import Host owns its modal independently of AppWorkbench's overlay list.
    getLive: () => ({ enabled: true, overlayOpen: false }),
  }));
  mocks.selected = new Set();
  renderHost();
  const [, second] = candidateRows();
  act(() => second.focus());
  await user.keyboard(" ");
  expect(mocks.toggle).toHaveBeenCalledExactlyOnceWith("cand-2");
  expect(document.activeElement).toBe(second);
  await user.keyboard("{Enter}");
  expect(mocks.preview).toHaveBeenCalledExactlyOnceWith("cand-2", "rev-2");
  for (const event of [
    { key: "q" }, { key: "Process", keyCode: 229 }, { key: "x", isComposing: true },
    { key: "v", ctrlKey: true }, { key: "v", metaKey: true },
  ]) {
    fireEvent.keyDown(second, event);
    expect(document.activeElement).toBe(second);
  }
  await user.keyboard("/");
  expect(document.activeElement).toBe(screen.getByRole("searchbox"));
  expect(composerFocus).not.toHaveBeenCalled();
  expect(composer.textContent).toBe("Existing draft");
});

it("tabs into the candidate list and moves actual focus before selecting or previewing", async () => {
  const user = userEvent.setup();
  mocks.selected = new Set();
  renderHost();
  const [first, second] = candidateRows();
  screen.getByLabelText("sessionImport.onlyArchived").focus();
  await user.tab();
  expect(document.activeElement).toBe(first);
  await user.keyboard("{ArrowDown}");
  expect(document.activeElement).toBe(second);
  expect(mocks.scrollIntoView).toHaveBeenLastCalledWith({ block: "nearest" });
  expect(first.tabIndex).toBe(-1);
  expect(second.tabIndex).toBe(0);
  await user.keyboard(" ");
  expect(mocks.toggle).toHaveBeenCalledExactlyOnceWith("cand-2");
  await user.keyboard("{Enter}");
  expect(mocks.preview).toHaveBeenCalledExactlyOnceWith("cand-2", "rev-2");
  await user.keyboard("{Home}");
  expect(document.activeElement).toBe(first);
  await user.keyboard("{ArrowUp}");
  expect(document.activeElement).toBe(first);
  await user.keyboard("{End}{ArrowDown}");
  expect(document.activeElement).toBe(second);
});

it("keeps nested controls and text entry in charge of their own keys", async () => {
  const user = userEvent.setup();
  mocks.selected = new Set();
  renderHost();
  const [first, second] = candidateRows();
  act(() => first.focus());
  await user.tab();
  const checkbox = first.querySelector("input");
  expect(document.activeElement).toBe(checkbox);
  await user.keyboard(" ");
  expect(mocks.toggle).toHaveBeenCalledExactlyOnceWith("cand-1");
  await user.tab({ shift: true });
  expect(document.activeElement).toBe(first);
  const preview = second.querySelector<HTMLButtonElement>("[data-import-peek]")!;
  act(() => preview.focus());
  await user.keyboard("{ArrowUp}{Enter}");
  expect(document.activeElement).toBe(preview);
  expect(mocks.preview).toHaveBeenCalledExactlyOnceWith("cand-2", "rev-2");
  const filter = screen.getByRole("searchbox");
  filter.focus();
  await user.keyboard("two{ArrowLeft}{Enter}");
  expect(document.activeElement).toBe(filter);
  expect(mocks.loadMoreScan).not.toHaveBeenCalled();
  expect(mocks.preview).toHaveBeenCalledTimes(1);
});

it("ignores candidate actions and navigation during IME, modified shortcuts, or busy work", async () => {
  mocks.selected = new Set();
  const view = renderHost();
  const [first] = candidateRows();
  act(() => first.focus());
  for (const key of ["ArrowDown", "End", " ", "Enter", "ArrowRight", "/"]) {
    fireEvent.keyDown(first, { key, isComposing: true });
    fireEvent.keyDown(first, { key, keyCode: 229 });
    fireEvent.keyDown(first, { key, ctrlKey: true });
    fireEvent.keyDown(first, { key, altKey: true });
    fireEvent.keyDown(first, { key, metaKey: true });
  }
  expect(document.activeElement).toBe(first);
  expect(first.dataset.focused).toBe("1");
  expect(mocks.toggle).not.toHaveBeenCalled();
  expect(mocks.preview).not.toHaveBeenCalled();
  expect(mocks.loadMoreScan).not.toHaveBeenCalled();
  mocks.busy = true;
  view.rerender(<SessionImportHost locale="en" />);
  for (const key of ["ArrowDown", "End", " ", "Enter", "ArrowRight"]) {
    fireEvent.keyDown(first, { key });
  }
  expect(document.activeElement).toBe(first);
  expect(first.dataset.focused).toBe("1");
  expect(mocks.toggle).not.toHaveBeenCalled();
  expect(mocks.preview).not.toHaveBeenCalled();
  expect(mocks.loadMoreScan).not.toHaveBeenCalled();
});

it("navigates new discoveries and preserves the active candidate when the list is reordered", async () => {
  const user = userEvent.setup();
  mocks.selected = new Set();
  mocks.candidates = [mocks.candidate];
  mocks.extraCandidates = [mocks.candidate2];
  const view = renderHost();
  const [first, extra] = candidateRows();
  act(() => first.focus());
  await user.keyboard("{ArrowDown} ");
  expect(document.activeElement).toBe(extra);
  expect(mocks.toggle).toHaveBeenCalledExactlyOnceWith("cand-2");
  mocks.candidates = [mocks.candidate2, mocks.candidate];
  mocks.extraCandidates = [];
  view.rerender(<SessionImportHost locale="en" />);
  const [moved, last] = candidateRows();
  expect(moved.tabIndex).toBe(0);
  expect(last.tabIndex).toBe(-1);
  expect(document.activeElement).toBe(moved);
  await user.keyboard("{ArrowDown}");
  expect(document.activeElement).toBe(last);
});

it("keeps keyboard paging inside the candidate list when the active row is replaced", async () => {
  const user = userEvent.setup();
  mocks.selected = new Set();
  const view = renderHost();
  const [, second] = candidateRows();
  act(() => second.focus());
  await user.keyboard("{ArrowRight}");
  expect(mocks.loadMoreScan).toHaveBeenCalledTimes(1);
  mocks.candidates = [{ ...mocks.candidate, candidateId: "page-two", title: "New page" }];
  view.rerender(<SessionImportHost locale="en" />);
  const [next] = candidateRows();
  expect(document.activeElement).toBe(next);
  expect(next.tabIndex).toBe(0);
  await user.keyboard(" ");
  expect(mocks.toggle).toHaveBeenCalledExactlyOnceWith("page-two");
});

it("returns focus to the filter if a focused candidate disappears without a replacement", () => {
  mocks.selected = new Set();
  const view = renderHost();
  const [first] = candidateRows();
  act(() => first.focus());
  mocks.candidates = [];
  view.rerender(<SessionImportHost locale="en" />);
  expect(document.activeElement).toBe(screen.getByRole("searchbox"));
});

function pageCandidates(first: number, count: number): SourceCandidate[] {
  return Array.from({ length: count }, (_, index) => ({
    ...mocks.candidate,
    candidateId: `page-item-${first + index}`,
    title: `Synthetic ${first + index}`,
  }));
}

it("keeps keyboard pagination reachable through 50/50/20 pages and back to the first page", async () => {
  const user = userEvent.setup();
  mocks.selected = new Set();
  mocks.candidates = pageCandidates(1, 50);
  const view = renderHost();
  const next = screen.getByRole("button", { name: "sessionImport.nextPage" });
  act(() => next.focus());
  await user.keyboard("{Enter}");
  expect(mocks.loadMoreScan).toHaveBeenCalledTimes(1);
  mocks.pageIndex = 1;
  mocks.candidates = pageCandidates(51, 50);
  view.rerender(<SessionImportHost locale="en" />);
  expect(candidateRows()).toHaveLength(50);
  expect(candidateRows()[0].textContent).toContain("Synthetic 51");
  expect(document.activeElement).toBe(next);
  await user.keyboard(" ");
  expect(mocks.loadMoreScan).toHaveBeenCalledTimes(2);
  mocks.pageIndex = 2;
  mocks.nextCursor = null;
  mocks.candidates = pageCandidates(101, 20);
  view.rerender(<SessionImportHost locale="en" />);
  expect(candidateRows()).toHaveLength(20);
  expect(candidateRows()[19].textContent).toContain("Synthetic 120");
  const previous = screen.getByRole("button", { name: "sessionImport.previousPage" });
  expect(document.activeElement).toBe(previous);
  await user.keyboard("{Enter}");
  expect(mocks.previousScanPage).toHaveBeenCalledTimes(1);
  mocks.pageIndex = 1;
  mocks.nextCursor = "last-page";
  mocks.candidates = pageCandidates(51, 50);
  view.rerender(<SessionImportHost locale="en" />);
  expect(document.activeElement).toBe(previous);
  await user.keyboard(" ");
  expect(mocks.previousScanPage).toHaveBeenCalledTimes(2);
  mocks.pageIndex = 0;
  mocks.candidates = pageCandidates(1, 50);
  view.rerender(<SessionImportHost locale="en" />);
  expect(document.activeElement).toBe(screen.getByRole("button", { name: "sessionImport.nextPage" }));
  expect(mocks.toggle).not.toHaveBeenCalled();
  expect(mocks.commit).not.toHaveBeenCalled();
});

it("returns first-page boundary focus to Next when Previous is removed", async () => {
  mocks.pageIndex = 1;
  const user = userEvent.setup();
  const view = renderHost();
  act(() => screen.getByRole("button", { name: "sessionImport.previousPage" }).focus());
  await user.keyboard("{Enter}");
  mocks.pageIndex = 0;
  view.rerender(<SessionImportHost locale="en" />);
  expect(mocks.previousScanPage).toHaveBeenCalledOnce();
  expect(document.activeElement).toBe(screen.getByRole("button", { name: "sessionImport.nextPage" }));
});

it("returns removed pagination focus to the filter when neither direction remains", () => {
  const view = renderHost();
  act(() => screen.getByRole("button", { name: "sessionImport.nextPage" }).focus());
  mocks.nextCursor = null;
  mocks.candidates = [];
  view.rerender(<SessionImportHost locale="en" />);
  expect(document.activeElement).toBe(screen.getByRole("searchbox"));
});

it("recovers pagination focus after the busy control loses focus, not while work is pending", () => {
  const view = renderHost();
  const next = screen.getByRole("button", { name: "sessionImport.nextPage" });
  act(() => next.focus());
  mocks.busy = true;
  view.rerender(<SessionImportHost locale="en" />);
  // jsdom does not blur a button once it is disabled. Model the browser's
  // focus loss explicitly without treating a disabled .blur() as evidence.
  act(() => {
    document.body.setAttribute("tabindex", "-1");
    document.body.focus();
    document.body.removeAttribute("tabindex");
  });
  expect(document.activeElement).toBe(document.body);
  view.rerender(<SessionImportHost locale="en" />);
  expect(document.activeElement).toBe(document.body);
  mocks.busy = false;
  view.rerender(<SessionImportHost locale="en" />);
  expect(document.activeElement).toBe(next);
});

it("does not restore pagination focus after a deliberate blur or a move to filtering", () => {
  const view = renderHost();
  const next = screen.getByRole("button", { name: "sessionImport.nextPage" });
  act(() => next.focus());
  act(() => next.blur());
  mocks.nextCursor = null;
  mocks.pageIndex = 1;
  view.rerender(<SessionImportHost locale="en" />);
  expect(document.activeElement).toBe(document.body);
  const previous = screen.getByRole("button", { name: "sessionImport.previousPage" });
  act(() => previous.focus());
  const filter = screen.getByRole("searchbox");
  act(() => filter.focus());
  mocks.pageIndex = 0;
  view.rerender(<SessionImportHost locale="en" />);
  expect(document.activeElement).toBe(filter);
});

it.each(["candidate", "pagination"] as const)("does not recover removed %s focus below a newer dialog", (kind) => {
  const view = renderHost();
  const control = kind === "candidate" ? candidateRows()[0]
    : screen.getByRole("button", { name: "sessionImport.nextPage" });
  act(() => control.focus());
  const overlay = document.createElement("div");
  overlay.setAttribute("role", "dialog");
  const search = document.createElement("input");
  overlay.append(search);
  document.body.append(overlay);
  // Install the real production focus stack while the original control still
  // owns focus; a newly mounted portal can briefly have no focused child.
  const release = installDialogFocus(() => overlay, { initialFocus: "none", restoreFocus: false });
  try {
    mocks.candidates = pageCandidates(51, 50);
    mocks.pageIndex = 1;
    mocks.nextCursor = null;
    view.rerender(<SessionImportHost locale="en" />);
    expect(document.activeElement).toBe(document.body);
    act(() => search.focus());
    view.rerender(<SessionImportHost locale="en" />);
    expect(document.activeElement).toBe(search);
  } finally {
    release();
    overlay.remove();
  }
});

it("does not restore a removed candidate after focus has deliberately left it", () => {
  mocks.selected = new Set();
  const view = renderHost();
  const [first] = candidateRows();
  act(() => first.focus());
  act(() => first.blur());
  mocks.candidates = [{ ...mocks.candidate, candidateId: "page-two", title: "New page" }];
  view.rerender(<SessionImportHost locale="en" />);
  expect(document.activeElement).toBe(document.body);
});

it("keeps filtering focused and leaves a reachable candidate after filtering or page replacement", async () => {
  const user = userEvent.setup();
  mocks.selected = new Set();
  const view = renderHost();
  const [, second] = candidateRows();
  act(() => second.focus());
  await user.keyboard("/");
  const filter = screen.getByRole("searchbox");
  expect(document.activeElement).toBe(filter);
  await user.type(filter, "one");
  expect(document.activeElement).toBe(filter);
  expect(candidateRows()).toHaveLength(1);
  expect(candidateRows()[0].tabIndex).toBe(0);
  await user.type(filter, "missing");
  expect(candidateRows()).toHaveLength(0);
  expect(document.activeElement).toBe(filter);
  await user.clear(filter);
  mocks.candidates = [{ ...mocks.candidate, candidateId: "page-two", title: "New page" }];
  view.rerender(<SessionImportHost locale="en" />);
  expect(document.activeElement).toBe(filter);
  expect(candidateRows()[0].tabIndex).toBe(0);
  screen.getByLabelText("sessionImport.onlyArchived").focus();
  await user.tab();
  expect(document.activeElement).toBe(candidateRows()[0]);
});

it("ensureSelected plus peek while preview is busy invokes preview IPC once", async () => {
  const pending = deferred<{ previewId: string; adapter: unknown }>();
  mocks.preview.mockReset();
  mocks.preview.mockReturnValue(pending.promise);
  renderHost();
  const peek = document.querySelector("[data-import-peek]") as HTMLButtonElement;
  fireEvent.click(peek);
  fireEvent.click(peek);
  expect(mocks.preview).toHaveBeenCalledTimes(1);
  await act(async () => {
    pending.resolve({
      previewId: "preview-cand-1",
      adapter: { outcome: "complete", activeLeaf: "leaf-a", graph: { events: [{}] } },
    });
  });
});

it("peek load-more double click invokes events IPC once while page is busy", async () => {
  await act(async () => {
    renderHost();
  });
  fireEvent.click(document.querySelector("[data-import-peek]") as HTMLButtonElement);
  expect(await screen.findByText("sessionImport.loadMore")).toBeTruthy();
  const pending = deferred<{ events: unknown[]; nextCursor: string | null }>();
  mocks.events.mockClear();
  mocks.events.mockReturnValueOnce(pending.promise);
  const btn = screen.getByText("sessionImport.loadMore") as HTMLButtonElement;
  fireEvent.click(btn);
  fireEvent.click(btn);
  expect(mocks.events).toHaveBeenCalledTimes(1);
  await act(async () => {
    pending.resolve({
      events: [
        {
          eventId: "p2",
          kind: "message",
          role: "user",
          text: "p2",
          selectable: true,
          defaultSelected: true,
        },
      ],
      nextCursor: null,
    });
  });
});

it("keeps peek and content pane usable in a 320px wrapper", async () => {
  const wrap = document.createElement("div");
  wrap.style.width = "320px";
  wrap.style.maxWidth = "320px";
  document.body.appendChild(wrap);
  await act(async () => {
    renderHost(wrap);
  });
  fireEvent.click(document.querySelector("[data-import-peek]") as HTMLButtonElement);
  const pane = document.querySelector("[data-import-content]") as HTMLElement;
  expect(pane).not.toBeNull();
  expect(document.querySelector("[data-import-peek]")).not.toBeNull();
  expect(await screen.findByText("sessionImport.loadMore")).toBeTruthy();
  wrap.remove();
});
