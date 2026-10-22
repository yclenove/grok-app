import { beforeEach, expect, test, vi } from "vitest";
import {
  sessionImportIdentityAssociationInspect,
  type IdentityAssociationRequest,
  type IdentityAssociationReview,
} from "./sessionMigration";
import { defaultSelection } from "@/lib/sessionImport/selection";

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("./host", () => ({ invoke }));
beforeEach(() => vi.resetAllMocks());

function request(): IdentityAssociationRequest {
  return {
    version: 1,
    importId: "imp:ba6616b6-b9e8-440a-b01e-ab6db40c27a7",
    expectedRevision: "028cbb7c-61ff-4d05-bd60-601124babd21",
    preview: {
      previewId: "32172371-4ba8-47f5-9536-683e189dd358", leafId: "event:2",
      selection: { ...defaultSelection(), eventIds: ["event:1", "event:2"],
        pinnedEventIds: ["event:1"], assetIds: ["asset:1"] },
    },
  };
}

test("public API only inspects an explicit target and fixed raw selection", async () => {
  const owner = request();
  const review: IdentityAssociationReview = {
    version: 1, importId: owner.importId, expectedRevision: owner.expectedRevision,
    previewId: owner.preview.previewId, sourceId: "synthetic-source",
    oldNativeSessionId: "transcript", currentNativeSessionId: "synthetic:current",
    oldNormalizerVersion: "1.0.0", currentNormalizerVersion: "1.0.6",
    selectionHash: "1".repeat(64), reviewDigest: "2".repeat(64),
  };
  invoke.mockResolvedValue(review);
  expect(await sessionImportIdentityAssociationInspect(owner)).toBe(review);
  expect(invoke).toHaveBeenCalledExactlyOnceWith("session_import_identity_association_inspect", { request: owner });
});

test("copies every nested scope before transport without mutating the caller", async () => {
  const owner = request();
  const original = structuredClone(owner);
  const pending = sessionImportIdentityAssociationInspect(owner);
  owner.importId = "later-target";
  owner.expectedRevision = "later-revision";
  owner.preview.previewId = "later-preview";
  owner.preview.leafId = "later-leaf";
  owner.preview.selection.eventIds.push("late-event");
  owner.preview.selection.pinnedEventIds.push("late-pin");
  owner.preview.selection.assetIds.push("late-asset");
  owner.preview.selection.fields.messageText = false;
  owner.preview.selection.includeSubagents = true;
  expect(invoke).toHaveBeenCalledExactlyOnceWith("session_import_identity_association_inspect", { request: original });
  await pending;
});

test("emits declared fields only, never caller paths, receipts or authorization flags", async () => {
  const owner = request();
  const extended = {
    ...owner, path: "not-a-wire-capability", authorized: true,
    preview: { ...owner.preview, sourceId: "not-a-wire-field",
      selection: { ...owner.preview.selection, receipt: {},
        fields: { ...owner.preview.selection.fields, bypass: true } } },
  };
  await sessionImportIdentityAssociationInspect(extended);
  expect(invoke).toHaveBeenCalledExactlyOnceWith("session_import_identity_association_inspect", { request: owner });
  expect(extended.authorized).toBe(true);
});

test("does not trim IDs, deduplicate, reorder or expand raw selection", async () => {
  const owner = request();
  owner.importId = ` ${owner.importId} `;
  owner.preview.selection.eventIds = ["event:2", " event:1 ", "event:2"];
  owner.preview.selection.pinnedEventIds = [" event:1 ", " event:1 "];
  await sessionImportIdentityAssociationInspect(owner);
  expect(invoke).toHaveBeenCalledExactlyOnceWith("session_import_identity_association_inspect", { request: owner });
});

test.each(["PLAN_STALE", "MAPPING_CONFLICT", "SOURCE_BUSY", "HISTORY_ACCESS_UNAVAILABLE", "RESOURCE_LIMIT"])(
  "%s propagates once without another target, retry or commit", async (code) => {
    const error = new Error(`${code}: synthetic inspection refusal`);
    invoke.mockRejectedValue(error);
    await expect(sessionImportIdentityAssociationInspect(request())).rejects.toBe(error);
    expect(invoke).toHaveBeenCalledTimes(1);
    expect(invoke.mock.calls[0][0]).toBe("session_import_identity_association_inspect");
  },
);
