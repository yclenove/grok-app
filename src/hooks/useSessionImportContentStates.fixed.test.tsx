/** @vitest-environment jsdom */
import { act, cleanup } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import type { FixedPreviewAuthority, FixedPreviewPage } from "@/lib/sessionImport/fixedPreviewTransfer";
import { defaultSelection } from "@/lib/sessionImport/selection";
import { groupDeferred } from "@/test/sessionImportGroupFixture";
import { fixedTransferPage } from "@/test/sessionImportFixedPreviewFixture";
import { beginFixed, fixedContentHarness, openOrdinary } from "@/test/sessionImportFixedContentHarness";

afterEach(cleanup);
test.each(["a", "b"])("fixed %s adoption reads only the exact compared preview/leaf even when live source is offline", async (id) => {
  const h = fixedContentHarness(id), authorize = vi.fn<FixedPreviewAuthority>(async (_request, owns) => owns());
  h.preview.mockRejectedValue(new Error("SOURCE_CHANGED: source is offline"));
  h.events.mockResolvedValue(fixedTransferPage());
  await act(async () => { expect(await h.result.current.adoptFixedPreview(h.request, authorize)).toBe(true); });
  expect(h.preview).not.toHaveBeenCalled();
  expect(h.events).toHaveBeenCalledExactlyOnceWith(h.request.preview.previewId, null, 20, h.request.comparison.leafId);
  expect(authorize).toHaveBeenCalledTimes(1);
  expect(h.result.current.active?.previewId).toBe(h.request.preview.previewId);
  expect(h.result.current.active?.snapshotId).toBe(h.request.comparison.snapshotDigest);
  expect(h.result.current.active?.sourceRevision).toBe(h.request.member.candidate.sourceRevision);
  expect(h.result.current.active?.selection.selectedEventIds).toEqual(["fixed-one", "fixed-two"]);
  expect(h.result.current.active?.selection.autoSelectPageDefaults).toBe(false);
  expect(h.result.current.fixedTransfer).toEqual({ busy: false, error: null });
});

test("non-default branch uses requested page defaults, never the adapter's active-branch IDs", async () => {
  const h = fixedContentHarness(), authorize = vi.fn<FixedPreviewAuthority>(async () => true);
  h.request.comparison.leafId = "branch-b";
  const page = fixedTransferPage(["branch-only", "subagent-only"]);
  Object.assign(page.events[1], { defaultSelected: false, subagent: true });
  h.events.mockResolvedValue(page);
  await act(async () => { expect(await h.result.current.adoptFixedPreview(h.request, authorize)).toBe(true); });
  expect(h.events).toHaveBeenCalledWith(h.request.preview.previewId, null, 20, "branch-b");
  expect(h.result.current.active?.selection.leaf).toBe("branch-b");
  expect(h.result.current.active?.selection.selectedEventIds).toEqual(["branch-only"]);
  expect(h.result.current.active?.selection.autoSelectPageDefaults).toBe(true);
  expect(h.result.current.active?.selection.fields).toEqual(defaultSelection().fields);
  expect(h.result.current.active?.selection.includeSubagents).toBe(false);
});

test("rollback active ancestor is adopted even when not listed as a terminal leaf", async () => {
  const h = fixedContentHarness();
  Object.assign(h.request.preview.adapter as object, { activeLeaf: "ancestor-b", leaves: ["branch-b"] });
  h.request.comparison.leafId = "ancestor-b";
  h.events.mockResolvedValue(fixedTransferPage());
  await act(async () => { expect(await h.result.current.adoptFixedPreview(h.request, async () => true)).toBe(true); });
  expect(h.result.current.active?.selection.leaves).toEqual(["ancestor-b", "branch-b"]);
  expect(h.result.current.active?.selection.leaf).toBe("ancestor-b");
});

test.each(["a", "b"])("edited retained %s requires explicit discard confirmation before any read", async (id) => {
  const h = fixedContentHarness(), candidateId = await openOrdinary(h, id);
  act(() => { h.result.current.dispatchSelection(candidateId, { type: "toggleEvent", eventId: "old-one" }); });
  const original = h.result.current.byCandidate[candidateId], authorize = vi.fn<FixedPreviewAuthority>(async () => true);
  h.events.mockClear();
  await act(async () => { expect(await h.result.current.adoptFixedPreview(h.request, authorize)).toBe(false); });
  expect(h.events).not.toHaveBeenCalled();
  expect(authorize).not.toHaveBeenCalled();
  expect(h.result.current.byCandidate[candidateId]).toBe(original);
  expect(h.result.current.byCandidate[candidateId].dirty).toBe(true);
});

