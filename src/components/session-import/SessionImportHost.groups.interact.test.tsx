/** @vitest-environment jsdom */
import { act, cleanup, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { groupCandidate, groupClient, groupDeferred, groupMetadata, groupScope } from "@/test/sessionImportGroupFixture";
import type { ScanState } from "@/lib/api/sessionMigration";
import type { GroupSelectionMembership, SourceGroupMemberPage } from "@/lib/api/sessionMigrationGroups";
import { openSessionImport } from "@/lib/sessionImport/open";
import { SessionImportHost } from "./SessionImportHost";
import type { SourceComparisonRequest } from "@/lib/api/sessionMigrationComparison";
import { comparisonPage } from "@/test/sessionImportComparisonFixture";
import { previewComparisonResponse } from "@/test/sessionImportComparisonPreviewFixture";

const ipc = vi.hoisted(() => ({ invoke: vi.fn(), members: vi.fn() }));
vi.mock("@/lib/api/host", async (original) => ({ ...await original<typeof import("@/lib/api/host")>(), invoke: ipc.invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn().mockResolvedValue(() => {}) }));
let host: ReturnType<typeof groupClient>;
let scan: ScanState;
const originalScroll = Object.getOwnPropertyDescriptor(HTMLElement.prototype, "scrollIntoView");
beforeEach(() => {
  vi.clearAllMocks();
  host = groupClient();
  ipc.members.mockReset().mockResolvedValue(memberPage());
  scan = { scanId: groupScope.scanId, epoch: 1, state: "completed", stateVersion: 1, candidates: [],
    discoveredCount: 3, complete: true, issueCount: 0, extraCount: 0,
    selection: { version: 1, selectionId: "legacy-selector", scanId: groupScope.scanId,
      epoch: 1, mode: "explicit", selectedCount: 0, stateVersion: 1 } };
  ipc.invoke.mockImplementation(async (command: string, args: Record<string, unknown> = {}) => {
    switch (command) {
      case "session_import_recovery": return { expired: false };
      case "session_import_entries": return { items: [], nextCursor: null, total: 0, providers: [] };
      case "session_import_list_roots": case "session_import_sources": case "projects_list": return [];
      case "session_import_scan_start": case "session_import_scan_get": case "session_import_scan_cancel": return scan;
      case "session_import_scan_page": return { candidates: [groupCandidate("a"), groupCandidate("alternate"), groupCandidate("b")],
        nextCursor: null, complete: true, discoveredCount: 3 };
      case "session_import_scan_selection_membership": return { selectionId: scan.selection.selectionId,
        stateVersion: scan.selection.stateVersion, selectedIds: [] };
      case "session_import_group_selection_get": return host.client.get(groupScope.scanId, 1);
      case "session_import_scan_groups": return host.client.groups(groupScope.scanId, 1);
      case "session_import_group_selection_membership": return host.client.membership(groupScope.scanId, 1,
        String(args.selectionId), args.groupIds as string[]);
      case "session_import_group_selection_set": return host.client.set(args, args.groupIds as string[], Boolean(args.selected));
      case "session_import_group_selection_clear": return host.client.clear(args);
      case "session_import_group_selection_choose": return host.client.choose(args, String(args.groupId), String(args.candidateId));
      case "session_import_scan_group_members": return ipc.members(args);
      case "session_import_scan_group_member": return memberPage().members.find((member) => member.candidate.candidateId === args.candidateId);
      case "session_import_preview": return { previewId: `preview-${args.candidateId}`, adapter: {
        outcome: "complete", activeLeaf: "leaf", graph: { events: [{}] },
        inventory: { imported: { messages: 1, toolCalls: 0, attachments: 0 } },
      } };
      case "session_import_preview_events": return { events: [{ eventId: "event-1", kind: "message", role: "user",
        text: `body-${args.previewId}`, selectable: true, defaultSelected: true }], nextCursor: null };
      default: throw new Error(`Unexpected synthetic IPC: ${command}`);
    }
  });
  Object.defineProperty(HTMLElement.prototype, "scrollIntoView", { configurable: true, value: vi.fn() });
});
afterEach(() => {
  cleanup();
  if (originalScroll) Object.defineProperty(HTMLElement.prototype, "scrollIntoView", originalScroll);
  else Reflect.deleteProperty(HTMLElement.prototype, "scrollIntoView");
  window.history.replaceState(null, "", "/");
});

test("real Host opens read-only A/B comparison without selecting or preparing an import, then restores its trigger", async () => {
  const user = await start();
  const original = ipc.invoke.getMockImplementation()!;
  ipc.invoke.mockImplementation(async (command: string, args: Record<string, unknown> = {}) => {
    if (command === "session_import_preview") {
      const id = String(args.candidateId).split(":").at(-1)!;
      const response = previewComparisonResponse(id);
      if (id === "a") response.adapter.eventCount = 3;
      return response;
    }
    if (command === "session_import_source_comparison") {
      const request = args.request as SourceComparisonRequest;
      const page = comparisonPage(request);
      page.left.groupId = groupMetadata("a").groupId;
      page.right.groupId = groupMetadata("a").groupId;
      return page;
    }
    return original(command, args);
  });
  const trigger = within(row("a")).getByRole("button", { name: "Compare versions" });
  const checkbox = within(row("a")).getByRole("checkbox");
  expect((checkbox as HTMLInputElement).checked).toBe(false);
  await user.click(trigger);
  const dialog = await screen.findByRole("dialog", { name: "Compare versions" });
  await user.click(await within(dialog).findByRole("button", { name: /Transcript · alternate/ }));
  await within(dialog).findByText("Position 1");
  expect(ipc.invoke.mock.calls.filter(([command]) => command === "session_import_scan_group_member")).toHaveLength(1);
  expect(dialog.querySelector(".session-import__comparison-source")?.textContent).toContain("/synthetic/root");
  expect(host.client.choose).not.toHaveBeenCalled();
  expect((checkbox as HTMLInputElement).checked).toBe(false);
  expect(ipc.invoke.mock.calls.some(([command]) => ["session_import_plan", "session_import_prepare_batch_start", "session_import_commit"].includes(command))).toBe(false);
  await user.keyboard("{Escape}");
  await waitFor(() => expect(screen.queryByRole("dialog", { name: "Compare versions" })).toBeNull());
  expect(document.activeElement).toBe(trigger);
});
function row(id: string) {
  return document.querySelector<HTMLLIElement>(`[data-import-candidate='${groupMetadata(id).groupId}']`)!;
}
function memberPage(ids = ["a", "alternate"], nextCursor: string | null = null, group = groupMetadata("a")): SourceGroupMemberPage {
  return { version: 1, groupId: group.groupId, memberCount: 2, complete: true, stateVersion: 1, nextCursor,
    members: ids.map((id) => ({ groupId: group.groupId, format: "cursor-transcript", sourceId: group.sourceId,
      rootPath: "/synthetic/root", legacyId: `transcript:${id}`, location: null,
      candidate: { ...groupCandidate(id), nativeSessionId: group.nativeSessionId } })) };
}
async function start() {
  const user = userEvent.setup();
  render(<SessionImportHost locale="en" />);
  await act(async () => { openSessionImport({}); });
  await user.click(await screen.findByRole("button", { name: "Scan all defaults" }));
  await waitFor(() => expect(row("a")).toBeTruthy());
  return user;
}

test("real Host keeps stable group DOM and disables stale selection between CAS ACK and membership", async () => {
  const user = await start();
  const original = row("a");
  const pending = groupDeferred<GroupSelectionMembership>();
  host.client.membership.mockReturnValueOnce(pending.promise);
  await user.click(screen.getByRole("button", { name: "Select visible" }));
  expect(row("a")).toBe(original);
  expect((within(original).getByRole("checkbox") as HTMLInputElement).disabled).toBe(true);
  expect((screen.getByRole("button", { name: "Next" }) as HTMLButtonElement).disabled).toBe(true);
  await act(async () => { pending.resolve(await host.client.membership(groupScope.scanId, 1, "synthetic-selector",
    [groupMetadata("a").groupId, groupMetadata("b").groupId])); });
  expect(row("a")).toBe(original);
  expect((within(original).getByRole("checkbox") as HTMLInputElement).checked).toBe(true);
  expect(ipc.invoke.mock.calls.filter(([name]) => name === "session_import_scan_page")).toHaveLength(0);
  expect(ipc.invoke.mock.calls.filter(([name]) => name === "session_import_preview")).toHaveLength(0);
});

test("real Host uses group IDs for bulk but candidate IDs for preview and clears only a changed preference", async () => {
  const user = await start();
  await user.click(within(row("b")).getByRole("button", { name: "Preview" }));
  await screen.findByText(`body-preview-${groupCandidate("b").candidateId}`);
  await user.click(within(row("a")).getByRole("button", { name: "Preview" }));
  await screen.findByText(`body-preview-${groupCandidate("a").candidateId}`);
  const original = row("a");
  await host.client.choose({}, groupMetadata("a").groupId, groupCandidate("alternate").candidateId);
  await user.click(screen.getByRole("button", { name: "Refresh" }));
  expect(row("a")).toBe(original);
  expect(original.dataset.importPreviewCandidate).toBe(groupCandidate("alternate").candidateId);
  expect(screen.queryByText(`body-preview-${groupCandidate("a").candidateId}`)).toBeNull();
  await user.click(within(row("b")).getByRole("button", { name: "Preview" }));
  await screen.findByText(`body-preview-${groupCandidate("b").candidateId}`);
  expect(ipc.invoke.mock.calls.filter(([name, args]) => name === "session_import_preview" &&
    args.candidateId === groupCandidate("b").candidateId)).toHaveLength(1);
  await user.click(screen.getByRole("button", { name: "Select visible" }));
  expect(host.client.set).toHaveBeenLastCalledWith(expect.objectContaining({ selectionId: "synthetic-selector" }),
    [groupMetadata("a").groupId, groupMetadata("b").groupId], true);
});

test("real Host lazily opens a solid portaled version menu and switches the same group row without a body read", async () => {
  const user = await start();
  const original = row("a");
  expect(ipc.members).not.toHaveBeenCalled();
  const trigger = within(original).getByRole("button", { name: "Versions (2)" });
  await user.click(trigger);
  const menu = await screen.findByRole("menu", { name: "Choose source version" });
  expect(menu.parentElement).toBe(document.body);
  expect(menu.classList.contains("context-menu")).toBe(true);
  expect(ipc.members).toHaveBeenCalledExactlyOnceWith({ scanId: groupScope.scanId, epoch: 1,
    groupId: groupMetadata("a").groupId, limit: 50 });
  const choice = within(menu).getByRole("menuitemradio", { name: /alternate/ });
  await user.click(choice);
  await waitFor(() => expect(screen.queryByRole("menu", { name: "Choose source version" })).toBeNull());
  expect(row("a")).toBe(original);
  expect(original.dataset.importPreviewCandidate).toBe(groupCandidate("alternate").candidateId);
  expect(host.client.choose).toHaveBeenCalledExactlyOnceWith(expect.objectContaining({ expectedStateVersion: 1 }),
    groupMetadata("a").groupId, groupCandidate("alternate").candidateId);
  expect(document.activeElement).toBe(trigger);
  expect(within(original).getByRole("checkbox").getAttribute("checked")).toBeNull();
  expect(ipc.invoke.mock.calls.filter(([name]) => name === "session_import_preview")).toHaveLength(0);
});

async function mode(user: ReturnType<typeof userEvent.setup>, label: string) {
  await user.click(screen.getByRole("button", { name: "Selection mode" }));
  const option = await screen.findByRole("option", { name: label });
  await user.click(within(option).getByRole("button"));
}

test("independent candidates are an explicit reversible UI choice; changing domains expires its preview", async () => {
  const user = await start();
  await mode(user, "Independent candidates");
  await waitFor(() => expect(document.querySelectorAll("[data-import-candidate]")).toHaveLength(3));
  const alternate = document.querySelector<HTMLLIElement>(`[data-import-candidate='${groupCandidate("alternate").candidateId}']`)!;
  await user.click(within(alternate).getByRole("button", { name: "Preview" }));
  await screen.findByText(`body-preview-${groupCandidate("alternate").candidateId}`);
  await mode(user, "Grouped sessions");
  await waitFor(() => expect(row("a")).toBeTruthy());
  expect(document.querySelectorAll("[data-import-candidate]")).toHaveLength(2);
  expect(screen.queryByText(`body-preview-${groupCandidate("alternate").candidateId}`)).toBeNull();
  expect(host.client.get).toHaveBeenCalledTimes(2);
});

test("failed grouped initialization is visible and only an explicit UI choice reads independent candidates", async () => {
  host.client.get.mockRejectedValueOnce(new Error("PLAN_STALE: synthetic group setup failure"));
  const user = userEvent.setup();
  render(<SessionImportHost locale="en" />);
  await act(async () => { openSessionImport({}); });
  await user.click(await screen.findByRole("button", { name: "Scan all defaults" }));
  await screen.findByRole("alert");
  expect(ipc.invoke.mock.calls.filter(([name]) => name === "session_import_scan_page")).toHaveLength(0);
  await mode(user, "Independent candidates");
  await waitFor(() => expect(document.querySelectorAll("[data-import-candidate]")).toHaveLength(3));
  expect(host.client.get).toHaveBeenCalledTimes(1);
});

test("failed member CAS retains the authorized preview; refresh can review the current choice", async () => {
  const user = await start();
  await user.click(within(row("a")).getByRole("button", { name: "Preview" }));
  const body = await screen.findByText(`body-preview-${groupCandidate("a").candidateId}`);
  host.client.choose.mockRejectedValueOnce(new Error("PLAN_STALE: synthetic CAS mismatch"));
  await user.click(within(row("a")).getByRole("button", { name: "Versions (2)" }));
  const menu = await screen.findByRole("menu");
  await user.click(within(menu).getByRole("menuitemradio", { name: /alternate/ }));
  await within(menu).findByRole("alert");
  expect(screen.getByText(body.textContent!)).toBe(body);
  expect(row("a").dataset.importPreviewCandidate).toBe(groupCandidate("a").candidateId);
  await user.click(within(menu).getByRole("menuitem", { name: "Refresh" }));
  await waitFor(() => expect(within(menu).queryByRole("alert")).toBeNull());
  expect(within(menu).getByRole("menuitemradio", { name: /alternate/ }).getAttribute("aria-checked")).toBe("false");
  expect(screen.getByText(body.textContent!)).toBe(body);
});

test("an accepted member change followed by a read failure expires old content and stays reviewable", async () => {
  const user = await start();
  const original = row("a");
  await user.click(within(original).getByRole("button", { name: "Preview" }));
  await screen.findByText(`body-preview-${groupCandidate("a").candidateId}`);
  await user.click(within(original).getByRole("button", { name: "Versions (2)" }));
  const menu = await screen.findByRole("menu");
  host.client.membership.mockRejectedValueOnce(new Error("SOURCE_BUSY: synthetic read failure after ACK"));
  await user.click(within(menu).getByRole("menuitemradio", { name: /alternate/ }));
  await within(menu).findByRole("alert");
  expect(row("a")).toBe(original);
  expect(screen.queryByText(`body-preview-${groupCandidate("a").candidateId}`)).toBeNull();
  expect((within(original).getByRole("checkbox") as HTMLInputElement).disabled).toBe(true);
  await user.click(within(menu).getByRole("menuitem", { name: "Refresh" }));
  await waitFor(() => expect(within(menu).queryByRole("alert")).toBeNull());
  expect(row("a")).toBe(original);
  expect(original.dataset.importPreviewCandidate).toBe(groupCandidate("alternate").candidateId);
  expect(within(menu).getByRole("menuitemradio", { name: /alternate/ }).getAttribute("aria-checked")).toBe("true");
  expect((within(original).getByRole("checkbox") as HTMLInputElement).disabled).toBe(false);
  expect(ipc.invoke.mock.calls.filter(([name]) => name === "session_import_preview")).toHaveLength(1);
});

test("opening another group's versions replaces the one active menu and keeps selection unchanged", async () => {
  const user = await start();
  ipc.members.mockResolvedValueOnce(memberPage()).mockResolvedValueOnce(memberPage(["b"], null, groupMetadata("b")));
  await user.click(within(row("a")).getByRole("button", { name: "Versions (2)" }));
  await screen.findByRole("menu");
  await user.click(within(row("b")).getByRole("button", { name: "Versions (2)" }));
  await waitFor(() => expect(screen.getAllByRole("menu")).toHaveLength(1));
  const menu = screen.getByRole("menu");
  await waitFor(() => expect(within(menu).getAllByRole("menuitemradio")).toHaveLength(1));
  expect(within(menu).getByRole("menuitemradio").textContent).toContain("b");
  expect(within(row("a")).getByRole("button", { name: "Versions (2)" }).getAttribute("aria-expanded")).toBe("false");
  expect(host.client.set).not.toHaveBeenCalled();
  expect(host.client.choose).not.toHaveBeenCalled();
});

test("real Host scans explicitly selected installation roots with locationId/host, never source relocation", async () => {
  const registration = { sourceId: "synthetic-installation", provider: "cursor", rootPath: "/synthetic/primary", profile: "default" };
  const locations = [{ version: 1, sourceId: registration.sourceId, provider: "cursor", host: "local",
    locationId: `primary:${registration.sourceId}`, rootPath: registration.rootPath, kind: "primary" },
  { version: 1, sourceId: registration.sourceId, provider: "cursor", host: "local",
    locationId: "00000000-0000-0000-0000-000000000001", rootPath: "/synthetic/associated", kind: "associated" }];
  const original = ipc.invoke.getMockImplementation()!;
  ipc.invoke.mockImplementation((command, args) => {
    if (command === "session_import_sources") return Promise.resolve([registration]);
    if (command === "session_import_source_locations") return Promise.resolve({ locations, totalCount: 2, nextCursor: null });
    return original(command, args);
  });
  const user = userEvent.setup();
  render(<SessionImportHost locale="en" />);
  await act(async () => { openSessionImport({}); });
  const trigger = await screen.findByRole("button", { name: "History locations" });
  await user.click(trigger);
  const dialog = await screen.findByRole("dialog", { name: "History locations" });
  await user.click(await within(dialog).findByRole("checkbox", { name: /synthetic\/primary/ }));
  await user.click(within(dialog).getByRole("checkbox", { name: /synthetic\/associated/ }));
  await user.click(within(dialog).getByRole("button", { name: "Use roots (2)" }));
  expect(document.activeElement).toBe(trigger);
  await user.click(screen.getByRole("button", { name: "Scan" }));
  await waitFor(() => expect(row("a")).toBeTruthy());
  const requests = ipc.invoke.mock.calls.filter(([name]) => name === "session_import_scan_start");
  expect(requests).toHaveLength(1);
  expect(requests[0][1]).toEqual({ req: { sources: locations.map((location) => ({ provider: "cursor", sourceId: registration.sourceId,
    path: location.rootPath, locationId: location.locationId, host: "local" })) } });
  expect(ipc.invoke.mock.calls.filter(([name]) => ["session_import_source_register", "session_import_source_associate",
    "session_import_preview"].includes(name))).toHaveLength(0);
});
