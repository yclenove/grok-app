import { expect, test } from "vitest";
import type { WorkspaceMapping } from "@/lib/api/sessionMigration";
import { isPrimaryMapping, mappingRows } from "./mappingView";

function row(partial: Partial<WorkspaceMapping> & { rootId: string }): WorkspaceMapping {
  return {
    unassigned: false,
    userChoiceVersion: 1,
    ...partial,
  };
}

test("primary follows the mapping flag, not list order", () => {
  const rows = [
    row({ rootId: "a", primary: false, explicitTarget: "C:\\a" }),
    row({ rootId: "b", primary: true, explicitTarget: "C:\\b" }),
  ];
  expect(isPrimaryMapping(rows[0], 0, rows)).toBe(false);
  expect(isPrimaryMapping(rows[1], 1, rows)).toBe(true);
});

test("mapping labels keep the cwd suggestion when the user picked another project", () => {
  const listed = mappingRows([
    row({
      rootId: "r1",
      explicitTarget: "D:\\picked",
      suggestedTarget: "C:\\src",
      targetProjectId: "user-proj",
    }),
  ]);
  expect(listed[0].label).toBe("D:\\picked");
  expect(listed[0].suggestedTarget).toBe("C:\\src");
  expect(listed[0].targetProjectId).toBe("user-proj");
});

test("same path from different sources remains independently labelled and mapped", () => {
  const listed = mappingRows([
    row({ rootId: "source-a", source: { sourceId: "a", provider: "codex" }, suggestedTarget: "/repo", targetProjectId: "p1" }),
    row({ rootId: "source-b", source: { sourceId: "b", provider: "codex" }, suggestedTarget: "/repo", targetProjectId: "p2" }),
  ], [{ sourceId: "a", provider: "codex", profile: "laptop", rootPath: "/archive/a" }]);
  expect(listed).toHaveLength(2);
  expect(listed.map((item) => item.label)).toEqual(["/repo", "/repo"]);
  expect(listed.map((item) => item.targetProjectId)).toEqual(["p1", "p2"]);
  expect(listed[0].sourceLabel).toBe("codex · laptop · /archive/a · a");
  expect(listed[1].sourceLabel).toBe("codex · b");
  expect(listed[1].source).toEqual({ sourceId: "b", provider: "codex" });
});
