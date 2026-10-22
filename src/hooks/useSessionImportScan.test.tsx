/** @vitest-environment jsdom */
import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import type { CandidateSelection, ScanState, SourceCandidate } from "@/lib/api/sessionMigration";
import { useSessionImportScan } from "./useSessionImportScan";

const mocks = vi.hoisted(() => ({
  start: vi.fn(), get: vi.fn(), page: vi.fn(), cancel: vi.fn(), resume: vi.fn(),
  freeze: vi.fn(), clear: vi.fn(), set: vi.fn(), invert: vi.fn(), membership: vi.fn(),
}));
vi.mock("@/lib/api/sessionMigration", () => ({
  sessionImportScanStart: mocks.start, sessionImportScanGet: mocks.get, sessionImportScanPage: mocks.page,
  sessionImportScanCancel: mocks.cancel, sessionImportScanContinue: mocks.resume,
  sessionImportScanSelectionFreeze: mocks.freeze,
  sessionImportScanSelectionClear: mocks.clear,
  sessionImportScanSelectionSet: mocks.set,
  sessionImportScanSelectionInvert: mocks.invert,
  sessionImportScanSelectionMembership: mocks.membership,
}));

const candidate = (id: string): SourceCandidate => ({ candidateId: id, provider: "codex", nativeSessionId: id,
  title: id, locator: id, sourceRevision: id, completeness: "metadata-preview" });
let hostSelected = new Set<string>();
let selectionVersion = 1;
const selection = (scanId = "a", mode: CandidateSelection["mode"] = "explicit"): CandidateSelection => ({
  version: 1,
  selectionId: `selection-${scanId}`,
  scanId,
  epoch: 1,
  mode,
  frozenThrough: mode === "frozen-scan" ? 70 : null,
  selectedCount: hostSelected.size,
  stateVersion: selectionVersion,
});
const state = (id: string, status: ScanState["state"] = "scanning", version = 1): ScanState => ({
  scanId: id, epoch: 1, state: status, stateVersion: version, candidates: [], discoveredCount: 70,
  complete: ["completed", "failed", "cancelled"].includes(status), issueCount: 0, extraCount: 0,
  selection: selection(id),
});
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => { resolve = done; });
  return { promise, resolve };
}

beforeEach(() => {
  vi.useFakeTimers(); vi.resetAllMocks();
  hostSelected = new Set();
  selectionVersion = 1;
  mocks.start.mockResolvedValue(state("a"));
  mocks.get.mockImplementation((id: string) => Promise.resolve(state(id)));
  mocks.page.mockResolvedValue({ candidates: [candidate("a1"), candidate("a2")], nextCursor: null, discoveredCount: 2, complete: false });
  mocks.cancel.mockImplementation((id: string) => Promise.resolve(state(id, "cancelled", 3)));
  mocks.membership.mockImplementation((_scanId: string, _epoch: number, selectionId: string, ids: string[]) =>
    Promise.resolve({ selectionId, stateVersion: selectionVersion, selectedIds: ids.filter((id) => hostSelected.has(id)) }));
  mocks.set.mockImplementation((scanId: string, _epoch: number, _selectionId: string, ids: string[], selected: boolean) => {
    for (const id of ids) {
      if (selected) hostSelected.add(id);
      else hostSelected.delete(id);
    }
    selectionVersion += 1;
    return Promise.resolve(selection(scanId));
  });
  mocks.invert.mockImplementation((scanId: string, _epoch: number, _selectionId: string, ids: string[]) => {
    for (const id of ids) {
      if (!hostSelected.delete(id)) hostSelected.add(id);
    }
    selectionVersion += 1;
    return Promise.resolve(selection(scanId));
  });
  mocks.clear.mockImplementation((scanId: string) => {
    hostSelected.clear();
    selectionVersion += 1;
    return Promise.resolve(selection(scanId));
  });
  mocks.freeze.mockImplementation((scanId: string) => {
    hostSelected = new Set(["a1", "a2", "off-page"]);
    selectionVersion += 1;
    return Promise.resolve(selection(scanId, "frozen-scan"));
  });
});
afterEach(() => { cleanup(); vi.useRealTimers(); });

test("V117 scan startup status ends after the Host accepts the request", async () => {
  const pending = deferred<ScanState>();
  mocks.start.mockReturnValueOnce(pending.promise);
  const error = vi.fn();
  const { result } = renderHook(() => useSessionImportScan(error));
  expect(result.current.scanStarting).toBe(false);
  let starting!: Promise<boolean>;
  act(() => { starting = result.current.startScan(); });
  expect(result.current.scanStarting).toBe(true);
  expect(result.current.scanBusy).toBe(true);
  await act(async () => { pending.resolve(state("a")); await starting; });
  expect(result.current.scanStarting).toBe(false);
  expect(result.current.scanning).toBe(true);
});

