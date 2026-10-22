/** @vitest-environment jsdom */
import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { createT } from "@/i18n";
import type { SourceWorkspace, SourceWorkspaceRoot } from "@/lib/api/sessionMigration";
import { WorkspaceRelocationControl } from "./WorkspaceRelocationControl";

const mocks = vi.hoisted(() => ({ get: vi.fn(), preview: vi.fn(), save: vi.fn(), file: vi.fn() }));
vi.mock("@/lib/api/sessionMigration", () => ({ sessionImportWorkspaceGet: mocks.get }));
vi.mock("@/lib/api/workspaceRelocation", () => ({ workspaceRelocationPreview: mocks.preview, workspaceRelocationSave: mocks.save }));
vi.mock("@/lib/api/fs", () => ({ pickAttachFiles: mocks.file, pickDirectory: vi.fn() }));
afterEach(() => { cleanup(); localStorage.clear(); vi.resetAllMocks(); });

test("dialog shows root changes, mappings and primary choice before an explicit save", async () => {
  const root = (id: string): SourceWorkspaceRoot => ({ rootId: id, originalPath: `/source/${id}`, pathSyntax: "posix", host: "local", availability: "missing", evidence: [] });
  const before: SourceWorkspace = { sourceWorkspaceId: "ws:one", displayName: "one", workspaceFile: "/old.code-workspace", evidence: [], roots: [root("kept"), root("removed")],
    defaultRootId: "removed", mappingChoices: [{ rootId: "removed", targetProjectId: "old-target", primary: true }, { rootId: "kept", targetProjectId: null, explicitTarget: "/kept-target", primary: false }] };
  const after: SourceWorkspace = { ...before, workspaceFile: "/new.code-workspace", roots: [root("kept"), root("added")], defaultRootId: "kept", mappingChoices: [
    { rootId: "kept", targetProjectId: null, explicitTarget: "/kept-target", primary: true }, { rootId: "added", targetProjectId: null, primary: false },
  ] };
  mocks.file.mockResolvedValue([after.workspaceFile]);
  mocks.get.mockResolvedValue({ workspace: before, revision: "a".repeat(64) });
  mocks.preview.mockResolvedValue({ before, after, request: { sourceWorkspaceId: before.sourceWorkspaceId, path: after.workspaceFile, expectedRevision: "a".repeat(64) }, fingerprint: "b".repeat(64), proposalHash: "c".repeat(64), retainedRootIds: ["kept"], addedRootIds: ["added"], removedRoots: [root("removed")] });
  mocks.save.mockResolvedValue({ workspace: after, revision: "saved" });
  const saved = vi.fn();
  render(<WorkspaceRelocationControl workspace={before} disabled={false} onSaved={saved} tr={createT("en")} projects={[{ id: "old-target", name: "Prior project" }]} />);
  fireEvent.click(screen.getByRole("button", { name: "Relocate source workspace" }));
  const dialog = screen.getByRole("dialog");
  expect((within(dialog).getByRole("button", { name: "Save" }) as HTMLButtonElement).disabled).toBe(true);
  fireEvent.click(within(dialog).getByRole("button", { name: "Choose the new location" }));
  await within(dialog).findByText("/new.code-workspace");
  expect(within(dialog).getByText("/kept-target")).toBeTruthy();
  expect(within(dialog).getByText("Prior project")).toBeTruthy();
  expect(within(dialog).getByText("/source/added")).toBeTruthy();
  expect(mocks.save).not.toHaveBeenCalled();
  fireEvent.click(within(dialog).getByRole("button", { name: "Save" }));
  await vi.waitFor(() => expect(saved).toHaveBeenCalledWith(after));
  expect(screen.queryByRole("dialog")).toBeNull();
});
