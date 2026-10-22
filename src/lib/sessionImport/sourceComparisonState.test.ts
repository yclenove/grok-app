import { expect, test } from "vitest";
import { comparisonOwner, comparisonPage } from "@/test/sessionImportComparisonFixture";
import { copyComparisonOwner, validateComparisonOwner, validateComparisonPage } from "./sourceComparisonState";

test("fixed pair copy includes separate scan, both preview IDs, revisions and leaves", () => {
  const owner = comparisonOwner();
  validateComparisonOwner(owner);
  const copy = copyComparisonOwner(owner);
  owner.scope.generation = 9;
  owner.left.leafId = "later";
  owner.right.sourceRevision = "new";
  expect(copy.scope.generation).toBe(1);
  expect(copy.left.leafId).toBe("main");
  expect(copy.right.sourceRevision).toBe("revision-B");
});

test("accepts two bounded pages and explicit missing tail without an equivalence claim", () => {
  const owner = comparisonOwner();
  const first = comparisonPage(owner);
  validateComparisonPage(owner, first, 0, null);
  const last = comparisonPage(owner, 2);
  validateComparisonPage(owner, last, 2, first);
  expect(last.complete).toBe(true);
  expect(last.equivalence).toBe("unverified");
  expect(last.rows[0].right).toBeNull();
});

test.each(["same-candidate", "same-preview", "foreign-candidate", "empty-leaf", "epoch", "generation"])(
  "rejects invalid pair %s before a read", (kind) => {
    const owner = comparisonOwner();
    if (kind === "same-candidate") owner.right.candidateId = owner.left.candidateId;
    if (kind === "same-preview") owner.right.previewId = owner.left.previewId;
    if (kind === "foreign-candidate") owner.right.candidateId = "candidate:other-scan:B";
    if (kind === "empty-leaf") owner.left.leafId = "";
    if (kind === "epoch") owner.scope.epoch = 0;
    if (kind === "generation") owner.scope.generation = -1;
    expect(() => validateComparisonOwner(owner)).toThrow("PLAN_STALE");
  },
);

test.each(["version", "alignment", "equivalence"])("rejects unsupported %s semantics", (kind) => {
  const owner = comparisonOwner();
  const page = comparisonPage(owner);
  Object.assign(page, { [kind]: kind === "version" ? 2 : "verified" });
  expect(() => validateComparisonPage(owner, page, 0, null)).toThrow("SCHEMA_UNSUPPORTED");
});

test.each([
  "left-owner", "right-owner", "group", "revision", "leaf", "summary-count", "title-type", "outcome",
  "row-limit", "row-position", "gap", "empty-forward", "total", "complete", "cursor",
  "excerpt-limit", "digest", "span", "time-origin", "event-kind", "row-uncertain", "metadata", "missing-page-uncertainty",
])("rejects mixed/broken DTO %s without adopting a page", (kind) => {
  const owner = comparisonOwner();
  const page = comparisonPage(owner);
  if (kind === "left-owner") page.left.previewId = "foreign";
  if (kind === "right-owner") page.right.candidateId = owner.left.candidateId;
  if (kind === "group") page.right.groupId = "another-group";
  if (kind === "revision") page.left.sourceRevision = "new-revision";
  if (kind === "leaf") page.right.leafId = "other-leaf";
  if (kind === "summary-count") page.left.eventCount = -1;
  if (kind === "title-type") Object.assign(page.left, { title: 5 });
  if (kind === "outcome") Object.assign(page.right, { outcome: "ready" });
  if (kind === "row-limit") page.rows = Array.from({ length: 51 }, () => page.rows[0]);
  if (kind === "row-position") page.rows[0].position = -1;
  if (kind === "gap") page.rows[1].position = 2;
  if (kind === "empty-forward") page.rows = [];
  if (kind === "total") page.totalPositions = 4;
  if (kind === "complete") page.complete = true;
  if (kind === "cursor") page.nextCursor = null;
  if (kind === "excerpt-limit") page.rows[0].left!.excerpt = "😀".repeat(513);
  if (kind === "digest") page.rows[0].right!.fieldsDigest = "";
  if (kind === "span") page.rows[0].left!.spans[0].block = -1;
  if (kind === "time-origin") Object.assign(page.rows[0].left!, { timeOrigin: "import-time" });
  if (kind === "event-kind") Object.assign(page.rows[0].left!, { kind: "future-event" });
  if (kind === "row-uncertain") Object.assign(page.rows[0], { uncertain: undefined });
  if (kind === "metadata") Object.assign(page, { metadataDifferences: ["title", "foreign"] });
  if (kind === "missing-page-uncertainty") page.rows[0].uncertain = true;
  expect(() => validateComparisonPage(owner, page, 0, null)).toThrow("PLAN_STALE");
});

test.each(["snapshotDigest", "eventCount", "title", "cwd", "outcome"] as const)(
  "rejects changed fixed summary %s between pages", (field) => {
    const owner = comparisonOwner();
    const previous = comparisonPage(owner);
    const page = comparisonPage(owner, 2);
    Object.assign(page.right, { [field]: field === "eventCount" ? 3 : field === "outcome" ? "partial" : "changed" });
    expect(() => validateComparisonPage(owner, page, 2, previous)).toThrow("PLAN_STALE");
  },
);

test.each(["tail-present", "missing-marker", "missing-uncertainty"])("rejects broken missing tail %s", (kind) => {
  const owner = comparisonOwner();
  const page = comparisonPage(owner, 2);
  if (kind === "tail-present") page.rows[0].right = { ...page.rows[0].left! };
  if (kind === "missing-marker") page.rows[0].differences = [];
  if (kind === "missing-uncertainty") page.rows[0].uncertain = false;
  expect(() => validateComparisonPage(owner, page, 2, comparisonPage(owner))).toThrow("PLAN_STALE");
});

test("bounds encoded response bytes, not JavaScript characters", () => {
  const owner = comparisonOwner();
  const page = comparisonPage(owner);
  page.rows[0].left!.spans[0].record = "壁".repeat(90_000);
  expect(() => validateComparisonPage(owner, page, 0, null)).toThrow("RESOURCE_LIMIT");
});

test("empty branch pair terminates normally and partial outcomes remain uncertain", () => {
  const owner = comparisonOwner();
  const page = comparisonPage(owner);
  page.left.eventCount = 0;
  page.right.eventCount = 0;
  page.left.outcome = "partial";
  page.totalPositions = 0;
  page.rows = [];
  page.complete = true;
  page.nextCursor = null;
  page.uncertain = true;
  expect(() => validateComparisonPage(owner, page, 0, null)).not.toThrow();
  page.uncertain = false;
  expect(() => validateComparisonPage(owner, page, 0, null)).toThrow("PLAN_STALE");
});
