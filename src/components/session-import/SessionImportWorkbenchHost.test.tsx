/** @vitest-environment jsdom */
import "@/test/jsdomStubs";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeAll, expect, test, vi } from "vitest";
import { createT, LOCALES, loadAllLocaleCatalogs } from "@/i18n";
import { nativeWebviewCoverDepth } from "@/lib/nativeWebviewCover";
import { SessionImportWorkbenchHost } from "./SessionImportWorkbenchHost";

const first = "97e20f19-42f4-47c1-b5e2-8e36dc7699af";
const second = "ef77d9f6-f70e-43ac-a982-4a5b6ee0d075";
vi.mock("./SessionImportHost", () => ({ SessionImportHost: ({ onOpenNativeSession }: {
  onOpenNativeSession?: (id: string) => void;
}) => <>
  <button onClick={() => onOpenNativeSession?.(first)}>Open first fixture</button>
  <button onClick={() => onOpenNativeSession?.(second)}>Open second fixture</button>
</> }));
function deferred() {
  let resolve!: () => void;
  let reject!: (cause: unknown) => void;
  const promise = new Promise<void>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}
beforeAll(loadAllLocaleCatalogs);
afterEach(() => { cleanup(); vi.restoreAllMocks(); });

test.each(LOCALES)("%s displays a translated workspace failure and retries the same durable chat", async (locale) => {
  const tr = createT(locale);
  const retry = deferred();
  const open = vi.fn().mockRejectedValueOnce(new Error("WORKSPACE_UNRESOLVED: internal path"))
    .mockReturnValueOnce(retry.promise);
  render(<SessionImportWorkbenchHost locale={locale} onOpenSession={open} />);
  fireEvent.click(screen.getByRole("button", { name: "Open first fixture" }));
  const alert = await screen.findByRole("alert");
  expect(alert.textContent).toBe(tr("sessionImport.err.workspaceUnresolved"));
  expect(alert.textContent).not.toContain("internal path");
  fireEvent.click(screen.getByRole("button", { name: tr("sessionImport.openDraft") }));
  expect(open.mock.calls).toEqual([[first], [first]]);
  expect(screen.getByRole("status").textContent).toBe(tr("sessionImport.workspace.busy"));
  expect(screen.getByRole("button", { name: tr("sessionImport.openDraft") }).hasAttribute("disabled")).toBe(true);
  fireEvent.keyDown(document, { key: "Escape" });
  fireEvent.mouseDown(screen.getByRole("presentation"));
  expect(screen.getByRole("dialog")).toBeTruthy();
  await act(async () => { retry.resolve(); await retry.promise; });
  expect(screen.queryByRole("dialog")).toBeNull();
  expect(nativeWebviewCoverDepth()).toBe(0);
});

test("Chinese missing-chat copy distinguishes deletion from an unknown failure", async () => {
  const tr = createT("zh");
  render(<SessionImportWorkbenchHost locale="zh" onOpenSession={vi.fn().mockRejectedValue(new Error("SESSION_OPEN_MISSING"))} />);
  fireEvent.click(screen.getByRole("button", { name: "Open first fixture" }));
  expect((await screen.findByRole("alert")).textContent).toBe(tr("session.openInNewWindowMissing"));
  fireEvent.click(screen.getByRole("button", { name: tr("common.close") }));
  expect(screen.queryByRole("dialog")).toBeNull();
  expect(nativeWebviewCoverDepth()).toBe(0);
});

test("unknown errors show product copy without exposing transport details", async () => {
  render(<SessionImportWorkbenchHost locale="en" onOpenSession={vi.fn().mockRejectedValue(new Error("transport private details"))} />);
  fireEvent.click(screen.getByRole("button", { name: "Open first fixture" }));
  expect((await screen.findByRole("alert")).textContent).toBe(createT("en")("sessionImport.err.generic"));
});

test("an older open failure cannot replace a newer successful target", async () => {
  const old = deferred();
  const open = vi.fn().mockReturnValueOnce(old.promise).mockResolvedValueOnce(undefined);
  render(<SessionImportWorkbenchHost locale="en" onOpenSession={open} />);
  fireEvent.click(screen.getByRole("button", { name: "Open first fixture" }));
  await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Open second fixture" })); });
  await act(async () => { old.reject(new Error("old failure")); await old.promise.catch(() => {}); });
  expect(open.mock.calls).toEqual([[first], [second]]);
  expect(screen.queryByRole("dialog")).toBeNull();
});

test("an older successful reply cannot dismiss a newer retryable failure", async () => {
  const old = deferred();
  const open = vi.fn().mockReturnValueOnce(old.promise).mockRejectedValueOnce(new Error("WORKSPACE_UNRESOLVED"));
  render(<SessionImportWorkbenchHost locale="en" onOpenSession={open} />);
  fireEvent.click(screen.getByRole("button", { name: "Open first fixture" }));
  fireEvent.click(screen.getByRole("button", { name: "Open second fixture" }));
  await screen.findByRole("alert");
  await act(async () => { old.resolve(); await old.promise; });
  expect(screen.getByRole("alert").textContent).toBe(createT("en")("sessionImport.err.workspaceUnresolved"));
});

test("unmounting prevents a pending open failure from adding a modal", async () => {
  const pending = deferred();
  const view = render(<SessionImportWorkbenchHost locale="en" onOpenSession={() => pending.promise} />);
  fireEvent.click(screen.getByRole("button", { name: "Open first fixture" }));
  view.unmount();
  await act(async () => { pending.reject(new Error("late failure")); await pending.promise.catch(() => {}); });
  expect(screen.queryByRole("dialog")).toBeNull();
  expect(nativeWebviewCoverDepth()).toBe(0);
});
