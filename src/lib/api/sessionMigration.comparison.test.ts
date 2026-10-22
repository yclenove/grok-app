import { beforeEach, expect, test, vi } from "vitest";
import {
  sessionImportSourceComparison,
  type SourceComparisonEvent,
  type SourceComparisonPage,
  type SourceComparisonRequest,
} from "./sessionMigration";

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("./host", () => ({ invoke }));
beforeEach(() => vi.resetAllMocks());

function request(): SourceComparisonRequest {
  return {
    version: 1,
    left: { previewId: "preview-A", candidateId: "member-A", sourceRevision: "rev-A", leafId: "leaf-A" },
    right: { previewId: "preview-B", candidateId: "member-B", sourceRevision: "rev-B", leafId: "leaf-B" },
  };
}

function page(): SourceComparisonPage {
  const owner = request();
  const event: SourceComparisonEvent = {
    eventId: "event-A", kind: "message", timestamp: null, timeOrigin: "unknown",
    spans: [{ artifactId: "source-A", record: "line:2", block: 0 }],
    excerpt: "相同摘要", truncated: true, fieldsDigest: "full-body-A",
  };
  return {
    version: 1, alignment: "position-only", equivalence: "unverified",
    left: { ...owner.left, snapshotDigest: "snapshot-A", groupId: "group", eventCount: 2,
      title: "A", cwd: null, outcome: "partial" },
    right: { ...owner.right, snapshotDigest: "snapshot-B", groupId: "group", eventCount: 1,
      title: null, cwd: "D:\\work", outcome: "complete" },
    metadataDifferences: ["title", "cwd", "relations"], totalPositions: 2,
    rows: [
      { position: 0, left: event, right: { ...event, eventId: "event-B", fieldsDigest: "full-body-B" },
        differences: ["blocks", "role"], metadataDifferences: ["event-id", "source-time"], uncertain: true },
      { position: 1, left: { ...event, eventId: "tail-A" }, right: null,
        differences: ["missing-side"], metadataDifferences: [], uncertain: true },
    ],
    nextCursor: null, complete: true, uncertain: true,
  };
}

test("forwards both fixed preview/branch scopes through the public API barrel", async () => {
  const owner = request();
  const result = page();
  invoke.mockResolvedValue(result);
  expect(await sessionImportSourceComparison(owner)).toBe(result);
  expect(invoke).toHaveBeenCalledExactlyOnceWith("session_import_source_comparison", { request: owner });
});

test("copies caller scopes synchronously without retaining mutable side references", async () => {
  const initial = request();
  const left = { ...initial.left };
  const right = { ...initial.right };
  const owner: SourceComparisonRequest = { ...initial, left, right };
  const original = structuredClone(owner);
  invoke.mockResolvedValue(page());
  const pending = sessionImportSourceComparison(owner);
  left.previewId = "reopened";
  right.leafId = "changed-branch";
  owner.cursor = "later-page";
  owner.limit = 1;
  expect(invoke).toHaveBeenCalledExactlyOnceWith("session_import_source_comparison", { request: original });
  const sent = invoke.mock.calls[0][1].request;
  expect(sent).not.toBe(owner);
  expect(sent.left).not.toBe(owner.left);
  expect(sent.right).not.toBe(owner.right);
  await pending;
});

test("leaves frozen caller data untouched and emits only declared native request fields", async () => {
  const owner = request();
  const left = Object.freeze({ ...owner.left, selected: true });
  const right = Object.freeze({ ...owner.right, title: "not a comparison claim" });
  const extended = Object.freeze({ ...owner, left, right, scanId: "not a wire field" });
  await sessionImportSourceComparison(extended);
  expect(invoke).toHaveBeenCalledExactlyOnceWith("session_import_source_comparison", { request: owner });
  expect(left.selected).toBe(true);
  expect(right.title).toBe("not a comparison claim");
});

test.each([
  { cursor: " signed:A→B/leaf+snapshot ", limit: 100 },
  { cursor: null, limit: null },
  { cursor: "", limit: 0 },
])("preserves opaque cursor and limit without frontend normalization: %j", async (options) => {
  const owner = { ...request(), ...options };
  await sessionImportSourceComparison(owner);
  expect(invoke).toHaveBeenCalledExactlyOnceWith("session_import_source_comparison", { request: owner });
});

test("does not sort sides or reuse a cursor under a different ordered pair", async () => {
  const owner = request();
  const swapped = { ...owner, left: owner.right, right: owner.left, cursor: "A-B-signed-page" };
  const error = new Error("CURSOR_INVALID: ordered snapshot owner changed");
  invoke.mockRejectedValue(error);
  await expect(sessionImportSourceComparison(swapped)).rejects.toBe(error);
  expect(invoke).toHaveBeenCalledExactlyOnceWith("session_import_source_comparison", { request: swapped });
});

test.each(["PLAN_STALE", "RESOURCE_LIMIT", "SCHEMA_UNSUPPORTED"])(
  "%s propagates without choosing/rescanning/retrying another member", async (code) => {
    const error = new Error(`${code}: fixed preview unavailable`);
    invoke.mockRejectedValue(error);
    await expect(sessionImportSourceComparison(request())).rejects.toBe(error);
    expect(invoke).toHaveBeenCalledTimes(1);
  },
);

test("retains uncertainty, missing tail, provenance and full digests even for matching excerpts", async () => {
  const result = page();
  invoke.mockResolvedValue(result);
  const received = await sessionImportSourceComparison(request());
  expect(received).toBe(result);
  expect(received.complete).toBe(true);
  expect(received.equivalence).toBe("unverified");
  expect(received.rows[0].left?.excerpt).toBe(received.rows[0].right?.excerpt);
  expect(received.rows[0].left?.fieldsDigest).not.toBe(received.rows[0].right?.fieldsDigest);
  expect(received.rows[1].right).toBeNull();
  expect(received.rows[0].left?.spans).toEqual([{ artifactId: "source-A", record: "line:2", block: 0 }]);
  expect(received.uncertain).toBe(true);
});
