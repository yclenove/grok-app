/** @vitest-environment jsdom */
import { act, cleanup, render, renderHook, screen, waitFor, fireEvent } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { SessionImportContinuationModal } from "@/components/session-import/SessionImportContinuationModal";
import type { HandoffDraft } from "@/lib/api/sessionMigration";
import { openImportedHistory, openSessionImport } from "@/lib/sessionImport/open";
import { defaultSelection } from "@/lib/sessionImport/selection";
import { useSessionImportUi } from "./useSessionImport";

const EDITED = "交接🙂\n第二行";
const IMP = "imp:00000000-0000-4000-8000-000000000153";
const mocks = vi.hoisted(() => ({
  entries: vi.fn().mockResolvedValue({ items: [] }),
  recovery: vi.fn().mockResolvedValue({ draft: null, expired: false }),
  continuePrepare: vi.fn(),
  workspace: vi.fn(),
  selection: vi.fn(),
  create: vi.fn(),
  restore: vi.fn(),
  discard: vi.fn(),
  listRoots: vi.fn().mockResolvedValue([]),
  sources: vi.fn().mockResolvedValue([]),
  projectsList: vi.fn().mockResolvedValue([]),
  listModels: vi.fn(),
  opened: vi.fn(),
}));
vi.mock("@/lib/api", () => ({
  sessionImportEntries: mocks.entries,
  sessionImportRecovery: mocks.recovery,
  sessionImportContinuePrepare: mocks.continuePrepare,
  sessionImportContinueWorkspace: mocks.workspace,
  sessionImportContinueSelection: mocks.selection,
  sessionImportContinueCreate: mocks.create,
  sessionImportContinueRestore: mocks.restore,
  sessionImportContinueDiscard: mocks.discard,
  sessionImportListRoots: mocks.listRoots,
  sessionImportSources: mocks.sources,
  projectsList: mocks.projectsList,
  listModels: mocks.listModels,
}));
vi.mock("@/lib/api/workspace", () => ({ workspacesList: vi.fn().mockResolvedValue([]) }));

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => { resolve = done; });
  return { promise, resolve };
}
function draftFor(leaf: string, text = `原始:${leaf}`): HandoffDraft {
  return {
    draftId: "d1", draftHash: "h1", importId: IMP, revisionId: "rev-1", leaf: "resolved-leaf", text,
    coverage: "full", estimatedTokens: 4, runtimeCapability: "fragment-only",
    selection: { ...defaultSelection(), eventIds: ["message-205", "message-206"] },
  };
}
async function openMessage(result: { current: ReturnType<typeof useSessionImportUi> }) {
  await act(async () => openImportedHistory({ importId: IMP, revision: "rev-1", leaf: "viewed-leaf" }));
  await act(async () => result.current.prepareContinue(IMP, "message-205", "rev-1"));
  act(() => result.current.setDraft({ ...result.current.draft!, text: EDITED }));
  act(() => result.current.hideContinuation());
}

beforeEach(() => {
  vi.clearAllMocks();
  mocks.recovery.mockResolvedValue({ draft: null, expired: false });
  mocks.entries.mockResolvedValue({ items: [] });
  mocks.listRoots.mockResolvedValue([]);
  mocks.sources.mockResolvedValue([]);
  mocks.projectsList.mockResolvedValue([]);
  mocks.continuePrepare.mockImplementation(async (_importId: string, opts: { leaf?: string | null }) => draftFor(opts.leaf ?? ""));
  mocks.discard.mockResolvedValue(undefined);
  mocks.restore.mockRejectedValue(new Error("offline"));
  mocks.create.mockResolvedValue({ appSessionId: "native-session" });
  window.history.replaceState(null, "", "/");
});
afterEach(() => {
  expect(mocks.listModels).not.toHaveBeenCalled();
  cleanup();
  window.history.replaceState(null, "", "/");
});

test("message and toolbar entries do not share a hidden edit", async () => {
  const { result } = renderHook(() => useSessionImportUi());
  await openMessage(result);
  expect(result.current.continuationVisible).toBe(false);
  expect(result.current.draft?.text).toBe(EDITED);
  expect(result.current.readerId).toBe(IMP);
  await act(async () => result.current.prepareContinue(IMP, "message-205", "rev-1"));
  expect(mocks.continuePrepare).toHaveBeenCalledTimes(1);
  expect(result.current.continuationVisible).toBe(true);
  expect(result.current.draft?.text).toBe(EDITED);
  act(() => result.current.hideContinuation());
  await act(async () => result.current.prepareContinue(IMP, "viewed-leaf", "rev-1"));
  expect(mocks.continuePrepare).toHaveBeenCalledTimes(2);
  expect(mocks.continuePrepare).toHaveBeenLastCalledWith(IMP, { projectId: null, leaf: "viewed-leaf", revision: "rev-1" });
  expect(result.current.draft?.text).toBe("原始:viewed-leaf");
  expect(mocks.discard).not.toHaveBeenCalled();
});

