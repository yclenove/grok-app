import { beforeEach, expect, test, vi } from "vitest";
import {
  sessionImportGroupSelectionChoose, sessionImportGroupSelectionClear,
  sessionImportGroupSelectionFreeze, sessionImportGroupSelectionGet,
  sessionImportGroupSelectionInvert, sessionImportGroupSelectionMembership,
  sessionImportGroupSelectionPage, sessionImportGroupSelectionSet,
} from "./sessionMigration";
const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("./host", () => ({ invoke }));
beforeEach(() => vi.resetAllMocks());
const owner = { scanId: "scan", epoch: 1, selectionId: "groups", expectedStateVersion: 3 };

test("fixed-group lookup preserves its independent domain descriptor", async () => {
  const result = { domain: "fixed-groups", selectionId: "groups", selectedCount: 2 };
  invoke.mockResolvedValue(result);
  expect(await sessionImportGroupSelectionGet("scan", 1)).toBe(result);
  expect(invoke).toHaveBeenCalledExactlyOnceWith("session_import_group_selection_get", { scanId: "scan", epoch: 1 });
});

test.each([
  ["freeze", () => sessionImportGroupSelectionFreeze(owner), "session_import_group_selection_freeze"],
  ["clear", () => sessionImportGroupSelectionClear(owner), "session_import_group_selection_clear"],
] as const)("%s forwards the expected version without raw-candidate fallback", async (_name, operation, command) => {
  const result = { stateVersion: 4 };
  invoke.mockResolvedValue(result);
  expect(await operation()).toBe(result);
  expect(invoke).toHaveBeenCalledExactlyOnceWith(command, owner);
});

test("set and invert send bounded group IDs, not candidate IDs", async () => {
  await sessionImportGroupSelectionSet(owner, ["group-a"], false);
  expect(invoke).toHaveBeenLastCalledWith("session_import_group_selection_set", { ...owner, groupIds: ["group-a"], selected: false });
  await sessionImportGroupSelectionInvert(owner, ["group-a", "group-b"]);
  expect(invoke).toHaveBeenLastCalledWith("session_import_group_selection_invert", { ...owner, groupIds: ["group-a", "group-b"] });
  expect(invoke).toHaveBeenCalledTimes(2);
});

test("member switch preserves the complete group/member/version claim and change result", async () => {
  const result = { groupId: "group-a", previousCandidateId: "member-a", candidateId: "member-b", selected: false };
  invoke.mockResolvedValue(result);
  expect(await sessionImportGroupSelectionChoose(owner, "group-a", "member-b")).toBe(result);
  expect(invoke).toHaveBeenCalledExactlyOnceWith("session_import_group_selection_choose", { ...owner, groupId: "group-a", candidateId: "member-b" });
});

test("pages and membership retain fixed-selection ownership and opaque cursor/filter claims", async () => {
  await sessionImportGroupSelectionPage("scan", 1, "groups", { cursor: "signed", limit: 20, selectedOnly: false });
  expect(invoke).toHaveBeenLastCalledWith("session_import_group_selection_page", { scanId: "scan", epoch: 1, selectionId: "groups", cursor: "signed", limit: 20, selectedOnly: false });
  await sessionImportGroupSelectionMembership("scan", 1, "groups", ["group-a"]);
  expect(invoke).toHaveBeenLastCalledWith("session_import_group_selection_membership", { scanId: "scan", epoch: 1, selectionId: "groups", groupIds: ["group-a"] });
});

test("stale member edits propagate instead of automatically retrying against a newer selection", async () => {
  invoke.mockRejectedValue(new Error("PLAN_STALE: reload selection"));
  await expect(sessionImportGroupSelectionChoose(owner, "group-a", "member-b")).rejects.toThrow("PLAN_STALE");
  expect(invoke).toHaveBeenCalledTimes(1);
});
