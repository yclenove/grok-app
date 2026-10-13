// @vitest-environment jsdom
// Real React/store flows with synthetic Tauri replies; native evidence is separate.
import { act, cleanup, fireEvent, render, renderHook, screen } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import "@testing-library/jest-dom/vitest";
import { createT, loadLocaleCatalog, LOCALES } from "@/i18n";
import * as api from "@/lib/api";
import { embeddedBrowserLifecycle as life } from "@/lib/embeddedBrowserLifecycle";
import { nativeWebviewCoverDepth } from "@/lib/nativeWebviewCover";
import { useEmbeddedBrowserRecovery } from "@/hooks/useEmbeddedBrowserRecovery";
import { EmbeddedBrowserCloseNotice } from "./EmbeddedBrowserCloseNotice";
import { EmbeddedBrowser } from "./EmbeddedBrowser";

vi.mock("@/lib/api", () => ({
  isTauri: () => true,
  sideBrowserClose: vi.fn(), sideBrowserCreate: vi.fn(), sideBrowserEval: vi.fn(),
  sideBrowserInstallDownloadHook: vi.fn(), sideBrowserNavigate: vi.fn(), sideBrowserReload: vi.fn(),
}));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => undefined) }));
vi.mock("@tauri-apps/api/window", () => ({ getCurrentWindow: () => ({ label: "main" }) }));
vi.mock("@tauri-apps/api/dpi", () => ({
  LogicalPosition: class { constructor(public x: number, public y: number) {} },
  LogicalSize: class { constructor(public width: number, public height: number) {} },
}));
const native = vi.hoisted(() => ({
  setPosition: vi.fn(async (): Promise<void> => undefined), setSize: vi.fn(async (): Promise<void> => undefined),
  show: vi.fn(async (): Promise<void> => undefined), hide: vi.fn(async (): Promise<void> => undefined),
}));
vi.mock("@tauri-apps/api/webview", () => ({ Webview: { getByLabel: vi.fn(async () => native) } }));

function deferred() {
  let resolve!: () => void;
  let reject!: (error: Error) => void;
  const promise = new Promise<void>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}
let nextId = 0;
const labels = new Set<string>();
function label() { const value = `resource-browser-recovery-${++nextId}`; labels.add(value); return value; }
const settle = async () => {
  await act(async () => { await vi.advanceTimersByTimeAsync(0); });
};
async function failPage(value: string, error = "original native download still pending") {
  const lease = life.retain(value, "Owned download page");
  lease.release();
  vi.mocked(api.sideBrowserClose).mockRejectedValueOnce(new Error(error));
  await act(async () => { await life.retryClose(value).catch(() => undefined); });
}

beforeEach(async () => {
  vi.useFakeTimers(); vi.clearAllMocks();
  vi.mocked(api.sideBrowserClose).mockResolvedValue();
  vi.mocked(api.sideBrowserCreate).mockResolvedValue();
  vi.mocked(api.sideBrowserEval).mockResolvedValue('"complete"');
  vi.mocked(api.sideBrowserInstallDownloadHook).mockResolvedValue();
  await Promise.all([
    import("@tauri-apps/api/webview"), import("@tauri-apps/api/window"), import("@tauri-apps/api/dpi"),
  ]);
});
afterEach(async () => {
  cleanup();
  vi.mocked(api.sideBrowserClose).mockResolvedValue();
  await vi.advanceTimersByTimeAsync(150);
  for (const value of labels) await life.retryClose(value);
  labels.clear();
  expect(nativeWebviewCoverDepth()).toBe(0);
  vi.useRealTimers();
});

it("shows the failed close after its tab is gone and retries only the retained label", async () => {
  const value = label();
  await failPage(value);
  const original = deferred();
  vi.mocked(api.sideBrowserClose).mockImplementationOnce(() => original.promise);
  render(<EmbeddedBrowserCloseNotice locale="en" />);
  expect(screen.getByRole("dialog", { name: "Browser" })).toBeInTheDocument();
  expect(screen.getByRole("status")).toHaveTextContent("This page could not be closed.");
  expect(screen.getByText("Owned download page")).toBeInTheDocument();
  expect(screen.getByText("The page is still retained. Retry closing before reopening it.")).toBeInTheDocument();
  expect(nativeWebviewCoverDepth()).toBe(1);
  fireEvent.click(screen.getByRole("button", { name: "Retry closing" }));
  await settle();
  const busy = screen.getByRole("button", { name: "Closing page…" });
  expect(busy).toBeDisabled();
  fireEvent.click(busy);
  expect(api.sideBrowserClose).toHaveBeenCalledTimes(2);
  expect(api.sideBrowserClose).toHaveBeenLastCalledWith(value);
  expect(screen.getByRole("dialog")).toBeInTheDocument();
  await act(async () => { original.resolve(); await original.promise; });
  await settle();
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  expect(nativeWebviewCoverDepth()).toBe(0);
});

