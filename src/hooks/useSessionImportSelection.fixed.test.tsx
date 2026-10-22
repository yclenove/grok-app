/** @vitest-environment jsdom */
import { act, cleanup } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { fixedAdoptionHarness, fixedContentOverrides } from "@/test/sessionImportFixedAdoptionHarness";
import { groupCandidate, groupDeferred, groupMetadata } from "@/test/sessionImportGroupFixture";
import { fixedTransferPage, fixedTransferRequest } from "@/test/sessionImportFixedPreviewFixture";
import type { FixedAdoptionTransfer } from "./useSessionImportFixedAdoption";
import type { FixedPreviewTransferRequest } from "@/lib/sessionImport/fixedPreviewTransfer";

afterEach(cleanup);
test("selection owns fixed adoption across preflight, native ACK, binding and content commit", async () => {
  const h = await fixedAdoptionHarness();
  expect(h.result.current.selection.selectionReady).toBe(true);
  expect(typeof Reflect.get(h.result.current.selection, "adoptFixedPreview")).toBe("function");
  let accepted = false;
  await act(async () => { accepted = await h.result.current.selection.adoptFixedPreview(h.request, h.transfer()); });
  expect(accepted).toBe(true);
  expect(h.ipc.preview).not.toHaveBeenCalled();
  expect(h.ipc.events).toHaveBeenCalledWith("fixed-preview-alternate", null, 20, "leaf-alternate");
  expect(h.ipc.choose).toHaveBeenCalledTimes(1);
  expect(h.result.current.selection.fixedAdoption.phase).toBe("adopted");
  expect(h.result.current.content.active?.previewId).toBe("fixed-preview-alternate");
  expect(h.result.current.selection.busy).toBe(false);
});

type Harness = Awaited<ReturnType<typeof fixedAdoptionHarness>>;
async function retain(h: Harness, id: string, dirty = false) {
  h.ipc.preview.mockResolvedValueOnce(fixedTransferRequest(id).preview);
  await act(async () => {
    h.result.current.selection.rememberPreview(groupCandidate(id).candidateId);
    await h.result.current.content.open(groupCandidate(id));
    if (dirty) h.result.current.content.dispatchSelection(groupCandidate(id).candidateId, { type: "toggleEvent", eventId: "fixed-one" });
  });
  h.ipc.preview.mockClear(); h.ipc.events.mockClear();
}
async function adopt(h: Harness, discardEdits = false, request = h.request) {
  let accepted = false;
  await act(async () => {
    accepted = await h.result.current.selection.adoptFixedPreview(request, h.transfer(discardEdits));
  });
  return accepted;
}

test("preflight reserves both domains synchronously and blocks all public competing commands", async () => {
  const h = await fixedAdoptionHarness(), page = groupDeferred<ReturnType<typeof fixedTransferPage>>();
  h.ipc.events.mockReturnValueOnce(page.promise);
  let flight!: Promise<boolean>;
  act(() => {
    flight = h.result.current.selection.adoptFixedPreview(h.request, h.transfer());
    // Recovery reads synchronous transaction facts, not an uncommitted render.
    const status = h.result.current.selection.getFixedAdoptionStatus();
    expect(status.phase).toBe("adopting");
    status.phase = "idle";
    expect(h.result.current.selection.getFixedAdoptionStatus().phase).toBe("adopting");
  });
  const before = Object.fromEntries(Object.entries(h.ipc).map(([name, mock]) => [name, mock.mock.calls.length]));
  const run = vi.fn();
  await act(async () => {
    const selection = h.result.current.selection;
    expect(selection.rememberPreview(groupCandidate("a").candidateId)).toBe(false);
    expect(await selection.prepare([], run)).toBeNull();
    expect(await selection.chooseGroupMember(groupMetadata("a").groupId, groupCandidate("alternate").candidateId)).toBe(false);
    expect(await selection.selectCurrent()).toBe(false);
    expect(await selection.selectThese([groupMetadata("a").groupId])).toBe(false);
    expect(await selection.invertThese([groupMetadata("a").groupId])).toBe(false);
    expect(await selection.toggle(groupMetadata("a").groupId)).toBe(false);
    expect(await selection.clearSelected()).toBe(false);
    expect(await selection.refresh()).toBe(false);
    expect(await selection.startScan()).toBe(false);
    expect(await selection.useIndependentCandidates()).toBe(false);
    expect(await selection.useFixedGroups()).toBe(false);
    expect(await selection.adoptFixedPreview(h.request, h.transfer())).toBe(false);
    await selection.changePage(1); await selection.reopen();
    await h.result.current.content.open(groupCandidate("b"));
  });
  expect(run).not.toHaveBeenCalled();
  for (const [name, mock] of Object.entries(h.ipc)) expect(mock.mock.calls.length).toBe(before[name]);
  expect(h.result.current.selection.busy).toBe(true);
  await act(async () => { page.resolve(fixedTransferPage()); expect(await flight).toBe(true); });
});

