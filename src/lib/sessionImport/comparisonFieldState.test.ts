import { expect, test } from "vitest";
import type { SourceComparisonFieldPage } from "@/lib/api/sessionMigrationComparisonField";
import { comparisonFieldContext, comparisonFieldPage } from "@/test/sessionImportComparisonFieldFixture";
import { validateFieldContext, validateFieldPage } from "./comparisonFieldState";

test("available segments and unavailable fields have distinct bounded schemas", () => {
  const context = comparisonFieldContext();
  validateFieldContext(context);
  validateFieldPage(context, "kind", comparisonFieldPage(context), 0, null);
  validateFieldPage(context, "summary", comparisonFieldPage(context, "summary"), 0, null);
});
const invalid: Array<[string, (page: SourceComparisonFieldPage) => void]> = [
  ["version", (p) => { p.version = 99 as 1; }],
  ["pair", (p) => { p.left.previewId = "foreign"; }],
  ["reversed", (p) => { [p.left, p.right] = [p.right, p.left]; }],
  ["leaf", (p) => { p.right.leafId = "other"; }],
  ["side", (p) => { p.side = "right"; }],
  ["position", (p) => { p.position = 2; }],
  ["event", (p) => { p.eventId = "other"; }],
  ["body digest", (p) => { p.fieldsDigest = "b".repeat(64); }],
  ["field", (p) => { p.field = "role"; }],
  ["field digest", (p) => { p.fieldDigest = "bogus"; }],
  ["start", (p) => { p.startByte = 1; }],
  ["length", (p) => { ++p.endByte; }],
  ["total", (p) => { p.totalBytes = -1; }],
  ["complete", (p) => { p.complete = true; }],
  ["loop", (p) => { p.nextCursor = "attempt"; }],
  ["missing cursor", (p) => { p.nextCursor = null; }],
  ["empty progress", (p) => { p.text = ""; p.endByte = 0; }],
  ["duplicates", (p) => { p.fieldNames.push("kind"); }],
  ["pointer", (p) => { p.fieldNames.push("/payload/path"); }],
  ["unlisted", (p) => { p.fieldNames = ["role"]; }],
  ["unavailable with body", (p) => { p.available = false; }],
  ["surrogate", (p) => { p.text = "\ud800"; p.endByte = 3; }],
  ["over budget", (p) => { p.text = "x".repeat(4097); p.endByte = 4097; p.totalBytes = 5000; }],
];
test.each(invalid)("rejects malformed %s", (_name, mutate) => {
  const context = comparisonFieldContext(), page = comparisonFieldPage(context);
  mutate(page);
  expect(() => validateFieldPage(context, "kind", page, 0, null, "attempt")).toThrow();
});
test("field identity, total, availability and names remain fixed across pages", () => {
  const context = comparisonFieldContext(), previous = comparisonFieldPage(context), next = comparisonFieldPage(context, "kind", 4);
  validateFieldPage(context, "kind", next, 4, previous);
  for (const mutate of [(p: SourceComparisonFieldPage) => { p.fieldDigest = "b".repeat(64); },
    (p: SourceComparisonFieldPage) => { p.totalBytes = 20; }, (p: SourceComparisonFieldPage) => { p.fieldNames.push("summary"); }]) {
    const bad = structuredClone(next); mutate(bad);
    expect(() => validateFieldPage(context, "kind", bad, 4, previous)).toThrow();
  }
});
test("null-value text is an available JSON field, not absent content", () => {
  const context = comparisonFieldContext(), page = comparisonFieldPage(context);
  Object.assign(page, { text: "null", totalBytes: 4, endByte: 4, nextCursor: null, complete: true });
  validateFieldPage(context, "kind", page, 0, null);
});
