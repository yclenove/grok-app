/** @vitest-environment jsdom */
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { useState } from "react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { GlassModal } from "@/components/GlassModal";
import { useImportContinuation } from "@/hooks/useImportContinuation";
import type { useSessionImportUi } from "@/hooks/useSessionImport";
import type { HandoffDraft } from "@/lib/api/sessionMigration";
import { defaultSelection } from "@/lib/sessionImport/selection";
import { SessionImportContinuationModal } from "./SessionImportContinuationModal";

const EDITED = "交接🙂\n第二行";
const NARROW = "只保留选中范围";
const IMP = "imp:1";
const REV = "rev-1";
const api = vi.hoisted(() => ({
  sessionImportContinuePrepare: vi.fn(),
  sessionImportContinueWorkspace: vi.fn(),
  sessionImportContinueSelection: vi.fn(),
  sessionImportContinueCreate: vi.fn(),
  sessionImportContinueRestore: vi.fn(),
  sessionImportContinueDiscard: vi.fn(),
  listModels: vi.fn(),
}));
const opened = vi.hoisted(() => vi.fn());
const readerClosed = vi.hoisted(() => vi.fn());
const activity = vi.hoisted(() => vi.fn());
const workspaces = vi.hoisted(() => vi.fn());
vi.mock("@/lib/api", () => api);
vi.mock("@/lib/api/workspace", () => ({ workspacesList: workspaces }));

type PrepareOpts = { projectId?: string | null; leaf?: string | null; revision?: string | null };

function prepared(importId: string, opts: PrepareOpts = {}): HandoffDraft {
  return {
    draftId: "d1", draftHash: "h1", importId, revisionId: opts.revision ?? REV, leaf: "resolved-leaf",
    text: `原始:${importId}:${opts.leaf ?? ""}:${opts.revision ?? ""}:${opts.projectId ?? ""}`,
    coverage: "full", estimatedTokens: 8, runtimeCapability: "fragment-only", projectId: opts.projectId ?? null,
    selection: { ...defaultSelection(), eventIds: ["message-205", "message-206"] },
  };
}
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => { resolve = done; });
  return { promise, resolve };
}
function editor() {
  return screen.getByRole("textbox", { name: /Edit the first message/ }) as HTMLTextAreaElement;
}
function retained() {
  return screen.getByTestId("retained-draft").textContent ?? "";
}
async function openHandoff(name = "Handoff from message-205") {
  const trigger = screen.getByRole("button", { name });
  trigger.focus();
  fireEvent.click(trigger);
  await screen.findByRole("dialog", { name: "Handoff to Grok" });
  await waitFor(() => expect(document.activeElement).toBe(screen.getByRole("button", { name: "Close" })));
  return trigger;
}
async function dismiss(kind: "x" | "escape" | "backdrop", trigger: HTMLElement) {
  if (kind === "x") fireEvent.click(screen.getByRole("button", { name: "Close" }));
  else if (kind === "escape") fireEvent.keyDown(document, { key: "Escape" });
  else {
    const overlays = document.querySelectorAll(".overlay");
    const overlay = overlays.item(overlays.length - 1);
    if (!overlay) throw new Error("missing overlay");
    fireEvent.mouseDown(overlay);
  }
  await waitFor(() => expect(screen.queryByRole("dialog", { name: "Handoff to Grok" })).toBeNull());
  expect(screen.getByRole("dialog", { name: "Imported history" })).toBeTruthy();
  expect(readerClosed).not.toHaveBeenCalled();
  expect(document.activeElement).toBe(trigger);
  expect(screen.getByTestId("retained-visible").textContent).toBe("no");
}
function Harness() {
  const [project, setProject] = useState<string | null>("proj-a");
  const [shown, setShown] = useState("message-205");
  const [readerOpen, setReaderOpen] = useState(true);
  const state = useImportContinuation(project, activity);
  const ui = { ...state, busy: state.continuationBusy, projects: [] } as unknown as ReturnType<typeof useSessionImportUi>;
  const handoff = (leaf: string, revision = REV, importId = IMP) => { void state.prepareContinue(importId, leaf, revision); };
  return <>
    <GlassModal open={readerOpen} onClose={() => { readerClosed(); setReaderOpen(false); }}
      title="Imported history" closeLabel="Close history">
      <p>正在阅读 {shown}</p>
      <button type="button" onClick={() => setShown("message-206")}>Next event</button>
      <button type="button" onClick={() => handoff("message-205")}>Handoff from message-205</button>
      <button type="button" onClick={() => handoff("message-206")}>Handoff from message-206</button>
      <button type="button" onClick={() => handoff("viewed-leaf")}>Handoff from toolbar</button>
      <button type="button" onClick={() => handoff("message-205", "rev-2")}>Handoff other revision</button>
      <button type="button" onClick={() => handoff("message-205", REV, "imp:2")}>Handoff other history</button>
      <button type="button" onClick={() => setProject("proj-b")}>Use project proj-b</button>
      <button type="button" onClick={() => { void state.selectContinuationWorkspace("w1"); }}>Select workspace</button>
      <button type="button" onClick={() => { void state.selectContinuationScope({ ...defaultSelection(), eventIds: ["message-205"] }); }}>Apply narrowed scope</button>
    </GlassModal>
    <SessionImportContinuationModal ui={ui} locale="en" onOpenNativeSession={opened} />
    <div data-testid="retained-draft">{state.draft?.text ?? ""}</div>
    <div data-testid="retained-visible">{state.continuationVisible ? "yes" : "no"}</div>
    <div data-testid="retained-locked">{state.continuationLocked ? "yes" : "no"}</div>
  </>;
}