test("pre-CAS expired snapshot holds only the exact comparison and requires explicit release", async () => {
  const h = await fixedAdoptionHarness();
  h.ipc.events.mockRejectedValueOnce(new Error("PLAN_STALE: fixed snapshot expired"));
  expect(await adopt(h)).toBe(false);
  expect(h.result.current.selection.getFixedAdoptionStatus().phase).toBe("stale");
  expect(h.result.current.selection.ownsFixedComparison(h.request.context)).toBe(true);
  expect(h.result.current.selection.dismissFixedAdoptionStatus()).toBe(false);
  expect(await adopt(h)).toBe(false);
  expect(h.ipc.events).toHaveBeenCalledTimes(1);
  expect(h.ipc.choose).not.toHaveBeenCalled();
  act(() => { h.result.current.selection.cancelFixedAdoption(); });
  expect(await adopt(h)).toBe(true);
});

test("owned ACK retains dirty prior and unrelated content, but prepare only receives eligible adopted overrides", async () => {
  const h = await fixedAdoptionHarness();
  await act(async () => { await h.result.current.selection.selectCurrent(); });
  await retain(h, "a", true); await retain(h, "b", true);
  const prior = h.result.current.content.byCandidate[groupCandidate("a").candidateId];
  const unrelated = h.result.current.content.byCandidate[groupCandidate("b").candidateId];
  expect(await adopt(h, true)).toBe(true);
  expect(h.result.current.content.byCandidate[groupCandidate("a").candidateId]).toBe(prior);
  expect(h.result.current.content.byCandidate[groupCandidate("b").candidateId]).toBe(unrelated);
  const run = vi.fn(async () => null);
  await act(async () => { await h.result.current.selection.prepare(fixedContentOverrides(h), run); });
  expect(run).toHaveBeenCalledTimes(1);
  const [view, overrides] = run.mock.calls[0] as unknown as [{ descriptor: { stateVersion: number } }, ReturnType<typeof fixedContentOverrides>];
  expect(view.descriptor.stateVersion).toBe(3);
  expect(overrides.map((slot) => slot.candidateId)).toEqual([groupCandidate("b").candidateId, groupCandidate("alternate").candidateId]);
  expect(overrides[1]).toMatchObject({ previewId: h.request.preview.previewId, leafId: h.request.comparison.leafId });
  expect(overrides[1].selection.eventIds).toEqual(["fixed-one", "fixed-two"]);
  expect(h.ipc.preview).not.toHaveBeenCalled();
});

test("dirty consent is required before any events or choose dispatch", async () => {
  const h = await fixedAdoptionHarness(); await retain(h, "a", true);
  expect(await adopt(h)).toBe(false);
  expect(h.ipc.events).not.toHaveBeenCalled(); expect(h.ipc.choose).not.toHaveBeenCalled();
  expect(h.result.current.content.byCandidate[groupCandidate("a").candidateId].dirty).toBe(true);
});

