/** @vitest-environment jsdom */
import "@testing-library/jest-dom/vitest";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { createT } from "@/i18n";
import type { SourceCandidate } from "@/lib/api/sessionMigration";
import { ImportCopyButton } from "./ImportCopyButton";
import { SessionImportCandidateRow } from "./SessionImportCandidateRow";

const tr = createT("en");
const writeText = vi.fn();
const clipboardDescriptor = Object.getOwnPropertyDescriptor(navigator, "clipboard");
const candidate: SourceCandidate = {
  candidateId: "synthetic-one", provider: "generic-file", nativeSessionId: "one",
  title: "Synthetic import", locator: "C:/synthetic/one.json", sourceRevision: "one",
  completeness: "body",
};
function row(overrides: Partial<Parameters<typeof SessionImportCandidateRow>[0]> = {}) {
  return <ul><SessionImportCandidateRow candidate={candidate} selected={false} focused
    peekBusy={false} tr={tr} onFocus={vi.fn()} onToggle={vi.fn()} onPeek={vi.fn()} {...overrides} /></ul>;
}
function deferred() {
  let resolve!: () => void;
  let reject!: (reason: Error) => void;
  const promise = new Promise<void>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}
function installClipboard() {
  Object.defineProperty(navigator, "clipboard", { configurable: true, value: { writeText } });
}
beforeEach(() => { writeText.mockReset().mockResolvedValue(undefined); installClipboard(); });
afterEach(() => {
  cleanup();
  if (clipboardDescriptor) Object.defineProperty(navigator, "clipboard", clipboardDescriptor);
  else Reflect.deleteProperty(navigator, "clipboard");
});

test("source copy waits for acknowledgement, prevents duplicates and reports success", async () => {
  const pending = deferred();
  writeText.mockReturnValue(pending.promise);
  render(row());
  const copy = screen.getByRole("button", { name: tr("sessionImport.copySource") });
  fireEvent.click(copy);
  expect(copy).toBeDisabled();
  fireEvent.click(copy);
  expect(writeText).toHaveBeenCalledExactlyOnceWith(candidate.locator);
  expect(screen.queryByRole("status")).not.toBeInTheDocument();
  await act(async () => { pending.resolve(); await pending.promise; });
  expect(screen.getByRole("status")).toHaveTextContent(tr("sessionImport.clipboardCopied"));
  expect(copy).toBeEnabled();
});

test("denied source copy shows a recoverable error and retry replaces the notice", async () => {
  writeText.mockRejectedValueOnce(new Error("synthetic clipboard denied"));
  render(row());
  const copy = screen.getByRole("button", { name: tr("sessionImport.copySource") });
  await act(async () => { fireEvent.click(copy); });
  expect(screen.getByRole("alert")).toHaveTextContent(tr("sessionImport.clipboardFailed"));
  expect(copy).toBeEnabled();
  await act(async () => { fireEvent.click(copy); });
  expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  expect(screen.getByRole("status")).toHaveTextContent(tr("sessionImport.clipboardCopied"));
  expect(writeText).toHaveBeenCalledTimes(2);
});

test("missing clipboard support is reported without hiding the candidate or its preview", async () => {
  Object.defineProperty(navigator, "clipboard", { configurable: true, value: undefined });
  render(row());
  await act(async () => { fireEvent.click(screen.getByRole("button", { name: tr("sessionImport.copySource") })); });
  expect(screen.getByRole("alert")).toHaveTextContent(tr("sessionImport.clipboardFailed"));
  expect(screen.getByRole("button", { name: tr("sessionImport.preview") })).toBeEnabled();
  expect(screen.getByText(candidate.title)).toBeInTheDocument();
});

test.each(["{Enter}", " "])("source copy is keyboard operable with %s without selecting or previewing", async (key) => {
  const user = userEvent.setup();
  installClipboard();
  const onToggle = vi.fn();
  const onPeek = vi.fn();
  render(row({ onToggle, onPeek }));
  const copy = screen.getByRole("button", { name: tr("sessionImport.copySource") });
  copy.focus();
  await user.keyboard(key);
  expect(writeText).toHaveBeenCalledExactlyOnceWith(candidate.locator);
  expect(screen.getByRole("status")).toHaveTextContent(tr("sessionImport.clipboardCopied"));
  expect(onToggle).not.toHaveBeenCalled();
  expect(onPeek).not.toHaveBeenCalled();
});

test("late copy completion cannot mark a different source as copied or unlock its pending copy", async () => {
  const old = deferred();
  const next = deferred();
  writeText.mockReturnValueOnce(old.promise).mockReturnValueOnce(next.promise);
  const { rerender } = render(row());
  fireEvent.click(screen.getByRole("button", { name: tr("sessionImport.copySource") }));
  const second = { ...candidate, candidateId: "synthetic-two", locator: "C:/synthetic/two.json" };
  rerender(row({ candidate: second }));
  const copy = screen.getByRole("button", { name: tr("sessionImport.copySource") });
  expect(copy).toBeEnabled();
  fireEvent.click(copy);
  await act(async () => { old.resolve(); await old.promise; });
  expect(copy).toBeDisabled();
  expect(screen.queryByRole("status")).not.toBeInTheDocument();
  await act(async () => { next.resolve(); await next.promise; });
  expect(screen.getByRole("status")).toHaveTextContent(tr("sessionImport.clipboardCopied"));
  expect(writeText.mock.calls).toEqual([[candidate.locator], [second.locator]]);
});

test("an unmounted source copy rejection is handled and cannot contaminate a remounted row", async () => {
  const pending = deferred();
  writeText.mockReturnValueOnce(pending.promise);
  const first = render(row());
  fireEvent.click(screen.getByRole("button", { name: tr("sessionImport.copySource") }));
  first.unmount();
  render(row());
  await act(async () => { pending.reject(new Error("synthetic late denial")); await pending.promise.catch(() => {}); });
  expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  expect(screen.queryByRole("status")).not.toBeInTheDocument();
  expect(screen.getByRole("button", { name: tr("sessionImport.copySource") })).toBeEnabled();
});

test("candidate without a locator does not render an unusable copy action", () => {
  render(row({ candidate: { ...candidate, locator: "" } }));
  expect(screen.queryByRole("button", { name: tr("sessionImport.copySource") })).not.toBeInTheDocument();
});

test("read-only source copy remains independent of selection and preview busy gates", async () => {
  render(row({ disabled: true, peekBusy: true }));
  expect(screen.getByRole("checkbox")).toBeDisabled();
  expect(screen.getByRole("button", { name: tr("sessionImport.preview") })).toBeDisabled();
  const copy = screen.getByRole("button", { name: tr("sessionImport.copySource") });
  expect(copy).toBeEnabled();
  await act(async () => { fireEvent.click(copy); });
  expect(screen.getByRole("status")).toHaveTextContent(tr("sessionImport.clipboardCopied"));
});

test("shared text-copy callers retain their original accessible label", async () => {
  render(<ImportCopyButton text="Synthetic transcript" tr={tr} />);
  await act(async () => { fireEvent.click(screen.getByRole("button", { name: tr("sessionImport.copyText") })); });
  expect(writeText).toHaveBeenCalledExactlyOnceWith("Synthetic transcript");
  expect(screen.getByRole("status")).toHaveTextContent(tr("sessionImport.clipboardCopied"));
});
