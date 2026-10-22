import { beforeEach, expect, test, vi } from "vitest";
import { sessionImportEntry } from "./sessionMigration";
const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("./host", () => ({ invoke }));
beforeEach(() => vi.resetAllMocks());

test("exact metadata IPC uses only the requested opaque import ID", async () => {
  const entry = { importId: "imp:outside-first-page", title: "Original title", alias: "Local alias" };
  invoke.mockResolvedValue(entry);
  expect(await sessionImportEntry(entry.importId)).toBe(entry);
  expect(invoke).toHaveBeenCalledExactlyOnceWith("session_import_entry", { importId: entry.importId });
});

test("missing metadata stays null rather than selecting another entry", async () => {
  invoke.mockResolvedValue(null);
  expect(await sessionImportEntry("imp:missing")).toBeNull();
});

test("readonly catalog failures propagate for independent metadata retry", async () => {
  invoke.mockRejectedValue(new Error("catalog unreadable"));
  await expect(sessionImportEntry("imp:missing")).rejects.toThrow("catalog unreadable");
  expect(invoke).toHaveBeenCalledTimes(1);
});
