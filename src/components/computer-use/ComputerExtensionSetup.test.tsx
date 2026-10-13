// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import "@testing-library/jest-dom/vitest";
import { createT, loadLocaleCatalog, LOCALES } from "@/i18n";
import { ComputerExtensionSetup } from "./ComputerExtensionSetup";
import * as api from "@/lib/api/computerExtension";
vi.mock("@/lib/api/computerExtension", () => ({ computerExtensionInfo: vi.fn(), computerExtensionExport: vi.fn(), computerExtensionOpen: vi.fn() }));
const info = { version: "1.1.0", extensionId: "bgegbabkegkanjbmjbeaockdijnbkjgi", minimumBrowserVersion: "112", sha256: "a".repeat(64), fileCount: 44 };
const exported = { ...info, path: "H:/App data/browser-extension/owned-bundle" };
function deferred<T>() { let resolve!: (value: T) => void; const promise = new Promise<T>(yes => { resolve = yes; }); return { resolve, promise }; }
async function settle() { await act(async () => { await Promise.resolve(); }); }
async function expand() {
  const details = document.querySelector("details")!;
  details.open = true; fireEvent(details, new Event("toggle")); await settle(); return details;
}
beforeEach(() => {
  vi.resetAllMocks();
  vi.mocked(api.computerExtensionInfo).mockResolvedValue(info);
  vi.mocked(api.computerExtensionExport).mockResolvedValue(exported);
  vi.mocked(api.computerExtensionOpen).mockResolvedValue();
});
afterEach(cleanup);
it("mounting reads/writes nothing; expanding reads metadata but does not publish or grant", async () => {
  render(<ComputerExtensionSetup locale="en" />); await settle();
  expect(api.computerExtensionInfo).not.toHaveBeenCalled(); await expand();
  expect(api.computerExtensionInfo).toHaveBeenCalledTimes(1);
  expect(screen.getByText(/Version 1.1.0/)).toBeInTheDocument();
  expect(api.computerExtensionExport).not.toHaveBeenCalled(); expect(api.computerExtensionOpen).not.toHaveBeenCalled();
});
it("an explicit export focuses its complete path, opens only on a separate gesture and copies the full path", async () => {
  const writeText = vi.fn().mockResolvedValue(undefined);
  Object.defineProperty(navigator, "clipboard", { configurable: true, value: { writeText } });
  render(<ComputerExtensionSetup locale="en" />); await expand();
  const get = screen.getByRole("button", { name: "Get bundled extension" }); get.focus(); fireEvent.click(get); await settle();
  expect(api.computerExtensionExport).toHaveBeenCalledTimes(1); expect(api.computerExtensionOpen).not.toHaveBeenCalled();
  expect(screen.getByLabelText("Extension folder")).toHaveValue(exported.path);
  expect(screen.getByLabelText("Extension folder")).toHaveFocus();
  fireEvent.click(screen.getByRole("button", { name: "Open folder" })); await settle(); expect(api.computerExtensionOpen).toHaveBeenCalledExactlyOnceWith();
  fireEvent.click(screen.getByRole("button", { name: "Copy folder path" })); await settle(); expect(writeText).toHaveBeenCalledExactlyOnceWith(exported.path);
  expect(screen.getByText("Copied")).toBeInTheDocument();
});
it("a failed export has a real explicit retry; a failed open retains manual path access", async () => {
  vi.mocked(api.computerExtensionExport).mockRejectedValueOnce(new Error("disk full"));
  vi.mocked(api.computerExtensionOpen).mockRejectedValueOnce(new Error("no OS handler"));
  render(<ComputerExtensionSetup locale="en" />); await expand();
  fireEvent.click(screen.getByRole("button", { name: "Get bundled extension" })); await settle();
  expect(screen.getByRole("alert")).toHaveTextContent("Existing files were not replaced");
  fireEvent.click(screen.getByRole("button", { name: "Get bundled extension" })); await settle();
  expect(api.computerExtensionExport).toHaveBeenCalledTimes(2);
  fireEvent.click(screen.getByRole("button", { name: "Open folder" })); await settle();
  expect(screen.getByRole("alert")).toHaveTextContent("open it manually"); expect(screen.getByLabelText("Extension folder")).toHaveValue(exported.path);
});
it.each(["collapse", "unmount"] as const)("%s fences a late export result and duplicate gestures never publish twice", async action => {
  const reply = deferred<api.ComputerExtensionExport>(); vi.mocked(api.computerExtensionExport).mockReturnValueOnce(reply.promise);
  const rendered = render(<ComputerExtensionSetup locale="en" />); const details = await expand();
  const get = screen.getByRole("button", { name: "Get bundled extension" });
  fireEvent.click(get); fireEvent.click(get); expect(api.computerExtensionExport).toHaveBeenCalledTimes(1);
  if (action === "collapse") { details.open = false; fireEvent(details, new Event("toggle")); }
  else rendered.unmount();
  await act(async () => reply.resolve(exported));
  expect(screen.queryByLabelText("Extension folder")).toBeNull(); expect(api.computerExtensionOpen).not.toHaveBeenCalled();
});
it("metadata errors can be explicitly retried, and a changed bundle cannot produce a path", async () => {
  vi.mocked(api.computerExtensionInfo).mockRejectedValueOnce(new Error("missing bundle"));
  render(<ComputerExtensionSetup locale="en" />); await expand();
  expect(screen.getByRole("alert")).toHaveTextContent("Repair or reinstall");
  fireEvent.click(screen.getByRole("button", { name: "Check again" })); await settle();
  vi.mocked(api.computerExtensionExport).mockResolvedValueOnce({ ...exported, sha256: "b".repeat(64) });
  fireEvent.click(screen.getByRole("button", { name: "Get bundled extension" })); await settle();
  expect(screen.queryByLabelText("Extension folder")).toBeNull(); expect(screen.getByRole("alert")).toBeInTheDocument();
});
it.each(LOCALES)("shows the actual bundle version/ID and manual steps in %s", async locale => {
  await loadLocaleCatalog(locale); const tr = createT(locale);
  render(<ComputerExtensionSetup locale={locale} />); await expand();
  fireEvent.click(screen.getByRole("button", { name: tr("cu.extensionSetup.get") })); await settle();
  expect(screen.getByText(tr("cu.panel.pairingIdentity", { id: info.extensionId }))).toBeInTheDocument();
  expect(screen.getByText(tr("cu.extensionSetup.stepLoad"))).toBeInTheDocument();
  expect(screen.getByLabelText(tr("cu.extensionSetup.path"))).toHaveValue(exported.path);
});
