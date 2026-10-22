import { describe, expect, it } from "vitest";
import type { ChatMessage } from "@/lib/session";
import {
  createStreamA11yTracker,
  resolveStreamA11yAnnouncement,
} from "./streamA11y";

const assistant = (id: string, streaming: boolean, isError = false): ChatMessage => ({
  id,
  role: "assistant",
  content: streaming ? "" : "done",
  streaming,
  isError,
});

describe("stream accessibility announcements", () => {
  it("announces completion only when the streamed assistant row settles", () => {
    const tracker = createStreamA11yTracker();
    expect(
      resolveStreamA11yAnnouncement(tracker, {
        streaming: true,
        messages: [assistant("a1", true)],
        hasError: false,
      }),
    ).toBe("streaming");
    expect(
      resolveStreamA11yAnnouncement(tracker, {
        streaming: false,
        messages: [assistant("a1", false)],
        hasError: false,
      }),
    ).toBe("done");
  });

  it("does not call a rejected turn complete after its optimistic row is removed", () => {
    const tracker = createStreamA11yTracker();
    resolveStreamA11yAnnouncement(tracker, {
      streaming: true,
      messages: [assistant("old", false), assistant("pending", true)],
      hasError: false,
    });
    expect(
      resolveStreamA11yAnnouncement(tracker, {
        streaming: false,
        messages: [assistant("old", false)],
        hasError: true,
      }),
    ).toBe("clear");
  });

  it("does not announce completion for an error assistant", () => {
    const tracker = createStreamA11yTracker();
    resolveStreamA11yAnnouncement(tracker, {
      streaming: true,
      messages: [assistant("a1", true)],
      hasError: false,
    });
    expect(
      resolveStreamA11yAnnouncement(tracker, {
        streaming: false,
        messages: [assistant("a1", false, true)],
        hasError: false,
      }),
    ).toBe("clear");
  });
});
