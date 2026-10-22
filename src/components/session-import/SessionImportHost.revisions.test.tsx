/** @vitest-environment jsdom */
import "@testing-library/jest-dom/vitest";
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { openImportedHistory } from "@/lib/sessionImport/open";
import { SessionImportHost } from "./SessionImportHost";

const mocks = vi.hoisted(() => ({ invoke: vi.fn(), entries: [] as import("@/lib/api/sessionMigration").CatalogEntry[] }));
vi.mock("@/lib/api/host", () => ({ invoke: mocks.invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn().mockResolvedValue(() => {}) }));
vi.mock("@/hooks/useImportedCatalog", () => ({ useImportedCatalog: () => ({ entries: mocks.entries, refreshVersion: 0 }) }));
vi.mock("./ImportedWorkspaceAssociation", () => ({ ImportedWorkspaceAssociation: () => null }));
vi.mock("./SessionImportContinuationModal", () => ({ SessionImportContinuationModal: () => null }));
vi.mock("@/lib/nativeWebviewCover", async (importOriginal) => ({
  ...await importOriginal<typeof import("@/lib/nativeWebviewCover")>(),
  acquireNativeWebviewCover: () => () => {},
}));

const originalScroll = Object.getOwnPropertyDescriptor(HTMLElement.prototype, "scrollIntoView");

const IMPORT = "imp:00000000-0000-4000-8000-000000000001";
const OLD = "00000000-0000-4000-8000-000000000001";
const NEW = "00000000-0000-4000-8000-000000000002";
const calls = (name: string) => mocks.invoke.mock.calls.filter(([command]) => command === name);
beforeEach(() => {
  Object.defineProperty(HTMLElement.prototype, "scrollIntoView", { configurable: true, value: vi.fn() });
  window.history.replaceState(null, "", "/");
  mocks.entries = [{ importId: IMPORT, installationId: "fixture", provider: "cursor", nativeSessionId: "native", title: "Archive", hidden: false, createdAt: "2026-10-15", currentRevision: NEW }];
  mocks.invoke.mockReset().mockImplementation(async (command, args) => {
    if (command === "session_import_events" || command === "session_import_event_context") {
      const revision = args.revision ?? NEW;
      return { revision, leaf: revision === OLD ? "old-leaf" : "new-leaf", events: [{ id: "event", role: "user", text: revision === OLD ? "Old preserved body" : "New imported body" }],
        previousCursor: null, nextCursor: null, total: 1, start: 0, end: 1,
        targetIndex: 0, ordinal: 0, position: 0, anchorCursor: "context-cursor" };
    }
    if (command === "session_import_revisions") return {
      importId: IMPORT, currentRevision: NEW, snapshot: "a".repeat(64), total: 2, previousCursor: null, nextCursor: null,
      items: [NEW, OLD].map((revisionId) => ({ revisionId, createdAt: "2026-10-15T00:00:00Z", outcome: "complete" })),
    };
    if (command === "session_import_search") return { hits: [], previousCursor: null, nextCursor: null };
    if (command === "session_import_export_markdown") return "# Exact selected revision";
    if (command === "session_import_continue_prepare") return { draftId: "draft", importId: IMPORT, revisionId: args.revision, leaf: args.leaf, text: "Draft", draftHash: "hash", coverage: "complete", estimatedTokens: 2, runtimeCapability: "unavailable" };
    throw new Error(`Unexpected IPC: ${command}`);
  });
  Object.defineProperty(navigator, "clipboard", { configurable: true, value: { writeText: vi.fn().mockResolvedValue(undefined) } });
});
afterEach(() => {
  cleanup(); window.history.replaceState(null, "", "/");
  if (originalScroll) Object.defineProperty(HTMLElement.prototype, "scrollIntoView", originalScroll);
  else Reflect.deleteProperty(HTMLElement.prototype, "scrollIntoView");
});

