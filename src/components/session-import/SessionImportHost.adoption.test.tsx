/** @vitest-environment jsdom */
import { adoptionUiFixture } from "@/test/sessionImportAdoptionUiFixture";
import "@testing-library/jest-dom/vitest";
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { SessionImportHost } from "./SessionImportHost";
import { groupCandidate, groupDeferred, groupMetadata } from "@/test/sessionImportGroupFixture";
import { openSessionImport } from "@/lib/sessionImport/open";
import { fixedTransferPage } from "@/test/sessionImportFixedPreviewFixture";
import { createT } from "@/i18n";

let fixture: ReturnType<typeof adoptionUiFixture>;
const scroll = Object.getOwnPropertyDescriptor(HTMLElement.prototype, "scrollIntoView");
beforeEach(() => {
  fixture = adoptionUiFixture();
  Object.defineProperty(HTMLElement.prototype, "scrollIntoView", { configurable: true, value: vi.fn() });
});
afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
  if (scroll) Object.defineProperty(HTMLElement.prototype, "scrollIntoView", scroll);
  else Reflect.deleteProperty(HTMLElement.prototype, "scrollIntoView");
  window.history.replaceState(null, "", "/");
});
function row(id: string) {
  return document.querySelector<HTMLLIElement>(`[data-import-candidate='${groupMetadata(id).groupId}']`)!;
}
async function start(selected = true) {
  const user = userEvent.setup();
  render(<SessionImportHost locale="en" />);
  await act(async () => { openSessionImport({}); });
  await user.click(await screen.findByRole("button", { name: "Scan all defaults" }));
  await waitFor(() => expect(row("a")).toBeTruthy());
  if (selected) await user.click(screen.getByRole("button", { name: "Select visible" }));
  return user;
}
async function compare(user: ReturnType<typeof userEvent.setup>) {
  await user.click(within(row("a")).getByRole("button", { name: "Compare versions" }));
  const dialog = await screen.findByRole("dialog", { name: "Compare versions" });
  await user.click(await within(dialog).findByRole("button", { name: /Transcript · alternate/ }));
  await within(dialog).findByText("Position 1");
  return dialog;
}

test("actual Host adopts compared B and sends its exact fixed content to native prepare without live repreview", async () => {
  const user = await start(), dialog = await compare(user);
  const button = within(dialog).queryByRole("button", { name: "Use version B" });
  expect(button).not.toBeNull();
  const reads = fixture.calls("session_import_preview").length;
  const invoke = fixture.ipc.invoke.getMockImplementation()!;
  fixture.ipc.invoke.mockImplementation((command, args) => command === "session_import_preview"
    ? Promise.reject(new Error("SOURCE_CHANGED: source now offline")) : invoke(command, args));
  await user.click(button!);
  await waitFor(() => expect(screen.queryByRole("dialog", { name: "Compare versions" })).toBeNull());
  expect(row("a").dataset.importPreviewCandidate).toBe(groupCandidate("alternate").candidateId);
  expect(fixture.calls("session_import_preview")).toHaveLength(reads);
  expect(fixture.calls("session_import_prepare_start")).toHaveLength(0);
  expect(fixture.ipc.choose).toHaveBeenCalledTimes(1);
  await user.click(screen.getByRole("button", { name: "Next" }));
  await waitFor(() => expect(fixture.calls("session_import_prepare_start")).toHaveLength(1));
  expect(fixture.calls("session_import_prepare_start")[0][1]).toMatchObject({
    scanId: "synthetic-scan", epoch: 1, selectionId: "synthetic-selector",
    overrides: [{ candidateId: groupCandidate("alternate").candidateId,
      previewId: "fixed-preview-alternate", leafId: "leaf-alternate",
      selection: { eventIds: ["fixed-one", "fixed-two"], assetIds: [], pinnedEventIds: [], includeSubagents: false } }],
  });
  await waitFor(() => expect(document.querySelector(".session-import-wizard--prepare")).toBeTruthy());
  expect(fixture.calls("session_import_prepare_cancel")).toHaveLength(0);
  expect(fixture.calls("session_import_commit")).toHaveLength(0);
});

