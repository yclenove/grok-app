// @vitest-environment jsdom
// Real shared product UI with synthetic APIs; not a native permission proof.
import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import "@testing-library/jest-dom/vitest";
import { deferred, key, pendingView, setupTaskUi } from "./taskUiTestSupport";
import { createT, loadLocaleCatalog, LOCALES } from "@/i18n";
import * as pickerApi from "@/lib/api/computerTaskPicker";
import * as taskApi from "@/lib/api/computerTask";
import { ComputerPanel } from "./ComputerPanel";
import { ComputerTaskCardLive } from "./ComputerTaskCardLive";

let ui: ReturnType<typeof setupTaskUi>;
const existing = () => ({ ...pendingView(), pickerSurfaces: ["existing-tabs"], request: { ...pendingView().request!, surfaces: ["existing-tabs"] } });
let pairingExpiresAtMs = 0;
const ch = () => ({ nonce: "synthetic-nonce", instanceId: "instance", verificationCode: "123456", endpoint: "http://127.0.0.1:32100", installedExtensionId: "extension", expiresAtMs: pairingExpiresAtMs });
const preparation = (confirmed = false) => ({ sessionId: "chat", key, pickerId: "synthetic-picker", maxSelection: 2, surface: "existing-tabs" as const,
  pairing: ch(), pairingConfirmed: confirmed, ready: false, availableTabs: 0, incompatibleTabs: 0, borrowedTabs: 0 });
async function settle() { await act(async () => { await Promise.resolve(); }); }
beforeEach(() => {
  pairingExpiresAtMs = Date.now() + 60_000;
  ui = setupTaskUi();
  vi.mocked(pickerApi.computerTaskPickerPreparation).mockImplementation(async () => preparation());
  vi.mocked(pickerApi.computerTaskPickerPairingConfirm).mockResolvedValue();
  vi.mocked(pickerApi.computerTaskPickerLoad).mockImplementation(async picker => ({ ...picker, candidates: [{ candidateId: "original-tab@doc", title: "Shared fixture tab", appName: "Chrome", kind: "tab", surface: "existing-tabs" }], truncated: false }));
});
afterEach(() => { cleanup(); vi.useRealTimers(); });
async function start() {
  const rendered = render(<><ComputerTaskCardLive locale="en" sessionId="chat" /><ComputerPanel locale="en" sessionId="chat" runId={null} /></>);
  await settle(); await ui.emit(existing()); return rendered;
}
const allow = () => screen.getAllByRole("button", { name: "Choose shared tabs for this task" });

it("card/panel share one task-bound preparation and only its owner opens the code", async () => {
  await start(); expect(pickerApi.computerTaskPickerPreparation).not.toHaveBeenCalled();
  fireEvent.click(allow()[0]); await settle();
  const dialog = screen.getByRole("dialog", { name: "Choose shared tabs for this task" });
  expect(screen.getAllByRole("dialog")).toHaveLength(1); expect(screen.queryByTestId("cu-pairing-code")).toBeNull();
  expect(pickerApi.computerTaskPickerPreparation).toHaveBeenCalledTimes(1);
  expect(pickerApi.computerTaskPickerLoad).not.toHaveBeenCalled(); expect(pickerApi.computerTaskPickerApprove).not.toHaveBeenCalled();
  vi.mocked(pickerApi.computerTaskPickerPreparation).mockImplementationOnce(async () => preparation(true));
  fireEvent.click(within(dialog).getByRole("button", { name: "Confirm pairing in App" })); await settle();
  expect(pickerApi.computerTaskPickerPairingConfirm).toHaveBeenCalledExactlyOnceWith(expect.objectContaining({ sessionId: "chat", key, pickerId: "synthetic-picker", surface: "existing-tabs" }), "synthetic-nonce");
  expect(within(dialog).getByTestId("cu-pairing-code")).toHaveValue("123456");
  expect(pickerApi.computerTaskPickerLoad).not.toHaveBeenCalled();
  vi.mocked(pickerApi.computerTaskPickerPreparation).mockImplementationOnce(async () => ({ ...preparation(), pairing: null, ready: true, availableTabs: 1 }));
  fireEvent.click(within(dialog).getByRole("button", { name: "Check shared tabs" })); await settle();
  expect(screen.queryByTestId("cu-pairing-code")).toBeNull();
  expect(within(dialog).getByRole("button", { name: "Enable and allow this task" })).toBeDisabled();
  fireEvent.click(within(dialog).getByRole("button", { name: /Shared fixture tab/ }));
  fireEvent.click(within(dialog).getByRole("button", { name: "Enable and allow this task" })); await settle();
  expect(pickerApi.computerTaskPickerApprove).toHaveBeenCalledExactlyOnceWith(expect.objectContaining({ surface: "existing-tabs" }), ["original-tab@doc"]);
  expect(taskApi.computerTaskPreview).not.toHaveBeenCalled(); // Receipt is not authority.
});

it("a preparation failure has a visible retry and never uses global pairing", async () => {
  await start(); vi.mocked(pickerApi.computerTaskPickerPreparation).mockRejectedValueOnce(new Error("synthetic busy or unavailable"));
  fireEvent.click(allow()[1]); await settle();
  expect(screen.getAllByRole("alert")).toHaveLength(1);
  expect(screen.getByRole("alert")).toHaveTextContent("Could not check shared tabs");
  fireEvent.click(screen.getByRole("button", { name: "Check shared tabs" })); await settle();
  expect(screen.queryByRole("alert")).toBeNull(); expect(pickerApi.computerTaskPickerBegin).toHaveBeenCalledTimes(1);
  expect(pickerApi.computerTaskPickerPairingConfirm).not.toHaveBeenCalled();
});

