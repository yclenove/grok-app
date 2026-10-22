import { expect, test } from "vitest";
import { peekSummaryFromAdapter } from "./peekSummary";

test("reads inventory, issues, outcome, and graph leaf from adapter JSON", () => {
  const summary = peekSummaryFromAdapter({
    outcome: "partial",
    inventory: {
      imported: { messages: 4, toolCalls: 2, toolResults: 2, attachments: 1 },
      missing: { attachments: 1 },
      unsupported: { messages: 1, toolCalls: 0, toolResults: 0 },
      sourceTokens: 8800,
    },
    issues: [{ code: "unknown-block", message: "skipped widget" }],
    graph: {
      cwd: "/proj",
      activeLeaf: "leaf-9",
      events: [{}, {}, {}],
    },
  });
  expect(summary.outcome).toBe("partial");
  expect(summary.messages).toBe(4);
  expect(summary.tools).toBe(2);
  expect(summary.attachments).toBe(1);
  expect(summary.missing).toBe(1);
  expect(summary.unsupported).toBe(1);
  expect(summary.sourceTokens).toBe(8800);
  expect(summary.eventCount).toBe(3);
  expect(summary.cwd).toBe("/proj");
  expect(summary.leaf).toBe("leaf-9");
  expect(summary.issues[0].message).toBe("skipped widget");
});

test("empty adapter is zeros, not a throw", () => {
  expect(peekSummaryFromAdapter(null).messages).toBe(0);
  expect(peekSummaryFromAdapter({}).issues).toEqual([]);
});
