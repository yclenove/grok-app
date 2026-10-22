import { beforeEach, expect, test, vi } from "vitest";
import { importedWorkspaceGet, importedWorkspaceGroups, importedWorkspaceOptions, importedWorkspaceUpdate } from "./sessionWorkspaceAssociation";
import { sessionImportEntries } from "./sessionMigration";
const { invoke } = vi.hoisted(() => ({ invoke: vi.fn().mockResolvedValue({}) }));
vi.mock("./host", () => ({ invoke }));
beforeEach(() => { invoke.mockClear(); });

test("association IPC sends only explicit metadata and a bounded page request", async () => {
  const request = { importId: "imp:fixture", expectedVersion: 0, requestId: "request", choice: null };
  await importedWorkspaceGet(request.importId);
  await importedWorkspaceOptions(request.importId, "page");
  await importedWorkspaceUpdate(request);
  expect(invoke.mock.calls).toEqual([
    ["session_import_workspace_association_get", { importId: request.importId }],
    ["session_import_workspace_association_options", { importId: request.importId, cursor: "page", limit: 50 }],
    ["session_import_workspace_association_update", { request }],
  ]);
});

test("workspace grouping and history filtering send stable IDs and independent cursors", async () => {
  await importedWorkspaceGroups("groups-page", "ws:selected");
  await sessionImportEntries("entries-page", 50, {
    query: "keyword", provider: "codex", sort: "oldest",
    workspace: { kind: "workspace", sourceWorkspaceId: "ws:selected" },
  });
  await sessionImportEntries(null, 50, { workspace: { kind: "unassigned" } });
  await sessionImportEntries();
  expect(invoke.mock.calls).toEqual([
    ["session_import_workspace_groups", { cursor: "groups-page", limit: 50, selectedId: "ws:selected" }],
    ["session_import_entries", { cursor: "entries-page", limit: 50, query: "keyword", provider: "codex", sort: "oldest", workspace: { kind: "workspace", sourceWorkspaceId: "ws:selected" } }],
    ["session_import_entries", { cursor: null, limit: 50, query: null, provider: null, sort: null, workspace: { kind: "unassigned" } }],
    ["session_import_entries", { cursor: null, limit: null, query: null, provider: null, sort: null, workspace: null }],
  ]);
});
