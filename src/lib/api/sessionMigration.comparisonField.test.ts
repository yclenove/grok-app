import { beforeEach, expect, test, vi } from "vitest";
import { sessionImportSourceComparisonField, type SourceComparisonFieldRequest } from "./sessionMigration";
import { comparisonDeferred } from "@/test/sessionImportComparisonFixture";
import { comparisonFieldContext, comparisonFieldPage } from "@/test/sessionImportComparisonFieldFixture";
const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("./host", () => ({ invoke }));
beforeEach(() => { invoke.mockReset(); });
function request(): SourceComparisonFieldRequest {
  const context = comparisonFieldContext();
  return { version: 1, left: context.owner.left, right: context.owner.right, side: context.side, position: context.position, field: "blocks",
    expectedFieldsDigest: context.fieldsDigest, cursor: "signed-field-only", limitBytes: 4096 };
}
test("public wrapper copies the complete ordered pair and does not rescan or create plans", async () => {
  const pending = comparisonDeferred<ReturnType<typeof comparisonFieldPage>>();
  invoke.mockReturnValue(pending.promise);
  const base = request(), left = { ...base.left }, right = { ...base.right };
  const original = { ...base, left, right }, copy = structuredClone(original);
  const promise = sessionImportSourceComparisonField(original);
  left.leafId = "changed";
  right.sourceRevision = "changed";
  expect(invoke).toHaveBeenCalledExactlyOnceWith("session_import_source_comparison_field", { request: {
    version: 1, left: copy.left, right: copy.right, side: copy.side, position: copy.position,
    field: copy.field, expectedFieldsDigest: copy.expectedFieldsDigest, cursor: copy.cursor, limitBytes: copy.limitBytes } });
  const response = comparisonFieldPage(); pending.resolve(response);
  expect(await promise).toEqual(response);
});
test("optional bytes/cursor are preserved, native errors propagate without fallback", async () => {
  invoke.mockRejectedValue(new Error("PLAN_STALE: expired field"));
  const r = request(); r.cursor = null; r.limitBytes = null;
  await expect(sessionImportSourceComparisonField(r)).rejects.toThrow("PLAN_STALE");
  expect(invoke).toHaveBeenCalledTimes(1);
  expect(invoke.mock.calls[0][1].request).toMatchObject({ cursor: null, limitBytes: null });
});

test("extended frozen summaries emit only the declared comparison identities, never UI metadata", async () => {
  const r = request();
  const extended = Object.freeze({ ...r, left: Object.freeze({ ...r.left, title: "display only", snapshotDigest: "extra" }),
    right: Object.freeze({ ...r.right, selected: true }), provider: "not-a-field-request", scope: comparisonFieldContext().owner.scope });
  invoke.mockResolvedValue(comparisonFieldPage());
  await sessionImportSourceComparisonField(extended);
  expect(invoke).toHaveBeenCalledExactlyOnceWith("session_import_source_comparison_field", { request: r });
  expect(extended.left.title).toBe("display only");
});