test("ACK plus read failure keeps edits and only explicit reconciliation reads the same preview without another choose", async () => {
  const h = await fixedAdoptionHarness(); await retain(h, "a", true);
  const prior = h.result.current.content.byCandidate[groupCandidate("a").candidateId];
  h.ipc.membership.mockRejectedValueOnce(new Error("read offline"));
  expect(await adopt(h, true)).toBe(false);
  expect(h.result.current.selection.fixedAdoption.phase).toBe("reconcile-required");
  expect(h.result.current.content.byCandidate[prior.candidateId]).toBe(prior);
  expect(h.result.current.content.byCandidate[groupCandidate("alternate").candidateId]).toBeUndefined();
  expect(await adopt(h, true)).toBe(false);
  expect(h.ipc.choose).toHaveBeenCalledTimes(1);
  let accepted = false;
  await act(async () => { accepted = await h.result.current.selection.reconcileFixedPreview(h.transfer(true)); });
  expect(accepted).toBe(true);
  expect(h.ipc.choose).toHaveBeenCalledTimes(1);
  expect(h.ipc.events).toHaveBeenCalledTimes(2);
  expect(h.ipc.preview).not.toHaveBeenCalled();
  expect(h.result.current.content.byCandidate[prior.candidateId]).toBe(prior);
});

test.each(["source", "wizard", "unmount", "comparison"])("%s close during preflight expires late success", async (close) => {
  const h = await fixedAdoptionHarness(), page = groupDeferred<ReturnType<typeof fixedTransferPage>>();
  h.ipc.events.mockReturnValueOnce(page.promise);
  let flight!: Promise<boolean>;
  act(() => { flight = h.result.current.selection.adoptFixedPreview(h.request, h.transfer()); });
  act(() => {
    if (close === "source") h.result.current.selection.clearSource();
    else if (close === "wizard") { h.result.current.selection.close(); h.result.current.content.closeWizard(); }
    else if (close === "unmount") h.unmount();
    else { h.result.current.selection.cancelFixedAdoption(); h.result.current.content.cancelFixedTransfer(); }
  });
  await act(async () => { page.resolve(fixedTransferPage()); expect(await flight).toBe(false); });
  expect(h.ipc.choose).not.toHaveBeenCalled();
  if (close !== "unmount") expect(h.result.current.content.byCandidate[groupCandidate("alternate").candidateId]).toBeUndefined();
});

test("comparison cancel after dispatch retains the exact pending-command tombstone until ACK settles", async () => {
  const h = await fixedAdoptionHarness(); await retain(h, "a", true);
  const prior = h.result.current.content.byCandidate[groupCandidate("a").candidateId];
  const ack = groupDeferred<Awaited<ReturnType<typeof h.host.client.choose>>>();
  const choose = h.ipc.choose.getMockImplementation()!;
  h.ipc.choose.mockImplementationOnce(async (...args) => { const value = await choose(...args); await ack.promise; return value; });
  let flight!: Promise<boolean>;
  await act(async () => { flight = h.result.current.selection.adoptFixedPreview(h.request, h.transfer(true)); });
  expect(h.ipc.choose).toHaveBeenCalledTimes(1);
  act(() => { h.result.current.selection.cancelFixedAdoption(); h.result.current.content.cancelFixedTransfer(); });
  expect(await adopt(h, true)).toBe(false);
  await act(async () => { ack.resolve({} as never); expect(await flight).toBe(false); });
  expect(h.result.current.content.byCandidate[prior.candidateId]).toBe(prior);
  expect(h.result.current.selection.fixedAdoption).toMatchObject({ phase: "stale", busy: false });
  expect(h.result.current.selection.candidates[0].candidateId).toBe(h.request.member.candidate.candidateId);
  expect(h.ipc.choose).toHaveBeenCalledTimes(1);
});

