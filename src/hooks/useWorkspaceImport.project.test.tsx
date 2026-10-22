/** @vitest-environment jsdom */
import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { useWorkspaceImport } from "./useWorkspaceImport";
import type { WorkspaceImportPreview } from "@/lib/api/sessionMigration";

const mocks = vi.hoisted(() => ({ preview: vi.fn(), create: vi.fn(), pick: vi.fn(), save: vi.fn() }));
vi.mock("@/lib/api/sessionMigration", () => ({ sessionImportWorkspacePreview: mocks.preview, sessionImportProjectCreate: mocks.create, sessionImportWorkspaceSave: mocks.save }));
vi.mock("@/lib/api/fs", () => ({ pickDirectory: mocks.pick }));
afterEach(() => { cleanup(); vi.resetAllMocks(); vi.restoreAllMocks(); });

const preview: WorkspaceImportPreview = { source: { path: "/source.code-workspace", kind: "file" }, fingerprint: "f1", savedRevision: null,
  workspace: { sourceWorkspaceId: "source", displayName: "Example", evidence: [], roots: [
    { rootId: "root", originalPath: "/old", pathSyntax: "posix", host: "local", availability: "missing", evidence: [] },
  ] },
};
const created = { project: { id: "new-project", name: "New project", path: "/target", trusted: false }, created: true };

async function open(result: { current: ReturnType<typeof useWorkspaceImport> }) {
  mocks.preview.mockResolvedValue(preview);
  mocks.pick.mockResolvedValue("/target");
  await act(() => result.current.load(preview.source));
  await act(() => result.current.chooseTarget("root"));
}

test("project creation is explicit, defaults to an untrusted record and only updates the draft mapping", async () => {
  const changed = vi.fn();
  mocks.create.mockResolvedValue(created);
  const { result } = renderHook(() => useWorkspaceImport(changed));
  await open(result);
  expect(mocks.create).not.toHaveBeenCalled();
  await act(() => result.current.createProject("root"));
  expect(mocks.create.mock.calls[0][1]).toBe("/target");
  expect(result.current.choices[0]).toEqual({ rootId: "root", primary: true, targetProjectId: "new-project", explicitTarget: undefined });
  expect(result.current.createdProjects[0].name).toBe("New project");
  expect(result.current.projectNotice).toEqual({ rootId: "root", name: "New project" });
  expect(mocks.save).not.toHaveBeenCalled();
  expect(changed).toHaveBeenCalledOnce();
});

test("failed create keeps selected path and uses the same request ID on retry", async () => {
  mocks.create.mockRejectedValueOnce("MAPPING_CONFLICT: disk").mockResolvedValueOnce(created);
  const { result } = renderHook(useWorkspaceImport);
  await open(result);
  await act(() => result.current.createProject("root"));
  const requestId = mocks.create.mock.calls[0][0];
  expect(result.current.error).toContain("MAPPING_CONFLICT");
  expect(result.current.choices[0].explicitTarget).toBe("/target");
  await act(() => result.current.createProject("root"));
  expect(mocks.create.mock.calls[1]).toEqual([requestId, "/target"]);
  expect(result.current.error).toBeNull();
});

test.each(["throw", "reject"])("a %s from list refresh does not misreport committed creation", async (failure) => {
  const warning = vi.spyOn(console, "warn").mockImplementation(() => {});
  const changed = vi.fn(() => {
    if (failure === "throw") throw new Error("refresh failed");
    return Promise.reject(new Error("refresh failed"));
  });
  mocks.create.mockResolvedValue(created);
  const { result } = renderHook(() => useWorkspaceImport(changed));
  await open(result);
  await act(() => result.current.createProject("root"));
  expect(result.current.error).toBeNull();
  expect(result.current.choices[0].targetProjectId).toBe(created.project.id);
  expect(result.current.projectNotice?.name).toBe(created.project.name);
  expect(result.current.busy).toBe(false);
  expect(warning).toHaveBeenCalledOnce();
});

test("duplicate clicks and stale completions cannot replace another workspace draft", async () => {
  let resolve!: (value: typeof created) => void;
  mocks.create.mockReturnValue(new Promise<typeof created>((yes) => { resolve = yes; }));
  const changed = vi.fn();
  const { result } = renderHook(() => useWorkspaceImport(changed));
  await open(result);
  let creating!: Promise<void>;
  act(() => { creating = result.current.createProject("root"); });
  await act(() => result.current.createProject("root"));
  expect(mocks.create).toHaveBeenCalledOnce();
  act(() => result.current.showSaved({ ...preview.workspace, sourceWorkspaceId: "second" }));
  await act(async () => { resolve(created); await creating; });
  expect(result.current.workspace?.sourceWorkspaceId).toBe("second");
  expect(result.current.projectNotice).toBeNull();
  expect(result.current.choices[0].targetProjectId).toBeNull();
  expect(changed).toHaveBeenCalledOnce();
});

test("source roots and saved project mappings are never implicit create targets", async () => {
  mocks.preview.mockResolvedValue(preview);
  const { result } = renderHook(useWorkspaceImport);
  await act(() => result.current.load(preview.source));
  await act(() => result.current.createProject("root"));
  expect(mocks.create).not.toHaveBeenCalled();
  act(() => result.current.setProject("root", "existing"));
  await act(() => result.current.createProject("root"));
  expect(mocks.create).not.toHaveBeenCalled();
});
