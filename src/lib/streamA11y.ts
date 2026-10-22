import type { ChatMessage } from "@/lib/session";

export type StreamA11yAnnouncement = "streaming" | "done" | "clear" | null;

export type StreamA11yTracker = {
  wasStreaming: boolean;
  assistantIds: Set<string>;
};

export function createStreamA11yTracker(): StreamA11yTracker {
  return { wasStreaming: false, assistantIds: new Set() };
}

/** Announce Done only when an assistant row observed streaming still exists settled. */
export function resolveStreamA11yAnnouncement(
  tracker: StreamA11yTracker,
  input: {
    streaming: boolean;
    messages: ChatMessage[];
    hasError: boolean;
  },
): StreamA11yAnnouncement {
  if (input.streaming) {
    for (const message of input.messages) {
      if (message.role === "assistant" && message.streaming) {
        tracker.assistantIds.add(message.id);
      }
    }
    const started = !tracker.wasStreaming;
    tracker.wasStreaming = true;
    return started ? "streaming" : null;
  }

  const ended = tracker.wasStreaming;
  const completed =
    ended &&
    !input.hasError &&
    input.messages.some(
      (message) =>
        tracker.assistantIds.has(message.id) &&
        message.role === "assistant" &&
        !message.streaming &&
        !message.isError,
    );
  tracker.wasStreaming = false;
  tracker.assistantIds.clear();
  return completed ? "done" : ended ? "clear" : null;
}
