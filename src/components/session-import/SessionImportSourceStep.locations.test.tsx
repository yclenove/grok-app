/** @vitest-environment jsdom */
import { act, cleanup, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { createT } from "@/i18n";
import { scanSourcesForChoice } from "@/lib/sessionImport/sourceChoice";
import { groupDeferred } from "@/test/sessionImportGroupFixture";
import type { SourceLocation } from "@/lib/api/sessionMigrationLocations";
import { SessionImportSourceStep } from "./SessionImportSourceStep";

const ipc = vi.hoisted(() => ({ list: vi.fn(), associate: vi.fn(), pick: vi.fn(), register: vi.fn() }));
vi.mock("@/lib/api", () => ({ pickDirectory: ipc.pick }));
vi.mock("@/lib/api/sessionMigrationLocations", async (original) => ({
  ...await original<typeof import("@/lib/api/sessionMigrationLocations")>(),
  sessionImportSourceLocations: ipc.list, sessionImportSourceAssociate: ipc.associate, sessionImportSourceRegister: ipc.register,
}));
const reg = { sourceId: "synthetic-installation", provider: "cursor" as const, rootPath: "/synthetic/primary", profile: "default" };
const primary: SourceLocation = { version: 1, sourceId: reg.sourceId, provider: "cursor", host: "local",
  locationId: `primary:${reg.sourceId}`, rootPath: reg.rootPath, kind: "primary" };
const associated: SourceLocation = { ...primary, kind: "associated", rootPath: "/synthetic/associated",
  locationId: "00000000-0000-0000-0000-000000000001" };
beforeEach(() => {
  vi.clearAllMocks();
  ipc.list.mockReset().mockResolvedValue({ locations: [primary, associated], totalCount: 2, nextCursor: null });
  ipc.associate.mockReset().mockResolvedValue(associated);
  ipc.pick.mockReset().mockResolvedValue(associated.rootPath);
  ipc.register.mockReset().mockResolvedValue(reg);
});
afterEach(cleanup);

function renderStep() {
  const onChoose = vi.fn();
  render(<SessionImportSourceStep tr={createT("en")} roots={[]} choice={null} busy={false}
    registrations={[reg]}
    onChoose={onChoose} onPickFolder={vi.fn()} onPickFile={vi.fn()} onRelocate={vi.fn()} onScan={vi.fn()} />);
  return onChoose;
}

test("registered Cursor installations expose explicit history-location management without passive reads", () => {
  const onChoose = renderStep();
  expect(screen.getByRole("button", { name: "History locations" })).toBeTruthy();
  expect(onChoose).not.toHaveBeenCalled();
  expect(ipc.list).not.toHaveBeenCalled();
  expect(ipc.associate).not.toHaveBeenCalled();
});

test("real SourceStep selects two explicit roots and preserves their existing scan wire references", async () => {
  const user = userEvent.setup();
  const onChoose = renderStep();
  const trigger = screen.getByRole("button", { name: "History locations" });
  await user.click(trigger);
  const dialog = await screen.findByRole("dialog", { name: "History locations" });
  await user.click(await within(dialog).findByRole("checkbox", { name: /synthetic\/primary/ }));
  await user.click(within(dialog).getByRole("checkbox", { name: /synthetic\/associated/ }));
  await user.click(within(dialog).getByRole("button", { name: "Use roots (2)" }));
  expect(onChoose).toHaveBeenCalledTimes(1);
  expect(scanSourcesForChoice(onChoose.mock.calls[0][0])).toEqual([primary, associated].map((location) => ({
    provider: "cursor", sourceId: reg.sourceId, locationId: location.locationId, host: "local", path: location.rootPath,
  })));
  expect(screen.queryByRole("dialog")).toBeNull();
  expect(document.activeElement).toBe(trigger);
  expect(ipc.associate).not.toHaveBeenCalled();
  expect(ipc.pick).not.toHaveBeenCalled();
});

test("folder choice is reviewed before explicit association and adding never auto-selects it", async () => {
  const user = userEvent.setup();
  const onChoose = renderStep();
  await user.click(screen.getByRole("button", { name: "History locations" }));
  const dialog = await screen.findByRole("dialog", { name: "History locations" });
  await within(dialog).findByRole("checkbox", { name: /synthetic\/primary/ });
  const picker = within(dialog).getByRole("button", { name: "Choose a history folder" });
  await user.click(picker);
  expect(ipc.associate).not.toHaveBeenCalled();
  expect(document.activeElement).toBe(picker);
  await user.click(within(dialog).getByRole("button", { name: "Associate folder" }));
  await within(dialog).findByText(`Associated ${associated.rootPath}`);
  await waitFor(() => expect(ipc.list).toHaveBeenCalledTimes(2));
  expect(ipc.associate).toHaveBeenCalledExactlyOnceWith({ sourceId: reg.sourceId, provider: "cursor", host: "local",
    expectedPrimaryRoot: reg.rootPath, path: associated.rootPath });
  expect((within(dialog).getByRole("checkbox", { name: /synthetic\/associated/ }) as HTMLInputElement).checked).toBe(false);
  expect(onChoose).not.toHaveBeenCalled();
});

test("unsupported providers do not expose association controls", () => {
  render(<SessionImportSourceStep tr={createT("en")} roots={[]} choice={null} busy={false}
    registrations={[{ ...reg, provider: "codex" }]}
    onChoose={vi.fn()} onPickFolder={vi.fn()} onPickFile={vi.fn()} onRelocate={vi.fn()} onScan={vi.fn()} />);
  expect(screen.queryByRole("button", { name: "History locations" })).toBeNull();
  expect(ipc.list).not.toHaveBeenCalled();
});

test("default Cursor registration is explicit, never a relocation; its original trigger survives close and reopen", async () => {
  const user = userEvent.setup();
  render(<SessionImportSourceStep tr={createT("en")} roots={[{ provider: "cursor", kind: "cursor-home",
    path: reg.rootPath, exists: true, readable: true }]} choice={null} busy={false} registrations={[]}
    onChoose={vi.fn()} onPickFolder={vi.fn()} onPickFile={vi.fn()} onRelocate={vi.fn()} onScan={vi.fn()} />);
  expect(ipc.register).not.toHaveBeenCalled();
  expect(ipc.list).not.toHaveBeenCalled();
  const trigger = screen.getByRole("button", { name: "History locations" });
  await user.click(trigger);
  const dialog = await screen.findByRole("dialog", { name: "History locations" });
  await within(dialog).findByRole("checkbox", { name: /synthetic\/primary/ });
  expect(ipc.register).toHaveBeenCalledExactlyOnceWith({ provider: "cursor", path: reg.rootPath });
  await user.keyboard("{Escape}");
  expect(screen.queryByRole("dialog")).toBeNull();
  expect(screen.getAllByRole("button", { name: "History locations" })).toHaveLength(1);
  expect(document.activeElement).toBe(trigger);
  await user.click(trigger);
  await screen.findByRole("dialog", { name: "History locations" });
  expect(ipc.register).toHaveBeenCalledTimes(1);
  expect(ipc.list).toHaveBeenCalledTimes(2);
  expect(ipc.associate).not.toHaveBeenCalled();
});

test("missing or unreadable default folders never expose an implicit registration action", () => {
  render(<SessionImportSourceStep tr={createT("en")} roots={[{ provider: "cursor", kind: "cursor-home",
    path: reg.rootPath, exists: true, readable: false }]} choice={null} busy={false} registrations={[]}
    onChoose={vi.fn()} onPickFolder={vi.fn()} onPickFile={vi.fn()} onRelocate={vi.fn()} onScan={vi.fn()} />);
  expect(screen.queryByRole("button", { name: "History locations" })).toBeNull();
  expect(ipc.register).not.toHaveBeenCalled();
});

test("a source intent change during registration cannot open the previous default directory's dialog", async () => {
  const pending = groupDeferred<typeof reg>();
  ipc.register.mockReturnValueOnce(pending.promise);
  const user = userEvent.setup();
  const props = { tr: createT("en"), roots: [{ provider: "cursor" as const, kind: "cursor-home", path: reg.rootPath,
    exists: true, readable: true }], registrations: [], busy: false,
    onChoose: vi.fn(), onPickFolder: vi.fn(), onPickFile: vi.fn(), onRelocate: vi.fn(), onScan: vi.fn() };
  const view = render(<SessionImportSourceStep {...props} choice={null} />);
  await user.click(screen.getByRole("button", { name: "History locations" }));
  view.rerender(<SessionImportSourceStep {...props} choice={{ kind: "custom-folder", provider: "codex", path: "/synthetic/new-intent" }} />);
  await act(async () => { pending.resolve(reg); });
  expect(screen.queryByRole("dialog", { name: "History locations" })).toBeNull();
  expect(ipc.list).not.toHaveBeenCalled();
});