beforeEach(() => {
  vi.resetAllMocks();
  workspaces.mockResolvedValue([]);
  api.sessionImportContinuePrepare.mockImplementation(async (importId: string, opts: PrepareOpts = {}) => prepared(importId, opts));
  api.sessionImportContinueWorkspace.mockImplementation(async () => ({ ...prepared(IMP), draftId: "d2", draftHash: "h2", text: "SERVER" }));
  api.sessionImportContinueSelection.mockImplementation(async () => ({
    ...prepared(IMP), draftId: "d3", draftHash: "h3", text: NARROW,
    selection: { ...defaultSelection(), eventIds: ["message-205"] },
  }));
  api.sessionImportContinueCreate.mockResolvedValue({ appSessionId: "native-session" });
  api.sessionImportContinueRestore.mockRejectedValue(new Error("offline"));
  api.sessionImportContinueDiscard.mockResolvedValue(undefined);
});
afterEach(() => {
  cleanup();
  expect(api.listModels).not.toHaveBeenCalled();
  vi.clearAllMocks();
});

test.each(["x", "escape", "backdrop"] as const)("%s hides the edit and the same action restores it", async (kind) => {
  render(<Harness />);
  await screen.findByRole("dialog", { name: "Imported history" });
  const trigger = await openHandoff();
  fireEvent.change(editor(), { target: { value: EDITED } });
  await dismiss(kind, trigger);
  expect(retained()).toBe(EDITED);
  expect(api.sessionImportContinuePrepare).toHaveBeenCalledTimes(1);
  const again = await openHandoff();
  expect(editor().value).toBe(EDITED);
  fireEvent.change(editor(), { target: { value: `${EDITED}\n又改一次` } });
  await dismiss(kind, again);
  await openHandoff();
  expect(editor().value).toBe(`${EDITED}\n又改一次`);
  expect(api.sessionImportContinuePrepare).toHaveBeenCalledTimes(1);
  expect(api.sessionImportContinuePrepare).toHaveBeenCalledWith(IMP, { projectId: "proj-a", leaf: "message-205", revision: REV });
  expect(api.sessionImportContinueDiscard).not.toHaveBeenCalled();
  expect(api.sessionImportContinueCreate).not.toHaveBeenCalled();
  expect(api.sessionImportContinueWorkspace).not.toHaveBeenCalled();
  expect(api.sessionImportContinueSelection).not.toHaveBeenCalled();
  expect(opened).not.toHaveBeenCalled();
  expect(activity).toHaveBeenCalledTimes(1);
});

test("the underlying history stays readable and the same trigger reopens the edit", async () => {
  render(<Harness />);
  await screen.findByRole("dialog", { name: "Imported history" });
  const trigger = await openHandoff();
  fireEvent.change(editor(), { target: { value: EDITED } });
  expect(screen.getByRole("dialog", { name: "Imported history" }).textContent).toContain("正在阅读 message-205");
  await dismiss("x", trigger);
  const next = screen.getByRole("button", { name: "Next event" });
  next.focus();
  fireEvent.click(next);
  expect(screen.getByRole("dialog", { name: "Imported history" }).textContent).toContain("正在阅读 message-206");
  expect(document.activeElement).toBe(next);
  expect(retained()).toBe(EDITED);
  expect(api.sessionImportContinuePrepare).toHaveBeenCalledTimes(1);
  await openHandoff();
  expect(editor().value).toBe(EDITED);
  expect(document.activeElement).toBe(screen.getByRole("button", { name: "Close" }));
  expect(document.activeElement).not.toBe(next);
});

