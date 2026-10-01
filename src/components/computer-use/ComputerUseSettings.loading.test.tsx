/** @vitest-environment jsdom */
import { StrictMode } from "react";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import "@testing-library/jest-dom/vitest";
import * as api from "@/lib/api/computerUse";
import { ComputerUseSettings } from "./ComputerUseSettings";

vi.mock("@/lib/api/computerUse", () => ({
  computerStatus: vi.fn(), computerSetEnabled: vi.fn(), computerRuntimeStatus: vi.fn(),
  computerRuntimeRepair: vi.fn(), computerRuntimeRollback: vi.fn(), computerExportBundle: vi.fn(),
  computerClearTraces: vi.fn(), computerClearStaging: vi.fn(), computerClearManagedProfiles: vi.fn(),
}));
function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: unknown) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}
const status = (featureEnabled: boolean) => ({ featureEnabled } as api.ComputerStatus);
const ready = { issues: [], canRepair: true, canRollback: false };
const advance = (ms: number) => act(async () => { await vi.advanceTimersByTimeAsync(ms); });
beforeEach(() => {
  vi.resetAllMocks(); vi.useFakeTimers();
  vi.mocked(api.computerStatus).mockResolvedValue(status(false));
  vi.mocked(api.computerRuntimeStatus).mockResolvedValue(ready);
});
afterEach(() => { cleanup(); vi.useRealTimers(); });

it.each(["feature", "runtime"])("bounds a hung %s read without displaying a healthy or writable state", async kind => {
  if (kind === "feature") vi.mocked(api.computerStatus).mockReturnValue(new Promise(() => {}));
  else vi.mocked(api.computerRuntimeStatus).mockReturnValue(new Promise(() => {}));
  render(<ComputerUseSettings locale="en" />);
  await advance(9_999);
  expect(screen.queryByRole("button", { name: "Retry" })).toBeNull();
  await advance(1);
  expect(screen.getByRole("alert")).toHaveTextContent("Could not load Computer Use settings. Retry to check the current state.");
  expect(screen.queryByText("Unavailable")).toBeNull();
  expect(screen.getByText("Runtime status not confirmed.")).toHaveAttribute("role", "status");
  expect(screen.getByRole("button", { name: "Retry" })).toBeEnabled();
  expect(screen.getByRole("switch")).toBeDisabled();
  expect(screen.queryByText("App-owned runtime is ready. System Node is not used.")).toBeNull();
  expect(api.computerSetEnabled).not.toHaveBeenCalled();
  expect(api.computerRuntimeRepair).not.toHaveBeenCalled();
});

it.each(["resolve", "reject"] as const)("ignores an expired %s while a newer read is pending", async outcome => {
  const old = deferred<api.ComputerStatus>();
  const current = deferred<api.ComputerStatus>();
  vi.mocked(api.computerStatus).mockReturnValueOnce(old.promise).mockReturnValueOnce(current.promise);
  render(<ComputerUseSettings locale="en" />);
  await advance(10_000);
  fireEvent.click(screen.getByRole("button", { name: "Retry" }));
  await act(async () => {
    if (outcome === "resolve") old.resolve(status(true)); else old.reject(new Error("late failure"));
  });
  expect(screen.getByRole("switch")).toBeDisabled();
  expect(screen.queryByRole("alert")).toBeNull();
  expect(screen.queryByText("App-owned runtime is ready. System Node is not used.")).toBeNull();
  await act(async () => current.resolve(status(false)));
  expect(screen.getByRole("switch")).toBeEnabled();
  expect(screen.getByRole("switch")).toHaveAttribute("aria-checked", "false");
  expect(api.computerStatus).toHaveBeenCalledTimes(2);
});

it.each(["resolve", "reject"] as const)("does not replace a successful retry with the expired %s", async outcome => {
  const old = deferred<api.ComputerRuntimeStatus>();
  vi.mocked(api.computerRuntimeStatus).mockReturnValueOnce(old.promise).mockResolvedValueOnce(ready);
  render(<ComputerUseSettings locale="en" />);
  await advance(10_000);
  fireEvent.click(screen.getByRole("button", { name: "Retry" }));
  await act(async () => {});
  expect(screen.getByRole("switch")).toBeEnabled();
  await act(async () => {
    if (outcome === "resolve") old.resolve({ ...ready, issues: [{ code: "missing_file", component: "old", action: "repair" }] });
    else old.reject(new Error("late runtime failure"));
  });
  expect(screen.queryByRole("alert")).toBeNull();
  expect(screen.getByText("App-owned runtime is ready. System Node is not used.")).toBeInTheDocument();
});

it("retires only its read deadline on unmount and absorbs late failures", async () => {
  const pending = deferred<api.ComputerStatus>();
  vi.mocked(api.computerStatus).mockReturnValue(pending.promise);
  const { unmount } = render(<ComputerUseSettings locale="en" />);
  await act(async () => {});
  unmount();
  expect(vi.getTimerCount()).toBe(0);
  await act(async () => pending.reject(new Error("late after unmount")));
  expect(api.computerSetEnabled).not.toHaveBeenCalled();
});

