import { expect, test } from "vitest";
import { fixedTransferPage, fixedTransferRequest } from "@/test/sessionImportFixedPreviewFixture";
import { defaultSelection } from "./selection";
import { copyFixedPreviewTransfer, fixedPreviewContentSlot, type FixedPreviewPage, type FixedPreviewTransferRequest } from "./fixedPreviewTransfer";

test("capture detaches every identity and grant before an async boundary", () => {
  const request = fixedTransferRequest(), captured = copyFixedPreviewTransfer(request);
  request.context.scope.generation++;
  request.member.candidate.candidateId = "changed";
  Object.assign(request.preview.adapter as object, { leaves: [], defaultSelection: null });
  request.comparison.snapshotDigest = "0".repeat(64);
  expect(captured.context.scope.generation).toBe(1);
  expect(captured.member.candidate.candidateId).toBe("candidate:synthetic-scan:b");
  expect(captured.comparison.snapshotDigest).toBe("b".repeat(64));
  expect(fixedPreviewContentSlot(captured, fixedTransferPage(), 11).requestSeq).toBe(11);
});

const invalidRequests: Array<[string, (request: FixedPreviewTransferRequest) => void]> = [
  ["scope", (r) => { r.context.scope.epoch = -1; }],
  ["member group", (r) => { r.member.groupId = "foreign"; }],
  ["member native identity", (r) => { r.member.candidate.nativeSessionId = "foreign"; }],
  ["comparison candidate", (r) => { r.comparison.candidateId = "foreign"; }],
  ["comparison revision", (r) => { r.comparison.sourceRevision = "foreign"; }],
  ["comparison group", (r) => { r.comparison.groupId = "foreign"; }],
  ["comparison preview", (r) => { r.comparison.previewId = "foreign"; }],
  ["branch", (r) => { r.comparison.leafId = "foreign"; }],
  ["snapshot digest", (r) => { r.comparison.snapshotDigest = "listing-revision"; }],
  ["title", (r) => { r.comparison.title = "foreign"; }],
  ["cwd", (r) => { r.comparison.cwd = "foreign"; }],
  ["outcome", (r) => { r.comparison.outcome = "failed"; }],
  ["empty branch", (r) => { r.comparison.eventCount = 0; }],
  ["fractional count", (r) => { r.comparison.eventCount = 1.5; }],
  ["over-count", (r) => { r.comparison.eventCount = 3; }],
  ["missing fields", (r) => { Object.assign(r.preview.adapter as object, { defaultSelection: { version: 2, eventIds: [] } }); }],
  ["pinned grant", (r) => { Object.assign(r.preview.adapter as object, { defaultSelection: { ...defaultSelection(), eventIds: ["fixed-one"], pinnedEventIds: ["fixed-one"] } }); }],
  ["asset grant", (r) => { Object.assign(r.preview.adapter as object, { defaultSelection: { ...defaultSelection(), assetIds: ["asset"] } }); }],
  ["subagent grant", (r) => { Object.assign(r.preview.adapter as object, { defaultSelection: { ...defaultSelection(), includeSubagents: true } }); }],
  ["reasoning grant", (r) => { Object.assign(r.preview.adapter as object, { defaultSelection: { ...defaultSelection(), fields: { ...defaultSelection().fields, visibleReasoning: true } } }); }],
  ["non-default field", (r) => { Object.assign(r.preview.adapter as object, { defaultSelection: { ...defaultSelection(), fields: { ...defaultSelection().fields, messageText: false } } }); }],
  ["duplicated event grants", (r) => { Object.assign(r.preview.adapter as object, { defaultSelection: { ...defaultSelection(), eventIds: ["fixed-one", "fixed-one"] } }); }],
  ["oversized record", (r) => { r.comparison.title = "x".repeat(32 * 1024 * 1024); }],
];
test.each(invalidRequests)("rejects %s before transfer", (_label, mutate) => {
  const request = fixedTransferRequest();
  mutate(request);
  expect(() => copyFixedPreviewTransfer(request)).toThrow();
});

const invalidPages: Array<[string, (page: FixedPreviewPage) => void]> = [
  ["short first page", (p) => { p.events.pop(); }],
  ["extra first page", (p) => { p.events.push({ ...p.events[0], eventId: "extra" }); }],
  ["non-final cursor", (p) => { p.nextCursor = "unexpected-next"; }],
  ["empty cursor", (p) => { p.nextCursor = ""; }],
  ["duplicate event", (p) => { p.events[1].eventId = p.events[0].eventId; }],
  ["empty event ID", (p) => { p.events[0].eventId = " "; }],
  ["unknown kind", (p) => { p.events[0].kind = "foreign"; }],
  ["missing flags", (p) => { delete p.events[0].defaultSelected; }],
  ["non-boolean flag", (p) => { Object.assign(p.events[0], { selectable: "true" }); }],
  ["subagent default", (p) => { p.events[0].subagent = true; }],
  ["system selectable", (p) => { p.events[0].role = "system"; }],
  ["metadata selectable", (p) => { p.events[0].kind = "metadata"; }],
  ["unknown role", (p) => { p.events[0].role = "foreign"; }],
  ["field array", (p) => { Object.assign(p.events[0], { fieldAvailability: [true] }); }],
  ["non-boolean field", (p) => { Object.assign(p.events[0], { fieldAvailability: { messageText: "true" } }); }],
  ["empty asset ID", (p) => { p.events[0].assetIds = [" "]; }],
  ["wrong default flags", (p) => { p.events[0].defaultSelected = false; }],
  ["oversized page", (p) => { p.events[0].text = "x".repeat(256 * 1024); }],
];
test.each(invalidPages)("rejects %s before preference authority is eligible", (_label, mutate) => {
  const request = copyFixedPreviewTransfer(fixedTransferRequest()), page = fixedTransferPage();
  mutate(page);
  expect(() => fixedPreviewContentSlot(request, page, 1)).toThrow();
});

test("twenty-row page is bounded but active defaults still retain valid later-page event IDs", () => {
  const request = fixedTransferRequest(), ids = Array.from({ length: 21 }, (_, i) => `event-${i}`);
  request.comparison.eventCount = 21;
  Object.assign(request.preview.adapter as object, { eventCount: 21, defaultSelection: { ...defaultSelection(), eventIds: ids } });
  const slot = fixedPreviewContentSlot(copyFixedPreviewTransfer(request), fixedTransferPage(ids.slice(0, 20), "next-page"), 5);
  expect(slot.selection.events).toHaveLength(20);
  expect(slot.selection.selectedEventIds).toEqual(ids);
  expect(slot.selection.autoSelectPageDefaults).toBe(false);
  expect(slot.selection.nextCursor).toBe("next-page");
  expect(slot.selection.archiveCount).toBe(21);
});

test("native repeated asset references remain displayable without creating asset grants", () => {
  const request = fixedTransferRequest(), page = fixedTransferPage();
  page.events[0].assetIds = ["repeated-asset", "repeated-asset"];
  const slot = fixedPreviewContentSlot(copyFixedPreviewTransfer(request), page, 1);
  expect(slot.selection.events[0].assetIds).toEqual(["repeated-asset", "repeated-asset"]);
  expect(slot.selection.assetIds).toEqual([]);
});
