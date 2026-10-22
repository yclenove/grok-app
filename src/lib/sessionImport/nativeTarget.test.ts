import { afterEach, expect, test, vi } from "vitest";
import { resolveImportedNativeTarget, type NativeTargetClient } from "./nativeTarget";

const id = "97e20f19-42f4-47c1-b5e2-8e36dc7699af";
const saved = { id, projectId: "project-1", title: "中文续聊", updatedAt: "2026-10-03T00:00:00Z",
  modelId: "grok-4.6", effort: "high", jsonSchema: '{"type":"object"}', extraRules: "source rules" };
const project = { id: "project-1", name: "中文 多根", path: "C:/synthetic/cwd", trusted: false, pathOk: true };
function client(): NativeTargetClient {
  return { sessionsList: vi.fn().mockResolvedValue([saved]), projectsList: vi.fn().mockResolvedValue([project]) };
}
afterEach(() => vi.useRealTimers());

test("reads durable metadata even when the sidebar has not received the new row", async () => {
  const target = await resolveImportedNativeTarget(id, client());
  expect(target.row).toMatchObject(saved);
  expect(target.project).toMatchObject(project);
});

test("an explicitly unbound session may open without inventing a project", async () => {
  const io = client();
  vi.mocked(io.sessionsList).mockResolvedValue([{ ...saved, projectId: null }]);
  expect((await resolveImportedNativeTarget(id, io)).project).toBeNull();
});

test("a missing project never falls back to Default workspace", async () => {
  const io = client();
  vi.mocked(io.projectsList).mockResolvedValue([]);
  await expect(resolveImportedNativeTarget(id, io)).rejects.toThrow("WORKSPACE_UNRESOLVED");
});

test("deleted or archived native rows are rejected", async () => {
  for (const rows of [[], [{ ...saved, archived: true }]]) {
    const io = client();
    vi.mocked(io.sessionsList).mockResolvedValue(rows);
    await expect(resolveImportedNativeTarget(id, io)).rejects.toThrow("SESSION_OPEN_MISSING");
  }
});

test.each(["imp:foreign", "imp:97e20f19-42f4-47c1-b5e2-8e36dc7699af", "", "not-a-native-id"])("invalid native target %s does not enter native lookup", async (target) => {
  const io = client();
  await expect(resolveImportedNativeTarget(target, io)).rejects.toThrow("SESSION_OPEN_MISSING");
  expect(io.sessionsList).not.toHaveBeenCalled();
  expect(io.projectsList).not.toHaveBeenCalled();
});

test("trims a native id before reading its durable row", async () => {
  expect((await resolveImportedNativeTarget(` ${id} `, client())).row.id).toBe(id);
});

test("a hung IPC leaves lookup after its bounded deadline", async () => {
  vi.useFakeTimers();
  const io = client();
  vi.mocked(io.sessionsList).mockReturnValue(new Promise(() => {}));
  const request = expect(resolveImportedNativeTarget(id, io)).rejects.toThrow("SESSION_OPEN_UNAVAILABLE");
  await vi.advanceTimersByTimeAsync(30_000);
  await request;
});
