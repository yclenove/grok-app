/** @vitest-environment jsdom */
import { StrictMode } from "react";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import "@testing-library/jest-dom/vitest";
import { createT, loadLocaleCatalog } from "@/i18n";
import { LOCALES } from "@/i18n/messages";
import { address, deferred, key, pendingView, runningView, setupTaskUi } from "./taskUiTestSupport";
import * as events from "@/lib/api/computerTaskEvents";
import * as taskApi from "@/lib/api/computerTask";
import * as pickerApi from "@/lib/api/computerTaskPicker";
import * as managedApi from "@/lib/api/computerTaskManaged";
import { ComputerPanel } from "./ComputerPanel";
import { ComputerTaskCardLive } from "./ComputerTaskCardLive";

let ui: ReturnType<typeof setupTaskUi>;
beforeEach(() => { ui = setupTaskUi(); vi.useFakeTimers(); });
afterEach(() => { cleanup(); vi.useRealTimers(); });
function managedView() {
  const host = pendingView();
  host.request!.surfaces = ["managed-browser"];
  host.pickerSurfaces = ["managed-browser"];
  return host;
}
async function start(shared = false) {
  const rendered = render(<StrictMode><ComputerPanel locale="en" sessionId="chat" runId={null} />
    {shared ? <ComputerTaskCardLive locale="en" sessionId="chat" /> : null}</StrictMode>);
  await act(async () => {}); await ui.emit(managedView());
  return rendered;
}
const approve = () => screen.getAllByRole("button", { name: "Enable and allow this task" });
const settle = () => act(async () => {});

it("mounts without preparation, states the actual managed policy and shares one approval between card and panel", async () => {
  await start(true);
  expect(screen.getAllByText("Isolated task browser · current tab only. Your existing browser stays private.")).toHaveLength(2);
  expect(managedApi.computerTaskManagedApprove).not.toHaveBeenCalled();
  const buttons = approve();
  await act(async () => { fireEvent.click(buttons[0]); fireEvent.click(buttons[1]); });
  expect(managedApi.computerTaskManagedApprove).toHaveBeenCalledExactlyOnceWith("chat", key);
  expect(screen.getAllByText("Preparing access…")).toHaveLength(2);
  expect(screen.queryByRole("dialog")).toBeNull();
  for (const call of Object.values(pickerApi)) expect(call).not.toHaveBeenCalled();
  expect(taskApi.computerTaskPreview).not.toHaveBeenCalled();
  expect(screen.getAllByRole("button", { name: "Stop" }).every((button) => !button.hasAttribute("disabled"))).toBe(true);
  expect(screen.getAllByRole("button", { name: "Cancel" })).toHaveLength(2);
  expect(screen.queryByText("Control authorized")).toBeNull();
});

it("can cancel before Host preparing arrives and isolates the late failure", async () => {
  const reply = deferred<Awaited<ReturnType<typeof managedApi.computerTaskManagedApprove>>>();
  vi.mocked(managedApi.computerTaskManagedApprove).mockReturnValueOnce(reply.promise);
  await start(); fireEvent.click(approve()[0]);
  fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
  expect(taskApi.computerTaskDecide).toHaveBeenCalledExactlyOnceWith("chat", key, "cancel");
  await act(async () => { reply.reject(new Error("old preparation")); });
  expect(screen.queryByRole("alert")).toBeNull();
});

it("Stop can overtake creation before a run exists without waiting for its reply", async () => {
  const reply = deferred<Awaited<ReturnType<typeof managedApi.computerTaskManagedApprove>>>();
  vi.mocked(managedApi.computerTaskManagedApprove).mockReturnValueOnce(reply.promise);
  await start(); fireEvent.click(approve()[0]);
  fireEvent.click(screen.getByRole("button", { name: "Stop" }));
  expect(taskApi.computerTaskDecide).toHaveBeenCalledExactlyOnceWith("chat", key, "stop");
  await act(async () => { reply.resolve({ requestId: key.requestId, revision: key.revision, runId: "old-run", decisionClass: "synthetic" }); });
  expect(screen.queryByText("Control authorized")).toBeNull();
  expect(taskApi.computerTaskPreview).not.toHaveBeenCalled();
  expect(screen.queryByRole("alert")).toBeNull();
});

