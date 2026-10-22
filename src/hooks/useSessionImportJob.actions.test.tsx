/** @vitest-environment jsdom */
import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import type { JobRecord } from "@/lib/api/sessionMigration";
import { useSessionImportJob } from "./useSessionImportJob";

const mocks = vi.hoisted(() => ({ get: vi.fn(), listen: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: mocks.listen }));
vi.mock("@/lib/api/sessionMigration", () => ({ sessionImportJobGet: mocks.get }));

function state(version: number, id = "a"): JobRecord {
  return { jobId: id, stateVersion: version, state: "completed_with_issues", requestId: "initial",
    planHash: "plan", createdAt: "2026-10-15T00:00:00Z", items: [] };
}

beforeEach(() => { vi.resetAllMocks(); mocks.listen.mockResolvedValue(vi.fn()); });
afterEach(cleanup);

test.each(["transport reply lost", "SNAPSHOT_UNAVAILABLE: cleanup completion lost"])("%s keeps one request across refresh and explicit retry, then a new intent gets a new identity", async (failure) => {
  mocks.get.mockResolvedValue(state(8));
  const operation = vi.fn().mockRejectedValueOnce(new Error(failure))
    .mockResolvedValueOnce(state(9)).mockResolvedValueOnce(state(10));
  const { result } = renderHook(() => useSessionImportJob(vi.fn()));
  act(() => result.current.setJob(state(5)));
  await act(async () => { await result.current.mutateJob(operation); });
  expect(operation).toHaveBeenCalledTimes(1);
  expect(result.current.job?.stateVersion).toBe(8);
  const first = operation.mock.calls[0][1];
  expect(first).toEqual({ expectedStateVersion: 5, requestId: expect.stringMatching(/^[a-f0-9-]{36}$/) });
  await act(async () => { await result.current.mutateJob(operation); });
  expect(operation.mock.calls[1][1]).toEqual(first);
  expect(result.current.job?.stateVersion).toBe(9);
  await act(async () => { await result.current.mutateJob(operation); });
  expect(operation.mock.calls[2][1].expectedStateVersion).toBe(9);
  expect(operation.mock.calls[2][1].requestId).not.toBe(first.requestId);
});

test.each(["JOB_STATE_CHANGED", "JOB_ACTION_CONFLICT"])("%s refreshes without reapplying and requires a new reviewed action", async (code) => {
  mocks.get.mockResolvedValue(state(7));
  const operation = vi.fn().mockRejectedValueOnce(`${code}: synthetic refusal`).mockResolvedValue(state(8));
  const error = vi.fn();
  const { result } = renderHook(() => useSessionImportJob(error));
  act(() => result.current.setJob(state(5)));
  await act(async () => { await result.current.mutateJob(operation); });
  expect(operation).toHaveBeenCalledTimes(1);
  expect(error).toHaveBeenCalledWith(`${code}: synthetic refusal`);
  expect(result.current.job?.stateVersion).toBe(7);
  await act(async () => { await result.current.mutateJob(operation); });
  expect(operation.mock.calls[1][1].expectedStateVersion).toBe(7);
  expect(operation.mock.calls[1][1].requestId).not.toBe(operation.mock.calls[0][1].requestId);
});

test("another action cannot inherit an uncertain request from the same job", async () => {
  mocks.get.mockResolvedValue(state(6));
  const original = vi.fn().mockRejectedValueOnce("transport lost").mockResolvedValue(state(8));
  const other = vi.fn().mockResolvedValue(state(7));
  const { result } = renderHook(() => useSessionImportJob(vi.fn()));
  act(() => result.current.setJob(state(5)));
  await act(async () => { await result.current.mutateJob(original); });
  await act(async () => { await result.current.mutateJob(other); });
  expect(other.mock.calls[0][1].expectedStateVersion).toBe(6);
  expect(other.mock.calls[0][1].requestId).not.toBe(original.mock.calls[0][1].requestId);
  await act(async () => { await result.current.mutateJob(original); });
  expect(original.mock.calls[1][1].expectedStateVersion).toBe(7);
  expect(original.mock.calls[1][1].requestId).not.toBe(original.mock.calls[0][1].requestId);
});

test("adopting another job and returning never resurrects its previous uncertain request", async () => {
  mocks.get.mockResolvedValue(state(6));
  const operation = vi.fn().mockRejectedValueOnce("transport lost").mockResolvedValueOnce(state(31, "b"))
    .mockResolvedValueOnce(state(41));
  const { result } = renderHook(() => useSessionImportJob(vi.fn()));
  act(() => result.current.setJob(state(5)));
  await act(async () => { await result.current.mutateJob(operation); });
  act(() => result.current.setJob(state(30, "b")));
  await act(async () => { await result.current.mutateJob(operation); });
  act(() => result.current.setJob(state(40)));
  await act(async () => { await result.current.mutateJob(operation); });
  expect(operation.mock.calls.map(([id, request]) => [id, request.expectedStateVersion])).toEqual([["a", 5], ["b", 30], ["a", 40]]);
  expect(new Set(operation.mock.calls.map(([, request]) => request.requestId)).size).toBe(3);
});
