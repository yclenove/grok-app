import { expect, test } from "vitest";
import { previewComparisonContext, previewComparisonMember, previewComparisonResponse } from "@/test/sessionImportComparisonPreviewFixture";
import { comparisonContextKey, comparisonPairOf, comparisonPreviewSummary, copyComparisonContext,
  emptyComparisonPreviews, validateComparisonContext, validateComparisonMember } from "./comparisonPreviewState";

test("pins separate context objects but metadata-only growth does not change its owner key", () => {
  const context = previewComparisonContext();
  validateComparisonContext(context);
  const copy = copyComparisonContext(context);
  const key = comparisonContextKey(context);
  context.group.memberCount++;
  context.primary.title = "metadata update";
  expect(comparisonContextKey(context)).toBe(key);
  context.primary.sourceRevision = "changed";
  context.scope.epoch++;
  expect(comparisonContextKey(context)).not.toBe(key);
  expect(copy.primary.sourceRevision).toBe("revision-a");
  expect(copy.scope.epoch).toBe(1);
  expect(copy.group.memberCount).toBe(2);
});

test.each(["primary", "source", "group", "provider", "host", "root", "format", "candidate"])(
  "refuses foreign/same %s member without choosing fallback", (kind) => {
    const context = previewComparisonContext();
    const member = previewComparisonMember();
    if (kind === "primary") member.candidate = context.primary;
    if (kind === "source") member.sourceId = "foreign";
    if (kind === "group") member.groupId = "another";
    if (kind === "provider") member.candidate.provider = "codex";
    if (kind === "host") context.group.host = "ssh";
    if (kind === "root") member.rootPath = "";
    if (kind === "format") Object.assign(member, { format: "foreign" });
    if (kind === "candidate") member.candidate.candidateId = "candidate:foreign-scan:b";
    expect(() => validateComparisonMember(context, member)).toThrow("PLAN_STALE");
  },
);

test("accepts native preview metadata and adds an active rollback ancestor to branch choices", () => {
  const response = previewComparisonResponse();
  response.adapter.activeLeaf = "rollback-ancestor";
  const summary = comparisonPreviewSummary(response);
  expect(summary.leaf).toBe("rollback-ancestor");
  expect(summary.leaves).toEqual(["rollback-ancestor", "leaf-a", "branch-a"]);
  expect(response.adapter.leaves).toEqual(["leaf-a", "branch-a"]);
});

test.each(["preview-id", "negative-count", "outcome", "leaf", "leaves", "duplicate", "events", "issues"])(
  "refuses malformed %s fixed preview projection", (kind) => {
    const response = previewComparisonResponse();
    if (kind === "preview-id") response.previewId = "";
    if (kind === "negative-count") response.adapter.eventCount = -1;
    if (kind === "outcome") response.adapter.outcome = "unknown";
    if (kind === "leaf") response.adapter.activeLeaf = "";
    if (kind === "leaves") Object.assign(response.adapter, { leaves: [1] });
    if (kind === "duplicate") response.adapter.leaves = ["same", "same"];
    if (kind === "events") Object.assign(response.adapter, { events: Array.from({ length: 101 }, () => ({})) });
    if (kind === "issues") Object.assign(response.adapter, { issues: undefined });
    expect(() => comparisonPreviewSummary(response)).toThrow("PLAN_STALE");
  },
);

test("empty fixed histories stay readable metadata but do not manufacture a comparable branch", () => {
  const response = previewComparisonResponse();
  response.adapter.eventCount = 0;
  response.adapter.leaves = [];
  response.adapter.activeLeaf = "";
  const summary = comparisonPreviewSummary(response);
  expect(summary.leaf).toBeNull();
  expect(summary.eventCount).toBe(0);
  expect(comparisonPairOf(emptyComparisonPreviews())).toBeNull();
});
