import { expect, test } from "vitest";
import { MAX_READER_EVENTS, retainRecentEvents } from "./readerBudget";

test("drops the oldest events once eight pages are exceeded", () => {
  const list = Array.from({ length: MAX_READER_EVENTS + 25 }, (_, i) => ({ id: `e${i}` }));
  const kept = retainRecentEvents(list) as Array<{ id: string }>;
  expect(kept).toHaveLength(MAX_READER_EVENTS);
  expect(kept[0].id).toBe("e25");
  expect(kept.at(-1)?.id).toBe(`e${MAX_READER_EVENTS + 24}`);
});

test("drops oldest events when text exceeds two mebibytes", () => {
  const bulky = { id: "big", text: "汉".repeat(200_000) };
  const list = Array.from({ length: 40 }, (_, i) =>
    i < 5 ? bulky : { id: `s${i}`, text: "x" },
  );
  const kept = retainRecentEvents(list) as Array<{ id: string }>;
  expect(kept.length).toBeLessThan(list.length);
  expect(kept.at(-1)?.id).toBe("s39");
});