async function edit(user: ReturnType<typeof userEvent.setup>, id: string, grants = false) {
  await user.click(within(row(id)).getByRole("button", { name: "Preview" }));
  const pane = document.querySelector<HTMLElement>(".session-import__preview-pane")!;
  await within(pane).findByText("fixed-one");
  await user.click(within(pane).getAllByRole("checkbox", { name: "User" })[0]);
  if (grants) {
    await user.click(pane.querySelector<HTMLButtonElement>(".session-import__options-toggle")!);
    const tr = createT("en");
    await user.click(within(pane).getByRole("checkbox", { name: tr("sessionImport.includeReasoning") }));
    await user.click(within(pane).getByRole("checkbox", { name: tr("sessionImport.includeSubagents") }));
  }
  return pane;
}
async function adopted(user: ReturnType<typeof userEvent.setup>, dialog: HTMLElement, side = "B") {
  await user.click(within(dialog).getByRole("button", { name: `Use version ${side}` }));
  await waitFor(() => expect(screen.queryByRole("dialog", { name: "Compare versions" })).toBeNull());
}

test.each(["A", "B"])("actual Host adoption of %s does not implicitly select its group or prepare", async (side) => {
  const user = await start(false), dialog = await compare(user);
  await adopted(user, dialog, side);
  expect(within(row("a")).getByRole("checkbox")).not.toBeChecked();
  expect(row("a").dataset.importPreviewCandidate).toBe(groupCandidate(side === "A" ? "a" : "alternate").candidateId);
  expect(screen.getByRole("button", { name: "Next" })).toBeDisabled();
  expect(fixture.calls("session_import_prepare_start")).toHaveLength(0);
  expect(fixture.calls("session_import_commit")).toHaveLength(0);
});

test("A adoption uses independently read primary metadata, including persisted same-member no-op", async () => {
  const user = await start(false);
  await adopted(user, await compare(user), "A");
  const before = await fixture.host.client.get("synthetic-scan", 1);
  await adopted(user, await compare(user), "A");
  const after = await fixture.host.client.get("synthetic-scan", 1);
  expect(after).toEqual(before);
  expect(fixture.ipc.choose).toHaveBeenCalledTimes(2);
  expect(fixture.calls("session_import_scan_group_member")).toHaveLength(2);
});

test("adopted B can explicitly compare back to A using the new primary, not the old dialog owner", async () => {
  const user = await start(false);
  await adopted(user, await compare(user));
  await user.click(within(row("a")).getByRole("button", { name: "Compare versions" }));
  const dialog = await screen.findByRole("dialog", { name: "Compare versions" });
  const versions = dialog.querySelector<HTMLElement>(".session-import__comparison-versions")!;
  await user.click((await within(versions).findByText("/synthetic/a.jsonl")).closest("button")!);
  await within(dialog).findByText("Position 1");
  await adopted(user, dialog);
  expect(row("a").dataset.importPreviewCandidate).toBe(groupCandidate("a").candidateId);
  expect(fixture.ipc.choose).toHaveBeenCalledTimes(2);
  expect(fixture.ipc.choose.mock.calls.at(-1)?.[0]).toMatchObject({ expectedStateVersion: 2 });
});

test("selected non-default comparison leaf is adopted exactly and sent to prepare", async () => {
  const user = await start();
  const dialog = await compare(user), side = dialog.querySelectorAll<HTMLElement>(".session-import__comparison-source")[1];
  await user.click(within(side).getByRole("button", { name: "Branch" }));
  const option = await screen.findByRole("option", { name: "branch-alternate" });
  await user.click(within(option).getByRole("button"));
  await waitFor(() => expect(within(side).getByRole("button", { name: "Use version B" })).toBeEnabled());
  await adopted(user, dialog);
  expect(fixture.ipc.events).toHaveBeenLastCalledWith(expect.objectContaining({ leaf: "branch-alternate" }));
  await user.click(screen.getByRole("button", { name: "Next" }));
  await waitFor(() => expect(fixture.calls("session_import_prepare_start")).toHaveLength(1));
  expect(fixture.calls("session_import_prepare_start")[0][1].overrides[0].leafId).toBe("branch-alternate");
});

