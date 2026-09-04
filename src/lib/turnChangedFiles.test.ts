import { describe, expect, it } from "vitest";
import type { TimelineUnit } from "./timelinePhases";
import {
  collectTurnModifiedPaths,
  splitTurnChangedFiles,
  turnChangedFileItems,
  TURN_CHANGED_FILES_VISIBLE_MAX,
} from "./turnChangedFiles";

function toolUnit(
  id: string,
  toolKind: string,
  path?: string,
): TimelineUnit {
  return {
    kind: "tool",
    si: 0,
    tool: {
      kind: "tool",
      toolCallId: id,
      title: toolKind,
      toolKind,
      status: "completed",
      path,
    },
  };
}

describe("collectTurnModifiedPaths", () => {
  it("collects unique edit paths in encounter order", () => {
    const units: TimelineUnit[] = [
      toolUnit("r1", "read_file", "/p/a.ts"),
      toolUnit("e1", "search_replace", "/p/a.ts"),
      toolUnit("e2", "write", "/p/b.ts"),
      toolUnit("e3", "search_replace", "/p/a.ts"),
      toolUnit("b1", "bash", "/tmp/out.log"),
    ];
    expect(collectTurnModifiedPaths(units)).toEqual([
      "/p/a.ts",
      "/p/b.ts",
    ]);
  });

  it("reads paths from phase tools", () => {
    const units: TimelineUnit[] = [
      {
        kind: "phase",
        id: "p0",
        items: [],
        thoughts: [],
        tools: [
          {
            kind: "tool",
            toolCallId: "e1",
            title: "Edit",
            toolKind: "str_replace",
            status: "completed",
            path: "/proj/src/App.tsx",
          },
        ],
        startSi: 0,
        endSi: 0,
        live: false,
        errorCount: 0,
        runningCount: 0,
      },
    ];
    expect(collectTurnModifiedPaths(units)).toEqual(["/proj/src/App.tsx"]);
  });

  it("returns empty when no edit tools have paths", () => {
    expect(
      collectTurnModifiedPaths([
        toolUnit("r1", "read_file", "/p/a.ts"),
        toolUnit("e1", "search_replace"),
      ]),
    ).toEqual([]);
  });
});

describe("turnChangedFileItems", () => {
  it("maps paths to basenames", () => {
    expect(turnChangedFileItems(["/a/b/c.ts", "plain.md"])).toEqual([
      { path: "/a/b/c.ts", name: "c.ts" },
      { path: "plain.md", name: "plain.md" },
    ]);
  });
});

describe("splitTurnChangedFiles", () => {
  it("keeps all items when under the visible max", () => {
    const items = turnChangedFileItems(["/a.ts", "/b.ts"]);
    expect(splitTurnChangedFiles(items)).toEqual({
      visible: items,
      hiddenCount: 0,
    });
  });

  it("hides overflow past the visible max", () => {
    const paths = Array.from(
      { length: TURN_CHANGED_FILES_VISIBLE_MAX + 3 },
      (_, i) => `/f${i}.ts`,
    );
    const items = turnChangedFileItems(paths);
    const split = splitTurnChangedFiles(items);
    expect(split.visible).toHaveLength(TURN_CHANGED_FILES_VISIBLE_MAX);
    expect(split.hiddenCount).toBe(3);
  });
});