test("a whitespace-only edit stays editable and cannot be created", async () => {
  render(<Harness />);
  await screen.findByRole("dialog", { name: "Imported history" });
  const trigger = await openHandoff();
  fireEvent.change(editor(), { target: { value: " \n\t " } });
  const create = screen.getByRole("button", { name: "Create continuation" }) as HTMLButtonElement;
  expect(create.disabled).toBe(true);
  fireEvent.click(create);
  await dismiss("escape", trigger);
  await openHandoff();
  expect(editor().value).toBe(" \n\t ");
  expect((screen.getByRole("button", { name: "Create continuation" }) as HTMLButtonElement).disabled).toBe(true);
  expect(api.sessionImportContinueCreate).not.toHaveBeenCalled();
  expect(api.sessionImportContinuePrepare).toHaveBeenCalledTimes(1);
});

test("a hidden edit is not reused for another anchor, revision, history, or project", async () => {
  render(<Harness />);
  await screen.findByRole("dialog", { name: "Imported history" });
  const trigger = await openHandoff();
  fireEvent.change(editor(), { target: { value: EDITED } });
  await dismiss("x", trigger);
  await openHandoff("Handoff from message-206");
  expect(editor().value).not.toContain("交接");
  expect(editor().value).toContain("message-206");
  await dismiss("x", screen.getByRole("button", { name: "Handoff from message-206" }));
  await openHandoff("Handoff other revision");
  expect(editor().value).toContain("rev-2");
  expect(editor().value).not.toContain("交接");
  await dismiss("x", screen.getByRole("button", { name: "Handoff other revision" }));
  await openHandoff("Handoff other history");
  expect(editor().value).toContain("imp:2");
  await dismiss("x", screen.getByRole("button", { name: "Handoff other history" }));
  fireEvent.click(screen.getByRole("button", { name: "Use project proj-b" }));
  await waitFor(() => expect(retained()).toBe(""));
  expect(screen.queryByRole("dialog", { name: "Handoff to Grok" })).toBeNull();
  expect(api.sessionImportContinueDiscard).not.toHaveBeenCalled();
  await openHandoff();
  expect(editor().value).toContain("proj-b");
  expect(editor().value).not.toContain("交接");
  expect(api.sessionImportContinuePrepare).toHaveBeenCalledTimes(5);
  expect(api.sessionImportContinuePrepare).toHaveBeenLastCalledWith(IMP, { projectId: "proj-b", leaf: "message-205", revision: REV });
});

test("the toolbar leaf does not inherit a hidden message-anchor edit", async () => {
  render(<Harness />);
  await screen.findByRole("dialog", { name: "Imported history" });
  const trigger = await openHandoff();
  fireEvent.change(editor(), { target: { value: EDITED } });
  await dismiss("backdrop", trigger);
  await openHandoff("Handoff from toolbar");
  expect(editor().value).toBe(`原始:${IMP}:viewed-leaf:${REV}:proj-a`);
  expect(api.sessionImportContinuePrepare).toHaveBeenCalledTimes(2);
  expect(api.sessionImportContinueDiscard).not.toHaveBeenCalled();
});

test("workspace selection keeps the edit across close and reopen", async () => {
  render(<Harness />);
  await screen.findByRole("dialog", { name: "Imported history" });
  const trigger = await openHandoff();
  fireEvent.change(editor(), { target: { value: EDITED } });
  fireEvent.click(screen.getByRole("button", { name: "Select workspace" }));
  await waitFor(() => expect(api.sessionImportContinueWorkspace).toHaveBeenCalledWith("d1", "h1", "w1"));
  expect(editor().value).toBe(EDITED);
  await dismiss("x", trigger);
  await openHandoff();
  expect(editor().value).toBe(EDITED);
  expect(api.sessionImportContinuePrepare).toHaveBeenCalledTimes(1);
  expect(api.sessionImportContinueWorkspace).toHaveBeenCalledTimes(1);
});

