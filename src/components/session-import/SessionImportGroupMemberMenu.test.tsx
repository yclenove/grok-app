/** @vitest-environment jsdom */
import { useState } from "react";
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { createT } from "@/i18n";
import type { SourceGroup, SourceGroupMemberPage } from "@/lib/api/sessionMigrationGroups";
import { groupCandidate, groupDeferred, groupMetadata, groupScope } from "@/test/sessionImportGroupFixture";
import { SessionImportGroupMemberPicker } from "./SessionImportGroupMemberPicker";

const ipc = vi.hoisted(() => ({ members: vi.fn(), choose: vi.fn(), refresh: vi.fn() }));
vi.mock("@/lib/api/sessionMigrationGroups", async (original) => ({
  ...await original<typeof import("@/lib/api/sessionMigrationGroups")>(), sessionImportScanGroupMembers: ipc.members,
}));
const savedScroll = Object.getOwnPropertyDescriptor(HTMLElement.prototype, "scrollIntoView");
function page(ids = ["a", "alternate"], cursor: string | null = null, group = groupMetadata("a")): SourceGroupMemberPage {
  return { version: 1, groupId: group.groupId, memberCount: Math.max(51, ids.length), complete: true,
    stateVersion: 1, nextCursor: cursor, members: ids.map((id) => ({ groupId: group.groupId,
      sourceId: group.sourceId, rootPath: "/synthetic/root", legacyId: `transcript:${id}`, location: null,
      format: "cursor-transcript", candidate: { ...groupCandidate(id), nativeSessionId: group.nativeSessionId } })) };
}
function Fixture({ group = groupMetadata("a"), disabled = false }: { group?: SourceGroup; disabled?: boolean }) {
  const [open, setOpen] = useState(false);
  const [candidateId, setCandidateId] = useState(group.firstMember.candidateId);
  return <div role="dialog"><button type="button">Before</button><ul><li tabIndex={0} data-import-candidate={group.groupId}>
    <SessionImportGroupMemberPicker scope={groupScope} group={group} candidateId={candidateId}
      open={open} disabled={disabled} busy={false} tr={createT("en")} onOpen={() => setOpen(true)} onClose={() => setOpen(false)}
      onRefresh={ipc.refresh} onChoose={async (id, next) => { const accepted = await ipc.choose(id, next);
        if (accepted) setCandidateId(next); return accepted; }} />
  </li></ul><button type="button">After</button></div>;
}
beforeEach(() => {
  vi.clearAllMocks();
  ipc.members.mockReset().mockResolvedValue(page());
  ipc.choose.mockReset().mockResolvedValue(true);
  ipc.refresh.mockReset().mockResolvedValue(true);
  Object.defineProperty(HTMLElement.prototype, "scrollIntoView", { configurable: true, value: vi.fn() });
});
afterEach(() => {
  cleanup();
  if (savedScroll) Object.defineProperty(HTMLElement.prototype, "scrollIntoView", savedScroll);
  else Reflect.deleteProperty(HTMLElement.prototype, "scrollIntoView");
});
async function open() {
  const user = userEvent.setup();
  render(<Fixture />);
  const trigger = screen.getByRole("button", { name: "Versions (2)" });
  await user.click(trigger);
  const menu = await screen.findByRole("menu", { name: "Choose source version" });
  await waitFor(() => expect(within(menu).getAllByRole("menuitemradio")).toHaveLength(2));
  return { user, menu, trigger };
}

test("loading is lazy; Escape closes only the menu, restores its trigger and expires a late page", async () => {
  const pending = groupDeferred<SourceGroupMemberPage>();
  ipc.members.mockReturnValueOnce(pending.promise);
  const user = userEvent.setup();
  render(<Fixture />);
  expect(ipc.members).not.toHaveBeenCalled();
  const trigger = screen.getByRole("button", { name: "Versions (2)" });
  await user.click(trigger);
  expect(await screen.findByText("Loading versions…")).toBeTruthy();
  await user.keyboard("{Escape}");
  expect(screen.queryByRole("menu")).toBeNull();
  expect(screen.getByRole("dialog")).toBeTruthy();
  expect(document.activeElement).toBe(trigger);
  await act(async () => { pending.resolve(page()); });
  expect(screen.queryByRole("menu")).toBeNull();
  expect(ipc.choose).not.toHaveBeenCalled();
});

test("keyboard navigation and Enter choose a member, not the surrounding checkbox or row", async () => {
  const { user, menu, trigger } = await open();
  const radios = within(menu).getAllByRole("menuitemradio");
  await waitFor(() => expect(document.activeElement).toBe(radios[0]));
  await user.keyboard("{ArrowDown}");
  expect(document.activeElement).toBe(radios[1]);
  await user.keyboard("{Enter}");
  await waitFor(() => expect(screen.queryByRole("menu")).toBeNull());
  expect(ipc.choose).toHaveBeenCalledExactlyOnceWith(groupMetadata("a").groupId, groupCandidate("alternate").candidateId);
  expect(document.activeElement).toBe(trigger);
});

test.each([false, true])("Tab resumes the trigger's logical %s direction after closing its portal", async (shift) => {
  const { user, trigger } = await open();
  await user.tab({ shift });
  expect(screen.queryByRole("menu")).toBeNull();
  expect(document.activeElement).toBe(shift ? trigger.closest("li") : screen.getByRole("button", { name: "After" }));
});

test("outside click keeps the user's new focus instead of restoring the version trigger", async () => {
  const { user } = await open();
  const after = screen.getByRole("button", { name: "After" });
  await user.click(after);
  expect(screen.queryByRole("menu")).toBeNull();
  expect(document.activeElement).toBe(after);
});