it("deferring the dialog restores other native pages without dropping the retained owner", async () => {
  const value = label();
  await failPage(value);
  render(<EmbeddedBrowserCloseNotice locale="en" />);
  fireEvent.click(screen.getByRole("button", { name: "Later" }));
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  expect(life.getState(value).phase).toBe("failed");
  expect(api.sideBrowserClose).toHaveBeenCalledTimes(1);
  expect(nativeWebviewCoverDepth()).toBe(0);
});

it("a failed explicit retry stays visible with an enabled retry and the new diagnostic", async () => {
  const value = label();
  await failPage(value);
  render(<EmbeddedBrowserCloseNotice locale="en" />);
  vi.mocked(api.sideBrowserClose).mockRejectedValueOnce(new Error("original callback still missing"));
  fireEvent.click(screen.getByRole("button", { name: "Retry closing" }));
  await settle();
  expect(screen.getByRole("button", { name: "Retry closing" })).toBeEnabled();
  expect(screen.getByText("original callback still missing")).toBeInTheDocument();
  expect(life.getState(value).phase).toBe("failed");
});

it("a failed same-label remount rejects create until retry physically closes the old view", async () => {
  const value = label();
  await failPage(value);
  const original = deferred();
  render(<EmbeddedBrowser url="http://127.0.0.1:8123/page" instanceId={value.replace('resource-browser-', '')} />);
  await settle();
  expect(screen.getByRole("alert")).toHaveTextContent("This page could not be closed.");
  expect(screen.getByText("original native download still pending").closest("details")).not.toHaveAttribute("open");
  expect(api.sideBrowserCreate).not.toHaveBeenCalled();
  vi.mocked(api.sideBrowserClose).mockImplementationOnce(() => original.promise);
  fireEvent.click(screen.getByRole("button", { name: "Retry closing" }));
  await settle();
  expect(screen.getByRole("button", { name: "Closing page…" })).toBeDisabled();
  expect(api.sideBrowserCreate).not.toHaveBeenCalled();
  await act(async () => { original.resolve(); await original.promise; });
  await settle();
  expect(api.sideBrowserCreate).toHaveBeenCalledExactlyOnceWith(expect.objectContaining({ label: value }));
  expect(screen.queryByRole("alert")).not.toBeInTheDocument();
});

it("a stale retry reply cannot reboot a later mount that reused the same label", async () => {
  const a = label(), b = label();
  const original = deferred();
  const hook = renderHook(({ value }) => useEmbeddedBrowserRecovery(value), { initialProps: { value: a } });
  vi.mocked(api.sideBrowserClose).mockImplementationOnce(() => original.promise);
  act(() => hook.result.current.retryClose());
  await settle();
  hook.rerender({ value: b }); hook.rerender({ value: a });
  await act(async () => { original.resolve(); await original.promise; });
  await settle();
  expect(hook.result.current.bootRevision).toBe(0);
  act(() => hook.result.current.retryClose()); await settle();
  expect(hook.result.current.bootRevision).toBe(1);
});

it.each(["setPosition", "setSize"] as const)("a late boot %s reply cannot repaint or initialize an unmounted page", async method => {
  const value = label();
  const original = deferred();
  native[method].mockImplementationOnce(() => original.promise);
  const view = render(<EmbeddedBrowser url="http://127.0.0.1:8123/page" instanceId={value.replace('resource-browser-', '')} />);
  await settle();
  expect(native[method]).toHaveBeenCalledTimes(1);
  view.unmount();
  await act(async () => { await vi.advanceTimersByTimeAsync(150); });
  expect(api.sideBrowserClose).toHaveBeenCalledExactlyOnceWith(value);
  await act(async () => { original.resolve(); await original.promise; });
  await settle();
  if (method === "setPosition") expect(native.setSize).not.toHaveBeenCalled();
  expect(native.show).not.toHaveBeenCalled();
  expect(native.hide).not.toHaveBeenCalled();
  expect(api.sideBrowserInstallDownloadHook).not.toHaveBeenCalled();
  expect(api.sideBrowserEval).not.toHaveBeenCalled();
  expect(api.sideBrowserNavigate).not.toHaveBeenCalled();
});

it.each(LOCALES)("renders actionable close recovery in %s", async locale => {
  await loadLocaleCatalog(locale);
  const value = label();
  await failPage(value);
  const tr = createT(locale);
  render(<EmbeddedBrowserCloseNotice locale={locale} />);
  expect(screen.getByRole("dialog", { name: tr("side.tab.browser") })).toBeInTheDocument();
  expect(screen.getByRole("status")).toHaveTextContent(tr("resources.browserCloseFailed"));
  expect(screen.getByRole("button", { name: tr("resources.browserCloseRetry") })).toBeEnabled();
  expect(screen.getByRole("button", { name: tr("resources.browserCloseLater") })).toBeEnabled();
});
