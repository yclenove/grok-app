import { beforeEach, expect, test, vi } from "vitest";
import { sessionImportScanGroupMember, sessionImportScanGroupMembers, sessionImportScanGroups } from "./sessionMigration";
const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("./host", () => ({ invoke }));
beforeEach(() => vi.resetAllMocks());

test("group pages forward scan, epoch, filter and opaque page claims", async () => {
  const page = { version: 1, groups: [], nextCursor: null, totalCount: 0, complete: true, stateVersion: 2 };
  invoke.mockResolvedValue(page);
  expect(await sessionImportScanGroups("scan", 1, {
    cursor: "opaque", limit: 200, filter: { provider: "cursor", sourceId: "installation" },
  })).toBe(page);
  expect(invoke).toHaveBeenCalledExactlyOnceWith("session_import_scan_groups", {
    scanId: "scan", epoch: 1, cursor: "opaque", limit: 200,
    filter: { provider: "cursor", sourceId: "installation" },
  });
});

test("member pages preserve full fixed source metadata and do not pick another member", async () => {
  const member = { candidate: { candidateId: "pinned", sourceRevision: "original" }, location: { locationId: "fixed-root" } };
  const page = { version: 1, groupId: "group", members: [member], nextCursor: "next", memberCount: 2 };
  invoke.mockResolvedValue(page);
  expect(await sessionImportScanGroupMembers("scan", 1, "group", { limit: 1 })).toBe(page);
  expect(invoke).toHaveBeenCalledExactlyOnceWith("session_import_scan_group_members", {
    scanId: "scan", epoch: 1, groupId: "group", limit: 1,
  });
});

test("expired or corrupt group pages propagate without rescan or relocation", async () => {
  invoke.mockRejectedValue(new Error("SCHEMA_UNSUPPORTED: corrupt group"));
  await expect(sessionImportScanGroups("scan", 1)).rejects.toThrow("SCHEMA_UNSUPPORTED");
  expect(invoke).toHaveBeenCalledTimes(1);
});

test("wrong member owner fails without fallback to a group primary", async () => {
  invoke.mockRejectedValue(new Error("PLAN_STALE: wrong owner"));
  await expect(sessionImportScanGroupMembers("scan", 1, "foreign-group")).rejects.toThrow("PLAN_STALE");
  expect(invoke).toHaveBeenCalledTimes(1);
});

test("exact member metadata forwards only the immutable scoped candidate identity", async () => {
  const member = { candidate: { candidateId: "fixed", sourceRevision: "original" } };
  invoke.mockResolvedValue(member);
  expect(await sessionImportScanGroupMember("scan", 1, "group", "fixed")).toBe(member);
  expect(invoke).toHaveBeenCalledExactlyOnceWith("session_import_scan_group_member", {
    scanId: "scan", epoch: 1, groupId: "group", candidateId: "fixed",
  });
});

test("missing exact member propagates without paging, rescanning or adopting the first member", async () => {
  invoke.mockRejectedValue(new Error("SOURCE_NOT_FOUND: fixed member missing"));
  await expect(sessionImportScanGroupMember("scan", 1, "group", "fixed")).rejects.toThrow("SOURCE_NOT_FOUND");
  expect(invoke).toHaveBeenCalledTimes(1);
});
