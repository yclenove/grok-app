import { expect, test } from "vitest";
import { scanUiStatus } from "./scanStatus";

test("a failed scan is not an empty result", () => {
  expect(scanUiStatus({ state: "failed", candidateCount: 0 })).toBe("failed");
});

test("scanning with no rows yet is not empty", () => {
  expect(scanUiStatus({ state: "scanning", candidateCount: 0 })).toBe("scanning");
  expect(scanUiStatus({ state: "queued", candidateCount: 0 })).toBe("queued");
  expect(scanUiStatus({ state: "cancelling", candidateCount: 2 })).toBe("cancelling");
  expect(scanUiStatus({ state: "paused", candidateCount: 0 })).toBe("paused");
});

test("completed with no sessions is empty; rows are ready", () => {
  expect(scanUiStatus({ state: "completed", candidateCount: 0 })).toBe("empty");
  expect(scanUiStatus({ state: "completed", candidateCount: 3 })).toBe("ready");
  expect(scanUiStatus({ state: null, candidateCount: 0 })).toBe("idle");
});
