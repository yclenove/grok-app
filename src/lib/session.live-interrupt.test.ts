import { describe, expect, it } from "vitest";
import { currentTurnHasEndMarker } from "./endOfTurn";
import {
  applyStreamChunk,
  applyTurnMarker,
  ensureBusyTurnStreaming,
  ensureContinuationForHostSnapshot,
  weaveToolsIntoAssistantSegments,
  type ChatMessage,
} from "./session";

const before: ChatMessage = {
  id: "before", role: "assistant", content: "Saved once; count 5", streaming: false,
};
const pause: ChatMessage = {
  id: "pause", role: "tool", marker: "turn_cancelled", content: "turn_cancelled|cancelled",
};
const rows = (): ChatMessage[] => [
  { id: "user", role: "user", content: "Count to ten" }, before, pause,
];
const stream = (
  messages: ChatMessage[],
  kind: "assistant" | "thought",
  messageId = "resumed",
) => applyStreamChunk(messages, {
  sessionId: "s", messageId, kind, text: "Continue from count 5", done: false,
});

describe("live continuation across Pause and Stop", () => {
  it.each(["assistant", "thought"] as const)("starts a separate %s response without another user message", (kind) => {
    const result = stream(rows(), kind);
    expect(result[1]).toBe(before);
    expect(result[2]).toBe(pause);
    expect(result[3]).toMatchObject({ id: "resumed", role: "assistant", streaming: true });
    expect(kind === "thought" ? result[3].thought : result[3].content).toBe("Continue from count 5");
  });

  it("keeps body and tools on the resumed side after fresh thought", () => {
    let messages = stream(rows(), "thought");
    messages.push({
      id: "tool-new", role: "tool", marker: "tool_step", toolCallId: "new",
      content: "tool_step|completed|click|new", toolStatus: "completed",
    });
    messages = stream(messages, "assistant");
    const projected = weaveToolsIntoAssistantSegments(messages);
    const replies = projected.filter((m) => m.role === "assistant");
    expect(replies.map((m) => m.id)).toEqual(["before", "resumed"]);
    expect(replies[0].content).toBe(before.content);
    expect(replies[0].segments?.some((s) => s.kind === "tool")).toBeFalsy();
    expect(replies[1].segments?.flatMap((s) => s.kind === "tool" ? [s.toolCallId] : [])).toEqual(["new"]);
  });

  it.each(["assistant", "thought"] as const)("ignores a retired named %s chunk after the terminal boundary", (kind) => {
    const messages = stream(rows(), "assistant");
    expect(stream(messages, kind, "before")).toBe(messages);
    expect(stream(rows(), kind, "before")).toEqual(rows());
  });

  it("adds a new Stop after resumed output and deduplicates its Host echo", () => {
    const resumed = stream(rows(), "assistant");
    expect(currentTurnHasEndMarker(resumed)).toBe(false);
    const stopped = applyTurnMarker(resumed, {
      messageId: "local-stop", marker: "turn_end", reason: "user_stop", content: "turn_end|user_stop",
    });
    expect(stopped.map((m) => m.id)).toEqual(["user", "before", "pause", "resumed", "local-stop"]);
    expect(stopped[3].streaming).toBe(false);
    expect(currentTurnHasEndMarker(stopped)).toBe(true);
    expect(applyTurnMarker(stopped, {
      messageId: "host-stop", marker: "turn_cancelled", reason: "user_stop",
    })).toBe(stopped);
  });

  it("recognizes a new empty continuation shell but ignores delayed old tool receipts", () => {
    const shell: ChatMessage[] = [
      ...rows(), { id: "a-pending-resume", role: "assistant", content: "", streaming: true },
    ];
    expect(currentTurnHasEndMarker(shell)).toBe(false);
    const lateTool: ChatMessage[] = [
      ...rows(), { id: "late-tool", role: "tool", marker: "tool_step", content: "completed" },
    ];
    expect(currentTurnHasEndMarker(lateTool)).toBe(true);
  });

  it("supports legacy terminal rows and keeps ordinary marker text within a response", () => {
    const legacy = rows();
    legacy[2] = { ...legacy[2], marker: undefined };
    expect(stream(legacy, "assistant")[3]?.id).toBe("resumed");
    const plain: ChatMessage[] = [
      before, { id: "text", role: "assistant", content: "turn_cancelled|cancelled describes the format" },
    ];
    expect(stream(plain, "assistant")).toHaveLength(2);
  });

  it("a late done-only old chunk does not settle the resumed assistant", () => {
    const messages = stream(rows(), "assistant");
    const result = applyStreamChunk(messages, {
      sessionId: "s", messageId: "before", kind: "assistant", text: "", done: true,
    });
    expect(result[3]?.streaming).toBe(true);
  });

  it("busy journal rehydration does not revive the pre-Pause response", () => {
    expect(ensureBusyTurnStreaming(rows(), "streaming")[1]).toBe(before);
  });

  it("fresh Host state opens a continuation before tokens, so early Stop is recorded", () => {
    const snapshot = { state: "streaming" as const, streamingMessageId: "host-new" };
    const pending = ensureContinuationForHostSnapshot(rows(), snapshot);
    expect(currentTurnHasEndMarker(pending)).toBe(false);
    expect(ensureContinuationForHostSnapshot(pending, snapshot)).toBe(pending);
    const stopped = applyTurnMarker(pending, { messageId: "new-stop", reason: "user_stop" });
    expect(stopped.at(-1)?.id).toBe("new-stop");
    expect(ensureContinuationForHostSnapshot(stopped, snapshot)).toBe(stopped);
    expect(stream(pending, "assistant", "actual-host-id").at(-1)?.id).toBe("actual-host-id");
  });

  it("late retired or stop-latched Host states cannot reopen the old response", () => {
    const messages = rows();
    expect(ensureContinuationForHostSnapshot(messages, { state: "streaming", streamingMessageId: "before" })).toBe(messages);
    expect(ensureContinuationForHostSnapshot(messages, { state: "ready", streamingMessageId: "new" })).toBe(messages);
    expect(ensureContinuationForHostSnapshot(messages, { state: "streaming", streamingMessageId: null })).toBe(messages);
  });
});