test("lost or rejected command response is uncertain, cannot automatically resend, and explicit cancel permits refresh", async () => {
  const h = await fixedAdoptionHarness(); await retain(h, "a", true);
  h.ipc.choose.mockRejectedValueOnce(new Error("transport lost"));
  expect(await adopt(h, true)).toBe(false);
  expect(h.result.current.selection.fixedAdoption.phase).toBe("uncertain");
  expect(await adopt(h, true)).toBe(false);
  await act(async () => { expect(await h.result.current.selection.reconcileFixedPreview(h.transfer())).toBe(false); });
  expect(h.ipc.choose).toHaveBeenCalledTimes(1);
  act(() => { h.result.current.selection.cancelFixedAdoption(); });
  await act(async () => { expect(await h.result.current.selection.refresh()).toBe(true); });
  expect(h.result.current.content.byCandidate[groupCandidate("a").candidateId].dirty).toBe(true);
});

test("another writer's coherent newer version cannot authorize the old accepted preview", async () => {
  const h = await fixedAdoptionHarness(); await retain(h, "a", true);
  const read = h.ipc.membership.getMockImplementation()!;
  h.ipc.membership.mockImplementationOnce(async (...args) => {
    await h.host.client.set({}, [groupMetadata("b").groupId], true);
    return read(...args);
  });
  expect(await adopt(h, true)).toBe(false);
  expect(h.result.current.selection.selectionReady).toBe(true);
  expect(h.result.current.selection.fixedAdoption.phase).toBe("stale");
  expect(h.result.current.content.byCandidate[groupCandidate("a").candidateId].dirty).toBe(true);
  expect(h.result.current.content.byCandidate[groupCandidate("alternate").candidateId]).toBeUndefined();
  expect(h.ipc.choose).toHaveBeenCalledTimes(1);
  await act(async () => { expect(await h.result.current.selection.reconcileFixedPreview(h.transfer())).toBe(false); });
});

test.each(["scope", "primary", "revision", "group"])("foreign initial %s is rejected before preflight", async (field) => {
  const h = await fixedAdoptionHarness(), request = structuredClone(h.request);
  if (field === "scope") request.context.scope.generation += 1;
  if (field === "primary") request.context.primary.candidateId = groupCandidate("b").candidateId;
  if (field === "revision") request.context.primary.sourceRevision += "-changed";
  if (field === "group") request.context.group.sourceId += "-changed";
  expect(await adopt(h, false, request)).toBe(false);
  expect(h.ipc.events).not.toHaveBeenCalled(); expect(h.ipc.choose).not.toHaveBeenCalled();
});

test("captures caller data before the first await", async () => {
  const h = await fixedAdoptionHarness(), request = structuredClone(h.request), page = groupDeferred<ReturnType<typeof fixedTransferPage>>();
  h.ipc.events.mockReturnValueOnce(page.promise);
  let flight!: Promise<boolean>;
  act(() => { flight = h.result.current.selection.adoptFixedPreview(request, h.transfer()); });
  request.member.candidate.candidateId = "foreign"; request.context.scope.generation += 5;
  request.comparison.leafId = "changed";
  await act(async () => { page.resolve(fixedTransferPage()); expect(await flight).toBe(true); });
  expect(h.result.current.content.active?.leafId).toBe("leaf-alternate");
});

test("A adoption creates one explicit preference, then persisted same-member adoption is a native no-op", async () => {
  const h = await fixedAdoptionHarness(), request = fixedTransferRequest("a");
  expect(await adopt(h, false, request)).toBe(true);
  const version = h.result.current.selection.selectionView?.descriptor?.stateVersion;
  expect(await adopt(h, false, request)).toBe(true);
  expect(h.result.current.selection.selectionView?.descriptor?.stateVersion).toBe(version);
  expect(h.ipc.choose).toHaveBeenCalledTimes(2);
  expect(h.ipc.preview).not.toHaveBeenCalled();
});

