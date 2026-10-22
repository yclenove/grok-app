/** @vitest-environment jsdom */
import { createElement } from "react";
import { cleanup, render } from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
import { ImportedEventList } from "@/components/session-import/ImportedEventList";
import {
  capReaderHits,
  MAX_READER_HITS,
  MAX_RENDERED_EVENTS,
  windowReaderEvents,
} from "./readerWindow";
import { retainRecentEvents } from "./readerBudget";

afterEach(() => cleanup());

test("hit state remains bounded while paging forward", () => {
  const source = Array.from({ length: 10_000 }, (_, i) => ({ eventId: `e${i}` }));
  const result = capReaderHits(source);
  expect(result.windowed).toBe(true);
  expect(result.items).toHaveLength(MAX_READER_HITS);
  expect(result.items[0].eventId).toBe(`e${10_000 - MAX_READER_HITS}`);
  expect(result.items.at(-1)?.eventId).toBe("e9999");
});

test.each(["e0", "e5000", "e9999"])(
  "10,000 event DOM window stays bounded and contains anchor %s",
  (anchor) => {
    const source = Array.from({ length: 10_000 }, (_, i) => ({
      id: `e${i}`,
      kind: "message",
      role: "user",
      blocks: [{ kind: "text", text: `body-${i}` }],
    }));
    const result = windowReaderEvents(source, anchor);
    expect(result.windowed).toBe(true);
    expect(result.items).toHaveLength(MAX_RENDERED_EVENTS);
    expect(result.items.some((event) => (event as { id: string }).id === anchor)).toBe(true);
    render(
      createElement(ImportedEventList, {
        events: result.items,
        highlightId: anchor,
        tr: (key) => key,
      }),
    );
    expect(document.querySelectorAll(".session-import__events > li")).toHaveLength(
      MAX_RENDERED_EVENTS,
    );
  },
);

test("a single event over the retained byte budget remains addressable", () => {
  const event = {
    id: "oversize",
    kind: "message",
    role: "assistant",
    blocks: [{ kind: "text", text: "x".repeat(2 * 1024 * 1024 + 1) }],
  };
  const retained = retainRecentEvents([event]);
  expect(retained).toHaveLength(1);
  expect((retained[0] as { id: string }).id).toBe("oversize");
});