test("adoption that removes its filtered old row does not restore focus to a disconnected trigger", async () => {
  const user = await start();
  await user.type(screen.getByRole("searchbox", { name: createT("en")("sessionImport.filterSessions") }), "a.jsonl");
  const trigger = within(row("a")).getByRole("button", { name: "Compare versions" });
  await adopted(user, await compare(user));
  expect(trigger.isConnected).toBe(false);
  expect(document.activeElement?.isConnected).toBe(true);
  expect(screen.getByRole("dialog", { name: "Import sessions and workspaces" })).toContainElement(document.activeElement as HTMLElement);
});

test("same-target edited A uses fresh defaults after consent, never inherits old grants", async () => {
  const user = await start(); await edit(user, "a", true);
  fixture.batch.selectionStateVersion = 2;
  const dialog = await compare(user);
  await user.click(within(dialog).getByRole("button", { name: "Use version A" }));
  await user.click(within(await screen.findByRole("dialog", { name: "Replace edited selection?" })).getByRole("button", { name: "Use fresh defaults" }));
  await waitFor(() => expect(screen.queryByRole("dialog", { name: "Compare versions" })).toBeNull());
  await user.click(screen.getByRole("button", { name: "Next" }));
  await waitFor(() => expect(fixture.calls("session_import_prepare_start")).toHaveLength(1));
  expect(fixture.calls("session_import_prepare_start")[0][1].overrides).toEqual([expect.objectContaining({
    candidateId: groupCandidate("a").candidateId, previewId: "fixed-preview-a", leafId: "leaf-a",
    selection: expect.objectContaining({ eventIds: ["fixed-one", "fixed-two"], includeSubagents: false,
      fields: expect.objectContaining({ visibleReasoning: false }), pinnedEventIds: [], assetIds: [] }),
  })]);
});

test("dirty consent cancel preserves edits and dispatches zero fixed reads or member commands", async () => {
  const user = await start();
  await edit(user, "a");
  const dialog = await compare(user), reads = fixture.ipc.events.mock.calls.length;
  await user.click(within(dialog).getByRole("button", { name: "Use version B" }));
  const consent = await screen.findByRole("dialog", { name: "Replace edited selection?" });
  expect(within(dialog).getByRole("button", { name: "Use version A" })).toBeDisabled();
  expect(screen.getByRole("button", { name: "Next" })).toBeDisabled();
  await user.click(within(consent).getByRole("button", { name: "Cancel" }));
  expect(fixture.ipc.events).toHaveBeenCalledTimes(reads);
  expect(fixture.ipc.choose).not.toHaveBeenCalled();
  await user.keyboard("{Escape}");
  const pane = document.querySelector<HTMLElement>(".session-import__preview-pane")!;
  expect(within(pane).getAllByRole("checkbox", { name: "User" })[0]).not.toBeChecked();
});

test("dirty confirm resets target defaults, excludes old review-only edits and retains unrelated edited overrides", async () => {
  const user = await start();
  await edit(user, "b"); await edit(user, "a", true);
  const dialog = await compare(user);
  const reads = fixture.calls("session_import_preview").length;
  await user.click(within(dialog).getByRole("button", { name: "Use version B" }));
  const consent = await screen.findByRole("dialog", { name: "Replace edited selection?" });
  await user.click(within(consent).getByRole("button", { name: "Use fresh defaults" }));
  await waitFor(() => expect(screen.queryByRole("dialog", { name: "Compare versions" })).toBeNull());
  await user.click(screen.getByRole("button", { name: "Next" }));
  await waitFor(() => expect(fixture.calls("session_import_prepare_start")).toHaveLength(1));
  const overrides = fixture.calls("session_import_prepare_start")[0][1].overrides;
  expect(overrides).toEqual([
    expect.objectContaining({ candidateId: groupCandidate("b").candidateId, selection: expect.objectContaining({ eventIds: ["fixed-two"] }) }),
    expect.objectContaining({ candidateId: groupCandidate("alternate").candidateId, selection: expect.objectContaining({
      eventIds: ["fixed-one", "fixed-two"], assetIds: [], pinnedEventIds: [], includeSubagents: false,
      fields: expect.objectContaining({ visibleReasoning: false }),
    }) }),
  ]);
  expect(fixture.calls("session_import_preview")).toHaveLength(reads);
  expect(fixture.calls("session_import_commit")).toHaveLength(0);
});