test("an omitted leaf uses the reader leaf and does not restore a different anchor", async () => {
  const { result } = renderHook(() => useSessionImportUi());
  await openMessage(result);
  await act(async () => result.current.prepareContinue(IMP));
  expect(mocks.continuePrepare).toHaveBeenLastCalledWith(IMP, { projectId: null, leaf: "viewed-leaf", revision: "rev-1" });
  expect(result.current.draft?.text).toBe("原始:viewed-leaf");
  expect(mocks.continuePrepare).toHaveBeenCalledTimes(2);
});

test("an explicit link is consumed once and the reader action reopens the edit", async () => {
  const { result } = renderHook(() => useSessionImportUi());
  await act(async () => openImportedHistory({ importId: IMP, revision: "rev-1", leaf: "viewed-leaf", continue: true }));
  expect(result.current.readerContinueRequested).toBe(true);
  await act(async () => result.current.prepareReaderContinue(IMP, "viewed-leaf", "rev-1"));
  expect(mocks.continuePrepare).toHaveBeenCalledTimes(1);
  act(() => result.current.setDraft({ ...result.current.draft!, text: EDITED }));
  act(() => result.current.hideContinuation());
  await act(async () => result.current.prepareReaderContinue(IMP, "viewed-leaf", "rev-1"));
  expect(mocks.continuePrepare).toHaveBeenCalledTimes(1);
  expect(result.current.continuationVisible).toBe(false);
  expect(result.current.draft?.text).toBe(EDITED);
  await act(async () => result.current.prepareContinue(IMP, "viewed-leaf", "rev-1"));
  expect(mocks.continuePrepare).toHaveBeenCalledTimes(1);
  expect(result.current.continuationVisible).toBe(true);
  expect(result.current.draft?.text).toBe(EDITED);
});

test("the same reader event keeps a hidden edit and a new revision drops it", async () => {
  const { result } = renderHook(() => useSessionImportUi());
  await openMessage(result);
  await act(async () => openImportedHistory({ importId: IMP, revision: "rev-1", leaf: "viewed-leaf", anchorEventId: "message-206" }));
  expect(result.current.draft?.text).toBe(EDITED);
  expect(result.current.continuationVisible).toBe(false);
  expect(result.current.readerAnchor).toBe("message-206");
  await act(async () => openImportedHistory({ importId: IMP, revision: "rev-2", leaf: "viewed-leaf" }));
  expect(result.current.draft).toBeNull();
  expect(result.current.readerRevision).toBe("rev-2");
  expect(mocks.continuePrepare).toHaveBeenCalledTimes(1);
  expect(mocks.discard).not.toHaveBeenCalled();
  expect(mocks.create).not.toHaveBeenCalled();
});

test("closing the reader clears a hidden edit and does not prepare or discard", async () => {
  const { result } = renderHook(() => useSessionImportUi());
  await openMessage(result);
  act(() => result.current.setReaderId(null));
  expect(result.current.draft).toBeNull();
  expect(result.current.readerId).toBeNull();
  expect(mocks.continuePrepare).toHaveBeenCalledTimes(1);
  expect(mocks.discard).not.toHaveBeenCalled();
});

test("a project change drops an unlocked edit without a reader change or discard", async () => {
  const { result } = renderHook(() => useSessionImportUi());
  await openMessage(result);
  expect(result.current.readerId).toBe(IMP);
  await act(async () => openSessionImport({ projectId: "proj-b" }));
  expect(result.current.draft).toBeNull();
  expect(result.current.readerId).toBe(IMP);
  expect(mocks.discard).not.toHaveBeenCalled();
  expect(mocks.continuePrepare).toHaveBeenCalledTimes(1);
  await act(async () => result.current.prepareContinue(IMP, "message-205", "rev-1"));
  expect(mocks.continuePrepare).toHaveBeenLastCalledWith(IMP, { projectId: "proj-b", leaf: "message-205", revision: "rev-1" });
  expect(result.current.draft?.text).toBe("原始:message-205");
});

