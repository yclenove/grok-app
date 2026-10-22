import { beforeEach, expect, test, vi } from "vitest";
import { sessionImportContinueCreate, sessionImportContinueWorkspace, sessionImportContinueRestore, sessionImportContinueDiscard, sessionImportContinuationGet, sessionImportContinuationSave } from "./sessionMigration";
const { invoke } = vi.hoisted(() => ({ invoke: vi.fn().mockResolvedValue({}) }));
vi.mock("./host", () => ({ invoke }));
beforeEach(() => invoke.mockClear());
test("continuation IPC preserves reviewed draft and stable creation identity", async () => {
  await sessionImportContinueWorkspace("draft", "hash", "workspace");
  await sessionImportContinueWorkspace("draft", "hash", null);
  await sessionImportContinueRestore("draft", "hash");
  await sessionImportContinueCreate("draft", "hash", "request", "edited text");
  await sessionImportContinueDiscard("draft", "hash");
  expect(invoke.mock.calls).toEqual([
    ["session_import_continue_workspace", { draftId: "draft", draftHash: "hash", workspaceId: "workspace" }],
    ["session_import_continue_workspace", { draftId: "draft", draftHash: "hash", workspaceId: null }],
    ["session_import_continue_restore", { draftId: "draft", draftHash: "hash" }],
    ["session_import_continue_create", { draftId: "draft", draftHash: "hash", requestId: "request", editedText: "edited text" }],
    ["session_import_continue_discard", { draftId: "draft", draftHash: "hash" }],
  ]);
});

test("native draft review and CAS save never invoke send or connect", async () => {
  await sessionImportContinuationGet("native-session");
  await sessionImportContinuationSave("native-session", "reviewed-text-hash", "edited context");
  expect(invoke.mock.calls).toEqual([
    ["session_import_continuation_get", { appSessionId: "native-session" }],
    ["session_import_continuation_save", { appSessionId: "native-session", expectedTextHash: "reviewed-text-hash", text: "edited context" }],
  ]);
});
