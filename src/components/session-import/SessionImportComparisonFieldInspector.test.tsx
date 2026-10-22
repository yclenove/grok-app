/** @vitest-environment jsdom */
import "@testing-library/jest-dom/vitest";
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { createT } from "@/i18n";
import type { SourceComparisonFieldRequest, SourceComparisonFieldPage } from "@/lib/api/sessionMigrationComparisonField";
import { comparisonFieldContext, comparisonFieldPage } from "@/test/sessionImportComparisonFieldFixture";
import { comparisonDeferred } from "@/test/sessionImportComparisonFixture";
import { SessionImportComparisonFieldInspector } from "./SessionImportComparisonFieldInspector";
const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@/lib/api/host", () => ({ invoke }));
const context = comparisonFieldContext(), tr = createT("en");
beforeEach(() => { invoke.mockReset().mockImplementation(async (command, { request }: { request: SourceComparisonFieldRequest }) => {
  if (command !== "session_import_source_comparison_field") throw new Error(`Unexpected ${command}`);
  return comparisonFieldPage(context, request.field, Number(request.cursor?.split(":").at(-1) ?? 0));
}); });
afterEach(cleanup);
const props = () => ({ context: structuredClone(context), fields: ["blocks", "summary"], tr, onClose: vi.fn() });
test("functional inspector has labelled source, JSON byte segments, replacement paging and keyboard field selection", async () => {
  const user = userEvent.setup();
  render(<SessionImportComparisonFieldInspector {...props()} />);
  const region = screen.getByRole("region", { name: tr("sessionImport.comparison.inspect") });
  const content = within(region);
  await content.findByLabelText(tr("sessionImport.comparison.jsonSegment"));
  expect(content.getByRole("heading")).toHaveTextContent("Version A");
  expect(content.getByText(tr("sessionImport.comparison.fieldNotice"))).toBeVisible();
  expect(content.getByRole("button", { name: tr("sessionImport.comparison.copySegment") })).toBeEnabled();
  const first = content.getByLabelText(tr("sessionImport.comparison.jsonSegment")).textContent;
  await user.click(content.getByRole("button", { name: tr("sessionImport.nextPage") }));
  await waitFor(() => expect(content.getByLabelText(tr("sessionImport.comparison.jsonSegment")).textContent).not.toBe(first));
  await user.click(content.getByRole("button", { name: tr("sessionImport.previousPage") }));
  await waitFor(() => expect(content.getByLabelText(tr("sessionImport.comparison.jsonSegment"))).toHaveTextContent(first!));
  const field = content.getByRole("button", { name: tr("sessionImport.comparison.fieldLabel") });
  field.focus(); await user.keyboard("{ArrowDown}");
  const summary = await screen.findByRole("option", { name: "summary" });
  await user.click(within(summary).getByRole("button"));
  await content.findByText(tr("sessionImport.comparison.fieldUnavailable"));
  expect(content.queryByLabelText(tr("sessionImport.comparison.jsonSegment"))).not.toBeInTheDocument();
  expect(invoke.mock.calls.every(([name]) => name === "session_import_source_comparison_field")).toBe(true);
});
test("busy failure preserves old segment, copy/retry works and exact attempted cursor is reused", async () => {
  const user = userEvent.setup(), original = invoke.getMockImplementation()!;
  invoke.mockImplementationOnce(original).mockRejectedValueOnce(new Error("PLAN_STALE: test field"));
  render(<SessionImportComparisonFieldInspector {...props()} />);
  await screen.findByLabelText(tr("sessionImport.comparison.jsonSegment"));
  await user.click(screen.getByRole("button", { name: tr("sessionImport.nextPage") }));
  const retry = await screen.findByRole("button", { name: tr("sessionImport.comparison.retryField") });
  expect(screen.getByLabelText(tr("sessionImport.comparison.jsonSegment"))).toBeVisible();
  expect(screen.getByRole("button", { name: tr("sessionImport.copyText") })).toBeEnabled();
  await user.click(retry); await waitFor(() => expect(screen.queryByRole("alert")).not.toBeInTheDocument());
  expect(invoke.mock.calls[1][1].request.cursor).toBe(invoke.mock.calls[2][1].request.cursor);
});
test("IME Enter does not change field; close stays usable during pending read and late error expires", async () => {
  const user = userEvent.setup(), onClose = vi.fn();
  render(<SessionImportComparisonFieldInspector {...props()} onClose={onClose} />);
  await screen.findByLabelText(tr("sessionImport.comparison.jsonSegment"));
  const field = screen.getByRole("button", { name: tr("sessionImport.comparison.fieldLabel") });
  expect(fireEvent.keyDown(field, { key: "Enter", isComposing: true, keyCode: 229 })).toBe(false);
  expect(invoke).toHaveBeenCalledTimes(1);
  const pending = comparisonDeferred<SourceComparisonFieldPage>(); invoke.mockReturnValueOnce(pending.promise);
  await user.click(screen.getByRole("button", { name: tr("sessionImport.nextPage") }));
  await user.click(screen.getByRole("button", { name: tr("sessionImport.comparison.closeFields") }));
  expect(onClose).toHaveBeenCalledOnce(); cleanup();
  await act(async () => { pending.reject(new Error("old")); await Promise.resolve(); });
});

test("field picker stays mounted during a new field read; another choice expires the earlier error without losing focus", async () => {
  const user = userEvent.setup(), original = invoke.getMockImplementation()!;
  const pending = comparisonDeferred<SourceComparisonFieldPage>();
  invoke.mockImplementation((command, args: { request: SourceComparisonFieldRequest }) =>
    args.request.field === "blocks" ? pending.promise : original(command, args));
  render(<SessionImportComparisonFieldInspector {...props()} />);
  await screen.findByLabelText(tr("sessionImport.comparison.jsonSegment"));
  const trigger = screen.getByRole("button", { name: tr("sessionImport.comparison.fieldLabel") });
  await user.click(trigger);
  await user.click(within(await screen.findByRole("option", { name: "blocks" })).getByRole("button"));
  expect(trigger).toBeInTheDocument();
  expect(trigger).toHaveFocus();
  await user.click(trigger);
  await user.click(within(await screen.findByRole("option", { name: "role" })).getByRole("button"));
  await screen.findByLabelText(tr("sessionImport.comparison.jsonSegment"));
  expect(trigger).toHaveTextContent("role");
  await act(async () => { pending.reject(new Error("old blocks error")); await Promise.resolve(); });
  expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  expect(trigger).toHaveFocus();
  expect(invoke).toHaveBeenCalledTimes(3);
});