test("IME confirmation is prevented from choosing a menu item", async () => {
  const { menu } = await open();
  const option = within(menu).getAllByRole("menuitemradio")[1];
  expect(fireEvent.keyDown(option, { key: "Enter", isComposing: true })).toBe(false);
  expect(fireEvent.keyDown(option, { key: " ", keyCode: 229 })).toBe(false);
  expect(ipc.choose).not.toHaveBeenCalled();
});

test("next/previous replaces at most 50 metadata rows and never loads all versions", async () => {
  ipc.members.mockResolvedValueOnce(page(Array.from({ length: 50 }, (_, index) => `member-${index}`), "signed-next"))
    .mockResolvedValueOnce(page(["last"]))
    .mockResolvedValueOnce(page(["a", "alternate"], "renewed-next"));
  const user = userEvent.setup();
  render(<Fixture />);
  await user.click(screen.getByRole("button", { name: "Versions (2)" }));
  const menu = await screen.findByRole("menu");
  await waitFor(() => expect(within(menu).getAllByRole("menuitemradio")).toHaveLength(50));
  await user.click(within(menu).getByRole("menuitem", { name: "Next page" }));
  await waitFor(() => expect(within(menu).getAllByRole("menuitemradio")).toHaveLength(1));
  expect(ipc.members.mock.calls[1][3]).toEqual({ limit: 50, cursor: "signed-next" });
  await user.click(within(menu).getByRole("menuitem", { name: "Previous page" }));
  await waitFor(() => expect(within(menu).getAllByRole("menuitemradio")).toHaveLength(2));
  expect(ipc.members.mock.calls[2][3]).toEqual({ limit: 50 });
});

test("a failed next page retains the accepted metadata and retry uses the attempted cursor", async () => {
  ipc.members.mockResolvedValueOnce(page(["a", "alternate"], "signed-next"))
    .mockRejectedValueOnce(new Error("SOURCE_BUSY: synthetic"))
    .mockResolvedValueOnce(page(["last"]));
  const { user, menu } = await open();
  await user.click(within(menu).getByRole("menuitem", { name: "Next page" }));
  await within(menu).findByRole("alert");
  expect(within(menu).getAllByRole("menuitemradio")).toHaveLength(2);
  await user.click(within(menu).getByRole("menuitem", { name: "Retry versions" }));
  await waitFor(() => expect(within(menu).getAllByRole("menuitemradio")).toHaveLength(1));
  expect(ipc.members.mock.calls[2][3]).toEqual({ limit: 50, cursor: "signed-next" });
});

test("same-turn duplicate chooses send one action; failure remains reviewable and refresh does not pretend rollback", async () => {
  const pending = groupDeferred<boolean>();
  ipc.choose.mockReturnValueOnce(pending.promise);
  const { user, menu } = await open();
  const option = within(menu).getAllByRole("menuitemradio")[1];
  act(() => { fireEvent.click(option); fireEvent.click(option); });
  expect(ipc.choose).toHaveBeenCalledTimes(1);
  expect(within(menu).getByText("Changing version…")).toBeTruthy();
  await act(async () => { pending.resolve(false); });
  await within(menu).findByRole("alert");
  expect(within(menu).getAllByRole("menuitemradio")[0].getAttribute("aria-checked")).toBe("true");
  await user.click(within(menu).getByRole("menuitem", { name: "Refresh" }));
  expect(ipc.refresh).toHaveBeenCalledTimes(1);
  expect(screen.getByRole("menu")).toBe(menu);
  expect(within(menu).queryByRole("alert")).toBeNull();
});

test("closing during a choose expires its popup completion; a reopened menu is not dismissed", async () => {
  const pending = groupDeferred<boolean>();
  ipc.choose.mockReturnValueOnce(pending.promise);
  const { user, menu, trigger } = await open();
  await user.click(within(menu).getAllByRole("menuitemradio")[1]);
  await user.keyboard("{Escape}");
  await user.click(trigger);
  const reopened = await screen.findByRole("menu");
  await act(async () => { pending.resolve(true); });
  expect(screen.getByRole("menu")).toBe(reopened);
});

test("empty live pages remain reachable and show an honest empty state", async () => {
  ipc.members.mockResolvedValueOnce(page([]));
  const user = userEvent.setup();
  render(<Fixture />);
  await user.click(screen.getByRole("button", { name: "Versions (2)" }));
  expect(await screen.findByText("No versions on this page.")).toBeTruthy();
  expect(screen.queryByRole("menuitemradio")).toBeNull();
  await user.keyboard("{Escape}");
  expect(screen.queryByRole("menu")).toBeNull();
});

test("changing group identity expires the old metadata reply", async () => {
  const pending = groupDeferred<SourceGroupMemberPage>();
  ipc.members.mockReturnValueOnce(pending.promise).mockResolvedValueOnce(page(["b"], null, groupMetadata("b")));
  const user = userEvent.setup();
  const view = render(<Fixture />);
  await user.click(screen.getByRole("button", { name: "Versions (2)" }));
  view.rerender(<Fixture group={groupMetadata("b")} />);
  const menu = await screen.findByRole("menu");
  await waitFor(() => expect(within(menu).getAllByRole("menuitemradio")).toHaveLength(1));
  await act(async () => { pending.resolve(page()); });
  expect(within(menu).getAllByRole("menuitemradio")).toHaveLength(1);
  expect(within(menu).getByRole("menuitemradio").textContent).toContain("b");
});