test("V117 a failed start releases only the startup indication", async () => {
  mocks.start.mockRejectedValueOnce(new Error("SOURCE_BUSY: fixture"));
  const error = vi.fn();
  const { result } = renderHook(() => useSessionImportScan(error));
  await act(async () => { expect(await result.current.startScan()).toBe(false); });
  expect(result.current.scanStarting).toBe(false);
  expect(result.current.scanBusy).toBe(false);
  expect(error).toHaveBeenCalledWith("Error: SOURCE_BUSY: fixture");
});

test("V117 paging and selection work cannot masquerade as starting a scan", async () => {
  mocks.start.mockResolvedValueOnce(state("a", "completed"));
  mocks.get.mockResolvedValue(state("a", "completed"));
  mocks.page.mockResolvedValue({ candidates: [candidate("a1")], nextCursor: "page2", discoveredCount: 2, complete: true });
  const error = vi.fn();
  const { result } = renderHook(() => useSessionImportScan(error));
  await act(async () => { await result.current.startScan(); });
  const page = deferred<{ candidates: SourceCandidate[]; nextCursor: null }>();
  mocks.page.mockReturnValueOnce(page.promise);
  let paging!: Promise<void>;
  act(() => { paging = result.current.changePage(1); });
  expect(result.current.scanBusy).toBe(true);
  expect(result.current.scanStarting).toBe(false);
  await act(async () => { page.resolve({ candidates: [candidate("a2")], nextCursor: null }); await paging; });
  const membership = deferred<CandidateSelection>();
  mocks.set.mockReturnValueOnce(membership.promise);
  let selecting!: Promise<void>;
  act(() => { selecting = result.current.selectThese(["a2"]); });
  expect(result.current.scanBusy).toBe(true);
  expect(result.current.scanStarting).toBe(false);
  await act(async () => { membership.resolve(selection()); await selecting; });
  expect(result.current.scanBusy).toBe(false);
});

test("scan start releases the UI and polling is bounded", async () => {
  const error = vi.fn(); const { result } = renderHook(() => useSessionImportScan(error));
  await act(async () => { await result.current.startScan(); });
  expect(result.current.scanBusy).toBe(false); expect(result.current.scanning).toBe(true);
  expect(mocks.get).toHaveBeenCalledTimes(1);
  await act(async () => { await vi.advanceTimersByTimeAsync(1500); });
  expect(mocks.get).toHaveBeenCalledTimes(4);
  expect(result.current.candidates).toHaveLength(2);
  expect(error).not.toHaveBeenCalled();
});

test("selectThese and invertThese only touch the given ids", async () => {
  const error = vi.fn(); const { result } = renderHook(() => useSessionImportScan(error));
  await act(async () => { await result.current.startScan(); });
  await act(async () => { await result.current.selectThese(["a1", "a2"]); });
  expect([...result.current.selected].sort()).toEqual(["a1", "a2"]);
  await act(async () => { await result.current.invertThese(["a2", "a3"]); });
  expect([...result.current.selected]).toEqual(["a1"]);
  expect(result.current.selectedCount).toBe(2);
});

test("clearSelected drops every checkbox without touching the scan", async () => {
  const error = vi.fn(); const { result } = renderHook(() => useSessionImportScan(error));
  await act(async () => { await result.current.startScan(); });
  await act(async () => { await result.current.selectCurrent(); });
  expect(result.current.selected.size).toBe(2);
  await act(async () => { await result.current.clearSelected(); });
  expect(result.current.selected.size).toBe(0);
  expect(result.current.scanState?.scanId).toBe("a");
});

test("deselecting after select-all changes the only selection used by import", async () => {
  const error = vi.fn(); const { result } = renderHook(() => useSessionImportScan(error));
  await act(async () => { await result.current.startScan(); });
  await act(async () => { await result.current.selectCurrent(); });
  await act(async () => { await result.current.toggle("a2"); });
  expect([...result.current.selected]).toEqual(["a1"]);
  expect(result.current.selectedCount).toBe(2);
  await act(async () => { await vi.advanceTimersByTimeAsync(500); });
  expect([...result.current.selected]).toEqual(["a1"]);
});

test("a late scan result cannot replace a newly selected source", async () => {
  const pending = deferred<ScanState>();
  mocks.get.mockImplementation((id: string) => id === "a" ? pending.promise : Promise.resolve(state(id)));
  const error = vi.fn(); const { result } = renderHook(() => useSessionImportScan(error));
  await act(async () => { await result.current.startScan(); });
  mocks.start.mockResolvedValue(state("b"));
  await act(async () => { await result.current.startScan([{ provider: "codex", path: "b" }]); });
  await act(async () => { pending.resolve(state("a", "completed", 99)); });
  expect(result.current.scanState?.scanId).toBe("b");
  expect(mocks.cancel).toHaveBeenCalledWith("a", 1);
  expect(result.current.selected.size).toBe(0);
});