it("shows a short preparation error and requires an explicit new gesture to retry", async () => {
  vi.mocked(managedApi.computerTaskManagedApprove).mockRejectedValueOnce(new Error("synthetic runtime unavailable"));
  await start(); await act(async () => { fireEvent.click(approve()[0]); });
  expect(screen.getByRole("alert")).toHaveTextContent("Could not prepare the task browser. Try again.");
  expect(approve()[0]).toBeEnabled();
  await act(async () => { await vi.advanceTimersByTimeAsync(2000); });
  expect(managedApi.computerTaskManagedApprove).toHaveBeenCalledTimes(1);
  await act(async () => { fireEvent.click(approve()[0]); });
  expect(managedApi.computerTaskManagedApprove).toHaveBeenCalledTimes(2);
  expect(screen.queryByRole("alert")).toBeNull();
});

it("waits for original Host authority before showing a preview, never for the local receipt alone", async () => {
  await start(); await act(async () => { fireEvent.click(approve()[0]); });
  expect(taskApi.computerTaskPreview).not.toHaveBeenCalled();
  const approved = runningView(); approved.request!.surfaces = ["managed-browser"];
  approved.pickerSurfaces = ["managed-browser"];
  approved.grants[0].members[0].surface = "managed-browser";
  approved.executions[0].target!.surface = "managed-browser";
  await ui.emit(approved);
  expect(screen.getByText("Control authorized")).toBeInTheDocument();
  expect(taskApi.computerTaskPreview).toHaveBeenCalledExactlyOnceWith("chat", address);
  expect(screen.queryByRole("button", { name: "Cancel" })).toBeNull();
});

it("changing the surface alone never creates an environment", async () => {
  await start(); const host = managedView();
  host.request!.surfaces = ["desktop", "managed-browser"];
  host.pickerSurfaces = ["desktop", "managed-browser"];
  await ui.emit(host);
  fireEvent.click(screen.getByRole("button", { name: "Surface" }));
  fireEvent.click(screen.getByRole("button", { name: createT("en")("cu.surface.managedBrowser") }));
  await settle();
  expect(managedApi.computerTaskManagedApprove).not.toHaveBeenCalled();
  expect(pickerApi.computerTaskPickerBegin).not.toHaveBeenCalled();
});

it("hiding the last consumer does not revoke an explicit managed approval", async () => {
  const reply = deferred<Awaited<ReturnType<typeof managedApi.computerTaskManagedApprove>>>();
  vi.mocked(managedApi.computerTaskManagedApprove).mockReturnValueOnce(reply.promise);
  const rendered = await start(); fireEvent.click(approve()[0]);
  rendered.unmount(); await settle();
  expect(taskApi.computerTaskDecide).not.toHaveBeenCalled();
  expect(pickerApi.computerTaskPickerCancel).not.toHaveBeenCalled();
  await act(async () => { reply.reject(new Error("owner retained in backend")); });
  expect(events.computerTaskUnsubscribe).toHaveBeenCalled();
});

it.each(LOCALES)("renders managed scope and retry in %s without permission side effects", async (locale) => {
  await loadLocaleCatalog(locale); const tr = createT(locale);
  render(<ComputerPanel locale={locale} sessionId="chat" runId={null} />);
  await settle(); await ui.emit(managedView());
  expect(screen.getByText(tr("cu.consent.managedScope"))).toBeInTheDocument();
  vi.mocked(managedApi.computerTaskManagedApprove).mockRejectedValueOnce(new Error("synthetic failure"));
  await act(async () => { fireEvent.click(screen.getByRole("button", { name: tr("cu.consent.enableAllow") })); });
  expect(screen.getByRole("alert")).toHaveTextContent(tr("cu.consent.managedFailed"));
});

it.each(LOCALES)("explains the managed-only macOS floor and unknown version in %s", async (locale) => {
  await loadLocaleCatalog(locale); const tr = createT(locale);
  render(<ComputerPanel locale={locale} sessionId="chat" runId={null} />);
  await settle(); await ui.emit(managedView());
  for (const [message, label] of [
    ["managed_browser_macos_requires_14: native macOS 13", "cu.consent.managedMacosMinimum"],
    ["managed_browser_os_version_unavailable: native query failed", "cu.consent.managedOsVersionUnavailable"],
  ] as const) {
    vi.mocked(managedApi.computerTaskManagedApprove).mockRejectedValueOnce(message);
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: tr("cu.consent.enableAllow") })); });
    expect(screen.getByRole("alert")).toHaveTextContent(tr(label));
    expect(screen.queryByText(tr("cu.consent.managedFailed"))).toBeNull();
    expect(screen.queryByText(tr("cu.workspace.authorized"))).toBeNull();
  }
  expect(managedApi.computerTaskManagedApprove).toHaveBeenCalledTimes(2);
  expect(taskApi.computerTaskPreview).not.toHaveBeenCalled();
  for (const call of Object.values(pickerApi)) expect(call).not.toHaveBeenCalled();
});