test("closing an uncommitted scope review does not regenerate the edit", async () => {
  render(<Harness />);
  await screen.findByRole("dialog", { name: "Imported history" });
  const trigger = await openHandoff();
  fireEvent.change(editor(), { target: { value: EDITED } });
  fireEvent.click(screen.getByRole("button", { name: "Review history permissions" }));
  expect(screen.getByRole("button", { name: "Keep current permissions" })).toBeTruthy();
  await dismiss("escape", trigger);
  await openHandoff();
  expect(editor().value).toBe(EDITED);
  expect(screen.queryByRole("button", { name: "Keep current permissions" })).toBeNull();
  expect(api.sessionImportContinueSelection).not.toHaveBeenCalled();
  expect(api.sessionImportContinuePrepare).toHaveBeenCalledTimes(1);
});

test("a narrowed scope replaces excluded prose and the new body survives close", async () => {
  render(<Harness />);
  await screen.findByRole("dialog", { name: "Imported history" });
  const trigger = await openHandoff();
  fireEvent.change(editor(), { target: { value: `${EDITED}\n不要带上这段` } });
  fireEvent.click(screen.getByRole("button", { name: "Apply narrowed scope" }));
  await waitFor(() => expect(editor().value).toBe(NARROW));
  expect(editor().value).not.toContain("不要带上这段");
  expect(editor().value).not.toContain("交接");
  await dismiss("x", trigger);
  await openHandoff();
  expect(editor().value).toBe(NARROW);
  expect(api.sessionImportContinueSelection).toHaveBeenCalledTimes(1);
  expect(api.sessionImportContinuePrepare).toHaveBeenCalledTimes(1);
});

test("cancel discards and the next open prepares a fresh body", async () => {
  render(<Harness />);
  await screen.findByRole("dialog", { name: "Imported history" });
  const trigger = await openHandoff();
  fireEvent.change(editor(), { target: { value: EDITED } });
  fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
  await waitFor(() => expect(screen.queryByRole("dialog", { name: "Handoff to Grok" })).toBeNull());
  expect(retained()).toBe("");
  expect(api.sessionImportContinueDiscard).toHaveBeenCalledWith("d1", "h1");
  await openHandoff();
  expect(editor().value).not.toContain("交接");
  expect(api.sessionImportContinuePrepare).toHaveBeenCalledTimes(2);
  expect(trigger.isConnected).toBe(true);
});

test("a failed cancel stays recoverable after the dialog is hidden", async () => {
  api.sessionImportContinueDiscard.mockRejectedValueOnce(new Error("SNAPSHOT_UNAVAILABLE: disk locked"));
  render(<Harness />);
  await screen.findByRole("dialog", { name: "Imported history" });
  const trigger = await openHandoff();
  fireEvent.change(editor(), { target: { value: EDITED } });
  fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
  await screen.findByRole("alert");
  expect(editor().value).toBe(EDITED);
  await dismiss("x", trigger);
  await openHandoff();
  expect(editor().value).toBe(EDITED);
  expect(screen.getByRole("alert")).toBeTruthy();
  expect(api.sessionImportContinueDiscard).toHaveBeenCalledTimes(1);
  expect(api.sessionImportContinueCreate).not.toHaveBeenCalled();
  expect(api.sessionImportContinuePrepare).toHaveBeenCalledTimes(1);
});

test("escape and backdrop do not dismiss a busy handoff or the reader under it", async () => {
  const pending = deferred<HandoffDraft>();
  api.sessionImportContinueWorkspace.mockReturnValueOnce(pending.promise);
  render(<Harness />);
  await screen.findByRole("dialog", { name: "Imported history" });
  const trigger = await openHandoff();
  fireEvent.change(editor(), { target: { value: EDITED } });
  fireEvent.click(screen.getByRole("button", { name: "Select workspace" }));
  await waitFor(() => expect(screen.queryByRole("button", { name: "Close" })).toBeNull());
  fireEvent.keyDown(document, { key: "Escape" });
  const overlays = document.querySelectorAll(".overlay");
  const overlay = overlays.item(overlays.length - 1);
  if (!overlay) throw new Error("missing overlay");
  fireEvent.mouseDown(overlay);
  expect(screen.getByRole("dialog", { name: "Handoff to Grok" })).toBeTruthy();
  expect(screen.getByRole("dialog", { name: "Imported history" })).toBeTruthy();
  expect(readerClosed).not.toHaveBeenCalled();
  pending.resolve({ ...prepared(IMP), draftId: "d2", draftHash: "h2", text: "SERVER" });
  await waitFor(() => expect(editor().value).toBe(EDITED));
  expect(screen.getByRole("button", { name: "Close" })).toBeTruthy();
  await dismiss("x", trigger);
  expect(retained()).toBe(EDITED);
});

