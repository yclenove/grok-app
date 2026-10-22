/** @vitest-environment jsdom */
import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { useSessionImportUi } from "./useSessionImport";

const mocks = vi.hoisted(() => ({
  entries: vi.fn().mockResolvedValue({ items: [] }),
  recovery: vi.fn().mockResolvedValue({ draft: null, expired: false }),
  continuePrepare: vi.fn(),
}));

// Keep the real UI, continuation, scan, prepare, workspace and job hooks.
// No external IPC is allowed; only the three boundary calls used here are stubbed.
vi.mock("@/lib/api", () => ({
  sessionImportEntries: mocks.entries,
  sessionImportRecovery: mocks.recovery,
  sessionImportContinuePrepare: mocks.continuePrepare,
}));

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (cause: unknown) => void;
  const promise = new Promise<T>((ok, fail) => { resolve = ok; reject = fail; });
  return { promise, resolve, reject };
}

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
  mocks.recovery.mockReset().mockResolvedValue({ draft: null, expired: false });
  mocks.continuePrepare.mockReset();
  window.history.replaceState(null, "", "/");
});

test("manual reader continuation forwards the viewed snapshot rather than unresolved route defaults", async () => {
  const { result } = renderHook(() => useSessionImportUi());
  await act(async () => result.current.setReaderId("imp:viewed"));
  await act(async () => result.current.prepareContinue("imp:viewed", "selected-event", "viewed-revision"));
  expect(mocks.continuePrepare).toHaveBeenLastCalledWith("imp:viewed", { projectId: null, leaf: "selected-event", revision: "viewed-revision" });
});
test("route continuation waits for and consumes exactly one accepted reader snapshot", async () => {
  const { result } = renderHook(() => useSessionImportUi());
  const { openImportedHistory } = await import("@/lib/sessionImport/open");
  await act(async () => openImportedHistory({ importId: "imp:00000000-0000-4000-8000-000000000105", continue: true }));
  expect(mocks.continuePrepare).not.toHaveBeenCalled();
  expect(result.current.readerContinueRequested).toBe(true);
  await act(async () => {
    await result.current.prepareReaderContinue(result.current.readerId!, "viewed-leaf", "viewed-revision");
    await result.current.prepareReaderContinue(result.current.readerId!, "viewed-leaf", "viewed-revision");
  });
  expect(mocks.continuePrepare).toHaveBeenCalledTimes(1);
  expect(mocks.continuePrepare).toHaveBeenCalledWith(result.current.readerId, { projectId: null, leaf: "viewed-leaf", revision: "viewed-revision" });
});


test.each(["close", "navigate"])("pending reader continuation is cancelled by %s", async (action) => {
  const { result } = renderHook(() => useSessionImportUi());
  const { openImportedHistory } = await import("@/lib/sessionImport/open");
  const id = "imp:00000000-0000-4000-8000-000000000106";
  await act(async () => openImportedHistory({ importId: id, continue: true }));
  const oldPrepare = result.current.prepareReaderContinue;
  await act(async () => {
    if (action === "close") result.current.setReaderId(null);
    else openImportedHistory({ importId: "imp:00000000-0000-4000-8000-000000000107" });
  });
  await act(async () => oldPrepare(id, "old-leaf", "old-revision"));
  expect(mocks.continuePrepare).not.toHaveBeenCalled();
  expect(result.current.readerContinueRequested).toBe(false);
});
test("an explicit route rejects a foreign reader scope before consuming its continuation intent", async () => {
  const { result } = renderHook(() => useSessionImportUi());
  const { openImportedHistory } = await import("@/lib/sessionImport/open");
  const id = "imp:00000000-0000-4000-8000-000000000108";
  await act(async () => openImportedHistory({ importId: id, revision: "fixed-rev", leaf: "fixed-leaf", continue: true }));
  await act(async () => result.current.prepareReaderContinue(id, "fixed-leaf", "foreign-rev"));
  await act(async () => result.current.prepareReaderContinue(id, "foreign-leaf", "fixed-rev"));
  expect(mocks.continuePrepare).not.toHaveBeenCalled();
  expect(result.current.readerContinueRequested).toBe(true);
  await act(async () => result.current.prepareReaderContinue(id, "fixed-leaf", "fixed-rev"));
  expect(mocks.continuePrepare).toHaveBeenCalledWith(id, { projectId: null, leaf: "fixed-leaf", revision: "fixed-rev" });
});

test.each(["draft", "expired", "failure"])("late startup recovery cannot replace a pending reader continuation (%s)", async (kind) => {
  const pending = deferred<{ draft?: { draftId: string; draftHash: string; text: string; importId: string }; expired?: boolean }>();
  mocks.recovery.mockReturnValueOnce(pending.promise);
  mocks.continuePrepare.mockResolvedValueOnce({ draftId: "requested-draft", draftHash: "new-hash", text: "requested", importId: "imp:00000000-0000-4000-8000-000000000109" });
  const { result } = renderHook(() => useSessionImportUi());
  const { openImportedHistory } = await import("@/lib/sessionImport/open");
  const id = "imp:00000000-0000-4000-8000-000000000109";
  await act(async () => openImportedHistory({ importId: id, continue: true }));
  await act(async () => {
    if (kind === "failure") pending.reject(new Error("stale recovery failed"));
    else pending.resolve(kind === "expired" ? { expired: true } : { draft: { draftId: "old-draft", draftHash: "old-hash", text: "stale", importId: "imp:old" } });
  });
  expect(result.current.draft).toBeNull();
  expect(result.current.restoreStatus).toBeNull();
  expect(result.current.error).toBeNull();
  expect(result.current.readerContinueRequested).toBe(true);
  await act(async () => result.current.prepareReaderContinue(id, "accepted-leaf", "accepted-rev"));
  expect(mocks.continuePrepare).toHaveBeenCalledWith(id, { projectId: null, leaf: "accepted-leaf", revision: "accepted-rev" });
  expect(result.current.draft).toMatchObject({ draftId: "requested-draft" });
});