test("startup recovery stays hidden with its edit and reopens without preparing", async () => {
  const recovered = draftFor("message-205", "恢复🙂原文");
  mocks.recovery.mockResolvedValueOnce({ draft: recovered, expired: false });
  const { result } = renderHook(() => useSessionImportUi());
  await act(async () => { await Promise.resolve(); });
  expect(result.current.draft?.text).toBe("恢复🙂原文");
  expect(result.current.restoreStatus).toBe("draft");
  act(() => result.current.setDraft({ ...result.current.draft!, text: EDITED }));
  act(() => result.current.hideContinuation());
  expect(result.current.continuationVisible).toBe(false);
  await act(async () => result.current.prepareContinue(IMP, recovered.leaf, recovered.revisionId));
  expect(mocks.continuePrepare).not.toHaveBeenCalled();
  expect(result.current.continuationVisible).toBe(true);
  expect(result.current.draft?.text).toBe(EDITED);
});

test("a locked recovery keeps its project constraint when the wizard project differs", async () => {
  const recovered: HandoffDraft = {
    ...draftFor("resolved-leaf", "冻结🙂正文"), creationRequestId: "durable-request", creationVersion: 1,
    workspaceProjectConstraint: "proj-a",
  };
  mocks.recovery.mockResolvedValueOnce({ draft: recovered, expired: false });
  const { result } = renderHook(() => useSessionImportUi());
  await act(async () => { await Promise.resolve(); });
  act(() => result.current.hideContinuation());
  await act(async () => result.current.prepareContinue(IMP, recovered.leaf, recovered.revisionId));
  expect(mocks.continuePrepare).not.toHaveBeenCalled();
  expect(result.current.draft?.text).toBe("冻结🙂正文");
  expect(result.current.continuationVisible).toBe(false);
  expect(result.current.continuationLocked).toBe(true);
  await act(async () => openSessionImport({ projectId: "proj-a" }));
  expect(result.current.draft?.text).toBe("冻结🙂正文");
  await act(async () => result.current.prepareContinue(IMP, recovered.leaf, recovered.revisionId));
  expect(mocks.continuePrepare).not.toHaveBeenCalled();
  expect(result.current.continuationVisible).toBe(true);
  expect(result.current.draft?.text).toBe("冻结🙂正文");
  expect(mocks.discard).not.toHaveBeenCalled();
});

test("workspace selection and a narrowed scope keep their own bodies", async () => {
  const { result } = renderHook(() => useSessionImportUi());
  await act(async () => openImportedHistory({ importId: IMP, revision: "rev-1", leaf: "viewed-leaf" }));
  await act(async () => result.current.prepareContinue(IMP, "message-205", "rev-1"));
  act(() => result.current.setDraft({ ...result.current.draft!, text: `${EDITED}\n不要带上这段` }));
  mocks.workspace.mockResolvedValueOnce({ ...result.current.draft, draftId: "d2", draftHash: "h2", text: "SERVER" });
  await act(async () => { await result.current.selectContinuationWorkspace("w1"); });
  expect(result.current.draft?.text).toBe(`${EDITED}\n不要带上这段`);
  mocks.selection.mockResolvedValueOnce({ ...result.current.draft, draftId: "d3", draftHash: "h3", text: "只保留选中范围" });
  await act(async () => { expect(await result.current.selectContinuationScope({ ...defaultSelection(), eventIds: ["message-205"] })).toBe(true); });
  expect(result.current.draft?.text).toBe("只保留选中范围");
  expect(result.current.draft?.text).not.toContain("不要带上这段");
  act(() => result.current.hideContinuation());
  await act(async () => result.current.prepareContinue(IMP, "message-205", "rev-1"));
  expect(mocks.continuePrepare).toHaveBeenCalledTimes(1);
  expect(result.current.draft?.text).toBe("只保留选中范围");
});

test("a failed cancel remains after hide, and success allows a fresh prepare", async () => {
  const { result } = renderHook(() => useSessionImportUi());
  await openMessage(result);
  await act(async () => result.current.prepareContinue(IMP, "message-205", "rev-1"));
  mocks.discard.mockRejectedValueOnce(new Error("SNAPSHOT_UNAVAILABLE: disk locked"));
  await act(async () => { expect(await result.current.discardContinuation()).toBe(false); });
  act(() => result.current.hideContinuation());
  expect(result.current.draft?.text).toBe(EDITED);
  expect(result.current.continuationError).toMatch(/SNAPSHOT_UNAVAILABLE/);
  mocks.discard.mockResolvedValueOnce(undefined);
  await act(async () => { expect(await result.current.discardContinuation()).toBe(true); });
  expect(result.current.draft).toBeNull();
  await act(async () => result.current.prepareContinue(IMP, "message-205", "rev-1"));
  expect(mocks.continuePrepare).toHaveBeenCalledTimes(2);
  expect(result.current.draft?.text).toBe("原始:message-205");
});