test("hiding a locked recovery does not unlock it or prepare a different anchor", async () => {
  api.sessionImportContinuePrepare.mockResolvedValueOnce({ ...prepared(IMP, { leaf: "message-205", revision: REV, projectId: "proj-a" }), text: "冻结🙂正文", creationRequestId: "durable-request" });
  render(<Harness />);
  await screen.findByRole("dialog", { name: "Imported history" });
  const trigger = await openHandoff();
  expect(editor().value).toBe("冻结🙂正文");
  expect(editor().disabled).toBe(true);
  expect(screen.getByTestId("retained-locked").textContent).toBe("yes");
  await dismiss("backdrop", trigger);
  expect(screen.getByTestId("retained-locked").textContent).toBe("yes");
  expect(retained()).toBe("冻结🙂正文");
  await openHandoff();
  expect(editor().value).toBe("冻结🙂正文");
  expect(editor().disabled).toBe(true);
  expect(api.sessionImportContinuePrepare).toHaveBeenCalledTimes(1);
  fireEvent.click(screen.getByRole("button", { name: "Handoff from message-206" }));
  await waitFor(() => expect(screen.queryByRole("dialog", { name: "Handoff to Grok" })).toBeNull());
  expect(retained()).toBe("冻结🙂正文");
  expect(screen.getByTestId("retained-locked").textContent).toBe("yes");
  expect(api.sessionImportContinuePrepare).toHaveBeenCalledTimes(1);
  expect(api.sessionImportContinueDiscard).not.toHaveBeenCalled();
  expect(api.sessionImportContinueCreate).not.toHaveBeenCalled();
});

test("two create clicks share one request and open the native session once", async () => {
  const pending = deferred<{ appSessionId: string }>();
  api.sessionImportContinueCreate.mockReturnValueOnce(pending.promise);
  render(<Harness />);
  await screen.findByRole("dialog", { name: "Imported history" });
  await openHandoff();
  fireEvent.change(editor(), { target: { value: EDITED } });
  const create = screen.getByRole("button", { name: "Create continuation" });
  fireEvent.click(create);
  fireEvent.click(create);
  expect(api.sessionImportContinueCreate).toHaveBeenCalledTimes(1);
  expect(api.sessionImportContinueCreate).toHaveBeenCalledWith("d1", "h1", "continuation:d1", EDITED);
  expect(opened).not.toHaveBeenCalled();
  pending.resolve({ appSessionId: "native-session" });
  await waitFor(() => expect(opened).toHaveBeenCalledTimes(1));
  expect(opened).toHaveBeenCalledWith("native-session");
  expect(screen.queryByRole("dialog", { name: "Handoff to Grok" })).toBeNull();
});

test("a lost create reply stays frozen after hide and retries the same request", async () => {
  api.sessionImportContinueCreate
    .mockRejectedValueOnce(new Error("lost reply"))
    .mockResolvedValueOnce({ appSessionId: "native-session" });
  render(<Harness />);
  await screen.findByRole("dialog", { name: "Imported history" });
  const trigger = await openHandoff();
  fireEvent.change(editor(), { target: { value: EDITED } });
  fireEvent.click(screen.getByRole("button", { name: "Create continuation" }));
  await screen.findByRole("alert");
  expect(editor().disabled).toBe(true);
  expect(editor().value).toBe(EDITED);
  await dismiss("x", trigger);
  expect(screen.getByTestId("retained-locked").textContent).toBe("yes");
  await openHandoff();
  expect(editor().value).toBe(EDITED);
  expect(editor().disabled).toBe(true);
  fireEvent.click(screen.getByRole("button", { name: "Create continuation" }));
  await waitFor(() => expect(opened).toHaveBeenCalledTimes(1));
  expect(api.sessionImportContinueCreate).toHaveBeenCalledTimes(2);
  expect(api.sessionImportContinueCreate.mock.calls[1]).toEqual(api.sessionImportContinueCreate.mock.calls[0]);
  expect(api.sessionImportContinuePrepare).toHaveBeenCalledTimes(1);
  expect(api.sessionImportContinueRestore).toHaveBeenCalledTimes(1);
});