it("starts a fresh read after StrictMode cleanup rather than inheriting the retired request", async () => {
  const old = deferred<api.ComputerStatus>();
  vi.mocked(api.computerStatus).mockReturnValueOnce(old.promise).mockResolvedValueOnce(status(false));
  render(<StrictMode><ComputerUseSettings locale="en" /></StrictMode>);
  await act(async () => {});
  expect(api.computerStatus).toHaveBeenCalledTimes(2);
  expect(screen.getByRole("switch")).toBeEnabled();
  await act(async () => old.resolve(status(true)));
  expect(screen.getByRole("switch")).toHaveAttribute("aria-checked", "false");
});

it("never applies the read timeout to an unresolved runtime repair or replays the mutation", async () => {
  vi.mocked(api.computerRuntimeRepair).mockReturnValue(new Promise(() => {}));
  render(<ComputerUseSettings locale="en" />);
  await act(async () => {});
  fireEvent.click(screen.getByRole("button", { name: "Repair runtime" }));
  await advance(20_000);
  expect(screen.getByRole("button", { name: "Repair runtime" })).toBeDisabled();
  expect(screen.getByRole("switch")).toBeDisabled();
  expect(screen.queryByRole("button", { name: "Retry" })).toBeNull();
  expect(api.computerRuntimeRepair).toHaveBeenCalledTimes(1);
});

it.each([
  ["repair", "resolve"], ["repair", "reject"],
  ["rollback", "resolve"], ["rollback", "reject"],
] as const)("bounds a hung readback after completed %s and ignores its late %s without replay", async (kind, outcome) => {
  const old = deferred<api.ComputerRuntimeStatus>();
  vi.mocked(api.computerRuntimeStatus)
    .mockResolvedValueOnce({ ...ready, canRollback: true })
    .mockReturnValueOnce(old.promise)
    .mockResolvedValueOnce(ready);
  render(<ComputerUseSettings locale="en" />);
  await act(async () => {});
  fireEvent.click(screen.getByRole("button", { name: kind === "repair" ? "Repair runtime" : "Roll back runtime" }));
  await act(async () => {});
  expect(screen.queryByText("App-owned runtime is ready. System Node is not used.")).toBeNull();
  await advance(10_000);
  expect(screen.getByRole("button", { name: "Retry" })).toBeEnabled();
  expect(screen.queryByText("Unavailable")).toBeNull();
  expect(screen.getByText("Runtime status not confirmed.")).toHaveAttribute("role", "status");
  expect(screen.getByRole("button", { name: "Repair runtime" })).toBeDisabled();
  expect(screen.getByRole("switch")).toBeDisabled();
  fireEvent.click(screen.getByRole("button", { name: "Retry" }));
  await act(async () => {});
  expect(screen.getByText("App-owned runtime is ready. System Node is not used.")).toBeInTheDocument();
  await act(async () => {
    if (outcome === "resolve") old.resolve({ ...ready, issues: [{ code: "hash_mismatch", component: "old", action: "repair" }] });
    else old.reject(new Error("late readback failure"));
  });
  expect(screen.queryByRole("alert")).toBeNull();
  expect(screen.getByText("App-owned runtime is ready. System Node is not used.")).toBeInTheDocument();
  expect(kind === "repair" ? api.computerRuntimeRepair : api.computerRuntimeRollback).toHaveBeenCalledTimes(1);
  expect(kind === "repair" ? api.computerRuntimeRollback : api.computerRuntimeRepair).not.toHaveBeenCalled();
});

it("hides pre-mutation health while native repair remains unresolved", async () => {
  vi.mocked(api.computerRuntimeRepair).mockReturnValue(new Promise(() => {}));
  render(<ComputerUseSettings locale="en" />);
  await act(async () => {});
  fireEvent.click(screen.getByRole("button", { name: "Repair runtime" }));
  expect(screen.queryByText("App-owned runtime is ready. System Node is not used.")).toBeNull();
  await advance(20_000);
  expect(screen.queryByRole("button", { name: "Retry" })).toBeNull();
  expect(api.computerRuntimeStatus).toHaveBeenCalledTimes(1);
  expect(api.computerRuntimeRepair).toHaveBeenCalledTimes(1);
});

it("preserves a native mutation error while independently refreshing current status", async () => {
  vi.mocked(api.computerRuntimeRepair).mockRejectedValue(new Error("repair failed"));
  render(<ComputerUseSettings locale="en" />);
  await act(async () => {});
  fireEvent.click(screen.getByRole("button", { name: "Repair runtime" }));
  await act(async () => {});
  expect(screen.getByRole("alert")).toBeInTheDocument();
  expect(api.computerRuntimeStatus).toHaveBeenCalledTimes(2);
  expect(api.computerRuntimeRepair).toHaveBeenCalledTimes(1);
});

it("does not start a readback when a native mutation settles after unmount", async () => {
  const mutation = deferred<void>();
  vi.mocked(api.computerRuntimeRepair).mockReturnValue(mutation.promise);
  const { unmount } = render(<ComputerUseSettings locale="en" />);
  await act(async () => {});
  fireEvent.click(screen.getByRole("button", { name: "Repair runtime" }));
  unmount();
  await act(async () => mutation.resolve());
  expect(vi.getTimerCount()).toBe(0);
  expect(api.computerRuntimeStatus).toHaveBeenCalledTimes(1);
  expect(api.computerRuntimeRepair).toHaveBeenCalledTimes(1);
});