test("confirmed same-candidate replacement captures consent and creates fresh grants while retaining unrelated content", async () => {
  const h = fixedContentHarness("a"), unrelated = await openOrdinary(h, "c"), candidateId = await openOrdinary(h, "a");
  act(() => {
    h.result.current.dispatchSelection(unrelated, { type: "toggleEvent", eventId: "old-two" });
    h.result.current.dispatchSelection(candidateId, { type: "toggleEvent", eventId: "old-one" });
    h.result.current.dispatchSelection(candidateId, { type: "setField", field: "visibleReasoning", value: true });
    h.result.current.dispatchSelection(candidateId, { type: "setSubagents", value: true });
    h.result.current.dispatchSelection(candidateId, { type: "toggleAsset", assetId: "old-asset" });
  });
  h.events.mockRejectedValueOnce(new Error("retained-page-error"));
  await act(async () => { await h.result.current.loadMore(candidateId); });
  const retained = h.result.current.byCandidate[unrelated], pending = groupDeferred<FixedPreviewPage>();
  const options = { discardEdits: true };
  h.events.mockReturnValueOnce(pending.promise);
  const flight = beginFixed(h, async () => true, options);
  options.discardEdits = false;
  await act(async () => { pending.resolve(fixedTransferPage()); expect(await flight).toBe(true); });
  const adopted = h.result.current.active!;
  expect(adopted.dirty).toBe(false);
  expect(adopted.error).toBeNull();
  expect(adopted.busy).toBe(false);
  expect(adopted.selection).toMatchObject({ selectedEventIds: ["fixed-one", "fixed-two"], pinnedEventIds: [],
    assetIds: [], includeSubagents: false, fields: defaultSelection().fields, nextCursor: null, pendingLeaf: null });
  expect(h.result.current.byCandidate[unrelated]).toBe(retained);
  expect(Object.keys(h.result.current.slotsByPreviewId)).not.toContain(`ordinary-${candidateId}`);
});

test.each(["read", "reject", "throw"])("%s failure preserves prior edited content, exposes only owned errors and permits an explicit retry", async (failure) => {
  const h = fixedContentHarness(), candidateId = await openOrdinary(h, "b");
  act(() => { h.result.current.dispatchSelection(candidateId, { type: "toggleEvent", eventId: "old-one" }); });
  const old = h.result.current.byCandidate[candidateId], authorize = vi.fn<FixedPreviewAuthority>(async () => {
    if (failure === "throw") throw new Error("CAS_FAILED: synthetic");
    return false;
  });
  if (failure === "read") h.events.mockRejectedValueOnce(new Error("SNAPSHOT_UNAVAILABLE: synthetic"));
  else h.events.mockResolvedValueOnce(fixedTransferPage());
  await act(async () => { expect(await h.result.current.adoptFixedPreview(h.request, authorize, { discardEdits: true })).toBe(false); });
  const retained = h.result.current.byCandidate[candidateId];
  expect(retained.selection).toBe(old.selection);
  expect(retained.dirty).toBe(true);
  expect(retained.previewId).toBe(old.previewId);
  expect(retained.requestSeq).toBeGreaterThan(old.requestSeq);
  expect(h.result.current.activePreviewId).toBe(old.previewId);
  expect(h.result.current.fixedTransfer.busy).toBe(false);
  if (failure === "read") expect(authorize).not.toHaveBeenCalled();
  if (failure !== "reject") expect(h.result.current.fixedTransfer.error).toMatch(/SNAPSHOT_UNAVAILABLE|CAS_FAILED/);
  h.events.mockResolvedValueOnce(fixedTransferPage());
  await act(async () => { expect(await h.result.current.adoptFixedPreview(h.request, async () => true, { discardEdits: true })).toBe(true); });
  expect(h.result.current.fixedTransfer.error).toBeNull();
});