test("authority is exact-request and single-use, not a generic CAS capability", async () => {
  const h = await fixedAdoptionHarness();
  const transfer: FixedAdoptionTransfer = { isCurrent: () => false, getError: () => null, run: async (request, authority) => {
    const changed: FixedPreviewTransferRequest = structuredClone(request); changed.comparison.leafId = "wrong";
    expect(await authority(changed, () => true)).toBe(false);
    expect(await authority(request, () => true)).toBe(true);
    expect(await authority(request, () => true)).toBe(false);
    return false; // A failed content commit must revoke private eligibility.
  } };
  await act(async () => { expect(await h.result.current.selection.adoptFixedPreview(h.request, transfer)).toBe(false); });
  expect(h.ipc.choose).toHaveBeenCalledTimes(1);
  expect(h.result.current.selection.fixedAdoption.phase).toBe("stale");
});

test.each(["invalidate", "close", "edit"])("content %s immediately after commit revokes only that flight's prepare eligibility", async (action) => {
  const h = await fixedAdoptionHarness(), transfer = h.transfer();
  await act(async () => {
    expect(await h.result.current.selection.adoptFixedPreview(h.request, { ...transfer, run: async (request, authority) => {
      const committed = await transfer.run(request, authority);
      expect(committed).toBe(true);
      if (action === "close") h.result.current.content.closeWizard();
      else if (action === "invalidate") h.result.current.content.invalidateCandidate(request.member.candidate.candidateId);
      else h.result.current.content.dispatchSelection(request.member.candidate.candidateId, { type: "toggleEvent", eventId: "fixed-one" });
      return committed;
    } })).toBe(false);
  });
  expect(h.result.current.selection.fixedAdoption.phase).toBe("stale");
  expect(h.ipc.choose).toHaveBeenCalledTimes(1);
  const run = vi.fn(async () => null);
  await act(async () => { expect(await h.result.current.selection.prepare(fixedContentOverrides(h), run)).toBeNull(); });
  expect(run).not.toHaveBeenCalled();
});

test("content owner expiry after private registration preserves prior and unrelated edits", async () => {
  const h = await fixedAdoptionHarness();
  await act(async () => { await h.result.current.selection.selectCurrent(); });
  await retain(h, "a", true); await retain(h, "b", true);
  const prior = h.result.current.content.byCandidate[groupCandidate("a").candidateId];
  const unrelated = h.result.current.content.byCandidate[groupCandidate("b").candidateId];
  const base = h.transfer(true);
  const transfer: FixedAdoptionTransfer = { ...base, run: async (request, authority) => base.run(request, async (fixed, owns) => {
    const accepted = await authority(fixed, owns);
    expect(accepted).toBe(true);
    h.result.current.content.cancelFixedTransfer();
    return accepted;
  }) };
  await act(async () => { expect(await h.result.current.selection.adoptFixedPreview(h.request, transfer)).toBe(false); });
  expect(h.result.current.content.byCandidate[prior.candidateId]).toBe(prior);
  expect(h.result.current.content.byCandidate[unrelated.candidateId]).toBe(unrelated);
  expect(h.result.current.content.byCandidate[groupCandidate("alternate").candidateId]).toBeUndefined();
  act(() => { h.result.current.selection.cancelFixedAdoption(); });
  const run = vi.fn(async () => null);
  await act(async () => { await h.result.current.selection.prepare(fixedContentOverrides(h), run); });
  expect(run).toHaveBeenCalledTimes(1);
  expect((run.mock.calls[0] as unknown as [unknown, ReturnType<typeof fixedContentOverrides>])[1].map((slot) => slot.candidateId)).toEqual([unrelated.candidateId]);
});

test.each(["success", "error"])("cancelled old preflight %s cannot overwrite a newer adoption or its binding", async (outcome) => {
  const h = await fixedAdoptionHarness(), page = groupDeferred<ReturnType<typeof fixedTransferPage>>();
  h.ipc.events.mockReturnValueOnce(page.promise);
  let old!: Promise<boolean>;
  act(() => { old = h.result.current.selection.adoptFixedPreview(h.request, h.transfer()); });
  act(() => { h.result.current.selection.cancelFixedAdoption(); h.result.current.content.cancelFixedTransfer(); });
  expect(await adopt(h)).toBe(true);
  const current = h.result.current.content.active;
  await act(async () => {
    if (outcome === "success") page.resolve(fixedTransferPage());
    else page.reject(new Error("old late error"));
    expect(await old).toBe(false);
  });
  expect(h.result.current.selection.fixedAdoption.phase).toBe("adopted");
  expect(h.result.current.content.active).toBe(current);
  expect(h.result.current.content.fixedPreviewTransfer.isCurrent(h.request)).toBe(true);
  expect(h.onError).not.toHaveBeenCalled();
});