test("real Host, domain bus and reader switch old/new revisions and pin search/export/continue through metadata refresh", async () => {
  const user = userEvent.setup();
  const view = render(<SessionImportHost locale="en" />);
  act(() => openImportedHistory({ importId: IMPORT, revision: NEW, leaf: "new-leaf", anchorEventId: "event" }));
  await screen.findByText("New imported body");
  expect(calls("session_import_revisions")).toHaveLength(0);
  await user.click(screen.getByText("History versions", { selector: "summary" }));
  await screen.findByRole("button", { name: "History versions" });
  await user.click(screen.getByRole("button", { name: "History versions" }));
  await user.click(within(screen.getByRole("option", { name: /00000001/ })).getByRole("button"));
  await screen.findByText("Old preserved body");
  expect(calls("session_import_events").at(-1)?.[1]).toMatchObject({ importId: IMPORT, revision: OLD, leaf: null });
  expect(window.location.hash).toContain(`/rev/${OLD}`);
  expect(window.location.hash).not.toContain("/leaf/");
  expect(window.location.hash).not.toContain("/event/");
  const input = screen.getByRole("searchbox");
  fireEvent.change(input, { target: { value: "old text" } });
  fireEvent.submit(input.closest("form")!);
  await waitFor(() => expect(calls("session_import_search").at(-1)?.[1]).toMatchObject({ revision: OLD, leaf: "old-leaf", query: "old text" }));
  const count = calls("session_import_events").length;
  mocks.entries = [{ ...mocks.entries[0], title: "Refreshed title", currentRevision: NEW }];
  view.rerender(<SessionImportHost locale="en" />);
  expect(calls("session_import_events")).toHaveLength(count);
  expect(input).toHaveValue("old text");
  await user.click(within(screen.getByText("History versions", { selector: "summary" }).parentElement!).getByRole("button", { name: "Refresh" }));
  await waitFor(() => expect(calls("session_import_revisions")).toHaveLength(2));
  expect(calls("session_import_events")).toHaveLength(count);
  expect(input).toHaveValue("old text");
  await user.click(screen.getByRole("button", { name: "Export Markdown" }));
  await waitFor(() => expect(calls("session_import_export_markdown").at(-1)?.[1]).toMatchObject({ revision: OLD, leaf: "old-leaf" }));
  await user.click(screen.getByRole("button", { name: "Continue in Grok" }));
  await waitFor(() => expect(calls("session_import_continue_prepare").at(-1)?.[1]).toMatchObject({ revision: OLD, leaf: "old-leaf" }));
  await user.click(screen.getByRole("button", { name: "History versions" }));
  await user.click(within(screen.getByRole("option", { name: /00000002.*Current/ })).getByRole("button"));
  await screen.findByText("New imported body");
  expect(screen.getByRole("searchbox")).toHaveValue("");
  expect(calls("session_import_events").at(-1)?.[1]).toMatchObject({ revision: NEW, leaf: null });
});

test("closing a loading version list discards late replies; keyboard selection and Escape retain reader focus", async () => {
  const user = userEvent.setup();
  let resolve!: (value: unknown) => void;
  const pending = new Promise((done) => { resolve = done; });
  const originalInvoke = mocks.invoke.getMockImplementation()!;
  let attempts = 0;
  mocks.invoke.mockImplementation((command, args) => command === "session_import_revisions" && ++attempts === 1
    ? pending : originalInvoke(command, args));
  render(<SessionImportHost locale="en" />);
  act(() => openImportedHistory({ importId: IMPORT, revision: NEW }));
  await screen.findByText("New imported body");
  expect(calls("session_import_revisions")).toHaveLength(0);
  const summary = screen.getByText("History versions", { selector: "summary" });
  await user.click(summary);
  await waitFor(() => expect(calls("session_import_revisions")).toHaveLength(1));
  await user.click(summary);
  await waitFor(() => expect(screen.queryByText("Loading versions…")).not.toBeInTheDocument());
  await user.click(summary);
  const trigger = await screen.findByRole("button", { name: "History versions" });
  await act(async () => resolve({ importId: IMPORT, currentRevision: OLD, snapshot: "b".repeat(64),
    total: 99, items: [{ revisionId: OLD, createdAt: "2026-10-14T00:00:00Z", outcome: "complete" }],
    previousCursor: null, nextCursor: null }));
  expect(calls("session_import_revisions")).toHaveLength(2);
  expect(screen.getByText("2 versions")).toBeInTheDocument();
  expect(trigger).toHaveTextContent("00000002");
  trigger.focus();
  await user.keyboard("{ArrowDown}{End}{Enter}");
  await screen.findByText("Old preserved body");
  await waitFor(() => expect(trigger).toHaveFocus());
  expect(window.location.hash).toContain(`/rev/${OLD}`);
  await user.keyboard("{ArrowUp}");
  await screen.findByRole("listbox", { name: "History versions" });
  await user.keyboard("{Escape}");
  expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
  expect(screen.getByRole("dialog")).toBeInTheDocument();
  expect(trigger).toHaveFocus();
});