test("owned ACK/read gap keeps the same dialog mounted and disables all comparison mutations", async () => {
  const user = await start(), dialog = await compare(user);
  const membership = fixture.ipc.membership.getMockImplementation()!, pending = groupDeferred<unknown>();
  fixture.ipc.membership.mockReturnValueOnce(pending.promise);
  await user.click(within(dialog).getByRole("button", { name: "Use version B" }));
  await waitFor(() => expect(fixture.ipc.membership.mock.results.at(-1)?.value).toBe(pending.promise));
  expect(screen.getByRole("dialog", { name: "Compare versions" })).toBe(dialog);
  expect(within(dialog).getByRole("button", { name: "Use version B" })).toBeDisabled();
  expect(within(dialog).getByRole("button", { name: /Transcript · alternate/ })).toBeDisabled();
  for (const button of within(dialog).getAllByRole("button", { name: "Read full fields" })) expect(button).toBeDisabled();
  for (const button of within(dialog).getAllByRole("button", { name: "Branch" })) expect(button).toBeDisabled();
  expect(screen.getByRole("button", { name: "Next" })).toBeDisabled();
  await act(async () => { pending.resolve(await membership("synthetic-scan", 1, "synthetic-selector", [groupMetadata("a").groupId, groupMetadata("b").groupId])); });
  await waitFor(() => expect(screen.queryByRole("dialog", { name: "Compare versions" })).toBeNull());
});

test("accepted read failure exposes repeated read-only retry without new CAS or live previews", async () => {
  const user = await start(), dialog = await compare(user);
  fixture.ipc.membership.mockRejectedValueOnce(new Error("SOURCE_BUSY: first post-ACK read offline"));
  const reads = fixture.calls("session_import_preview").length;
  await user.click(within(dialog).getByRole("button", { name: "Use version B" }));
  const retry = await within(dialog).findByRole("button", { name: "Retry content read" });
  expect(screen.getByRole("dialog", { name: "Compare versions" })).toBe(dialog);
  expect(within(dialog).queryByRole("button", { name: "Compare again" })).toBeNull();
  fixture.ipc.membership.mockRejectedValueOnce(new Error("SOURCE_BUSY: second read offline"));
  await user.click(retry);
  const secondRetry = await within(dialog).findByRole("button", { name: "Retry content read" });
  await waitFor(() => expect(secondRetry).toBeEnabled());
  expect(fixture.ipc.choose).toHaveBeenCalledTimes(1);
  await user.click(secondRetry);
  await waitFor(() => expect(screen.queryByRole("dialog", { name: "Compare versions" })).toBeNull());
  expect(fixture.ipc.choose).toHaveBeenCalledTimes(1);
  expect(fixture.ipc.events).toHaveBeenCalledTimes(3);
  expect(fixture.calls("session_import_preview")).toHaveLength(reads);
});

test("dirty accepted-read retry still requires consent and cancel makes no recovery calls", async () => {
  const user = await start(); await edit(user, "a");
  const dialog = await compare(user);
  fixture.ipc.membership.mockRejectedValueOnce(new Error("SOURCE_BUSY: post-ACK read offline"));
  await user.click(within(dialog).getByRole("button", { name: "Use version B" }));
  await user.click(within(await screen.findByRole("dialog", { name: "Replace edited selection?" })).getByRole("button", { name: "Use fresh defaults" }));
  const retry = await within(dialog).findByRole("button", { name: "Retry content read" });
  const reads = fixture.ipc.events.mock.calls.length;
  await user.click(retry);
  await user.click(within(await screen.findByRole("dialog", { name: "Replace edited selection?" })).getByRole("button", { name: "Cancel" }));
  expect(fixture.ipc.events).toHaveBeenCalledTimes(reads);
  expect(fixture.ipc.choose).toHaveBeenCalledTimes(1);
  await user.click(retry);
  await user.click(within(await screen.findByRole("dialog", { name: "Replace edited selection?" })).getByRole("button", { name: "Use fresh defaults" }));
  await waitFor(() => expect(screen.queryByRole("dialog", { name: "Compare versions" })).toBeNull());
  expect(fixture.ipc.choose).toHaveBeenCalledTimes(1);
});