test("one synchronous flight blocks duplicate adoption, ordinary open, branch/page edits and direct focus changes", async () => {
  const h = fixedContentHarness(), candidateId = await openOrdinary(h, "a");
  const old = h.result.current.active, pending = groupDeferred<FixedPreviewPage>(), authorize = vi.fn<FixedPreviewAuthority>(async () => true);
  h.preview.mockClear(); h.events.mockClear(); h.events.mockReturnValueOnce(pending.promise);
  const flight = beginFixed(h, authorize);
  await act(async () => {
    expect(await h.result.current.adoptFixedPreview(h.request, authorize)).toBe(false);
    await h.result.current.open({ candidateId: "other", sourceRevision: "other", title: "Other" });
    await h.result.current.loadMore(candidateId);
    await h.result.current.confirmLeaf(candidateId);
    h.result.current.dispatchSelection(candidateId, { type: "requestLeaf", leaf: "other" });
    h.result.current.clearError(candidateId);
    h.result.current.setActiveCandidateId(null);
  });
  expect(h.result.current.active).toBe(old);
  expect(h.result.current.activeCandidateId).toBe(candidateId);
  expect(h.result.current.fixedTransfer.busy).toBe(true);
  expect(h.preview).not.toHaveBeenCalled();
  expect(h.events).toHaveBeenCalledTimes(1);
  await act(async () => { pending.resolve(fixedTransferPage()); expect(await flight).toBe(true); });
  expect(authorize).toHaveBeenCalledTimes(1);
});

test.each(["a", "b"])("pending branch choice on %s prevents silent loss of the unconfirmed choice", async (id) => {
  const h = fixedContentHarness(), candidateId = await openOrdinary(h, id);
  act(() => { h.result.current.dispatchSelection(candidateId, { type: "requestLeaf", leaf: "branch-other" }); });
  const old = h.result.current.byCandidate[candidateId], authorize = vi.fn<FixedPreviewAuthority>(async () => true);
  h.events.mockClear();
  await act(async () => { expect(await h.result.current.adoptFixedPreview(h.request, authorize, { discardEdits: true })).toBe(false); });
  expect(h.events).not.toHaveBeenCalled();
  expect(authorize).not.toHaveBeenCalled();
  expect(h.result.current.byCandidate[candidateId]).toBe(old);
});

test("request, page and authority arguments are detached across both async boundaries", async () => {
  const h = fixedContentHarness(), read = groupDeferred<FixedPreviewPage>(), ack = groupDeferred<boolean>();
  const previewId = h.request.preview.previewId, page = fixedTransferPage();
  const authorize = vi.fn<FixedPreviewAuthority>(async (captured, owns) => {
    captured.comparison.leafId = "mutated-authority-leaf";
    captured.context.scope.generation++;
    expect(owns()).toBe(true);
    return ack.promise;
  });
  h.events.mockReturnValueOnce(read.promise);
  const flight = beginFixed(h, authorize);
  h.request.preview.previewId = "mutated-input";
  h.request.member.candidate.sourceRevision = "mutated-input-revision";
  h.request.comparison.snapshotDigest = "0".repeat(64);
  await act(async () => { read.resolve(page); await Promise.resolve(); });
  expect(authorize).toHaveBeenCalledTimes(1);
  page.events[0].eventId = "mutated-page-event";
  await act(async () => { ack.resolve(true); expect(await flight).toBe(true); });
  expect(h.result.current.active?.previewId).toBe(previewId);
  expect(h.result.current.active?.sourceRevision).toBe("revision-b");
  expect(h.result.current.active?.snapshotId).toBe("b".repeat(64));
  expect(h.result.current.active?.selection.events[0].eventId).toBe("fixed-one");
  expect(h.result.current.active?.selection.leaf).toBe("leaf-b");
});

test("incomplete target preview prevents adopting over its live first-page owner", async () => {
  const h = fixedContentHarness(), pending = groupDeferred<Awaited<ReturnType<typeof h.preview>>>();
  h.preview.mockReturnValueOnce(pending.promise);
  let ordinary!: Promise<void>;
  act(() => { ordinary = h.result.current.open({ candidateId: h.request.member.candidate.candidateId,
    sourceRevision: "revision-b", title: "b" }); });
  h.events.mockClear();
  await act(async () => { expect(await h.result.current.adoptFixedPreview(h.request, async () => true)).toBe(false); });
  expect(h.events).not.toHaveBeenCalled();
  await act(async () => { pending.reject(new Error("synthetic offline")); await ordinary; });
});
