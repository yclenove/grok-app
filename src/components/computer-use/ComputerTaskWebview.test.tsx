// @vitest-environment jsdom
// Host-local candidate IDs only; this UI test does not manufacture native authority.
import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import "@testing-library/jest-dom/vitest";
import { key, pendingView, setupTaskUi } from "./taskUiTestSupport";
import { createT, loadLocaleCatalog, LOCALES } from "@/i18n";
import * as pickerApi from "@/lib/api/computerTaskPicker";
import * as taskApi from "@/lib/api/computerTask";
import { ComputerPanel } from "./ComputerPanel";

let ui: ReturnType<typeof setupTaskUi>;
const appWebview = () => ({
  ...pendingView(),
  pickerSurfaces: ["app-webview"],
  request: { ...pendingView().request!, surfaces: ["app-webview"] },
});
async function settle() { await act(async () => { await Promise.resolve(); }); }

beforeEach(() => { ui = setupTaskUi(); });
afterEach(() => cleanup());

it("selects an App-owned page through the shared task picker without a desktop fallback", async () => {
  vi.mocked(pickerApi.computerTaskPickerLoad).mockImplementation(async picker => ({
    ...picker,
    candidates: [{ candidateId: "host-local-page", title: "Generated images", appName: "side-browser", kind: "webview", surface: "app-webview" }],
    truncated: false,
  }));
  render(<ComputerPanel locale="en" sessionId="chat" runId={null} />);
  await settle(); await ui.emit(appWebview());
  expect(screen.getByText("Only pages already open inside this app are listed. Select the pages this task may operate.")).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Choose App pages for this task" }));
  await settle();
  const dialog = screen.getByRole("dialog", { name: "Choose App pages for this task" });
  expect(pickerApi.computerTaskPickerBegin).toHaveBeenCalledExactlyOnceWith("chat", key, "app-webview");
  expect(pickerApi.computerTaskPickerLoad).toHaveBeenCalledTimes(1);
  fireEvent.click(within(dialog).getByRole("button", { name: /Generated images/ }));
  fireEvent.click(within(dialog).getByRole("button", { name: "Enable and allow this task" }));
  await settle();
  expect(pickerApi.computerTaskPickerApprove).toHaveBeenCalledExactlyOnceWith(
    expect.objectContaining({ surface: "app-webview" }), ["host-local-page"],
  );
  expect(taskApi.computerTaskPreview).not.toHaveBeenCalled();
});

it.each(LOCALES)("shows the App-page title, scope and empty state in %s", async locale => {
  await loadLocaleCatalog(locale);
  const tr = createT(locale);
  vi.mocked(pickerApi.computerTaskPickerLoad).mockImplementation(async picker => ({ ...picker, candidates: [], truncated: false }));
  render(<ComputerPanel locale={locale} sessionId="chat" runId={null} />);
  await settle(); await ui.emit(appWebview());
  expect(screen.getByText(tr("cu.consent.appWebviewScope"))).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: tr("cu.consent.selectAppPages") }));
  await settle();
  const dialog = screen.getByRole("dialog", { name: tr("cu.consent.selectAppPages") });
  expect(within(dialog).getByText(tr("cu.consent.appWebviewEmpty"))).toBeInTheDocument();
  expect(within(dialog).getByRole("button", { name: tr("cu.consent.enableAllow") })).toBeDisabled();
});
