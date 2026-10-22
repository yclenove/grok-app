/** @vitest-environment jsdom */
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { ImportedWorkspaceAssociation } from "./ImportedWorkspaceAssociation";

const mocks = vi.hoisted(() => ({ get: vi.fn(), options: vi.fn(), update: vi.fn() }));
vi.mock("@/lib/api/sessionWorkspaceAssociation", () => ({
  importedWorkspaceGet: mocks.get, importedWorkspaceOptions: mocks.options, importedWorkspaceUpdate: mocks.update,
}));
const choice = { sourceWorkspaceId: "ws:one", revision: "a".repeat(64), displayName: "Example workspace" };
const record = { importId: "imp:one", sourceId: "source", provider: "codex", version: 0, sourceReady: true, status: "unassigned", choice: null };
afterEach(() => { cleanup(); vi.resetAllMocks(); });
beforeEach(() => {
  mocks.get.mockResolvedValue(record);
  mocks.options.mockResolvedValue({ items: [choice], nextCursor: null });
  mocks.update.mockImplementation(async (input) => ({ ...record, choice: input.choice, version: input.expectedVersion + 1, status: input.choice ? "assigned" : "unassigned" }));
});
function open() {
  render(<ImportedWorkspaceAssociation importId="imp:one" locale="en" />);
  fireEvent.click(screen.getByText("Workspace", { selector: "summary" }));
}

test("keyboard selection, explicit save and clear use shared controls", async () => {
  const user = userEvent.setup();
  open();
  const select = await screen.findByRole("button", { name: "Workspace" });
  select.focus();
  await user.keyboard("{Enter}");
  const option = await screen.findByRole("option", { name: choice.displayName });
  within(option).getByRole("button").focus();
  await user.keyboard("{Enter}");
  expect(mocks.update).not.toHaveBeenCalled();
  await user.click(screen.getByRole("button", { name: "Save" }));
  await screen.findByText("Workspace link saved.");
  expect(mocks.update.mock.calls[0][0].choice).toEqual(choice);
  expect(screen.getByText(`Workspace · ${choice.displayName}`, { selector: "summary" })).toBeTruthy();
  await user.click(screen.getByRole("button", { name: "Clear link" }));
  await waitFor(() => expect(screen.queryByRole("button", { name: "Clear link" })).toBeNull());
  expect(mocks.update.mock.calls[1][0].choice).toBeNull();
  expect(document.querySelector("select")).toBeNull();
});

test("empty, failure and retry paths remain actionable", async () => {
  mocks.options.mockRejectedValueOnce("SOURCE_BUSY: locked").mockResolvedValue({ items: [], nextCursor: null });
  open();
  await screen.findByRole("alert");
  fireEvent.click(screen.getByRole("button", { name: "Refresh" }));
  await screen.findByText("No saved workspaces for this source.");
  expect(screen.queryByRole("alert")).toBeNull();
  expect(mocks.update).not.toHaveBeenCalled();
});

test("same-name workspaces expose their source location before saving", async () => {
  const first = { ...choice, sourceLocator: "folder:/machine/one" };
  const second = { ...choice, sourceWorkspaceId: "ws:two", sourceLocator: "folder:/machine/two" };
  mocks.options.mockResolvedValue({ items: [first, second], nextCursor: null });
  open();
  fireEvent.click(await screen.findByRole("button", { name: "Workspace" }));
  const option = await screen.findByRole("option", { name: /folder:\/machine\/two/ });
  expect(screen.getByRole("option", { name: /folder:\/machine\/one/ })).toBeTruthy();
  fireEvent.click(within(option).getByRole("button"));
  fireEvent.click(screen.getByRole("button", { name: "Save" }));
  await screen.findByText("Workspace link saved.");
  expect(mocks.update.mock.calls[0][0].choice).toEqual(second);
});

test("concurrent edit errors direct the user to refresh the workspace selection", async () => {
  mocks.update.mockRejectedValue("PLAN_STALE: workspace association changed");
  open();
  fireEvent.click(await screen.findByRole("button", { name: "Save" }));
  const error = await screen.findByRole("alert");
  expect(error.textContent).toBe("Workspace mappings changed. Refresh and save to confirm.");
  expect(screen.queryByText("Plan expired — preview again")).toBeNull();
});

test("long paths stay out of the selected name and remain readable before saving", async () => {
  const located = { ...choice, sourceLocator: `folder:C:/Users/Example/${"deep/".repeat(20)}project` };
  mocks.options.mockResolvedValue({ items: [located], nextCursor: null });
  open();
  fireEvent.click(await screen.findByRole("button", { name: "Workspace" }));
  const option = await screen.findByRole("option", { name: choice.displayName });
  fireEvent.click(within(option).getByRole("button"));
  const select = screen.getByRole("button", { name: "Workspace" });
  expect(select.textContent).toBe(choice.displayName);
  expect(screen.getByText(located.sourceLocator, { selector: "p" })).toBeTruthy();
  expect(mocks.update).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "Save" }));
  await screen.findByText("Workspace link saved.");
  expect(mocks.update.mock.calls[0][0].choice).toEqual(located);
});

test("missing saved workspace remains visible and can be cleared with unresolved source", async () => {
  mocks.get.mockResolvedValue({ ...record, choice, version: 2, sourceReady: false, status: "missing" });
  open();
  await screen.findByText("This workspace is unavailable. Choose another or clear the link.");
  expect(screen.getByText(`Workspace · ${choice.displayName}`, { selector: "summary" })).toBeTruthy();
  expect(screen.queryByRole("button", { name: "Save" })).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Clear link" }));
  await waitFor(() => expect(mocks.update).toHaveBeenCalledTimes(1));
  expect(mocks.update.mock.calls[0][0]).toMatchObject({ expectedVersion: 2, choice: null });
});

test("loading more keeps selection, blocks duplicate actions and reader shortcuts stay independent", async () => {
  let resolve!: (value: unknown) => void;
  mocks.options.mockResolvedValueOnce({ items: [choice], nextCursor: "next" })
    .mockReturnValueOnce(new Promise((done) => { resolve = done; }));
  const shortcut = vi.fn();
  render(<div onKeyDown={shortcut}><ImportedWorkspaceAssociation importId="imp:one" locale="en" /></div>);
  fireEvent.click(screen.getByText("Workspace", { selector: "summary" }));
  const select = await screen.findByRole("button", { name: "Workspace" });
  fireEvent.keyDown(select, { key: "n" });
  expect(shortcut).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "Load more" }));
  expect((screen.getByRole("button", { name: "Save" }) as HTMLButtonElement).disabled).toBe(true);
  fireEvent.click(screen.getByRole("button", { name: "Load more" }));
  expect(mocks.options).toHaveBeenCalledTimes(2);
  resolve({ items: [], nextCursor: null });
  await waitFor(() => expect(screen.queryByRole("button", { name: "Load more" })).toBeNull());
  expect((screen.getByRole("button", { name: "Save" }) as HTMLButtonElement).disabled).toBe(false);
});