test("lost acknowledgement cannot be replayed and requires explicit fresh comparison", async () => {
  const user = await start(), dialog = await compare(user);
  fixture.ipc.choose.mockRejectedValueOnce(new Error("lost synthetic response"));
  await user.click(within(dialog).getByRole("button", { name: "Use version B" }));
  const recompare = await within(dialog).findByRole("button", { name: "Compare again" });
  expect(within(dialog).getByRole("button", { name: "Use version B" })).toBeDisabled();
  expect(within(dialog).queryByRole("button", { name: "Retry content read" })).toBeNull();
  const reads = fixture.calls("session_import_preview").length;
  await user.click(recompare);
  await waitFor(() => expect(screen.getByRole("dialog", { name: "Compare versions" })).not.toBe(dialog));
  expect(fixture.calls("session_import_preview").length).toBe(reads + 1);
  expect(fixture.ipc.choose).toHaveBeenCalledTimes(1);
});

test("another native writer invalidates accepted authority without adopting old content", async () => {
  const user = await start(), dialog = await compare(user);
  const membership = fixture.ipc.membership.getMockImplementation()!;
  fixture.ipc.membership.mockImplementationOnce(async (...args) => {
    await fixture.host.client.set({}, [groupMetadata("b").groupId], false);
    return membership(...args);
  });
  await user.click(within(dialog).getByRole("button", { name: "Use version B" }));
  await within(dialog).findByRole("button", { name: "Compare again" });
  expect(within(dialog).queryByRole("button", { name: "Retry content read" })).toBeNull();
  expect(fixture.ipc.choose).toHaveBeenCalledTimes(1);
  expect(document.querySelector(".session-import__preview-pane .session-import__detail")).toBeNull();
  expect(fixture.calls("session_import_prepare_start")).toHaveLength(0);
});

test("expired fixed snapshot requires explicit recompare, never implicit live fallback", async () => {
  const user = await start(), dialog = await compare(user);
  fixture.ipc.events.mockRejectedValueOnce(new Error("PLAN_STALE: fixed snapshot expired"));
  const reads = fixture.calls("session_import_preview").length;
  await user.click(within(dialog).getByRole("button", { name: "Use version B" }));
  await within(dialog).findByRole("button", { name: "Compare again" });
  expect(within(dialog).getByRole("button", { name: "Use version B" })).toBeDisabled();
  expect(fixture.ipc.choose).not.toHaveBeenCalled();
  expect(fixture.calls("session_import_preview")).toHaveLength(reads);
});

test("invalid native adoption summary is refused without a command and offers explicit recompare", async () => {
  const user = await start(), invoke = fixture.ipc.invoke.getMockImplementation()!;
  fixture.ipc.invoke.mockImplementation(async (command, args) => {
    const result = await invoke(command, args);
    if (command === "session_import_source_comparison") result.right.snapshotDigest = "invalid-fixed-digest";
    return result;
  });
  const dialog = await compare(user);
  await user.click(within(dialog).getByRole("button", { name: "Use version B" }));
  await within(dialog).findByRole("button", { name: "Compare again" });
  expect(within(dialog).getByRole("button", { name: "Use version B" })).toBeDisabled();
  expect(fixture.ipc.events).not.toHaveBeenCalled();
  expect(fixture.ipc.choose).not.toHaveBeenCalled();
});

test("before-render double adoption dispatches a single preflight and zero automatic prepare", async () => {
  const user = await start(), dialog = await compare(user), pending = groupDeferred<ReturnType<typeof fixedTransferPage>>();
  fixture.ipc.events.mockReturnValueOnce(pending.promise);
  const button = within(dialog).getByRole("button", { name: "Use version B" });
  const next = screen.getByRole("button", { name: "Next" });
  act(() => { button.click(); button.click(); next.click(); });
  expect(fixture.ipc.events).toHaveBeenCalledTimes(1);
  expect(fixture.ipc.choose).not.toHaveBeenCalled();
  await act(async () => { pending.resolve(fixedTransferPage()); });
  await waitFor(() => expect(screen.queryByRole("dialog", { name: "Compare versions" })).toBeNull());
  expect(fixture.ipc.choose).toHaveBeenCalledTimes(1);
  expect(fixture.calls("session_import_prepare_start")).toHaveLength(0);
});

