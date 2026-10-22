/** @vitest-environment jsdom */
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { useEffect } from "react";
import { useImportContinuation } from "@/hooks/useImportContinuation";
import type { useSessionImportUi } from "@/hooks/useSessionImport";
import type { HandoffDraft } from "@/lib/api/sessionMigration";
import type { WorkspaceRecord } from "@/lib/multiRootWorkspace";
import { SessionImportContinuationModal } from "./SessionImportContinuationModal";
const mocks = vi.hoisted(() => ({ list: vi.fn(), workspace: vi.fn(), create: vi.fn(), discard: vi.fn(), open: vi.fn() }));
vi.mock("@/lib/api/workspace", () => ({ workspacesList: mocks.list }));
vi.mock("@/lib/api", () => ({ sessionImportContinueWorkspace: mocks.workspace,
  sessionImportContinueCreate: mocks.create, sessionImportContinueDiscard: mocks.discard }));
const draft: HandoffDraft = { draftId: "d1", draftHash: "h1", importId: "imp:1", revisionId: "r1", leaf: "l1",
  text: "handoff text", runtimeCapability: "fragment-only", coverage: "full", estimatedTokens: 10, creationVersion: 1 };
const workspace: WorkspaceRecord = { id: "w1", name: "Workspace one", primaryProjectId: "p1", capability: "contextOnly",
  roots: [{ path: "C:/synthetic/project", role: "primary", access: "write" }], updatedAt: "synthetic" };
const activity = () => {};
function Harness({ initial = draft }: { initial?: HandoffDraft }) {
  const state = useImportContinuation(null, activity);
  const { setDraft } = state;
  useEffect(() => { setDraft(initial); }, [initial, setDraft]);
  const ui = { ...state, busy: state.continuationBusy, projects: [] } as unknown as ReturnType<typeof useSessionImportUi>;
  return <SessionImportContinuationModal ui={ui} locale="en" onOpenNativeSession={mocks.open} />;
}
beforeEach(() => {
  vi.resetAllMocks();
  mocks.list.mockResolvedValue([workspace]);
  mocks.workspace.mockResolvedValue({ ...draft, draftId: "d2", draftHash: "h2", workspace, projectId: "p1" });
  mocks.create.mockResolvedValue({ appSessionId: "native-session" });
  mocks.discard.mockResolvedValue(undefined);
});
afterEach(cleanup);

test("keyboard workspace review shows roots, preserves edits and creates only after confirmation", async () => {
  const user = userEvent.setup();
  render(<Harness />);
  const textbox = await screen.findByRole("textbox", { name: /Edit the first message/ });
  fireEvent.change(textbox, { target: { value: "user edited handoff" } });
  const select = screen.getByRole("button", { name: "Execution workspace" });
  await waitFor(() => expect((select as HTMLButtonElement).disabled).toBe(false));
  select.focus();
  await user.keyboard("{Enter}");
  const option = await screen.findByRole("option", { name: workspace.name });
  within(option).getByRole("button").focus();
  await user.keyboard("{Enter}");
  await screen.findByText(workspace.roots[0].path);
  expect((textbox as HTMLTextAreaElement).value).toBe("user edited handoff");
  expect(mocks.create).not.toHaveBeenCalled();
  await user.click(screen.getByRole("button", { name: "Create continuation" }));
  await waitFor(() => expect(mocks.open).toHaveBeenCalledWith("native-session"));
  expect(mocks.create).toHaveBeenCalledWith("d2", "h2", "continuation:d2", "user edited handoff");
  expect(document.querySelector("select")).toBeNull();
});

test("a pending creation disables target and text edits, while cancel discards through Host", async () => {
  render(<Harness initial={{ ...draft, workspace, creationRequestId: "pending" }} />);
  const textbox = await screen.findByRole("textbox", { name: /Edit the first message/ });
  expect((textbox as HTMLTextAreaElement).disabled).toBe(true);
  expect((screen.getByRole("button", { name: "Execution workspace" }) as HTMLButtonElement).disabled).toBe(true);
  fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  expect(mocks.discard).toHaveBeenCalledWith("d1", "h1");
  expect(mocks.create).not.toHaveBeenCalled();
});

test("workspace loading failure is visible and refresh recovers without choosing a target", async () => {
  mocks.list.mockRejectedValueOnce("SOURCE_BUSY: locked");
  render(<Harness />);
  await screen.findByRole("alert");
  fireEvent.click(screen.getByRole("button", { name: "Refresh and review workspace" }));
  await waitFor(() => expect(screen.queryByRole("alert")).toBeNull());
  expect(mocks.workspace).not.toHaveBeenCalled();
  expect(mocks.create).not.toHaveBeenCalled();
});

test("closing an error dialog leaves its durable creation available for recovery", async () => {
  mocks.discard.mockRejectedValueOnce(new Error("SNAPSHOT_UNAVAILABLE: disk locked"));
  render(<Harness initial={{ ...draft, creationRequestId: "pending" }} />);
  await screen.findByRole("dialog");
  fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
  await screen.findByRole("alert");
  fireEvent.click(screen.getByRole("button", { name: "Close" }));
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  expect(mocks.discard).toHaveBeenCalledTimes(1);
  expect(mocks.create).not.toHaveBeenCalled();
});
