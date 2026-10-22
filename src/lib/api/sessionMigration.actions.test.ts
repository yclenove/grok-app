import { beforeEach, expect, test, vi } from "vitest";
import { sessionImportCancel, sessionImportRetryFailed, sessionImportUndo } from "./sessionMigration";

const mocks = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("./host", () => ({ invoke: mocks.invoke }));
beforeEach(() => vi.resetAllMocks());

test.each([
  ["session_import_cancel", sessionImportCancel],
  ["session_import_retry_failed", sessionImportRetryFailed],
  ["session_import_undo", sessionImportUndo],
] as const)("%s sends exactly the reviewed version and caller request identity", async (command, operation) => {
  const request = { expectedStateVersion: 7, requestId: "12345678-1234-4678-9abc-123456789abc" };
  const result = { jobId: "job", stateVersion: 8 };
  mocks.invoke.mockResolvedValue(result);
  expect(await operation("job", request)).toBe(result);
  expect(mocks.invoke).toHaveBeenCalledExactlyOnceWith(command, { jobId: "job", ...request });
});
