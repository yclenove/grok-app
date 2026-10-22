/** @vitest-environment jsdom */
import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
import { useContentSelection } from "./useContentSelection";

afterEach(() => cleanup());

test("hook reducer keeps page selection across load-more and drops a late page", () => {
  const { result } = renderHook(() => useContentSelection());
  act(() => {
    result.current.reset({
      previewId: "p1",
      leaf: "l1",
      leaves: ["l1"],
      events: [{ eventId: "e1", kind: "message", text: "a", selectable: true, defaultSelected: true }],
      nextCursor: "c1",
    });
  });
  const gen = result.current.state.generation;
  act(() => {
    result.current.appendPage({
      type: "appendPage",
      previewId: "p1",
      generation: gen,
      events: [{ eventId: "e2", kind: "message", text: "b", selectable: true, defaultSelected: true }],
      nextCursor: null,
    });
  });
  expect(result.current.state.selectedEventIds).toEqual(["e1", "e2"]);
  act(() => {
    result.current.appendPage({
      type: "appendPage",
      previewId: "p1",
      generation: gen - 1,
      events: [{ eventId: "late", kind: "message", text: "nope", selectable: true, defaultSelected: true }],
      nextCursor: null,
    });
  });
  expect(result.current.state.events.map((e) => e.eventId)).toEqual(["e1", "e2"]);
  expect(result.current.selection.eventIds.sort()).toEqual(["e1", "e2"]);
});
