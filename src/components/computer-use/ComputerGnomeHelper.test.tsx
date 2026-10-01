/** @vitest-environment jsdom */
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import "@testing-library/jest-dom/vitest";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import * as api from "@/lib/api/computerGnomeHelper";
import { ComputerGnomeHelper } from "./ComputerGnomeHelper";

vi.mock("@/lib/api/computerGnomeHelper", () => ({ computerHelperStatus: vi.fn(), computerHelperAction: vi.fn() }));
const installed = (overrides: Partial<api.HelperStatus> = {}): api.HelperStatus => ({ available: true, busy: false, featureEnabled: false,
  state: "disabled", shellVersion: "46.0", installation: "current", actions: ["enable", "repair", "disable"], ...overrides });
function deferred<T>() { let resolve!: (value: T) => void; let reject!: (e: Error) => void;
  const promise = new Promise<T>((r, j) => { resolve = r; reject = j; }); return { promise, resolve, reject }; }
beforeEach(() => { vi.resetAllMocks(); vi.mocked(api.computerHelperStatus).mockResolvedValue(installed()); });
afterEach(() => { cleanup(); vi.useRealTimers(); });
const button = (name: string) => screen.getByRole("button", { name });

it("read-only mount and cancelled confirmation never install or enable", async () => {
  render(<ComputerGnomeHelper locale="en" />);
  await waitFor(() => expect(button("Enable helper")).toBeEnabled());
  expect(api.computerHelperAction).not.toHaveBeenCalled();
  fireEvent.click(button("Enable helper"));
  expect(screen.getByRole("dialog")).toHaveTextContent("does not grant desktop control");
  fireEvent.click(within(screen.getByRole("dialog")).getByRole("button", { name: "Cancel" }));
  expect(api.computerHelperAction).not.toHaveBeenCalled();
});
it("retains mutation until the original reply, then performs fresh readback", async () => {
  const mutation = deferred<api.HelperStatus>();
  vi.mocked(api.computerHelperAction).mockReturnValue(mutation.promise);
  render(<ComputerGnomeHelper locale="en" />);
  await waitFor(() => expect(button("Enable helper")).toBeEnabled());
  fireEvent.click(button("Enable helper"));
  fireEvent.click(within(screen.getByRole("dialog")).getByRole("button", { name: "Confirm" }));
  expect(api.computerHelperAction).toHaveBeenCalledExactlyOnceWith("enable");
  expect(button("Refresh helper status")).toBeDisabled();
  expect(within(screen.getByRole("dialog")).getByRole("button", { name: "Cancel" })).toBeDisabled();
  vi.mocked(api.computerHelperStatus).mockResolvedValue(installed({ state: "ready", actions: ["disable"] }));
  await act(async () => mutation.resolve(installed()));
  await waitFor(() => expect(screen.getByText(/Helper responds/)).toBeVisible());
  expect(button("Enable helper")).toBeDisabled();
  expect(api.computerHelperStatus).toHaveBeenCalledTimes(2);
});
it("failed mutation is not replayed and cannot preserve old ready status", async () => {
  vi.mocked(api.computerHelperAction).mockRejectedValue(new Error("unconfirmed"));
  render(<ComputerGnomeHelper locale="en" />);
  await waitFor(() => expect(button("Enable helper")).toBeEnabled());
  fireEvent.click(button("Enable helper"));
  vi.mocked(api.computerHelperStatus).mockResolvedValue(installed({ state: "unconfirmed", actions: [] }));
  fireEvent.click(within(screen.getByRole("dialog")).getByRole("button", { name: "Confirm" }));
  await waitFor(() => expect(screen.getByRole("alert")).toHaveTextContent("do not assume"));
  expect(api.computerHelperAction).toHaveBeenCalledTimes(1);
  expect(button("Enable helper")).toBeDisabled();
});
it.each<api.HelperState>(["restart_required", "global_disabled", "conflict", "unsupported", "unavailable", "repair_required", "blocked", "missing", "unconfirmed"])(
  "renders %s without inventing actions", async (state) => {
    vi.mocked(api.computerHelperStatus).mockResolvedValue(installed({ state, actions: [] }));
    render(<ComputerGnomeHelper locale="en" />);
    await waitFor(() => expect(button("Refresh helper status")).toBeEnabled());
    for (const label of ["Enable helper", "Disable helper", "Repair helper", "Install helper"]) expect(button(label)).toBeDisabled();
    expect(api.computerHelperAction).not.toHaveBeenCalled();
  });
it("late read after its presentation deadline cannot enable controls", async () => {
  vi.useFakeTimers();
  const old = deferred<api.HelperStatus>();
  vi.mocked(api.computerHelperStatus).mockReturnValueOnce(old.promise).mockResolvedValue(installed({ state: "conflict", actions: [] }));
  render(<ComputerGnomeHelper locale="en" />);
  await act(async () => { vi.advanceTimersByTime(10_001); });
  expect(screen.getByRole("alert")).toBeVisible();
  await act(async () => { fireEvent.click(button("Refresh helper status")); });
  await act(async () => old.resolve(installed()));
  expect(button("Enable helper")).toBeDisabled();
  expect(screen.getByText(/Conflicting or unmanaged/)).toBeVisible();
});
it("a settings switch change reloads eligibility without an automatic mutation", async () => {
  const view = render(<ComputerGnomeHelper locale="en" featureEnabled />);
  await waitFor(() => expect(api.computerHelperStatus).toHaveBeenCalledTimes(1));
  view.rerender(<ComputerGnomeHelper locale="en" featureEnabled={false} />);
  await waitFor(() => expect(api.computerHelperStatus).toHaveBeenCalledTimes(2));
  expect(api.computerHelperAction).not.toHaveBeenCalled();
});
it("unmounting never restarts or cancels a mutation", async () => {
  const mutation = deferred<api.HelperStatus>();
  vi.mocked(api.computerHelperAction).mockReturnValue(mutation.promise);
  const view = render(<ComputerGnomeHelper locale="en" />);
  await waitFor(() => expect(button("Enable helper")).toBeEnabled());
  fireEvent.click(button("Enable helper"));
  fireEvent.click(within(screen.getByRole("dialog")).getByRole("button", { name: "Confirm" }));
  view.unmount();
  await act(async () => mutation.resolve(installed()));
  expect(api.computerHelperAction).toHaveBeenCalledTimes(1);
  expect(api.computerHelperStatus).toHaveBeenCalledTimes(1);
});
