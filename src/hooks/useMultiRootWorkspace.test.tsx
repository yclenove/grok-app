/** @vitest-environment jsdom */
import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import type { WorkspaceRecord } from "@/lib/multiRootWorkspace";
import { useMultiRootWorkspace } from "./useMultiRootWorkspace";

const mocks = vi.hoisted(() => ({ settings: vi.fn(), list: vi.fn(), get: vi.fn(), upsert: vi.fn(), bind: vi.fn(), setSettings: vi.fn(), sessions: vi.fn(), projects: vi.fn() }));
vi.mock("@/lib/api", () => ({ settingsGet: mocks.settings, workspacesForProject: mocks.list, workspaceGet: mocks.get, workspaceUpsert: mocks.upsert, sessionSetWorkspace: mocks.bind, settingsSet: mocks.setSettings, sessionsList: mocks.sessions, projectsList: mocks.projects }));

beforeEach(() => {
  mocks.sessions.mockResolvedValue([{ id: "session", projectId: "project" }]);
  mocks.projects.mockResolvedValue([{ id: "project", name: "Project", path: "/project" }]);
});
afterEach(() => { cleanup(); vi.resetAllMocks(); });

const target = { projectId: "project", projectName: "Project", projectPath: "/project", sessionId: "session", workspaceId: null };
const record = (id: string): WorkspaceRecord => ({ id, name: id, primaryProjectId: "project", roots: [{ path: "/project", role: "primary", access: "write" }], capability: "contextOnly", updatedAt: "fixture" });

test("choosing an imported workspace binds only after Save", async () => {
  const first = record("first");
  const imported = record("imported");
  mocks.settings.mockResolvedValue({ sessionDataMode: "shared" });
  mocks.list.mockResolvedValue([first, imported]);
  mocks.upsert.mockResolvedValue(imported);
  const { result } = renderHook(useMultiRootWorkspace);
  await act(() => result.current.openFor(target));
  expect(result.current.workspaces).toEqual([first, imported]);
  act(() => result.current.selectWorkspace(imported.id));
  expect(result.current.draft).toEqual(imported);
  expect(mocks.bind).not.toHaveBeenCalled();
  await act(() => result.current.save());
  expect(mocks.bind).toHaveBeenCalledWith("session", "imported");
});

test("late list results cannot reopen a closed manager", async () => {
  let resolve!: (rows: WorkspaceRecord[]) => void;
  mocks.settings.mockResolvedValue({ sessionDataMode: "shared" });
  mocks.list.mockReturnValue(new Promise<WorkspaceRecord[]>((yes) => { resolve = yes; }));
  const { result } = renderHook(useMultiRootWorkspace);
  let opening!: Promise<void>;
  await act(async () => { opening = result.current.openFor(target); });
  act(() => result.current.close());
  await act(async () => { resolve([record("late")]); await opening; });
  expect(result.current.open).toBe(false);
  expect(result.current.draft).toBeNull();
  expect(result.current.workspaces).toEqual([]);
});

test("workspace store errors leave no editable empty draft", async () => {
  mocks.settings.mockResolvedValue({ sessionDataMode: "shared" });
  mocks.list.mockRejectedValue("invalid workspace store");
  const { result } = renderHook(useMultiRootWorkspace);
  await act(() => result.current.openFor(target));
  expect(result.current.draft).toBeNull();
  expect(result.current.error).toContain("invalid workspace store");
  await act(() => result.current.save());
  expect(mocks.upsert).not.toHaveBeenCalled();
});

test("a closed save cannot bind a session or overwrite a newly opened workspace", async () => {
  mocks.settings.mockResolvedValue({ sessionDataMode: "shared" });
  mocks.list.mockResolvedValue([record("old")]);
  let resolve!: (row: WorkspaceRecord) => void;
  mocks.upsert.mockReturnValue(new Promise<WorkspaceRecord>((yes) => { resolve = yes; }));
  const { result } = renderHook(useMultiRootWorkspace);
  await act(() => result.current.openFor(target));
  let saving!: ReturnType<typeof result.current.save>;
  act(() => { saving = result.current.save(); });
  await act(() => result.current.save());
  expect(mocks.upsert).toHaveBeenCalledTimes(1);
  act(() => result.current.close());
  mocks.list.mockResolvedValue([record("new")]);
  await act(() => result.current.openFor(target));
  await act(async () => { resolve(record("old")); await saving; });
  expect(mocks.bind).not.toHaveBeenCalled();
  expect(result.current.draft?.id).toBe("new");
});

test("detach failure remains visible and does not signal closure", async () => {
  mocks.settings.mockResolvedValue({ sessionDataMode: "shared" });
  mocks.list.mockResolvedValue([record("existing")]);
  mocks.bind.mockRejectedValue("session unavailable");
  const { result } = renderHook(useMultiRootWorkspace);
  await act(() => result.current.openFor(target));
  let closed: boolean | undefined;
  await act(async () => { closed = await result.current.clearBinding(); });
  expect(closed).toBe(false);
  expect(result.current.error).toBe("session unavailable");
  expect(result.current.open).toBe(true);
});

test("recovery manager resolves current binding and relocated project from disk", async () => {
  mocks.sessions.mockResolvedValue([{ id: "session", projectId: "project", workspaceId: "current" }]);
  mocks.projects.mockResolvedValue([{ id: "project", name: "Moved project", path: "/moved" }]);
  mocks.list.mockResolvedValue([record("wrong-first"), record("current")]);
  mocks.get.mockResolvedValue(record("current"));
  const { result } = renderHook(useMultiRootWorkspace);
  await act(() => result.current.openFor({ ...target, workspaceId: "stale" }));
  expect(result.current.target?.projectPath).toBe("/moved");
  expect(result.current.draft?.id).toBe("current");
  expect(mocks.get).toHaveBeenCalledWith("current");
  expect(mocks.bind).not.toHaveBeenCalled();
});