test("page navigation replaces results, preserves selection, and supports going back", async () => {
  mocks.get.mockResolvedValue(state("a", "completed"));
  mocks.start.mockResolvedValue(state("a", "completed"));
  mocks.page.mockImplementation((_id: string, _epoch: number, cursor: string | null) => Promise.resolve({
    candidates: [candidate(cursor ? "a2" : "a1")], nextCursor: cursor ? null : "page-2", discoveredCount: 2, complete: true,
  }));
  const error = vi.fn(); const { result } = renderHook(() => useSessionImportScan(error));
  await act(async () => { await result.current.startScan(); });
  await act(async () => { await result.current.toggle("a1"); });
  await act(async () => { await result.current.changePage(1); });
  expect(result.current.candidates.map((c) => c.candidateId)).toEqual(["a2"]);
  expect(result.current.selected.has("a1")).toBe(false);
  expect(result.current.selectedCount).toBe(1);
  await act(async () => { await result.current.changePage(-1); });
  expect(result.current.candidates.map((c) => c.candidateId)).toEqual(["a1"]);
  expect(result.current.selected.has("a1")).toBe(true);
});

test("cancel stops polling and does not affect any import job", async () => {
  const error = vi.fn(); const { result } = renderHook(() => useSessionImportScan(error));
  await act(async () => { await result.current.startScan(); });
  mocks.get.mockResolvedValue(state("a", "cancelled", 3));
  await act(async () => { await result.current.controlScan("cancel"); });
  expect(result.current.scanning).toBe(false);
  const calls = mocks.get.mock.calls.length;
  await act(async () => { await vi.advanceTimersByTimeAsync(3000); });
  expect(mocks.get).toHaveBeenCalledTimes(calls);
});

test("paging while polling keeps the requested page", async () => {
  mocks.get.mockResolvedValue(state("a", "scanning"));
  mocks.page.mockImplementation((_id: string, _epoch: number, cursor: string | null) =>
    Promise.resolve({
      candidates: [candidate(cursor ? "a2" : "a1")],
      nextCursor: cursor ? null : "page-2",
      discoveredCount: 2,
      complete: false,
    }),
  );
  const error = vi.fn();
  const { result } = renderHook(() => useSessionImportScan(error));
  await act(async () => { await result.current.startScan(); });
  await act(async () => { await result.current.changePage(1); });
  expect(result.current.candidates.map((c) => c.candidateId)).toEqual(["a2"]);
  await act(async () => { await vi.advanceTimersByTimeAsync(500); });
  expect(mocks.page.mock.calls.some((call) => call[2] === "page-2")).toBe(true);
  expect(result.current.candidates.map((c) => c.candidateId)).toEqual(["a2"]);
});

test("continue after pause resumes polling", async () => {
  mocks.start.mockResolvedValue(state("a", "paused"));
  mocks.get.mockResolvedValue(state("a", "paused"));
  mocks.resume.mockResolvedValue(state("a", "queued", 2));
  const error = vi.fn();
  const { result } = renderHook(() => useSessionImportScan(error));
  await act(async () => { await result.current.startScan(); });
  expect(result.current.scanPaused).toBe(true);
  expect(result.current.scanning).toBe(false);
  const gets = mocks.get.mock.calls.length;
  mocks.get.mockResolvedValue(state("a", "scanning", 3));
  await act(async () => { await result.current.controlScan("continue"); });
  await act(async () => { await vi.advanceTimersByTimeAsync(500); });
  expect(mocks.get.mock.calls.length).toBeGreaterThan(gets);
});

test("host errors keep their code prefix", async () => {
  mocks.get.mockRejectedValue("PLAN_STALE: scan epoch mismatch");
  const error = vi.fn();
  const { result } = renderHook(() => useSessionImportScan(error));
  await act(async () => { await result.current.startScan(); });
  expect(error).toHaveBeenCalledWith("PLAN_STALE: scan epoch mismatch");
});

test("closing the wizard cancels an incomplete scan", async () => {
  const error = vi.fn();
  const { result } = renderHook(() => useSessionImportScan(error));
  await act(async () => { await result.current.startScan(); });
  act(() => { result.current.stopIncompleteScan(); });
  expect(mocks.cancel).toHaveBeenCalledWith("a", 1);
});

test("a queued response after unmount is cancelled", async () => {
  const pending = deferred<ScanState>(); mocks.start.mockReturnValue(pending.promise);
  const error = vi.fn(); const { result, unmount } = renderHook(() => useSessionImportScan(error));
  let start!: Promise<boolean>;
  act(() => { start = result.current.startScan(); });
  unmount();
  await act(async () => { pending.resolve(state("late")); await start; });
  expect(mocks.cancel).toHaveBeenCalledWith("late", 1);
  expect(mocks.get).not.toHaveBeenCalled();
});
