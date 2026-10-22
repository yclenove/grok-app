/** @vitest-environment jsdom */
import { act, cleanup } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import type { FixedPreviewAuthority, FixedPreviewPage } from "@/lib/sessionImport/fixedPreviewTransfer";
import { groupDeferred } from "@/test/sessionImportGroupFixture";
import { fixedTransferPage } from "@/test/sessionImportFixedPreviewFixture";
import { beginFixed, fixedContentHarness, openOrdinary } from "@/test/sessionImportFixedContentHarness";

afterEach(cleanup);
const expiredCases = (["read", "authority"] as const).flatMap((boundary) =>
  (["resolve", "reject"] as const).flatMap((outcome) =>
    (["cancel", "target", "primary", "close", "scan", "epoch", "generation", "unmount"] as const)
      .map((exit) => ({ boundary, outcome, exit }))),
);

test.each(expiredCases)("late $boundary $outcome after $exit cannot revive content, error or focus", async ({ boundary, outcome, exit }) => {
  const h = fixedContentHarness(), candidateId = await openOrdinary(h, "b");
  act(() => { h.result.current.dispatchSelection(candidateId, { type: "toggleEvent", eventId: "old-one" }); });
  const read = groupDeferred<FixedPreviewPage>(), ack = groupDeferred<boolean>();
  const authorize = vi.fn<FixedPreviewAuthority>(async () => ack.promise);
  h.events.mockReturnValueOnce(read.promise);
  const flight = beginFixed(h, authorize, { discardEdits: true });
  if (boundary === "authority") {
    await act(async () => { read.resolve(fixedTransferPage()); await Promise.resolve(); });
    expect(authorize).toHaveBeenCalledTimes(1);
  }
  if (exit === "unmount") h.unmount();
  else act(() => {
    if (exit === "cancel") h.result.current.cancelFixedTransfer();
    else if (exit === "target") h.result.current.invalidateCandidate(candidateId);
    else if (exit === "primary") h.result.current.invalidateCandidate(h.request.context.primary.candidateId);
    else if (exit === "close") h.result.current.closeWizard();
    else {
      const scope = { ...h.request.context.scope };
      if (exit === "scan") scope.scanId = "different-scan";
      if (exit === "epoch") scope.epoch++;
      if (exit === "generation") scope.generation++;
      h.result.current.ensureSelected([], scope.generation, scope);
    }
  });
  const before = h.result.current;
  await act(async () => {
    const error = new Error("obsolete-fixed-failure");
    if (boundary === "read") {
      if (outcome === "resolve") read.resolve(fixedTransferPage());
      else read.reject(error);
    } else {
      if (outcome === "resolve") ack.resolve(true);
      else ack.reject(error);
    }
    expect(await flight).toBe(false);
  });
  expect(h.result.current.byCandidate).toBe(before.byCandidate);
  expect(h.result.current.activePreviewId).toBe(before.activePreviewId);
  expect(h.result.current.fixedTransfer).toBe(before.fixedTransfer);
  expect(Object.keys(h.result.current.slotsByPreviewId)).not.toContain(h.request.preview.previewId);
  expect(h.onError).not.toHaveBeenCalled();
  if (boundary === "read") expect(authorize).not.toHaveBeenCalled();
  if (exit === "cancel" || exit === "primary") {
    expect(h.result.current.byCandidate[candidateId].dirty).toBe(true);
    expect(h.result.current.byCandidate[candidateId].selection.selectedEventIds).toEqual(["old-two"]);
  }
});

test.each(["read", "authority"])("old %s finally cannot clear a close/reopened same-scope transfer", async (boundary) => {
  const h = fixedContentHarness(), oldRead = groupDeferred<FixedPreviewPage>(), oldAck = groupDeferred<boolean>();
  h.events.mockReturnValueOnce(oldRead.promise);
  const oldAuthority = vi.fn<FixedPreviewAuthority>(async () => oldAck.promise);
  const oldFlight = beginFixed(h, oldAuthority);
  if (boundary === "authority") await act(async () => { oldRead.resolve(fixedTransferPage()); await Promise.resolve(); });
  act(() => {
    h.result.current.closeWizard();
    h.result.current.ensureSelected([], h.request.context.scope.generation, h.request.context.scope);
  });
  const freshRead = groupDeferred<FixedPreviewPage>(), freshAuthority = vi.fn<FixedPreviewAuthority>(async () => true);
  h.events.mockReturnValueOnce(freshRead.promise);
  const freshFlight = beginFixed(h, freshAuthority);
  await act(async () => {
    if (boundary === "read") oldRead.reject(new Error("old-read-failure"));
    else oldAck.resolve(true);
    expect(await oldFlight).toBe(false);
  });
  expect(h.result.current.fixedTransfer).toEqual({ busy: true, error: null });
  expect(h.result.current.active).toBeNull();
  await act(async () => { freshRead.resolve(fixedTransferPage()); expect(await freshFlight).toBe(true); });
  expect(h.result.current.activePreviewId).toBe(h.request.preview.previewId);
  expect(freshAuthority).toHaveBeenCalledTimes(1);
});

