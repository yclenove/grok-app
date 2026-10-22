import { expect, test } from "vitest";
import { nextJobCursorHistory } from "./jobCursor";

test("job cursor history stays at one entry per visited page", () => {
  let cursors: Array<string | null> = [null];
  for (let page = 0; page < 200; page += 1) {
    cursors = nextJobCursorHistory(cursors, page, `page-${page}`);
  }
  expect(cursors).toHaveLength(201);
  expect(cursors[0]).toBeNull();
  expect(cursors[200]).toBe("page-199");

  const revisited = nextJobCursorHistory(cursors, 0, "page-0-again");
  expect(revisited).toEqual([null, "page-0-again"]);
});