test.each(["wizard", "source", "unmount"])("%s close during ACK read rejects late reconciliation without touching a reopened owner", async (close) => {
  const h = await fixedAdoptionHarness(), read = groupDeferred<Awaited<ReturnType<typeof h.host.client.membership>>>();
  h.ipc.membership.mockReturnValueOnce(read.promise);
  let flight!: Promise<boolean>;
  await act(async () => { flight = h.result.current.selection.adoptFixedPreview(h.request, h.transfer()); });
  expect(h.ipc.choose).toHaveBeenCalledTimes(1);
  act(() => {
    if (close === "unmount") h.unmount();
    else if (close === "source") { h.result.current.selection.clearSource(); h.result.current.content.closeWizard(); }
    else { h.result.current.selection.close(); h.result.current.content.closeWizard(); }
  });
  if (close === "wizard") await act(async () => { await h.result.current.selection.reopen(); });
  await act(async () => { read.reject(new Error("late ACK read")); expect(await flight).toBe(false); });
  if (close !== "unmount") {
    expect(h.result.current.selection.fixedAdoption.phase).toBe("idle");
    expect(h.result.current.content.byCandidate[groupCandidate("alternate").candidateId]).toBeUndefined();
  }
  expect(h.onError).not.toHaveBeenCalled();
});

test("reconciliation can fail again without resending, and lease failure leaves fixed content unchanged", async () => {
  const h = await fixedAdoptionHarness(); await retain(h, "a", true);
  const prior = h.result.current.content.byCandidate[groupCandidate("a").candidateId];
  h.ipc.membership.mockRejectedValueOnce(new Error("first read"));
  expect(await adopt(h, true)).toBe(false);
  h.ipc.membership.mockRejectedValueOnce(new Error("retry read"));
  await act(async () => { expect(await h.result.current.selection.reconcileFixedPreview(h.transfer(true))).toBe(false); });
  expect(h.result.current.selection.fixedAdoption.phase).toBe("reconcile-required");
  h.ipc.events.mockRejectedValueOnce(new Error("PLAN_STALE: preview expired"));
  await act(async () => { expect(await h.result.current.selection.reconcileFixedPreview(h.transfer(true))).toBe(false); });
  expect(h.ipc.choose).toHaveBeenCalledTimes(1);
  expect(h.result.current.content.byCandidate[prior.candidateId]).toBe(prior);
  expect(h.result.current.content.fixedTransfer.error).toContain("preview expired");
  expect(h.result.current.selection.fixedAdoption.phase).toBe("stale");
  const events = h.ipc.events.mock.calls.length;
  await act(async () => { expect(await h.result.current.selection.reconcileFixedPreview(h.transfer(true))).toBe(false); });
  expect(h.ipc.events).toHaveBeenCalledTimes(events);
  expect(h.ipc.preview).not.toHaveBeenCalled();
});

test.each(["count", "selected", "previous"])("foreign ACK %s cannot grant adoption authority", async (field) => {
  const h = await fixedAdoptionHarness(), choose = h.ipc.choose.getMockImplementation()!;
  h.ipc.choose.mockImplementationOnce(async (...args) => {
    const ack = await choose(...args);
    if (field === "count") ack.selection.choiceCount += 1;
    if (field === "selected") ack.selected = true;
    if (field === "previous") ack.previousCandidateId = groupCandidate("b").candidateId;
    return ack;
  });
  expect(await adopt(h)).toBe(false);
  expect(h.result.current.content.byCandidate[groupCandidate("alternate").candidateId]).toBeUndefined();
  expect(h.result.current.selection.fixedAdoption.phase).toBe("uncertain");
  expect(h.ipc.choose).toHaveBeenCalledTimes(1);
});