test("create is exactly once, and a lost reply does not close the reader", async () => {
  const { result } = renderHook(() => useSessionImportUi());
  await act(async () => openImportedHistory({ importId: IMP, revision: "rev-1", leaf: "viewed-leaf" }));
  await act(async () => result.current.prepareContinue(IMP, "message-205", "rev-1"));
  act(() => result.current.setDraft({ ...result.current.draft!, text: EDITED }));
  const pending = deferred<{ appSessionId: string }>();
  mocks.create.mockReturnValueOnce(pending.promise);
  let first!: Promise<string | null>;
  let second!: Promise<string | null>;
  act(() => { first = result.current.createContinue(); second = result.current.createContinue(); });
  act(() => result.current.hideContinuation());
  expect(mocks.create).toHaveBeenCalledTimes(1);
  expect(result.current.continuationVisible).toBe(true);
  expect(result.current.readerId).toBe(IMP);
  await act(async () => {
    pending.resolve({ appSessionId: "native-session" });
    expect(await first).toBe("native-session");
    expect(await second).toBeNull();
  });
  expect(result.current.readerId).toBeNull();
  expect(result.current.draft).toBeNull();
  expect(mocks.create).toHaveBeenCalledWith("d1", "h1", "continuation:d1", EDITED);
  await act(async () => openImportedHistory({ importId: IMP, revision: "rev-1", leaf: "viewed-leaf" }));
  await act(async () => result.current.prepareContinue(IMP, "message-205", "rev-1"));
  act(() => result.current.setDraft({ ...result.current.draft!, text: EDITED }));
  mocks.create.mockRejectedValueOnce(new Error("lost reply"));
  await act(async () => { expect(await result.current.createContinue()).toBeNull(); });
  expect(result.current.readerId).toBe(IMP);
  expect(result.current.continuationLocked).toBe(true);
  act(() => result.current.hideContinuation());
  expect(result.current.draft?.text).toBe(EDITED);
  expect(result.current.continuationVisible).toBe(false);
  mocks.create.mockResolvedValueOnce({ appSessionId: "native-session" });
  await act(async () => result.current.prepareContinue(IMP, "message-205", "rev-1"));
  await act(async () => { expect(await result.current.createContinue()).toBe("native-session"); });
  expect(mocks.create.mock.calls[1]).toEqual(mocks.create.mock.calls[2]);
});

test("an older prepare cannot replace the reader after it closes", async () => {
  const { result } = renderHook(() => useSessionImportUi());
  const older = deferred<HandoffDraft>();
  mocks.continuePrepare.mockReturnValueOnce(older.promise);
  let first!: Promise<void>;
  act(() => { first = result.current.prepareContinue(IMP, "message-205", "rev-1"); });
  act(() => result.current.setReaderId(null));
  mocks.continuePrepare.mockResolvedValueOnce(draftFor("message-206", "新回复"));
  let second!: Promise<void>;
  act(() => { second = result.current.prepareContinue(IMP, "message-206", "rev-1"); });
  await act(async () => { older.resolve(draftFor("message-205", "旧回复")); await first; });
  expect(result.current.draft?.text).not.toBe("旧回复");
  await act(async () => { await second; });
  expect(result.current.draft?.text).toBe("新回复");
  expect(result.current.readerId).toBeNull();
});

test("the public hook drives the real dialog through hide and the same action", async () => {
  function Surface() {
    const ui = useSessionImportUi();
    return <>
      <button type="button" onClick={() => void ui.prepareContinue(IMP, "message-205", "rev-1")}>Handoff from message-205</button>
      <SessionImportContinuationModal ui={ui} locale="en" onOpenNativeSession={mocks.opened} />
    </>;
  }
  render(<Surface />);
  const trigger = screen.getByRole("button", { name: "Handoff from message-205" });
  trigger.focus();
  fireEvent.click(trigger);
  const editor = await screen.findByRole("textbox", { name: /Edit the first message/ }) as HTMLTextAreaElement;
  fireEvent.change(editor, { target: { value: EDITED } });
  await waitFor(() => expect(document.activeElement).toBe(screen.getByRole("button", { name: "Close" })));
  fireEvent.click(screen.getByRole("button", { name: "Close" }));
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  expect(document.activeElement).toBe(trigger);
  fireEvent.click(trigger);
  await screen.findByRole("dialog");
  expect((screen.getByRole("textbox", { name: /Edit the first message/ }) as HTMLTextAreaElement).value).toBe(EDITED);
  expect(mocks.continuePrepare).toHaveBeenCalledTimes(1);
  expect(mocks.discard).not.toHaveBeenCalled();
  expect(mocks.create).not.toHaveBeenCalled();
  expect(mocks.opened).not.toHaveBeenCalled();
});
