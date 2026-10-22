import { useEffect, useRef } from "react";
import type { createT } from "@/i18n";
import type { ChatMessage, SessionSnapshot } from "@/lib/session";
import {
  createStreamA11yTracker,
  resolveStreamA11yAnnouncement,
} from "@/lib/streamA11y";

export function useStreamA11yAnnouncements(input: {
  sessionState: SessionSnapshot["state"];
  sessionError: unknown;
  messages: ChatMessage[];
  localError: string | null;
  tr: ReturnType<typeof createT>;
  setNote: (value: string) => void;
}): void {
  const trackerRef = useRef(createStreamA11yTracker());
  useEffect(() => {
    const streaming =
      input.sessionState === "streaming" ||
      input.messages.some((message) => message.role === "assistant" && message.streaming);
    const announcement = resolveStreamA11yAnnouncement(trackerRef.current, {
      streaming,
      messages: input.messages,
      hasError: Boolean(input.localError || input.sessionError),
    });
    if (announcement === "streaming") input.setNote(input.tr("a11y.assistantStreaming"));
    if (announcement === "clear") input.setNote("");
    if (announcement !== "done") return;
    input.setNote(input.tr("a11y.assistantDone"));
    const timeout = window.setTimeout(() => input.setNote(""), 2500);
    return () => window.clearTimeout(timeout);
  }, [
    input.localError,
    input.messages,
    input.sessionError,
    input.sessionState,
    input.setNote,
    input.tr,
  ]);
}