test("same candidate with a different accepted source revision is not the compared fixed member", async () => {
  const h = await fixedAdoptionHarness(), read = h.ipc.membership.getMockImplementation()!;
  h.ipc.membership.mockImplementationOnce(async (...args) => {
    const membership = await read(...args);
    return { ...membership, choices: membership.choices.map((choice: { candidate: { sourceRevision: string }; groupId: string }) =>
      choice.groupId === h.request.context.group.groupId ? { ...choice, candidate: { ...choice.candidate, sourceRevision: "new-revision" } } : choice) };
  });
  expect(await adopt(h)).toBe(false);
  expect(h.result.current.selection.selectionReady).toBe(true);
  expect(h.result.current.selection.fixedAdoption.phase).toBe("stale");
  expect(h.result.current.content.byCandidate[groupCandidate("alternate").candidateId]).toBeUndefined();
});

test("review-only plus eligible content share the 200-slot bound before any CAS, and explicit clear reclaims quarantine", async () => {
  const h = await fixedAdoptionHarness(); await retain(h, "a");
  let primary = h.request.context.primary;
  for (let index = 1; index < 200; index += 1) {
    const request = fixedTransferRequest(`bounded-${index}`); request.context.primary = primary;
    expect(await adopt(h, false, request)).toBe(true);
    primary = request.member.candidate;
  }
  expect(Object.keys(h.result.current.content.byCandidate)).toHaveLength(200);
  const request = fixedTransferRequest("too-many"); request.context.primary = primary;
  const calls = h.ipc.choose.mock.calls.length, events = h.ipc.events.mock.calls.length;
  expect(await adopt(h, false, request)).toBe(false);
  expect(h.result.current.selection.fixedAdoption.error).toContain("RESOURCE_LIMIT");
  expect(h.ipc.choose).toHaveBeenCalledTimes(calls); expect(h.ipc.events).toHaveBeenCalledTimes(events);
  await act(async () => { expect(await h.result.current.selection.clearSelected()).toBe(true); });
  expect(h.result.current.content.byCandidate).toEqual({});
  expect(await adopt(h)).toBe(true);
}, 15000);

test("ACK-only reconciliation never bypasses the existing prepare-batch lock", async () => {
  let locked = false;
  const h = await fixedAdoptionHarness({ isLocked: () => locked });
  h.ipc.membership.mockRejectedValueOnce(new Error("read offline"));
  expect(await adopt(h)).toBe(false);
  const events = h.ipc.events.mock.calls.length;
  locked = true;
  await act(async () => { expect(await h.result.current.selection.reconcileFixedPreview(h.transfer())).toBe(false); });
  expect(h.ipc.events).toHaveBeenCalledTimes(events);
  expect(h.ipc.choose).toHaveBeenCalledTimes(1);
  locked = false;
  await act(async () => { expect(await h.result.current.selection.reconcileFixedPreview(h.transfer())).toBe(true); });
  expect(h.ipc.choose).toHaveBeenCalledTimes(1);
});

test("eligibility projection does not relax malformed or duplicate override validation", async () => {
  const h = await fixedAdoptionHarness(); await retain(h, "a");
  expect(await adopt(h)).toBe(true);
  const slots = fixedContentOverrides(h), prior = slots.find((slot) => slot.candidateId === groupCandidate("a").candidateId)!;
  const run = vi.fn(async () => null);
  await act(async () => {
    expect(await h.result.current.selection.prepare([...slots, prior], run)).toBeNull();
    expect(await h.result.current.selection.prepare(slots.map((slot) => slot === prior ? { ...slot, previewId: "" } : slot), run)).toBeNull();
  });
  expect(run).not.toHaveBeenCalled();
  expect(h.onError).toHaveBeenCalledWith(expect.stringContaining("invalid content override identities"));
});
