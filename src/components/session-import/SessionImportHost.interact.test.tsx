/** @vitest-environment jsdom */
import "@testing-library/jest-dom/vitest";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mappingKeyboardCases } from "@/test/sessionImportMappingKeyboardCases";
import { emptyContentSelection } from "@/lib/sessionImport/contentSelection";
import { installDialogFocus } from "@/lib/a11yFocus";
import type { ImportedRevisionHealth, JobRecord, PrepareBatchItem, PrepareBatchStatus, SourceCandidate, WorkspaceMapping } from "@/lib/api/sessionMigration";
import type { PendingDurablePlan } from "@/lib/sessionImport/durablePlan";
import type { ImportCommitPhase } from "@/hooks/useSessionImport";

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
  const slot = {
    candidateId: "cand-1",
    previewId: "prev-1",
    snapshotId: "snap",
    sourceRevision: "rev-1",
    title: "One",
    summary: {
      leaf: "leaf-a",
      eventCount: 1,
      messages: 1,
      tools: 0,
      attachments: 0,
      missing: 0,
      issues: [] as { code: string; message: string }[],
      outcome: "complete",
    },
    selection: {
      previewId: "prev-1",
      leaf: "leaf-a",
      leaves: ["leaf-a"],
      events: [] as unknown[],
      nextCursor: null as string | null,
      selectedEventIds: [] as string[],
      pinnedEventIds: [] as string[],
      assetIds: [] as string[],
      fields: {
        messageText: true,
        toolCallName: true,
        toolCallInput: true,
        toolResultContent: true,
        compactionSummary: true,
        visibleReasoning: false,
      },
      includeSubagents: false,
      pendingLeaf: null as string | null,
      generation: 0,
      archiveCount: 0,
      modelCount: 0,
      autoSelectPageDefaults: true,
    },
    generation: 1,
    requestSeq: 1,
    dirty: false,
    busy: false,
    error: null as string | null,
  };
  return {
    candidate,
    candidate2,
    slot,
    loadMore: vi.fn(),
    previousPage: vi.fn(),
    toggle: vi.fn(),
    openPeek: vi.fn(),
    confirmLeaf: vi.fn(),
    commit: vi.fn(),
    setStep: vi.fn(),
    abandonPrepare: vi.fn(),
    writeText: vi.fn().mockResolvedValue(undefined),
    confirm: vi.fn(() => true),
    busy: false,
    restoreStatus: null as null | "plan" | "job" | "draft" | "expired" | "failed",
    error: "SOURCE_BUSY: locked" as string | null,
    continuationError: null as string | null,
    pendingPlan: null as PendingDurablePlan | null,
    commitPhase: null as ImportCommitPhase,
    prepareBatch: null as PrepareBatchStatus | null,
    prepareItems: [] as PrepareBatchItem[],
    mappings: [{ rootId: "r1", unassigned: true, userChoiceVersion: 1 }] as WorkspaceMapping[],
    step: "content" as "source" | "content" | "prepare" | "mapping" | "result",
    job: null as JobRecord | null,
    retryFailed: vi.fn(),
    jobHasPrevious: false,
    jobItemsStale: false,
    nextJobPage: vi.fn(),
    previousJobPage: vi.fn(),
    refreshJobPage: vi.fn(),
    nextCursor: "cursor-2" as string | null,
    selected: new Set(["cand-1"]),
    readerId: null as string | null,
    readerAnchor: null as string | null,
    readerScope: { revision: "viewed-revision", leaf: "viewed-leaf" } as { revision: string; leaf: string } | null,
    catalogEntries: [] as import("@/lib/api/sessionMigration").CatalogEntry[],
    readerOptions: vi.fn(),
    readerError: null as string | null,
    readerHealth: null as ImportedRevisionHealth | null,
    readerEvents: [] as unknown[],
    readerHits: [] as { eventId: string; snippet: string; kind?: string }[],
    readerQuery: "",
    readerBusy: false,
    readerNotice: null as string | null,
    readerSearchError: null as string | null,
    readerPreviousCursor: null as string | null,
    readerNextCursor: null as string | null,
    readerLoadPrevious: vi.fn().mockResolvedValue(null as string | null),
    readerLoadMore: vi.fn().mockResolvedValue(null as string | null),
    ensureEvent: vi.fn().mockResolvedValue(true),
    createContinue: vi.fn().mockResolvedValue("native-1"),
    prepareContinue: vi.fn().mockResolvedValue(undefined),
    onOpenNative: vi.fn(),
    workspaceGet: vi.fn(),
    workspaceUpdate: vi.fn(),
  };
});

vi.mock("@/i18n", () => ({
  createT: () => (key: string, vars?: Record<string, unknown>) => {
    if (key === "sessionImport.jobProgress") return `${vars?.done} / ${vars?.total} finished`;
    if (key === "sessionImport.prepare.counts") return `${vars?.prepared} ready · ${vars?.failed} failed · ${vars?.skipped} skipped`;
    if (key === "sessionImport.moreSelected") return `${vars?.n} more`;
    if (key === "sessionImport.mappingTarget") return `Project mapping for ${vars?.source}: ${vars?.target}`;
    return key;
  },
  intlLocale: () => "en-US",
}));

