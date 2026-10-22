/** @vitest-environment jsdom */
import "@testing-library/jest-dom/vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, expect, test, vi } from "vitest";
import { GlassModal } from "@/components/GlassModal";
import { createT } from "@/i18n";
import { ImportedReaderActions } from "./ImportedReaderActions";

vi.mock("@/lib/nativeWebviewCover", () => ({ acquireNativeWebviewCover: () => () => {} }));
afterEach(cleanup);
const tr = createT("en");
function actions(overrides: Partial<Parameters<typeof ImportedReaderActions>[0]> = {}) {
  return <ImportedReaderActions tr={tr} busy={false} continueBusy={false}
    hasPrevious hasNext hasText hasIssue copyStatus="idle" exportNotice={null} exportError={null}
    onPage={vi.fn()} onCopy={vi.fn()} onIssue={vi.fn()} onExport={vi.fn()} onContinue={vi.fn()}
    onKeyDown={vi.fn()} {...overrides} />;
}

test("real modal portal contains the footer in its keyboard loop outside the body", async () => {
  const onCopy = vi.fn();
  const onKeyDown = vi.fn();
  const onContinue = vi.fn();
  const user = userEvent.setup();
  render(<GlassModal open onClose={vi.fn()} title="Imported history" closeLabel="Close reader" wrapBody
    footer={actions({ onCopy, onKeyDown, onContinue })}><p>Read-only transcript</p></GlassModal>);
  const close = screen.getByRole("button", { name: "Close reader" });
  const copy = screen.getByRole("button", { name: tr("sessionImport.copyVisible") });
  expect(copy.closest(".modal-actions")).not.toBeNull();
  expect(copy.closest(".modal-body")).toBeNull();
  await waitFor(() => expect(close).toHaveFocus());
  await user.tab({ shift: true });
  expect(screen.getByRole("button", { name: tr("sessionImport.continueInGrok") })).toHaveFocus();
  await user.tab();
  expect(close).toHaveFocus();
  copy.focus();
  await user.keyboard("{Enter}");
  expect(onCopy).toHaveBeenCalledOnce();
  expect(onKeyDown).toHaveBeenCalled();
  expect(onContinue).not.toHaveBeenCalled();
});

test("independent busy lanes do not disable continuation when copying", () => {
  const onContinue = vi.fn();
  render(actions({ copyStatus: "pending", busy: true, onContinue }));
  expect(screen.getByRole("button", { name: tr("sessionImport.copyVisible") })).toBeDisabled();
  expect(screen.getByRole("button", { name: tr("sessionImport.exportMarkdown") })).toBeDisabled();
  expect(screen.getByRole("button", { name: tr("sessionImport.loadMore") })).toBeDisabled();
  fireEvent.click(screen.getByRole("button", { name: tr("sessionImport.continueInGrok") }));
  expect(onContinue).toHaveBeenCalledOnce();
});

test("copy errors stay actionable without hiding transcript actions", () => {
  const onCopy = vi.fn();
  render(actions({ copyStatus: "failed", onCopy }));
  expect(screen.getByRole("alert")).toHaveTextContent(tr("sessionImport.clipboardFailed"));
  fireEvent.click(screen.getByRole("button", { name: tr("sessionImport.copyVisible") }));
  expect(onCopy).toHaveBeenCalledOnce();
});

test("pagination and issue actions dispatch their own commands", () => {
  const onPage = vi.fn();
  const onIssue = vi.fn();
  render(actions({ onPage, onIssue }));
  fireEvent.click(screen.getByRole("button", { name: tr("sessionImport.loadPrevious") }));
  fireEvent.click(screen.getByRole("button", { name: tr("sessionImport.loadMore") }));
  fireEvent.click(screen.getByRole("button", { name: tr("sessionImport.firstIssue") }));
  expect(onPage.mock.calls).toEqual([["previous"], ["next"]]);
  expect(onIssue).toHaveBeenCalledOnce();
});