test("a failed version-page request preserves the selected reader and retries the same destination", async () => {
  const user = userEvent.setup();
  const originalInvoke = mocks.invoke.getMockImplementation()!;
  let attempts = 0;
  mocks.invoke.mockImplementation((command, args) => {
    if (command !== "session_import_revisions") return originalInvoke(command, args);
    attempts += 1;
    if (attempts === 2) return Promise.reject(new Error("SOURCE_BUSY: temporarily locked"));
    return Promise.resolve({ importId: IMPORT, currentRevision: NEW, snapshot: "a".repeat(64), total: 2,
      items: [{ revisionId: args.cursor ? OLD : NEW, createdAt: "2026-10-15T00:00:00Z", outcome: "complete" }],
      previousCursor: args.cursor ? "first-page" : null, nextCursor: args.cursor ? null : "second-page" });
  });
  render(<SessionImportHost locale="en" />);
  act(() => openImportedHistory({ importId: IMPORT, revision: NEW }));
  await screen.findByText("New imported body");
  const summary = screen.getByText("History versions", { selector: "summary" });
  await user.click(summary);
  const controls = within(summary.parentElement!);
  const trigger = await controls.findByRole("button", { name: "History versions" });
  await user.click(controls.getByRole("button", { name: "Next page" }));
  await controls.findByRole("alert");
  expect(trigger).toBeDisabled();
  expect(screen.getByText("New imported body")).toBeInTheDocument();
  await user.click(controls.getByRole("button", { name: "Retry version list" }));
  await waitFor(() => expect(trigger).toBeEnabled());
  expect(calls("session_import_revisions").map(([, args]) => args.cursor)).toEqual([null, "second-page", "second-page"]);
  expect(calls("session_import_events")).toHaveLength(1);
  await user.click(trigger);
  expect(within(screen.getByRole("option", { name: /Viewing.*00000002/ })).getByRole("button")).toBeDisabled();
  await user.click(within(screen.getByRole("option", { name: /00000001/ })).getByRole("button"));
  await screen.findByText("Old preserved body");
});

test("an unavailable latest archive does not prevent explicitly opening a preserved older version", async () => {
  const user = userEvent.setup();
  const originalInvoke = mocks.invoke.getMockImplementation()!;
  mocks.invoke.mockImplementation((command, args) => command === "session_import_events" && !args.revision
    ? Promise.reject(new Error("SNAPSHOT_UNAVAILABLE: latest body unavailable")) : originalInvoke(command, args));
  render(<SessionImportHost locale="en" />);
  act(() => openImportedHistory({ importId: IMPORT }));
  await screen.findByRole("alert");
  await user.click(screen.getByText("History versions", { selector: "summary" }));
  const trigger = await screen.findByRole("button", { name: "History versions" });
  expect(trigger).toBeEnabled();
  await user.click(trigger);
  await user.click(within(screen.getByRole("option", { name: /00000001/ })).getByRole("button"));
  await screen.findByText("Old preserved body");
  expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  expect(calls("session_import_events").at(-1)?.[1]).toMatchObject({ revision: OLD, leaf: null });
});