vi.mock("@/lib/api/sessionWorkspaceAssociation", () => ({
  importedWorkspaceGet: mocks.workspaceGet,
  importedWorkspaceOptions: vi.fn().mockResolvedValue({ items: [], nextCursor: null }),
  importedWorkspaceUpdate: mocks.workspaceUpdate,
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

vi.mock("@/lib/api/workspace", () => ({ workspacesList: vi.fn().mockResolvedValue([]) }));

vi.mock("@/components/Select", () => ({
  Select: ({ "aria-label": label, disabled }: { "aria-label"?: string; disabled?: boolean }) => (
    <button type="button" role="combobox" disabled={disabled} aria-label={label ?? "combo"}>
      combo
    </button>
  ),
}));

vi.mock("@/hooks/useImportedCatalog", () => ({
  useImportedCatalog: () => ({ entries: mocks.catalogEntries }),
}));

vi.mock("@/hooks/useImportedReader", () => ({
  useImportedReader: (_id: string | null, _onError: unknown, options: unknown) => {
    mocks.readerOptions(options);
    return ({
    scope: mocks.readerScope,
    getScope: () => mocks.readerScope,
    events: mocks.readerEvents,
    health: mocks.readerHealth,
    hits: mocks.readerHits,
    hitsWindowed: false,
    eventsWindowed: false,
    previousCursor: mocks.readerPreviousCursor,
    nextCursor: mocks.readerNextCursor,
    error: mocks.readerError,
    busy: mocks.readerBusy,
    query: mocks.readerQuery,
    searchError: mocks.readerSearchError,
    setQuery: vi.fn(),
    search: vi.fn(),
    ensureEvent: mocks.ensureEvent,
    loadPrevious: mocks.readerLoadPrevious,
    loadMore: mocks.readerLoadMore,
    notice: mocks.readerNotice,
  }); },
}));

vi.mock("@/hooks/useSessionImportContentStates", () => ({
  useSessionImportContentStates: () => ({
    byCandidate: { "cand-1": mocks.slot },
    activeCandidateId: "cand-1",
    active: mocks.slot,
    setActiveCandidateId: vi.fn(),
    ensureSelected: vi.fn(),
    open: mocks.openPeek,
    loadMore: vi.fn(),
    confirmLeaf: mocks.confirmLeaf,
    dispatchSelection: vi.fn(),
    clearError: vi.fn(),
    closeWizard: vi.fn(),
    allowedSelectionOf: () => ({ version: 2, eventIds: ["e1"] }),
  }),
}));

vi.mock("@/hooks/useSessionImport", () => ({
  useSessionImportUi: () => ({
    open: true,
    setOpen: vi.fn(),
    step: mocks.step,
    setStep: mocks.setStep,
    error: mocks.error,
    continuationError: mocks.continuationError,
    restoreStatus: mocks.restoreStatus,
    pendingPlan: mocks.pendingPlan,
    commitPhase: mocks.commitPhase,
    associations: { confirmed: [], dialog: null, targets: null, busy: false, error: null,
      open: vi.fn(), close: vi.fn(), choose: vi.fn(), markReady: vi.fn(), confirm: vi.fn(), clear: vi.fn(),
      snapshot: () => [], isReviewing: () => false, nextTargets: vi.fn(), firstTargets: vi.fn() },
    busy: mocks.busy,
    canOpenJobHistory: !mocks.busy && mocks.step === "source",
    openExistingJob: vi.fn(),
    scanState: { state: "completed", extraCandidates: [] },
    candidates: [mocks.candidate, mocks.candidate2],
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
    cancelScan: vi.fn(),
    continueScan: vi.fn(),
    selectCurrent: vi.fn(),
    clearSelected: vi.fn(),
    selectThese: vi.fn(),
    invertThese: vi.fn(),
    refreshScan: vi.fn(),
    toggle: mocks.toggle,
    pageIndex: 0,
    nextCursor: mocks.nextCursor,
    loadMore: mocks.loadMore,
    previousPage: mocks.previousPage,
    previewSelected: vi.fn(),
    prepareBatch: mocks.prepareBatch,
    prepareItems: mocks.prepareItems,
    prepareNextCursor: null,
    prepareActive: false,
    cancelPrepare: vi.fn(),
    retryPrepare: vi.fn(),
    loadMorePrepare: vi.fn(),
    finishPrepare: vi.fn(),
    abandonPrepare: mocks.abandonPrepare,
    mappings: mocks.mappings,
    setMappingProject: vi.fn(),
    projectId: null,
    projects: [],
    commit: mocks.commit,
    setDraft: vi.fn(),
    job: mocks.job,
    jobActive: false,
    jobHasPrevious: mocks.jobHasPrevious,
    jobItemsStale: mocks.jobItemsStale,
    retryFailed: mocks.retryFailed,
    loadMoreJobItems: mocks.nextJobPage,
    previousJobItems: mocks.previousJobPage,
    refreshJobItems: mocks.refreshJobPage,
    readerId: mocks.readerId,
    readerRevision: null,
    readerLeaf: null,
    readerAnchor: mocks.readerAnchor,
    setReaderId: vi.fn(),
    createContinue: mocks.createContinue,
    prepareContinue: mocks.prepareContinue,
    draft: mocks.readerId
      ? { draftId: "d1", draftHash: "h1", text: "hi", importId: mocks.readerId }
      : null,
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
import { SessionImportContentDetail } from "./SessionImportContentDetail";

it("exposes import job history after restart without recovering a completed plan", () => {
  mocks.step = "source";
  mocks.error = null;
  render(<SessionImportHost locale="en" />);
  expect(screen.getByRole("button", { name: "sessionImport.jobs.title" })).toBeEnabled();
  expect(mocks.commit).not.toHaveBeenCalled();
});

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
  vi.unstubAllGlobals();
  mocks.busy = false;
  mocks.restoreStatus = null;
  mocks.error = "SOURCE_BUSY: locked";
  mocks.continuationError = null;
  mocks.pendingPlan = null;
  mocks.commitPhase = null;
  mocks.prepareBatch = null;
  mocks.prepareItems = [];
  mocks.mappings = [{ rootId: "r1", unassigned: true, userChoiceVersion: 1 }];
  mocks.step = "content";
  mocks.job = null;
  mocks.jobHasPrevious = false;
  mocks.jobItemsStale = false;
  mocks.nextCursor = "cursor-2";
  mocks.selected = new Set(["cand-1"]);
  mocks.readerId = null;
  mocks.readerAnchor = null;
  mocks.readerScope = { revision: "viewed-revision", leaf: "viewed-leaf" };
  mocks.catalogEntries = [];
  mocks.readerError = null;
  mocks.readerHealth = null;
  mocks.readerEvents = [];
  mocks.readerHits = [];
  mocks.readerQuery = "";
  mocks.readerBusy = false;
  mocks.readerNotice = null;
  mocks.readerSearchError = null;
  mocks.readerPreviousCursor = null;
  mocks.readerNextCursor = null;
  mocks.readerLoadPrevious.mockResolvedValue(null);
  mocks.readerLoadMore.mockResolvedValue(null);
});

beforeEach(() => {
  mocks.workspaceGet.mockImplementation(async (importId: string) => ({
    importId, version: 0, sourceId: "source", provider: "codex", sourceReady: true, choice: null, status: "unassigned",
  }));
  vi.stubGlobal("confirm", mocks.confirm);
  mocks.confirm.mockClear();
  Object.defineProperty(navigator, "clipboard", {
    configurable: true,
    value: { writeText: mocks.writeText },
  });
});

function renderHost() {
  return render(
    <SessionImportHost locale="en" onOpenNativeSession={mocks.onOpenNative} />,
  );
}

it("keeps reader actions outside the scrolling transcript", () => {
  mocks.readerId = "imp:footer";
  renderHost();
  const copy = screen.getByRole("button", { name: "sessionImport.copyVisible" });
  expect(copy.closest(".modal-actions")).not.toBeNull();
  expect(copy.closest(".modal-body")).toBeNull();
});

it("does not announce copied text until the clipboard accepts it", async () => {
  mocks.readerId = "imp:copy";
  mocks.readerEvents = [{ id: "event-copy", role: "user", text: "visible text" }];
  let resolve!: () => void;
  mocks.writeText.mockImplementationOnce(() => new Promise<void>((done) => { resolve = done; }));
  renderHost();
  fireEvent.click(screen.getByRole("button", { name: "sessionImport.copyVisible" }));
  try {
    expect(screen.queryByText("sessionImport.copiedExport")).not.toBeInTheDocument();
    expect(screen.queryByText("sessionImport.clipboardCopied")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "sessionImport.copyVisible" })).toBeDisabled();
  } finally {
    await act(async () => resolve());
  }
  expect(screen.getByText("sessionImport.clipboardCopied")).toBeInTheDocument();
});

const mappingBatch: PrepareBatchStatus = {
  version: 1, batchId: "batch", scanId: "scan", epoch: 1,
  selectionId: "selected", selectionStateVersion: 1,
  state: "completed-with-issues", stateVersion: 2,
  total: 2, prepared: 1, failed: 1, skipped: 0, active: 0, cancelRequested: false,
  createdAt: "2026-09-25T00:00:00Z", updatedAt: "2026-09-25T00:00:00Z",
};

it.each(["planning", "submitting"] as const)("shows accessible %s feedback while import startup owns the mapping controls", (phase) => {
  mocks.step = "mapping";
  mocks.busy = true;
  mocks.error = null;
  mocks.commitPhase = phase;
  const view = renderHost();
  const label = `sessionImport.start.${phase}`;
  expect(screen.getByRole("status")).toHaveTextContent(label);
  expect(screen.getByRole("status")).toHaveAttribute("aria-atomic", "true");
  const progress = screen.getByRole("progressbar", { name: label });
  expect(progress).not.toHaveAttribute("aria-valuenow");
  expect(progress.closest(".session-import__footer")).not.toBeNull();
  const publish = screen.getByRole("button", { name: "sessionImport.importSelected" });
  expect(publish).toBeDisabled();
  expect(screen.getByRole("button", { name: "sessionImport.back" })).toBeDisabled();
  fireEvent.click(publish);
  fireEvent.keyDown(document.querySelector('[data-import-mapping="1"]')!, { key: "Enter" });
  expect(mocks.commit).not.toHaveBeenCalled();
  mocks.commitPhase = null;
  mocks.busy = false;
  view.rerender(<SessionImportHost locale="en" />);
  expect(screen.queryByRole("progressbar")).not.toBeInTheDocument();
  expect(screen.queryByText(label)).not.toBeInTheDocument();
  expect(publish).toBeEnabled();
});

it("unrelated busy state does not imply that an import job is being created", () => {
  mocks.step = "mapping";
  mocks.busy = true;
  mocks.error = null;
  renderHost();
  expect(screen.queryByRole("progressbar")).not.toBeInTheDocument();
  expect(document.querySelector("[data-import-commit-phase]")).toBeNull();
});

it("mapping summarizes only prepared sessions, never failed original selections", () => {
  mocks.step = "mapping";
  mocks.selected = new Set(["cand-1", "cand-2"]);
  mocks.prepareBatch = mappingBatch;
  mocks.prepareItems = [
    { candidateId: "cand-1", title: "Prepared one", provider: "generic-file", sourceRevision: "rev-1", state: "prepared", attempt: 1 },
    { candidateId: "cand-2", title: "Rejected two", provider: "generic-file", sourceRevision: "rev-2", state: "failed", attempt: 1 },
  ];
  renderHost();
  const summary = document.querySelector(".session-import__selected-summary") as HTMLElement;
  expect(within(summary).getAllByRole("listitem")).toHaveLength(1);
  expect(summary).toHaveTextContent("Prepared one");
  expect(summary).not.toHaveTextContent("Rejected two");
  expect(summary).not.toHaveTextContent("Two");
  expect(screen.getByText("1 ready · 1 failed · 0 skipped")).toBeInTheDocument();
  expect(screen.getByText("sessionImport.prepare.onlyPrepared")).toBeInTheDocument();
  expect(mocks.selected.size).toBe(2);
  expect(mocks.toggle).not.toHaveBeenCalled();
});

it("mapping keeps global prepared count when its loaded page contains no ready titles", () => {
  mocks.step = "mapping";
  mocks.prepareBatch = { ...mappingBatch, total: 80, prepared: 70, failed: 10 };
  mocks.prepareItems = [{ candidateId: "cand-1", title: "Rejected", provider: "generic-file", sourceRevision: "rev-1", state: "failed", attempt: 1 }];
  renderHost();
  expect(screen.getByText("70 ready · 10 failed · 0 skipped")).toBeInTheDocument();
  expect(screen.getByText("70 more")).toBeInTheDocument();
  expect(document.querySelector(".session-import__selected-summary")?.textContent ?? "").not.toContain("One");
});

it("mapping bounds prepared title rendering and derives remaining count from the batch", () => {
  mocks.step = "mapping";
  mocks.prepareBatch = { ...mappingBatch, total: 80, prepared: 70, failed: 10 };
  mocks.prepareItems = Array.from({ length: 30 }, (_, index) => ({
    candidateId: `ready-${index}`, title: `Prepared ${index}`, provider: "generic-file", sourceRevision: "rev", state: "prepared", attempt: 1,
  }));
  renderHost();
  const summary = document.querySelector(".session-import__selected-summary") as HTMLElement;
  expect(within(summary).getAllByRole("listitem")).toHaveLength(20);
  expect(screen.getByText("50 more")).toBeInTheDocument();
  expect(summary).not.toHaveTextContent("Prepared 20");
});

it("places each result under its own accessible outcome group, including unfinished items", () => {
  mocks.step = "result";
  mocks.error = null;
  mocks.job = {
    jobId: "mixed", requestId: "request", planHash: "plan", state: "completed_with_issues", stateVersion: 8,
    createdAt: "2026-09-25T00:00:00Z", itemCount: 1000,
    progress: { done: 999, retryable: 2 },
    items: [
      { candidateId: "ok", title: "Complete history", phase: "committed", outcome: "complete" },
      { candidateId: "bad", title: "Broken history", phase: "failed", outcome: "failed" },
      { candidateId: "part", title: "Partial history", phase: "committed", outcome: "partial" },
      { candidateId: "wait", title: "Waiting history", phase: "queued" },
    ],
  };
  renderHost();
  for (const [group, title] of [["complete", "Complete history"], ["failed", "Broken history"], ["partial", "Partial history"], ["pending", "Waiting history"]]) {
    const section = screen.getByRole("region", { name: `sessionImport.outcomeGroup.${group}` });
    expect(within(section).getAllByRole("listitem")).toHaveLength(1);
    expect(within(section).getByText(title)).toBeVisible();
  }
  expect(screen.getByText("999 / 1000 finished")).toBeVisible();
});

it("offers retry for cancelled items beyond the first page and shows whole-job progress", async () => {
  mocks.step = "result";
  mocks.error = null;
  mocks.job = {
    jobId: "batch", requestId: "request", planHash: "plan", state: "cancelled", stateVersion: 1001,
    createdAt: "2026-09-20T00:00:00Z", itemCount: 1000, nextItemCursor: "page-2",
    items: Array.from({ length: 50 }, (_, n) => ({ candidateId: `c${n}`, phase: "committed", outcome: "complete" })),
    progress: { done: 1000, retryable: 800 },
  };
  const user = userEvent.setup();
  renderHost();
  expect(screen.getByText("1000 / 1000 finished")).toBeTruthy();
  await user.click(screen.getByRole("button", { name: "sessionImport.retry" }));
  expect(mocks.retryFailed).not.toHaveBeenCalled();
  await user.click(screen.getByRole("button", { name: "sessionImport.confirmRetry" }));
  expect(mocks.retryFailed).toHaveBeenCalledTimes(1);
});

it("job pager uses explicit navigation and holds row actions until a mutation page is refreshed", async () => {
  mocks.step = "result";
  mocks.error = null;
  mocks.jobHasPrevious = true;
  mocks.jobItemsStale = true;
  mocks.job = {
    jobId: "batch", requestId: "request", planHash: "plan", state: "cancelled", stateVersion: 12,
    createdAt: "2026-09-20T00:00:00Z", itemCount: 1000, nextItemCursor: "page-3",
    items: [{ candidateId: "later", phase: "committed", importId: "imp:kept" }],
    progress: { done: 1000, retryable: 800 },
  };
  const user = userEvent.setup();
  const rendered = renderHost();
  expect(screen.getByRole("button", { name: "sessionImport.openReader" })).toBeDisabled();
  expect(screen.getByRole("button", { name: "sessionImport.exportMarkdown" })).toBeDisabled();
  expect(screen.getByRole("button", { name: "sessionImport.continueInGrok" })).toBeDisabled();
  await user.click(screen.getByRole("button", { name: "sessionImport.previousPage" }));
  await user.click(screen.getByRole("button", { name: "sessionImport.nextPage" }));
  await user.click(screen.getByRole("button", { name: "sessionImport.refresh" }));
  expect(mocks.previousJobPage).toHaveBeenCalledOnce();
  expect(mocks.nextJobPage).toHaveBeenCalledOnce();
  expect(mocks.refreshJobPage).toHaveBeenCalledOnce();
  mocks.jobItemsStale = false;
  mocks.jobHasPrevious = false;
  mocks.job.nextItemCursor = null;
  rendered.rerender(<SessionImportHost locale="en" />);
  expect(screen.getByRole("button", { name: "sessionImport.openReader" })).not.toBeDisabled();
  expect(screen.getByRole("button", { name: "sessionImport.previousPage" })).toBeDisabled();
  expect(screen.getByRole("button", { name: "sessionImport.nextPage" })).toBeDisabled();
});

describe("SessionImportHost interaction", () => {
  it("starts previous-match navigation at the last hit before any selection", async () => {
    mocks.readerId = "imp:00000000-0000-4000-8000-000000000001";
    mocks.readerHits = ["e1", "e2", "e3"].map((eventId) => ({ eventId, snippet: eventId }));
    renderHost();
    fireEvent.click(screen.getByRole("button", { name: "sessionImport.prevHit" }));
    await vi.waitFor(() => expect(mocks.ensureEvent).toHaveBeenCalledWith("e3"));
    fireEvent.click(screen.getByRole("button", { name: "sessionImport.nextHit" }));
    await vi.waitFor(() => expect(mocks.ensureEvent).toHaveBeenLastCalledWith("e1"));
  });

  it.each([
    { key: "n", ctrlKey: true },
    { key: "p", metaKey: true },
    { key: "n", altKey: true },
    { key: "p", shiftKey: true },
    { key: "n", isComposing: true },
    { key: "n", keyCode: 229, isComposing: false },
    { key: "p", keyCode: 229, isComposing: false },
  ])("does not steal modified or composing reader shortcuts: %j", (key) => {
    mocks.readerId = "imp:00000000-0000-4000-8000-000000000001";
    mocks.readerHits = [{ eventId: "e1", snippet: "one" }];
    renderHost();
    const button = screen.getByRole("button", { name: "sessionImport.nextHit" });
    expect(fireEvent.keyDown(button, key)).toBe(true);
    expect(mocks.ensureEvent).not.toHaveBeenCalled();
  });

  it("keeps ordinary n/p navigation available without hijacking search typing", async () => {
    mocks.readerId = "imp:00000000-0000-4000-8000-000000000001";
    mocks.readerHits = ["e1", "e2"].map((eventId) => ({ eventId, snippet: eventId }));
    renderHost();
    const next = screen.getByRole("button", { name: "sessionImport.nextHit" });
    fireEvent.keyDown(next, { key: "n" });
    await vi.waitFor(() => expect(mocks.ensureEvent).toHaveBeenLastCalledWith("e1"));
    fireEvent.keyDown(next, { key: "p" });
    await vi.waitFor(() => expect(mocks.ensureEvent).toHaveBeenLastCalledWith("e2"));
    mocks.ensureEvent.mockClear();
    fireEvent.keyDown(screen.getByRole("searchbox", { name: "sessionImport.searchPlaceholder" }), { key: "n" });
    expect(mocks.ensureEvent).not.toHaveBeenCalled();
  });

  it("saving workspace membership leaves reader events and restored anchor in place", async () => {
    mocks.readerId = "imp:00000000-0000-4000-8000-000000000001";
    mocks.readerAnchor = "e5000";
    mocks.readerEvents = [{ id: "e5000", kind: "message", role: "user", text: "keep this event" }];
    mocks.workspaceUpdate.mockImplementation(async (input) => ({
      importId: input.importId, version: 1, sourceId: "source", provider: "codex", sourceReady: true, choice: null, status: "unassigned",
    }));
    Object.defineProperty(HTMLElement.prototype, "scrollIntoView", { configurable: true, value: vi.fn() });
    renderHost();
    await vi.waitFor(() => expect(mocks.ensureEvent).toHaveBeenCalledWith("e5000"));
    const anchorCalls = mocks.ensureEvent.mock.calls.length;
    const event = document.getElementById("import-ev-e5000");
    const control = document.querySelector("[data-import-workspace-association]") as HTMLElement;
    fireEvent.click(within(control).getByText("sessionImport.association.title", { selector: "summary" }));
    fireEvent.click(await within(control).findByRole("button", { name: "common.save" }));
    await within(control).findByText("sessionImport.association.saved");
    expect(document.getElementById("import-ev-e5000")).toBe(event);
    expect(event?.getAttribute("data-highlight")).toBe("1");
    expect(mocks.ensureEvent).toHaveBeenCalledTimes(anchorCalls);
    expect(mocks.readerLoadMore).not.toHaveBeenCalled();
    expect(mocks.readerLoadPrevious).not.toHaveBeenCalled();
  });
  it("shows source identity and distinct accessible controls for identical paths", () => {
    mocks.step = "mapping";
    mocks.mappings = ["a", "b"].map((id) => ({
      rootId: `source-${id}`, source: { sourceId: id, provider: "codex" },
      suggestedTarget: "/same/repo", unassigned: true, userChoiceVersion: 1,
    }));
    renderHost();
    for (const id of ["a", "b"]) {
      expect(screen.getByText(`sessionImport.workspace.sourceIdentity: codex · ${id}`)).toBeTruthy();
      expect(screen.getByRole("combobox", { name: `Project mapping for /same/repo · codex · ${id}: sessionImport.mapUnassigned` })).toBeTruthy();
    }
  });

  it("keeps a recovered plan reachable even without in-memory mapping rows", () => {
    mocks.pendingPlan = { planId: "persisted-plan", planHash: "persisted-hash" };
    mocks.mappings = [];
    renderHost();
    const mapping = screen.getByRole("button", { name: "sessionImport.step.mapping" });
    expect((mapping as HTMLButtonElement).disabled).toBe(false);
    fireEvent.click(mapping);
    expect(mocks.setStep).toHaveBeenCalledWith("mapping");
    fireEvent.click(screen.getByRole("button", { name: "sessionImport.step.source" }));
    expect(mocks.abandonPrepare).toHaveBeenCalledWith("source");
  });

  it("does not offer mapping edits that a persisted plan would ignore", () => {
    mocks.pendingPlan = { planId: "persisted-plan", planHash: "persisted-hash" };
    mocks.step = "mapping";
    renderHost();
    expect((screen.getByRole("combobox", { name: "Project mapping for r1: sessionImport.mapUnassigned" }) as HTMLButtonElement).disabled).toBe(true);
    fireEvent.click(document.querySelector("[data-import-commit]") as HTMLButtonElement);
    expect(mocks.commit).toHaveBeenCalledTimes(1);
  });

  it("disables commit and load-more while busy", () => {
    mocks.busy = true;
    mocks.step = "content";
    renderHost();
    const loadMore = document.querySelector(
      "[data-import-load-more]",
    ) as HTMLButtonElement;
    expect(loadMore.disabled).toBe(true);
    cleanup();
    mocks.step = "mapping";
    renderHost();
    const commit = document.querySelector(
      "[data-import-commit]",
    ) as HTMLButtonElement;
    expect(commit.disabled).toBe(true);
  });

  it("keeps mapping navigation disabled while publishing", () => {
    mocks.busy = true;
    mocks.step = "mapping";
    renderHost();
    const back = document.querySelector('[data-import-back="mapping"]') as HTMLButtonElement;
    expect(back.disabled).toBe(true);
    fireEvent.click(back);
    expect(mocks.abandonPrepare).not.toHaveBeenCalled();
    expect(mocks.commit).not.toHaveBeenCalled();
  });

  it("copies the visible import error string, not an empty stand-in", async () => {
    renderHost();
    const btn = document.querySelector(
      "[data-import-error-copy]",
    ) as HTMLButtonElement;
    fireEvent.click(btn);
    expect(mocks.writeText).toHaveBeenCalledWith("SOURCE_BUSY: locked");
  });

  it("keyboard load-more and peek use the same handlers as the buttons", () => {
    renderHost();
    const loadMore = document.querySelector(
      "[data-import-load-more]",
    ) as HTMLButtonElement;
    fireEvent.click(loadMore);
    expect(mocks.loadMore).toHaveBeenCalledTimes(1);
    fireEvent.keyDown(document.querySelector("[data-import-content]") as HTMLElement, {
      key: "ArrowRight",
    });
    expect(mocks.loadMore).toHaveBeenCalledTimes(2);
    fireEvent.keyDown(document.querySelector("[data-import-content]") as HTMLElement, {
      key: "Enter",
    });
    expect(mocks.openPeek).toHaveBeenCalledTimes(1);
    fireEvent.click(screen.getAllByText("sessionImport.preview")[0]);
    expect(mocks.openPeek).toHaveBeenCalledTimes(2);
  });

  it("keyboard load-more is a no-op while busy", () => {
    mocks.busy = true;
    renderHost();
    fireEvent.keyDown(document.querySelector("[data-import-content]") as HTMLElement, {
      key: "ArrowRight",
    });
    expect(mocks.loadMore).not.toHaveBeenCalled();
  });

  it("mapping Enter commits through the same handler as the import button", () => {
    mocks.step = "mapping";
    renderHost();
    const commit = document.querySelector(
      "[data-import-commit]",
    ) as HTMLButtonElement;
    fireEvent.click(commit);
    expect(mocks.commit).toHaveBeenCalledTimes(1);
    fireEvent.keyDown(document.querySelector("[data-import-mapping]") as HTMLElement, {
      key: "Enter",
    });
    expect(mocks.commit).toHaveBeenCalledTimes(2);
  });

  mappingKeyboardCases(() => {
    mocks.step = "mapping";
    renderHost();
    return document.querySelector("[data-import-mapping]") as HTMLElement;
  }, mocks.commit);

  it("Enter on mapping Back goes back and does not commit", async () => {
    const user = userEvent.setup();
    mocks.step = "mapping";
    renderHost();
    const back = document.querySelector(
      '[data-import-back="mapping"]',
    ) as HTMLButtonElement;
    back.focus();
    await user.keyboard("{Enter}");
    expect(mocks.commit).not.toHaveBeenCalled();
    expect(mocks.abandonPrepare).toHaveBeenCalledWith("content");
  });

  it("Enter on mapping Select trigger does not commit", async () => {
    const user = userEvent.setup();
    mocks.step = "mapping";
    renderHost();
    const combo = screen.getByRole("combobox");
    combo.focus();
    await user.keyboard("{Enter}");
    expect(mocks.commit).not.toHaveBeenCalled();
    expect(mocks.setStep).not.toHaveBeenCalled();
  });

  it("Enter on content Preview activates that row peek, not the focused list row", async () => {
    const user = userEvent.setup();
    renderHost();
    const peeks = document.querySelectorAll("[data-import-peek]");
    expect(peeks.length).toBeGreaterThan(1);
    const second = peeks[1] as HTMLButtonElement;
    second.focus();
    await user.keyboard("{Enter}");
    expect(mocks.openPeek).toHaveBeenCalledTimes(1);
    expect(mocks.openPeek.mock.calls[0][0]).toMatchObject({
      candidateId: "cand-2",
    });
  });

  it("Enter on content Back goes to source and does not peek", async () => {
    const user = userEvent.setup();
    renderHost();
    const back = document.querySelector(
      '[data-import-back="content"]',
    ) as HTMLButtonElement;
    back.focus();
    await user.keyboard("{Enter}");
    expect(mocks.openPeek).not.toHaveBeenCalled();
    expect(mocks.setStep).toHaveBeenCalledWith("source");
  });

  it("surfaces durable restore status for a pending plan", () => {
    mocks.restoreStatus = "plan";
    mocks.error = null;
    renderHost();
    const status = document.querySelector('[data-import-restore="plan"]');
    expect(status).not.toBeNull();
    expect(status?.textContent).toContain("sessionImport.restore.plan");
  });

  it("does not leak expired wizard recovery into a readable history or handoff", () => {
    mocks.readerId = "imp:00000000-0000-4000-8000-000000000001";
    mocks.error = "PLAN_STALE: durable plan expired";
    mocks.readerEvents = [{ id: "u1", kind: "message", role: "user", text: "saved history" }];
    const view = renderHost();
    const dialogs = screen.getAllByRole("dialog");
    const wizard = dialogs.find((dialog) => within(dialog).queryByText("sessionImport.title"))!;
    const reader = dialogs.find((dialog) => within(dialog).queryByText("sessionImport.readOnly"))!;
    const handoff = dialogs.find((dialog) => within(dialog).queryByText("sessionImport.handoffTitle"))!;
    expect(within(wizard).getByRole("alert").textContent).toContain("sessionImport.err.planStale");
    expect(within(reader).queryByRole("alert")).toBeNull();
    expect(within(handoff).queryByRole("alert")).toBeNull();
    expect(within(reader).getByText("saved history")).toBeTruthy();
    mocks.continuationError = "HISTORY_ACCESS_UNAVAILABLE: hidden";
    view.rerender(<SessionImportHost locale="en" onOpenNativeSession={mocks.onOpenNative} />);
    expect(within(reader).getByRole("alert").textContent).toContain("sessionImport.err.historyUnavailable");
    expect(within(handoff).getByRole("alert").textContent).toContain("sessionImport.err.historyUnavailable");
  });

  it("keeps revision diagnostics visible when event issue filtering has no matching messages", () => {
    mocks.readerId = "imp:partial";
    mocks.error = null;
    mocks.readerHealth = { outcome: "partial", issueCount: 1, diagnosticsLimited: false,
      issues: [{ code: "unknown-role", message: "Skipped source record" }] };
    mocks.readerEvents = [{ id: "u1", kind: "message", role: "user", text: "readable message" }];
    renderHost();
    const notice = screen.getByRole("region", { name: "sessionImport.revision.title" });
    fireEvent.click(screen.getByRole("button", { name: "sessionImport.issuesOnly" }));
    expect(screen.queryByText("readable message")).toBeNull();
    expect(screen.getByText("sessionImport.filterEmptyEvents")).toBeVisible();
    expect(notice).toBeVisible();
    fireEvent.click(within(notice).getByRole("button"));
    expect(within(notice).getByText("sessionImport.diagnostic.unknownRecord")).toBeVisible();
    expect(within(notice).queryByText("Skipped source record")).toBeNull();
  });

  it("does not expose stale revision diagnostics after reader access fails", () => {
    mocks.readerId = "imp:hidden";
    mocks.readerError = "HISTORY_ACCESS_UNAVAILABLE: hidden";
    mocks.readerHealth = { outcome: "partial", issueCount: 1, diagnosticsLimited: false,
      issues: [{ code: "hidden", message: "Old private diagnostic" }] };
    renderHost();
    expect(screen.queryByRole("region", { name: "sessionImport.revision.title" })).toBeNull();
    expect(screen.queryByText("Old private diagnostic")).toBeNull();
    expect(document.querySelector("[data-import-reader-error]")).not.toBeNull();
  });

  it("shows a reader error instead of empty history", () => {
    mocks.readerId = "imp:00000000-0000-4000-8000-000000000001";
    mocks.readerError = "HISTORY_ACCESS_UNAVAILABLE: hidden";
    mocks.error = null;
    renderHost();
    expect(document.querySelector("[data-import-reader-error]")).not.toBeNull();
    expect(screen.queryByText("sessionImport.filterEmptyEvents")).toBeNull();
  });

  it("restores an event anchor through the bounded reader lookup", async () => {
    mocks.readerId = "imp:00000000-0000-4000-8000-000000000001";
    mocks.readerAnchor = "e5000";
    mocks.readerEvents = [
      {
        id: "e5000",
        kind: "message",
        role: "user",
        blocks: [{ kind: "text", text: "middle" }],
      },
    ];
    mocks.error = null;
    const scrollIntoView = vi.fn();
    Object.defineProperty(HTMLElement.prototype, "scrollIntoView", {
      configurable: true,
      value: scrollIntoView,
    });
    renderHost();
    await vi.waitFor(() => expect(mocks.ensureEvent).toHaveBeenCalledWith("e5000"));
    await vi.waitFor(() => expect(scrollIntoView).toHaveBeenCalled());
    expect(document.getElementById("import-ev-e5000")?.getAttribute("data-highlight")).toBe("1");
  });

  it("keeps keyboard focus at the adjacent event when paging both directions", async () => {
    mocks.readerId = "imp:00000000-0000-4000-8000-000000000001";
    mocks.readerPreviousCursor = "before-5000";
    mocks.readerNextCursor = "after-5000";
    mocks.readerEvents = [{
      id: "e5000",
      kind: "message",
      role: "user",
      blocks: [{ kind: "text", text: "middle" }],
    }];
    mocks.readerLoadPrevious.mockImplementationOnce(async () => {
      mocks.readerEvents = [{
        id: "e4999",
        kind: "message",
        role: "assistant",
        blocks: [{ kind: "text", text: "previous" }],
      }];
      return "e4999";
    });
    mocks.readerLoadMore.mockImplementationOnce(async () => {
      mocks.readerEvents = [{
        id: "e5000",
        kind: "message",
        role: "user",
        blocks: [{ kind: "text", text: "next" }],
      }];
      return "e5000";
    });
    Object.defineProperty(HTMLElement.prototype, "scrollIntoView", {
      configurable: true,
      value: vi.fn(),
    });
    renderHost();

    fireEvent.click(screen.getByRole("button", { name: "sessionImport.loadPrevious" }));
    await vi.waitFor(() => expect(document.activeElement?.id).toBe("import-ev-e4999"));
    fireEvent.click(screen.getByRole("button", { name: "sessionImport.loadMore" }));
    await vi.waitFor(() => expect(document.activeElement?.id).toBe("import-ev-e5000"));
  });

  it("highlights a continuation source without stealing its trigger before async preparation", () => {
    mocks.readerId = "imp:00000000-0000-4000-8000-000000000001";
    mocks.readerEvents = [{ id: "e1", kind: "message", role: "user", text: "synthetic source" }];
    const frames: FrameRequestCallback[] = [];
    const raf = vi.spyOn(window, "requestAnimationFrame").mockImplementation((cb) => {
      frames.push(cb);
      return frames.length;
    });
    Object.defineProperty(HTMLElement.prototype, "scrollIntoView", { configurable: true, value: vi.fn() });
    try {
      renderHost();
      const trigger = screen.getByRole("button", { name: "sessionImport.continueFromHere" });
      trigger.focus();
      fireEvent.click(trigger);
      act(() => frames.splice(0).forEach((cb) => cb(0)));
      expect(document.getElementById("import-ev-e1")).toHaveAttribute("data-highlight", "1");
      expect(trigger).toHaveFocus();
      expect(mocks.prepareContinue).toHaveBeenCalledWith(mocks.readerId, "e1", "viewed-revision");
      expect(mocks.createContinue).not.toHaveBeenCalled();
    } finally {
      raf.mockRestore();
    }
  });

  it("a queued reader anchor cannot steal focus after another dialog takes ownership", () => {
    mocks.readerId = "imp:00000000-0000-4000-8000-000000000001";
    mocks.readerAnchor = "e1";
    mocks.readerEvents = [{ id: "e1", kind: "message", role: "user", text: "synthetic source" }];
    const frames: FrameRequestCallback[] = [];
    const raf = vi.spyOn(window, "requestAnimationFrame").mockImplementation((cb) => {
      frames.push(cb);
      return frames.length;
    });
    Object.defineProperty(HTMLElement.prototype, "scrollIntoView", { configurable: true, value: vi.fn() });
    let stop: (() => void) | undefined;
    try {
      renderHost();
      expect(frames.length).toBeGreaterThan(0);
      const editor = screen.getByRole("textbox", { name: "sessionImport.handoffEdit" });
      stop = installDialogFocus(() => editor.closest('[role="dialog"]'), { initialFocus: "none" });
      editor.focus();
      act(() => frames.splice(0).forEach((cb) => cb(0)));
      expect(editor).toHaveFocus();
      expect(document.getElementById("import-ev-e1")).toHaveAttribute("data-highlight", "1");
    } finally {
      stop?.();
      raf.mockRestore();
    }
  });

  it("continuation failure keeps the reader and does not navigate", async () => {
    mocks.readerId = "imp:00000000-0000-4000-8000-000000000001";
    mocks.error = null;
    mocks.createContinue.mockResolvedValueOnce(null);
    renderHost();
    fireEvent.click(screen.getByRole("button", { name: "sessionImport.createDraft" }));
    await Promise.resolve();
    expect(mocks.createContinue).toHaveBeenCalledTimes(1);
    expect(mocks.onOpenNative).not.toHaveBeenCalled();
    expect(document.querySelector("[data-import-reader-error]")).toBeNull();
  });

  it("creates a continuation once and navigates to the native session", async () => {
    mocks.readerId = "imp:00000000-0000-4000-8000-000000000001";
    mocks.error = null;
    renderHost();
    const btn = screen.getByRole("button", { name: "sessionImport.createDraft" });
    fireEvent.click(btn);
    await Promise.resolve();
    expect(mocks.createContinue).toHaveBeenCalledTimes(1);
    expect(mocks.onOpenNative).toHaveBeenCalledTimes(1);
    expect(mocks.onOpenNative).toHaveBeenCalledWith("native-1");
  });
});

describe("SessionImportContentDetail branch confirm", () => {
  it("confirms a leaf in-app without window.confirm", () => {
    const onConfirm = vi.fn();
    const onCancel = vi.fn();
    const state = {
      ...emptyContentSelection(),
      leaves: ["leaf-a", "leaf-b"],
      leaf: "leaf-a",
      pendingLeaf: "leaf-b",
    };
    render(
      <SessionImportContentDetail
        tr={(key) => key}
        state={state}
        busy={false}
        empty
        onToggleEvent={vi.fn()}
        onSelectPage={vi.fn()}
        onInvertPage={vi.fn()}
        onField={vi.fn()}
        onSubagents={vi.fn()}
        onAsset={vi.fn()}
        onRequestLeaf={vi.fn()}
        onConfirmLeaf={onConfirm}
        onCancelLeaf={onCancel}
        onLoadMore={vi.fn()}
      />,
    );
    fireEvent.click(screen.getByText("sessionImport.applySwitch"));
    expect(onConfirm).toHaveBeenCalledTimes(1);
    expect(mocks.confirm).not.toHaveBeenCalled();
    const detailSrc = readFileSync(
      join(__dirname, "SessionImportContentDetail.tsx"),
      "utf8",
    );
    const hostSrc = readFileSync(join(__dirname, "SessionImportHost.tsx"), "utf8");
    expect(detailSrc).not.toMatch("window.confirm");
    expect(detailSrc).not.toMatch("window.prompt");
    expect(detailSrc).not.toMatch("window.alert");
    expect(hostSrc).not.toMatch("window.confirm");
    expect(detailSrc).toContain("GlassModal");
    expect(detailSrc).toContain("onConfirmLeaf");
  });

  it("disables load-more while busy", () => {
    const state = {
      ...emptyContentSelection(),
      nextCursor: "more",
      events: [
        { eventId: "e1", kind: "message", role: "user", text: "hi", selectable: true },
      ],
    };
    render(
      <SessionImportContentDetail
        tr={(key) => key}
        state={state}
        busy
        empty={false}
        onToggleEvent={vi.fn()}
        onSelectPage={vi.fn()}
        onInvertPage={vi.fn()}
        onField={vi.fn()}
        onSubagents={vi.fn()}
        onAsset={vi.fn()}
        onRequestLeaf={vi.fn()}
        onConfirmLeaf={vi.fn()}
        onCancelLeaf={vi.fn()}
        onLoadMore={vi.fn()}
      />,
    );
    expect(
      (screen.getByText("sessionImport.loadMore") as HTMLButtonElement).disabled,
    ).toBe(true);
  });
});

it("background catalog revision changes preserve the reader input node and do not retarget its requested scope", () => {
  mocks.readerId = "imp:refresh";
  const view = renderHost();
  const input = screen.getByRole("searchbox", { name: "sessionImport.searchPlaceholder" });
  input.focus(); fireEvent.compositionStart(input);
  mocks.catalogEntries = [{ importId: mocks.readerId, installationId: "installation", provider: "codex", nativeSessionId: "native", title: "Refreshed", hidden: false, currentRevision: "background-new", createdAt: "2026-09-27T00:00:00Z" }];
  view.rerender(<SessionImportHost locale="en" onOpenNativeSession={mocks.onOpenNative} />);
  expect(screen.getByRole("searchbox", { name: "sessionImport.searchPlaceholder" })).toBe(input);
  expect(input).toHaveFocus();
  expect(mocks.readerOptions).toHaveBeenLastCalledWith({ revision: null, leaf: null });
  fireEvent.compositionEnd(input);
});
it("reader footer continues the accepted revision and leaf, not catalog latest", () => {
  mocks.readerId = "imp:footer-scope";
  renderHost();
  fireEvent.click(screen.getByRole("button", { name: "sessionImport.continueInGrok" }));
  expect(mocks.prepareContinue).toHaveBeenCalledWith(mocks.readerId, "viewed-leaf", "viewed-revision");
});
it("reader continuation stays disabled until a snapshot is accepted", () => {
  mocks.readerId = "imp:unresolved"; mocks.readerScope = null;
  renderHost();
  const button = screen.getByRole("button", { name: "sessionImport.continueInGrok" });
  expect(button).toBeDisabled(); fireEvent.click(button);
  expect(mocks.prepareContinue).not.toHaveBeenCalled();
});

it("reader host distinguishes idle, searching, no hits, and error without an empty matches column", () => {
  mocks.readerId = "imp:reader-states";
  mocks.readerEvents = [
    { id: "e1", kind: "message", role: "user", blocks: [{ text: "锚点 🙂" }] },
  ];
  mocks.readerHits = [];
  mocks.readerQuery = "";
  mocks.readerBusy = false;
  mocks.readerNotice = null;
  mocks.readerSearchError = null;
  const view = renderHost();
  const split = () => document.querySelector(".session-import__reader-split");
  expect(split()?.getAttribute("data-matches")).toBe("0");
  expect(document.querySelector(".session-import__reader-matches")).toBeNull();
  expect(screen.getByText("锚点 🙂")).toBeTruthy();
  expect(document.querySelector("[data-import-search-state]")).toBeNull();
  const query = screen.getByRole("searchbox", { name: "sessionImport.searchPlaceholder" });
  expect(query).toHaveValue("");

  mocks.readerBusy = true;
  mocks.readerQuery = "锚点 🙂";
  view.rerender(<SessionImportHost locale="en" />);
  expect(document.querySelector("[data-import-search-state='searching']")?.textContent).toBe("sessionImport.searchBusy");
  expect(document.querySelector(".session-import__reader-matches")).toBeNull();
  expect(screen.getByRole("searchbox", { name: "sessionImport.searchPlaceholder" })).toHaveValue("锚点 🙂");
  expect(screen.getByText("锚点 🙂")).toBeTruthy();

  mocks.readerBusy = false;
  mocks.readerNotice = "empty";
  view.rerender(<SessionImportHost locale="en" />);
  expect(document.querySelector("[data-import-search-state='empty']")?.textContent).toBe("sessionImport.searchNoHits");
  expect(document.querySelector("[data-import-search-state='searching']")).toBeNull();
  expect(document.querySelector(".session-import__reader-matches")).toBeNull();
  expect(screen.getByRole("searchbox", { name: "sessionImport.searchPlaceholder" })).toHaveValue("锚点 🙂");

  mocks.readerNotice = null;
  mocks.readerHits = [{ eventId: "e1", snippet: "page matches only", kind: "message" }];
  view.rerender(<SessionImportHost locale="en" />);
  expect(split()?.getAttribute("data-matches")).toBe("1");
  expect(document.querySelector(".session-import__reader-matches")).not.toBeNull();
  expect(screen.getByText("page matches only")).toBeTruthy();
  expect(screen.getByText("sessionImport.searchHits")).toBeTruthy();
  expect(screen.queryByText("sessionImport.searchNoHits")).toBeNull();

  mocks.readerHits = [];
  mocks.readerSearchError = "queryTooLong";
  view.rerender(<SessionImportHost locale="en" />);
  expect(document.querySelector("[data-import-search-state='error']")?.textContent).toBe("sessionImport.searchQueryTooLong");
  expect(document.querySelector("[data-import-search-state='searching']")).toBeNull();
  expect(document.querySelector(".session-import__reader-matches")).toBeNull();
  expect(screen.getByRole("searchbox", { name: "sessionImport.searchPlaceholder" })).toHaveValue("锚点 🙂");
});
