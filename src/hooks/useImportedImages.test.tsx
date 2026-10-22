/** @vitest-environment jsdom */
import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { useImportedImages } from "./useImportedImages";
import type { ImportedImageScope, ImportedImageView } from "@/lib/api/sessionMigrationImages";

const resolveImage = vi.hoisted(() => vi.fn());
vi.mock("@/lib/api/sessionMigrationImages", () => ({ resolveImportedImage: resolveImage }));
beforeEach(() => resolveImage.mockReset());
afterEach(cleanup);
const events = (...ids: string[]) => [{ id: "message", kind: "message", blocks: ids.map((asset_id) => ({ kind: "asset", asset_id })) }];
const preview = (previewId: string): ImportedImageScope => ({ kind: "preview", previewId });
const stored = (name: string): ImportedImageView => ({ state: "stored", thumbnail: `${name}-thumb`, original: `${name}-original`,
  image: { mime: "image/png", width: 600, height: 400, bytes: 1000 } });
function deferred() {
  let resolve!: (result: ImportedImageView) => void;
  const promise = new Promise<ImportedImageView>((done) => { resolve = done; });
  return { promise, resolve };
}

test("two native slots remain occupied after cancellation; queued old owners never start", async () => {
  const oldA = deferred(); const oldB = deferred();
  resolveImage.mockReturnValueOnce(oldA.promise).mockReturnValueOnce(oldB.promise)
    .mockImplementation(async (scope: ImportedImageScope, id: string) => stored(`${JSON.stringify(scope)}-${id}`));
  const view = renderHook(({ scope }) => useImportedImages(scope, events("a", "b", "c", "d"), "branch"),
    { initialProps: { scope: preview("old") } });
  expect(resolveImage).toHaveBeenCalledTimes(2);
  view.rerender({ scope: preview("new") });
  expect(resolveImage).toHaveBeenCalledTimes(2);
  await act(async () => { oldA.resolve(stored("stale-a")); oldB.resolve(stored("stale-b")); });
  await waitFor(() => expect(view.result.current.states.d.state).toBe("stored"));
  expect(resolveImage.mock.calls.slice(2).every(([scope]) => scope.previewId === "new")).toBe(true);
  expect(resolveImage).toHaveBeenCalledTimes(6);
  expect(JSON.stringify(view.result.current.states)).not.toContain("stale");
});

test("cache includes scope and branch, page extension reuses successes, retry is explicit", async () => {
  resolveImage.mockImplementation(async (_scope: ImportedImageScope, id: string) => stored(id));
  const view = renderHook(({ ids, branch }) => useImportedImages(preview("same"), events(...ids), branch),
    { initialProps: { ids: ["a"], branch: "first" } });
  await waitFor(() => expect(view.result.current.states.a.state).toBe("stored"));
  view.rerender({ ids: ["a", "b"], branch: "first" });
  await waitFor(() => expect(view.result.current.states.b.state).toBe("stored"));
  expect(resolveImage).toHaveBeenCalledTimes(2);
  resolveImage.mockRejectedValueOnce(new Error("PRIVATE_SOURCE_PATH"));
  view.rerender({ ids: ["a"], branch: "second" });
  await waitFor(() => expect(view.result.current.states.a.state).toBe("error"));
  expect(JSON.stringify(view.result.current.states)).not.toContain("PRIVATE_SOURCE_PATH");
  expect(resolveImage).toHaveBeenCalledTimes(3);
  act(() => view.result.current.retry("a"));
  await waitFor(() => expect(view.result.current.states.a.state).toBe("stored"));
  expect(resolveImage).toHaveBeenCalledTimes(4);
});

test("closing a reader drops late results and published revisions never reuse preview bytes", async () => {
  const pending = deferred();
  resolveImage.mockReturnValueOnce(pending.promise).mockResolvedValue(stored("published"));
  const view = renderHook(({ scope }: { scope: ImportedImageScope | null }) => useImportedImages(scope, events("a"), "owner"),
    { initialProps: { scope: preview("draft") as ImportedImageScope | null } });
  view.rerender({ scope: null });
  await act(async () => pending.resolve(stored("OLD_PRIVATE_BYTES")));
  expect(JSON.stringify(view.result.current.states)).not.toContain("OLD_PRIVATE_BYTES");
  view.rerender({ scope: { kind: "published", importId: "history", revisionId: "revision" } });
  await waitFor(() => expect(view.result.current.states.a.state).toBe("stored"));
  expect(resolveImage).toHaveBeenLastCalledWith({ kind: "published", importId: "history", revisionId: "revision" }, "a");
});
