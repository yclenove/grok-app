import { describe, expect, it } from "vitest";
import {
  findCurrentTurnAssistantIndex, mergeAssistantFragments, pickLatestTurnTool,
  pickRunningTurnTool, syncTurnToolsIntoAssistant, weaveToolsIntoAssistantSegments,
  type ChatMessage,
} from "./session";

const assistant = (id: string, content: string, streaming = false): ChatMessage => ({ id, role: "assistant", content, streaming });
const tool = (id: string, streaming = false): ChatMessage => ({
  id: `tool-${id}`, role: "tool", marker: "tool_step", content: `tool_step|completed|tool|${id}`,
  toolCallId: id, toolStatus: streaming ? "in_progress" : "completed", streaming,
});
const end = (marker: string, content = marker): ChatMessage => ({ id: "interrupt", role: "tool", marker, content });
const toolIds = (m: ChatMessage) => m.segments?.flatMap((s) => s.kind === "tool" ? [s.toolCallId] : []) ?? [];

describe("assistant continuation after an interrupted turn", () => {
  it.each([
    ["turn_cancelled", "turn_cancelled|cancelled"], ["turn_end", "turn_end|user_stop"],
    ["stream_stall", ""], ["end_of_turn", ""], ["", "turn_cancelled|host_exit"],
  ])("keeps %s between the old response and the continuation", (marker, content) => {
    const rows: ChatMessage[] = [
      { id: "user", role: "user", content: "Use the local fixture" },
      assistant("before-1", "Saved the note"), tool("before"), assistant("before-2", "Verifying"),
      end(marker, content), assistant("after-1", "Resumed the same task"), tool("after"),
      assistant("after-2", "Verified and completed"),
    ];
    const projected = weaveToolsIntoAssistantSegments(rows);
    const replies = projected.filter((m) => m.role === "assistant");
    expect(replies.map((m) => m.id)).toEqual(["before-2", "after-2"]);
    expect(projected.findIndex((m) => m.id === "interrupt")).toBeGreaterThan(projected.indexOf(replies[0]));
    expect(projected.findIndex((m) => m.id === "interrupt")).toBeLessThan(projected.indexOf(replies[1]));
    expect(toolIds(replies[0])).toEqual(["before"]);
    expect(toolIds(replies[1])).toEqual(["after"]);
    expect(projected.find((m) => m.id === "interrupt")).toBe(rows[4]);
    expect(rows.filter((m) => m.role === "assistant")).toHaveLength(4);
  });

  it("does not absorb a resumed finished response into an obsolete streaming response", () => {
    const rows = [assistant("before", "Old response", true), end("turn_cancelled"), assistant("after", "New result")];
    const replies = mergeAssistantFragments(rows).filter((m) => m.role === "assistant");
    expect(replies.map((m) => m.id)).toEqual(["before", "after"]);
    expect(replies[0].content).toBe("Old response");
    expect(replies[1].content).toBe("New result");
  });

  it("attaches live tools only to the assistant on their side of the interruption", () => {
    const rows = [assistant("before", "Old result"), tool("before"), end("turn_cancelled"), assistant("after", "New result"), tool("after")];
    expect(toolIds(syncTurnToolsIntoAssistant(rows, 0)[0])).toEqual(["before"]);
    expect(toolIds(syncTurnToolsIntoAssistant(rows, 3)[3])).toEqual(["after"]);
  });

  it("retired running rows cannot become the current continuation's assistant or tool", () => {
    const rows = [assistant("before", "Old response", true), tool("before", true), end("turn_cancelled"), assistant("after", "New response"), tool("after")];
    expect(findCurrentTurnAssistantIndex(rows)).toBe(3);
    expect(pickLatestTurnTool(rows)?.toolCallId).toBe("after");
    expect(pickRunningTurnTool(rows)).toBeNull();
    expect(findCurrentTurnAssistantIndex(rows.slice(0, 3))).toBe(-1);
  });

  it("does not treat marker text in a reply as an interruption", () => {
    const rows = [assistant("before", "Old response"), assistant("after", "turn_cancelled|cancelled is the journal format")];
    expect(mergeAssistantFragments(rows).filter((m) => m.role === "assistant")).toHaveLength(1);
    expect(findCurrentTurnAssistantIndex(rows)).toBe(1);
  });

  it("keeps context compaction within the same turn", () => {
    const compact = end("context_compact");
    const rows = [assistant("before", "Old response"), tool("before"), compact, assistant("after", "New response"), tool("after")];
    const projected = weaveToolsIntoAssistantSegments(rows);
    expect(projected.filter((m) => m.role === "assistant")).toHaveLength(1);
    expect(toolIds(projected[0])).toEqual(["before", "after"]);
    expect(projected.find((m) => m.id === compact.id)).toBe(compact);
  });

  it("weaves tools before a resumed answer after consecutive terminal markers", () => {
    const rows = [end("turn_cancelled"), { ...end("turn_end"), id: "second-end" }, tool("after"), assistant("after", "New response")];
    const projected = weaveToolsIntoAssistantSegments(rows);
    expect(projected.map((m) => m.id)).toEqual(rows.map((m) => m.id));
    expect(toolIds(projected[3])).toEqual(["after"]);
  });
});