test.each(["success", "error"])("superseded target page %s stays expired even after fixed read failure", async (outcome) => {
  const h = fixedContentHarness(), candidateId = await openOrdinary(h, "b"), oldPage = groupDeferred<FixedPreviewPage>();
  h.events.mockReturnValueOnce(oldPage.promise);
  let paging!: Promise<void>;
  act(() => { paging = h.result.current.loadMore(candidateId); });
  const old = h.result.current.byCandidate[candidateId];
  h.events.mockRejectedValueOnce(new Error("fixed-read-failed"));
  await act(async () => { expect(await h.result.current.adoptFixedPreview(h.request, async () => true)).toBe(false); });
  const retained = h.result.current.byCandidate[candidateId], transfer = h.result.current.fixedTransfer;
  expect(retained.requestSeq).toBeGreaterThan(old.requestSeq);
  expect(retained.selection).toBe(old.selection);
  await act(async () => {
    if (outcome === "success") oldPage.resolve(fixedTransferPage(["obsolete-page-event"]));
    else oldPage.reject(new Error("obsolete-page-error"));
    await paging;
  });
  expect(h.result.current.byCandidate[candidateId]).toBe(retained);
  expect(h.result.current.fixedTransfer).toBe(transfer);
  expect(h.onError).not.toHaveBeenCalled();
});

test("authority cancellation during its callback cannot commit despite returning true", async () => {
  const h = fixedContentHarness();
  h.events.mockResolvedValueOnce(fixedTransferPage());
  await act(async () => { expect(await h.result.current.adoptFixedPreview(h.request, async (_request, owns) => {
    expect(owns()).toBe(true);
    h.result.current.cancelFixedTransfer();
    expect(owns()).toBe(false);
    return true;
  })).toBe(false); });
  expect(h.result.current.active).toBeNull();
  expect(h.result.current.fixedTransfer).toEqual({ busy: false, error: null });
});

test("an unrelated candidate invalidation does not cancel the exact target/primary flight", async () => {
  const h = fixedContentHarness(), unrelated = await openOrdinary(h, "c"), pending = groupDeferred<FixedPreviewPage>();
  h.events.mockReturnValueOnce(pending.promise);
  const flight = beginFixed(h, async () => true);
  act(() => { h.result.current.invalidateCandidate(unrelated); });
  expect(h.result.current.fixedTransfer.busy).toBe(true);
  await act(async () => { pending.resolve(fixedTransferPage()); expect(await flight).toBe(true); });
});

test("a fresh dirty refusal clears the previous transfer diagnostic without touching retained slot errors or edits", async () => {
  const h = fixedContentHarness(), candidateId = await openOrdinary(h, "b");
  h.events.mockRejectedValueOnce(new Error("retained-page-error"));
  await act(async () => { await h.result.current.loadMore(candidateId); });
  h.events.mockRejectedValueOnce(new Error("previous-fixed-read-error"));
  await act(async () => { expect(await h.result.current.adoptFixedPreview(h.request, async () => true)).toBe(false); });
  expect(h.result.current.fixedPreviewTransfer.getError()).toContain("previous-fixed-read-error");
  act(() => { h.result.current.dispatchSelection(candidateId, { type: "toggleEvent", eventId: "old-one" }); });
  const slot = h.result.current.byCandidate[candidateId], calls = h.events.mock.calls.length, authorize = vi.fn();
  await act(async () => { expect(await h.result.current.adoptFixedPreview(h.request, authorize)).toBe(false); });
  expect(h.result.current.fixedPreviewTransfer.getError()).toBeNull();
  expect(h.result.current.fixedTransfer).toEqual({ busy: false, error: null });
  expect(h.result.current.byCandidate[candidateId]).toBe(slot);
  expect(slot.error).toContain("retained-page-error");
  expect(slot.dirty).toBe(true);
  expect(authorize).not.toHaveBeenCalled();
  expect(h.events).toHaveBeenCalledTimes(calls);
});