test("IME does not activate adoption and clipboard denial keeps localized visible feedback", async () => {
  const user = await start(), dialog = await compare(user), button = within(dialog).getByRole("button", { name: "Use version B" });
  expect(fireEvent.keyDown(button, { key: "Enter", isComposing: true, keyCode: 229 })).toBe(false);
  expect(fixture.ipc.events).not.toHaveBeenCalled();
  fixture.ipc.events.mockRejectedValueOnce(new Error("SOURCE_BUSY: synthetic read offline"));
  await user.click(button);
  await within(dialog).findByText("Could not apply this version.");
  vi.spyOn(navigator.clipboard, "writeText").mockRejectedValueOnce(new Error("denied"));
  await user.click(within(dialog).getByRole("button", { name: "Copy text" }));
  await within(dialog).findByText(createT("en")("sessionImport.clipboardFailed"));
  expect(fixture.ipc.choose).not.toHaveBeenCalled();
});

test("successful adoption reveals the narrow preview without focusing its hidden old trigger", async () => {
  vi.stubGlobal("ResizeObserver", class {
    constructor(private callback: (entries: { contentRect: { width: number } }[]) => void) {}
    observe() { this.callback([{ contentRect: { width: 600 } }]); }
    disconnect() {}
  });
  const user = await start(), dialog = await compare(user);
  await adopted(user, dialog);
  const list = document.querySelector<HTMLElement>(".session-import__candidates-pane")!;
  const preview = document.querySelector<HTMLElement>(".session-import__preview-pane")!;
  expect(list).toHaveAttribute("hidden");
  expect(preview).not.toHaveAttribute("hidden");
  expect(within(preview).getByText("fixed-one")).toBeVisible();
  expect(document.activeElement?.isConnected).toBe(true);
  expect(list.contains(document.activeElement)).toBe(false);
});

const closeCases = (["read", "command", "membership"] as const).flatMap((boundary) =>
  (["Escape", "overlay", "wizard", "source", "unmount"] as const).flatMap((exit) =>
    (["success", "failure"] as const).map((outcome) => ({ boundary, exit, outcome }))),
);
test.each(closeCases)("close via $exit during $boundary prevents late $outcome from reviving dialog/content/prepare", async ({ boundary, exit, outcome }) => {
  const user = await start(), dialog = await compare(user), pending = groupDeferred<unknown>();
  const mock = boundary === "read" ? fixture.ipc.events : boundary === "command" ? fixture.ipc.choose : fixture.ipc.membership;
  const original = mock.getMockImplementation()!;
  mock.mockImplementationOnce(async (...args) => { const response = await original(...args); await pending.promise; return response; });
  await user.click(within(dialog).getByRole("button", { name: "Use version B" }));
  await waitFor(() => expect(mock.mock.results.at(-1)?.type).toBe("return"));
  if (exit === "Escape") await user.keyboard("{Escape}");
  else if (exit === "overlay") fireEvent.mouseDown(dialog.parentElement!);
  else if (exit === "wizard") fireEvent.click(document.querySelector<HTMLButtonElement>(".session-import-wizard .modal-close")!);
  else if (exit === "source") fireEvent.click(document.querySelector<HTMLButtonElement>("[data-import-back='content']")!);
  else cleanup();
  expect(screen.queryByRole("dialog", { name: "Compare versions" })).toBeNull();
  await act(async () => {
    if (outcome === "success") pending.resolve(null);
    else pending.reject(new Error("obsolete-synthetic-failure"));
  });
  await waitFor(() => expect(screen.queryByRole("dialog", { name: "Compare versions" })).toBeNull());
  expect(document.querySelector(".session-import__preview-pane .session-import__detail")).toBeNull();
  expect(fixture.ipc.choose).toHaveBeenCalledTimes(boundary === "read" ? 0 : 1);
  expect(fixture.calls("session_import_prepare_start")).toHaveLength(0);
  expect(fixture.calls("session_import_commit")).toHaveLength(0);
});
