/** @vitest-environment jsdom */
import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import type { SourceWorkspace } from "@/lib/api/sessionMigration";
import type { WorkspaceRecord } from "@/lib/multiRootWorkspace";
import { useWorkspaceImport } from "./useWorkspaceImport";

const mocks = vi.hoisted(() => ({ get: vi.fn(), apply: vi.fn() }));
vi.mock("@/lib/api/sessionMigration", () => ({ sessionImportWorkspaceGet: mocks.get, sessionImportWorkspaceApply: mocks.apply }));
vi.mock("@/lib/api/fs", () => ({ pickDirectory: vi.fn() }));
afterEach(() => { cleanup(); vi.resetAllMocks(); });

const saved: SourceWorkspace = {
  sourceWorkspaceId: "source-one", displayName: "Imported", evidence: [],
  roots: [{ rootId: "a", originalPath: "/old", pathSyntax: "posix", host: "local", availability: "offline", evidence: [] }],
  mappingChoices: [{ rootId: "a", primary: true, targetProjectId: "p1" }],
};
const native: WorkspaceRecord = { id: "ws-native", name: "Imported", primaryProjectId: "p1", roots: [], capability: "contextOnly", updatedAt: "fixture" };

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: unknown) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}

test("explicit creation checks the displayed mappings, blocks duplicate clicks and sends a revision", async () => {
  const pending = deferred<WorkspaceRecord>();
  mocks.get.mockResolvedValue({ workspace: saved, revision: "reviewed-r1" });
  mocks.apply.mockReturnValue(pending.promise);
  const { result } = renderHook(useWorkspaceImport);
  act(() => result.current.showSaved(saved));
  expect(mocks.apply).not.toHaveBeenCalled();
  let creating!: Promise<void>;
  await act(async () => { creating = result.current.createNative(); });
  expect(result.current.busy).toBe(true);
  await act(() => result.current.createNative());
  expect(mocks.apply).toHaveBeenCalledTimes(1);
  expect(mocks.apply).toHaveBeenCalledWith({ sourceWorkspaceId: "source-one", expectedRevision: "reviewed-r1" });
  await act(async () => { pending.resolve(native); await creating; });
  expect(result.current.nativeWorkspace).toEqual(native);
  expect(result.current.busy).toBe(false);
});

test("a newer mapping is never silently used to create the native copy", async () => {
  mocks.get.mockResolvedValue({ workspace: { ...saved, displayName: "Changed elsewhere" }, revision: "r2" });
  const { result } = renderHook(useWorkspaceImport);
  act(() => result.current.showSaved(saved));
  await act(() => result.current.createNative());
  expect(mocks.apply).not.toHaveBeenCalled();
  expect(result.current.error).toContain("PLAN_STALE");
  expect(result.current.workspace).toEqual(saved);
});

test("failed native creation keeps the mapping and can be retried", async () => {
  mocks.get.mockResolvedValue({ workspace: saved, revision: "r1" });
  mocks.apply.mockRejectedValueOnce("SNAPSHOT_UNAVAILABLE: store failed").mockResolvedValueOnce(native);
  const { result } = renderHook(useWorkspaceImport);
  act(() => result.current.showSaved(saved));
  await act(() => result.current.createNative());
  expect(result.current.error).toContain("SNAPSHOT_UNAVAILABLE");
  expect(result.current.workspace).toEqual(saved);
  await act(() => result.current.createNative());
  expect(result.current.error).toBeNull();
  expect(result.current.nativeWorkspace).toEqual(native);
});

test("leaving during revision lookup prevents the subsequent write", async () => {
  const pending = deferred<{ workspace: SourceWorkspace; revision: string }>();
  mocks.get.mockReturnValue(pending.promise);
  const { result } = renderHook(useWorkspaceImport);
  act(() => result.current.showSaved(saved));
  let creating!: Promise<void>;
  act(() => { creating = result.current.createNative(); });
  act(() => result.current.reset());
  await act(async () => { pending.resolve({ workspace: saved, revision: "r1" }); await creating; });
  expect(mocks.apply).not.toHaveBeenCalled();
  expect(result.current.mode).toBeNull();
});

test.each(["resolve", "reject"])("late %s after reopening another source does not replace its state", async (outcome) => {
  const pending = deferred<WorkspaceRecord>();
  mocks.get.mockResolvedValue({ workspace: saved, revision: "r1" });
  mocks.apply.mockReturnValue(pending.promise);
  const { result } = renderHook(useWorkspaceImport);
  act(() => result.current.showSaved(saved));
  let creating!: Promise<void>;
  await act(async () => { creating = result.current.createNative(); });
  act(() => result.current.showSaved({ ...saved, sourceWorkspaceId: "second" }));
  await act(async () => { if (outcome === "resolve") pending.resolve(native); else pending.reject("failed"); await creating; });
  expect(result.current.workspace?.sourceWorkspaceId).toBe("second");
  expect(result.current.nativeWorkspace).toBeNull();
  expect(result.current.error).toBeNull();
});