it("Stop stays actionable during preparation and discards its late Share readiness", async () => {
  await start(); const reply = deferred<pickerApi.ComputerTaskPickerPreparation>();
  vi.mocked(pickerApi.computerTaskPickerPreparation).mockReturnValueOnce(reply.promise);
  fireEvent.click(allow()[0]); await settle();
  fireEvent.click(within(screen.getByRole("dialog")).getByRole("button", { name: "Stop" })); await settle();
  expect(taskApi.computerTaskDecide).toHaveBeenCalledExactlyOnceWith("chat", key, "stop");
  await act(async () => { reply.resolve({ ...preparation(), pairing: null, ready: true, availableTabs: 1 }); });
  expect(screen.queryByRole("dialog")).toBeNull(); expect(pickerApi.computerTaskPickerLoad).not.toHaveBeenCalled();
});

it("expired pairing hides the code and cannot confirm with a stale countdown", async () => {
  await start(); vi.mocked(pickerApi.computerTaskPickerPreparation).mockImplementationOnce(async () => ({ ...preparation(true), pairing: { ...ch(), expiresAtMs: Date.now() - 1 } }));
  fireEvent.click(allow()[0]); await settle();
  expect(screen.queryByTestId("cu-pairing-code")).toBeNull(); expect(screen.getByRole("alert")).toBeInTheDocument();
  expect(pickerApi.computerTaskPickerPairingConfirm).not.toHaveBeenCalled();
});

it("a slow background read keeps the confirmed copy control, feedback and keyboard focus stable", async () => {
  vi.useFakeTimers();
  vi.mocked(pickerApi.computerTaskPickerPreparation).mockImplementation(async () => preparation(true));
  const write = vi.fn().mockResolvedValue(undefined);
  Object.defineProperty(navigator, "clipboard", { configurable: true, value: { writeText: write } });
  await start(); fireEvent.click(allow()[0]); await settle();
  await act(async () => { await vi.advanceTimersByTimeAsync(0); });
  const copy = within(screen.getByRole("dialog")).getByRole("button", { name: "Copy App address" });
  copy.focus(); fireEvent.click(copy); await settle();
  expect(copy).toHaveFocus(); expect(screen.getByText("Copied")).toBeInTheDocument();
  const reply = deferred<pickerApi.ComputerTaskPickerPreparation>();
  vi.mocked(pickerApi.computerTaskPickerPreparation).mockReturnValueOnce(reply.promise);
  await act(async () => { await vi.advanceTimersByTimeAsync(1000); });
  expect(copy).toBeEnabled(); expect(copy).toHaveFocus(); expect(screen.getByText("Copied")).toBeInTheDocument();
  fireEvent.click(copy); await settle(); expect(write).toHaveBeenCalledTimes(2);
  await act(async () => { reply.resolve(preparation(true)); });
  expect(copy).toHaveFocus(); expect(copy).toBeEnabled();
});

it("confirm remains keyboard reachable during a slow poll and a queued gesture focuses the original address", async () => {
  vi.useFakeTimers(); await start(); fireEvent.click(allow()[0]); await settle();
  await act(async () => { await vi.advanceTimersByTimeAsync(0); });
  const confirm = within(screen.getByRole("dialog")).getByRole("button", { name: "Confirm pairing in App" });
  confirm.focus(); const reply = deferred<pickerApi.ComputerTaskPickerPreparation>();
  vi.mocked(pickerApi.computerTaskPickerPreparation).mockReturnValueOnce(reply.promise);
  await act(async () => { await vi.advanceTimersByTimeAsync(1000); });
  expect(confirm).toBeEnabled(); expect(confirm).toHaveFocus();
  fireEvent.click(confirm); await settle();
  expect(pickerApi.computerTaskPickerPairingConfirm).not.toHaveBeenCalled();
  vi.mocked(pickerApi.computerTaskPickerPreparation).mockResolvedValueOnce(preparation(true));
  await act(async () => { reply.resolve(preparation()); }); await settle();
  expect(pickerApi.computerTaskPickerPairingConfirm).toHaveBeenCalledTimes(1);
  expect(screen.getByLabelText("App address")).toHaveFocus();
});

it.each(LOCALES)("renders Pair/Share and busy/version states in %s without granting", async locale => {
  await loadLocaleCatalog(locale); const tr = createT(locale);
  render(<ComputerPanel locale={locale} sessionId="chat" runId={null} />); await settle(); await ui.emit(existing());
  vi.mocked(pickerApi.computerTaskPickerPreparation).mockImplementationOnce(async () => ({ ...preparation(), pairing: null, borrowedTabs: 2, incompatibleTabs: 1 }));
  fireEvent.click(screen.getByRole("button", { name: tr("cu.consent.selectTabs") })); await settle();
  expect(screen.getByRole("dialog", { name: tr("cu.consent.selectTabs") })).toBeInTheDocument();
  expect(screen.getByText(tr("cu.consent.existingShare"))).toBeInTheDocument();
  expect(screen.getByRole("button", { name: tr("cu.consent.existingRefresh") })).toBeEnabled();
  expect(pickerApi.computerTaskPickerLoad).not.toHaveBeenCalled(); expect(pickerApi.computerTaskPickerApprove).not.toHaveBeenCalled();
});
