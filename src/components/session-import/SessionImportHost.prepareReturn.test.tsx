/** @vitest-environment jsdom */
import { adoptionUiFixture } from "@/test/sessionImportAdoptionUiFixture";
import "@testing-library/jest-dom/vitest";
import { act, cleanup, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { SessionImportHost } from "./SessionImportHost";
import { groupCandidate, groupDeferred, groupMetadata } from "@/test/sessionImportGroupFixture";
import { openSessionImport } from "@/lib/sessionImport/open";
import { createT } from "@/i18n";

let fixture: ReturnType<typeof adoptionUiFixture>;
const tr = createT("en");
const scroll = Object.getOwnPropertyDescriptor(HTMLElement.prototype, "scrollIntoView");
beforeEach(() => {
  fixture = adoptionUiFixture();
  fixture.batch.total = 1;
  fixture.batch.prepared = 1;
  Object.defineProperty(HTMLElement.prototype, "scrollIntoView", { configurable: true, value: vi.fn() });
});
afterEach(() => {
  cleanup();
  if (scroll) Object.defineProperty(HTMLElement.prototype, "scrollIntoView", scroll);
  else Reflect.deleteProperty(HTMLElement.prototype, "scrollIntoView");
  window.history.replaceState(null, "", "/");
});
function row(id: string) {
  return document.querySelector<HTMLLIElement>(`[data-import-candidate='${groupMetadata(id).groupId}']`);
}
async function preparedB() {
  const user = userEvent.setup();
  render(<SessionImportHost locale="en" />);
  await act(async () => { openSessionImport({}); });
  await user.click(await screen.findByRole("button", { name: "Scan all defaults" }));
  await waitFor(() => expect(row("a")).not.toBeNull());
  await user.click(within(row("a")!).getByRole("button", { name: "Compare versions" }));
  const dialog = await screen.findByRole("dialog", { name: "Compare versions" });
  await user.click(await within(dialog).findByRole("button", { name: /Transcript · alternate/ }));
  await within(dialog).findByText("Position 1");
  await user.click(within(dialog).getByRole("button", { name: "Use version B" }));
  await waitFor(() => expect(screen.queryByRole("dialog", { name: "Compare versions" })).toBeNull());
  await user.click(within(row("a")!).getByRole("checkbox"));
  await user.click(screen.getByRole("button", { name: "Next" }));
  await screen.findByRole("button", { name: tr("sessionImport.prepare.continue") });
  return user;
}
async function reopenPrepared(user: ReturnType<typeof userEvent.setup>) {
  const wizard = screen.getByRole("dialog", { name: tr("sessionImport.title") });
  await user.click(within(wizard).getByRole("button", { name: "Close" }));
  expect(screen.queryByRole("dialog", { name: tr("sessionImport.title") })).toBeNull();
  await act(async () => { openSessionImport({}); });
  await screen.findByRole("button", { name: tr("sessionImport.prepare.continue") });
}

test("completed B prepare close/reopen/Back restores same-scan groups without reimport or implicit grants", async () => {
  const user = await preparedB();
  expect(fixture.calls("session_import_prepare_start")).toHaveLength(1);
  const scans = fixture.calls("session_import_scan_start").length;
  const previews = fixture.calls("session_import_preview").length;
  await reopenPrepared(user);
  await user.click(screen.getByRole("button", { name: "Back" }));
  await waitFor(() => expect(row("a")).not.toBeNull());
  expect(row("a")?.dataset.importPreviewCandidate).toBe(groupCandidate("alternate").candidateId);
  expect(within(row("b")!).getByRole("checkbox")).not.toBeChecked();
  expect(fixture.calls("session_import_scan_start")).toHaveLength(scans);
  expect(fixture.calls("session_import_preview")).toHaveLength(previews);
  expect(fixture.calls("session_import_prepare_start")).toHaveLength(1);
  expect(fixture.calls("session_import_commit")).toHaveLength(0);
  expect(document.querySelector(".session-import__preview-pane")).not.toHaveTextContent("fixed-one");
  await user.click(screen.getByRole("button", { name: "Refresh" }));
  await waitFor(() => expect(row("a")).not.toBeNull());
});

test("same-scan return read failure is visible and only explicit Refresh retries without another prepare", async () => {
  const user = await preparedB();
  await reopenPrepared(user);
  fixture.host.client.get.mockRejectedValueOnce(new Error("SOURCE_BUSY: synthetic return read"));
  const reads = fixture.host.client.get.mock.calls.length;
  await user.click(screen.getByRole("button", { name: "Back" }));
  await waitFor(() => expect(document.querySelector("[role='alert']")).not.toBeNull());
  expect(row("a")).toBeNull();
  expect(fixture.host.client.get).toHaveBeenCalledTimes(reads + 1);
  await user.click(screen.getByRole("button", { name: "Refresh" }));
  await waitFor(() => expect(row("a")).not.toBeNull());
  expect(row("a")?.dataset.importPreviewCandidate).toBe(groupCandidate("alternate").candidateId);
  expect(fixture.calls("session_import_prepare_start")).toHaveLength(1);
  expect(fixture.calls("session_import_commit")).toHaveLength(0);
});

test("a recovered prepare without a live scan returns to Source without discovering defaults", async () => {
  const invoke = fixture.ipc.invoke.getMockImplementation()!;
  fixture.ipc.invoke.mockImplementation((command, args) => command === "session_import_recovery"
    ? Promise.resolve({ prepare: fixture.batch, expired: false }) : invoke(command, args));
  const user = userEvent.setup();
  render(<SessionImportHost locale="en" />);
  await act(async () => { openSessionImport({}); });
  await screen.findByRole("button", { name: tr("sessionImport.prepare.continue") });
  await user.click(screen.getByRole("button", { name: "Back" }));
  await screen.findByRole("button", { name: "Scan all defaults" });
  expect(fixture.calls("session_import_scan_start")).toHaveLength(0);
  expect(fixture.calls("session_import_scan_groups")).toHaveLength(0);
  expect(fixture.calls("session_import_prepare_start")).toHaveLength(0);
  expect(fixture.calls("session_import_commit")).toHaveLength(0);
});

test.each(["success", "error"] as const)("close/reopen during return drops obsolete %s and safely rereads the current scan", async (outcome) => {
  const user = await preparedB();
  await reopenPrepared(user);
  const current = await fixture.host.client.get("synthetic-scan", 1);
  const pending = groupDeferred<typeof current>();
  fixture.host.client.get.mockReturnValueOnce(pending.promise);
  const reads = fixture.host.client.get.mock.calls.length;
  await user.click(screen.getByRole("button", { name: "Back" }));
  await waitFor(() => expect(fixture.host.client.get).toHaveBeenCalledTimes(reads + 1));
  const wizard = screen.getByRole("dialog", { name: tr("sessionImport.title") });
  await user.click(within(wizard).getByRole("button", { name: "Close" }));
  await act(async () => { openSessionImport({}); });
  expect(row("a")).toBeNull();
  await act(async () => {
    if (outcome === "success") pending.resolve(current);
    else pending.reject(new Error("SOURCE_BUSY: obsolete return read"));
  });
  await waitFor(() => expect(row("a")).not.toBeNull());
  expect(row("a")?.dataset.importPreviewCandidate).toBe(groupCandidate("alternate").candidateId);
  expect(document.querySelector("[role='alert']")).toBeNull();
  expect(fixture.calls("session_import_scan_start")).toHaveLength(1);
  expect(fixture.calls("session_import_prepare_start")).toHaveLength(1);
  expect(fixture.calls("session_import_commit")).toHaveLength(0);
});
