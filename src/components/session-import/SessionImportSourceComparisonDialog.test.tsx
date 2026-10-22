/** @vitest-environment jsdom */
import "@testing-library/jest-dom/vitest";
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { createT } from "@/i18n";
import type { SourceComparisonRequest } from "@/lib/api/sessionMigrationComparison";
import type { SourceComparisonFieldRequest } from "@/lib/api/sessionMigrationComparisonField";
import { comparisonFieldPage } from "@/test/sessionImportComparisonFieldFixture";
import { comparisonDeferred, comparisonPage } from "@/test/sessionImportComparisonFixture";
import { previewComparisonContext, previewComparisonMember, previewComparisonResponse } from "@/test/sessionImportComparisonPreviewFixture";
import { SessionImportSourceComparisonDialog } from "./SessionImportSourceComparisonDialog";

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@/lib/api/host", () => ({ invoke }));
vi.mock("@/lib/nativeWebviewCover", () => ({ acquireNativeWebviewCover: () => () => {} }));
const context = previewComparisonContext();
const tr = createT("en");
function membersPage() {
  return { version: 1, groupId: context.group.groupId, memberCount: 2, complete: true, stateVersion: 1, nextCursor: null,
    members: [previewComparisonMember("a"), { ...previewComparisonMember("b"), format: "cursor-composer" }] };
}
function pairedPage(request: SourceComparisonRequest) {
  const page = comparisonPage(request, request.cursor ? 2 : 0);
  page.left.groupId = context.group.groupId;
  page.right.groupId = context.group.groupId;
  return page;
}
beforeEach(() => {
  invoke.mockReset().mockImplementation(async (command: string, args: Record<string, unknown>) => {
    if (command === "session_import_preview") {
      const id = String(args.candidateId).split(":").at(-1)!;
      const response = previewComparisonResponse(id);
      if (id === "a") response.adapter.eventCount = 3;
      return response;
    }
    if (command === "session_import_scan_group_members") return membersPage();
    if (command === "session_import_scan_group_member") return previewComparisonMember("a");
    if (command === "session_import_source_comparison") return pairedPage(args.request as SourceComparisonRequest);
    throw new Error(`Unexpected synthetic IPC: ${command}`);
  });
});
afterEach(cleanup);
const calls = (command: string) => invoke.mock.calls.filter(([name]) => name === command);

test("A metadata comes from its exact fixed member even when the lazy B page omits A", async () => {
  const original = invoke.getMockImplementation()!;
  const exact = { ...previewComparisonMember("a"), rootPath: "/synthetic/exact-primary-root", format: "cursor-composer" };
  invoke.mockImplementation(async (command: string, args: Record<string, unknown>) => {
    if (command === "session_import_scan_group_member") return exact;
    if (command === "session_import_scan_group_members") return { ...membersPage(), members: [previewComparisonMember("b")] };
    return original(command, args);
  });
  render(<SessionImportSourceComparisonDialog context={context} tr={tr} onClose={vi.fn()} />);
  const card = screen.getByRole("region", { name: tr("sessionImport.comparison.sideA") });
  await within(card).findByText(exact.rootPath);
  expect(calls("session_import_scan_group_member")).toHaveLength(1);
  expect(calls("session_import_scan_group_member")[0][1]).toEqual({ scanId: context.scope.scanId,
    epoch: context.scope.epoch, groupId: context.group.groupId, candidateId: context.primary.candidateId });
  expect(calls("session_import_scan_group_members")).toHaveLength(1);
  expect(calls("session_import_preview")).toHaveLength(1);
});

test("A metadata errors never fall back to the lazy page and retry never rereads the fixed body", async () => {
  const user = userEvent.setup(), original = invoke.getMockImplementation()!;
  let fail = true;
  invoke.mockImplementation(async (command: string, args: Record<string, unknown>) => {
    if (command === "session_import_scan_group_member" && fail) {
      fail = false; throw new Error("SOURCE_NOT_FOUND: exact primary missing");
    }
    return original(command, args);
  });
  render(<SessionImportSourceComparisonDialog context={context} tr={tr} onClose={vi.fn()} />);
  const card = screen.getByRole("region", { name: tr("sessionImport.comparison.sideA") });
  const retry = await within(card).findByRole("button", { name: tr("sessionImport.versions.retry") });
  expect(within(card).queryByText(previewComparisonMember("a").rootPath)).not.toBeInTheDocument();
  await user.click(retry);
  await within(card).findByText(previewComparisonMember("a").rootPath);
  expect(calls("session_import_scan_group_member")).toHaveLength(2);
  expect(calls("session_import_scan_group_members")).toHaveLength(1);
  expect(calls("session_import_preview")).toHaveLength(1);
});

test("field entry reads the exact displayed A event only on click; close restores its trigger and row paging unmounts the reader", async () => {
  const user = userEvent.setup(), original = invoke.getMockImplementation()!;
  invoke.mockImplementation(async (command: string, args: Record<string, unknown>) => {
    if (command === "session_import_source_comparison_field") {
      const r = args.request as SourceComparisonFieldRequest;
      return comparisonFieldPage({ owner: { scope: context.scope, groupId: context.group.groupId,
        left: { ...r.left }, right: { ...r.right } }, side: r.side, position: r.position,
        eventId: `${r[r.side].previewId}:${r[r.side].leafId}:${r.position}`, fieldsDigest: r.expectedFieldsDigest }, r.field);
    }
    return original(command, args);
  });
  render(<SessionImportSourceComparisonDialog context={context} tr={tr} onClose={vi.fn()} />);
  await user.click(await screen.findByRole("button", { name: /Composer · b/ }));
  const row = (await screen.findByText(tr("sessionImport.comparison.position", { n: 1 }))).closest("li")!;
  const a = within(row).getByRole("region", { name: tr("sessionImport.comparison.sideA") });
  const trigger = within(a).getByRole("button", { name: tr("sessionImport.comparison.inspect") });
  expect(calls("session_import_source_comparison_field")).toHaveLength(0);
  await user.click(trigger);
  await screen.findByLabelText(tr("sessionImport.comparison.jsonSegment"));
  expect(calls("session_import_source_comparison_field")[0][1].request).toMatchObject({ side: "left", position: 0,
    field: "kind", expectedFieldsDigest: "0".repeat(64), limitBytes: 4096, left: { candidateId: context.primary.candidateId } });
  await user.click(screen.getByRole("button", { name: tr("sessionImport.comparison.closeFields") }));
  expect(trigger).toHaveFocus();
  await user.click(trigger); await screen.findByLabelText(tr("sessionImport.comparison.jsonSegment"));
  const footer = screen.getByRole("dialog").querySelector(".modal-actions") as HTMLElement;
  await user.click(within(footer).getByRole("button", { name: tr("sessionImport.nextPage") }));
  await waitFor(() => expect(screen.queryByLabelText(tr("sessionImport.comparison.jsonSegment"))).not.toBeInTheDocument());
  expect(calls("session_import_preview")).toHaveLength(2);
});

test("real hooks lazily load A and metadata, then B and comparison without any selection or provider mutation", async () => {
  const user = userEvent.setup();
  const onClose = vi.fn();
  render(<SessionImportSourceComparisonDialog context={context} tr={tr} onClose={onClose} />);
  const dialog = await screen.findByRole("dialog", { name: tr("sessionImport.comparison.title") });
  const b = await within(dialog).findByRole("button", { name: /Composer · b/ });
  await waitFor(() => expect(calls("session_import_preview")).toHaveLength(1));
  expect(calls("session_import_source_comparison")).toHaveLength(0);
  expect(calls("session_import_preview")[0][1]).toEqual({ candidateId: context.primary.candidateId, expectedSourceRevision: context.primary.sourceRevision });
  await user.click(b);
  await within(dialog).findByText(tr("sessionImport.comparison.position", { n: 1 }));
  const firstRow = within(dialog).getByText(tr("sessionImport.comparison.position", { n: 1 })).closest("li")!;
  expect(within(firstRow).getByRole("region", { name: tr("sessionImport.comparison.sideA") })).toBeVisible();
  expect(within(firstRow).getByRole("region", { name: tr("sessionImport.comparison.sideB") })).toBeVisible();
  expect(calls("session_import_preview")).toHaveLength(2);
  expect(calls("session_import_source_comparison")).toHaveLength(1);
  expect(calls("session_import_source_comparison")[0][1].request).toMatchObject({
    version: 1, limit: 50, left: { candidateId: context.primary.candidateId, sourceRevision: "revision-a", leafId: "leaf-a" },
    right: { candidateId: previewComparisonMember().candidate.candidateId, sourceRevision: "revision-b", leafId: "leaf-b" },
  });
  expect(invoke.mock.calls.every(([command]) => ["session_import_preview", "session_import_scan_group_member", "session_import_scan_group_members", "session_import_source_comparison"].includes(command))).toBe(true);
  expect(within(dialog).getByText(tr("sessionImport.comparison.notice"))).toBeVisible();
  const footer = dialog.querySelector(".modal-actions")!;
  await user.click(within(footer as HTMLElement).getByRole("button", { name: tr("sessionImport.nextPage") }));
  await within(dialog).findByText(tr("sessionImport.comparison.position", { n: 3 }));
  expect(within(dialog).queryByText(tr("sessionImport.comparison.position", { n: 1 }))).not.toBeInTheDocument();
  expect(within(dialog).getByText(tr("sessionImport.comparison.missing"))).toBeVisible();
  await user.click(within(footer as HTMLElement).getByRole("button", { name: tr("sessionImport.previousPage") }));
  await within(dialog).findByText(tr("sessionImport.comparison.position", { n: 1 }));
  expect(calls("session_import_source_comparison")[2][1].request.cursor).toBeUndefined();
});

test("IME confirmation cannot select B; Escape closes via the shared modal", async () => {
  const user = userEvent.setup();
  const onClose = vi.fn();
  render(<SessionImportSourceComparisonDialog context={context} tr={tr} onClose={onClose} />);
  const b = await screen.findByRole("button", { name: /Composer · b/ });
  const cancelled = fireEvent.keyDown(b, { key: "Enter", isComposing: true, keyCode: 229 });
  expect(cancelled).toBe(false);
  expect(calls("session_import_preview")).toHaveLength(1);
  expect(calls("session_import_source_comparison")).toHaveLength(0);
  await user.keyboard("{Escape}");
  expect(onClose).toHaveBeenCalledTimes(1);
});

test("metadata-only parent refresh does not reopen or reread fixed bodies", async () => {
  const props = { context: structuredClone(context), tr, onClose: vi.fn() };
  const { rerender } = render(<SessionImportSourceComparisonDialog {...props} />);
  await screen.findByRole("button", { name: /Composer · b/ });
  await waitFor(() => expect(calls("session_import_preview")).toHaveLength(1));
  const before = calls("session_import_scan_group_members").length;
  rerender(<SessionImportSourceComparisonDialog {...props} context={{ ...props.context,
    primary: { ...props.context.primary, title: "display-only" }, group: { ...props.context.group, memberCount: 3 } }} />);
  await act(async () => { await Promise.resolve(); });
  expect(calls("session_import_preview")).toHaveLength(1);
  expect(calls("session_import_scan_group_members")).toHaveLength(before);
});

test("B source failure keeps A, displays retry/copy, and retries only the pinned B revision", async () => {
  const user = userEvent.setup();
  const original = invoke.getMockImplementation()!;
  let fail = true;
  invoke.mockImplementation(async (command: string, args: Record<string, unknown>) => {
    if (command === "session_import_preview" && args.candidateId === previewComparisonMember().candidate.candidateId && fail) {
      fail = false; throw new Error("SOURCE_CHANGED: fixture B changed");
    }
    return original(command, args);
  });
  render(<SessionImportSourceComparisonDialog context={context} tr={tr} onClose={vi.fn()} />);
  await user.click(await screen.findByRole("button", { name: /Composer · b/ }));
  const retry = await screen.findByRole("button", { name: tr("sessionImport.comparison.retryPreview") });
  expect(screen.getByRole("button", { name: tr("sessionImport.copyText") })).toBeEnabled();
  expect(calls("session_import_source_comparison")).toHaveLength(0);
  await user.click(retry);
  await screen.findByText(tr("sessionImport.comparison.position", { n: 1 }));
  expect(calls("session_import_preview").map(([, args]) => args.candidateId)).toEqual([
    context.primary.candidateId, previewComparisonMember().candidate.candidateId, previewComparisonMember().candidate.candidateId,
  ]);
  expect(calls("session_import_preview")[2][1].expectedSourceRevision).toBe("revision-b");
});

test("closing during pending B ignores late results and does not invoke comparison", async () => {
  const user = userEvent.setup();
  const pending = comparisonDeferred<ReturnType<typeof previewComparisonResponse>>();
  const original = invoke.getMockImplementation()!;
  invoke.mockImplementation(async (command: string, args: Record<string, unknown>) => {
    if (command === "session_import_preview" && args.candidateId === previewComparisonMember().candidate.candidateId) return pending.promise;
    return original(command, args);
  });
  const { unmount } = render(<SessionImportSourceComparisonDialog context={context} tr={tr} onClose={vi.fn()} />);
  await user.click(await screen.findByRole("button", { name: /Composer · b/ }));
  unmount();
  await act(async () => { pending.resolve(previewComparisonResponse("b")); await pending.promise; });
  expect(calls("session_import_source_comparison")).toHaveLength(0);
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
});

test("failed comparison page retains the accepted page and retries the exact failed cursor", async () => {
  const user = userEvent.setup();
  const original = invoke.getMockImplementation()!;
  let fail = true;
  invoke.mockImplementation(async (command: string, args: Record<string, unknown>) => {
    const request = args.request as SourceComparisonRequest | undefined;
    if (command === "session_import_source_comparison" && request?.cursor && fail) {
      fail = false; throw new Error("PLAN_STALE: fixture page unavailable");
    }
    return original(command, args);
  });
  render(<SessionImportSourceComparisonDialog context={context} tr={tr} onClose={vi.fn()} />);
  await user.click(await screen.findByRole("button", { name: /Composer · b/ }));
  await screen.findByText(tr("sessionImport.comparison.position", { n: 1 }));
  const footer = screen.getByRole("dialog").querySelector<HTMLElement>(".modal-actions")!;
  await user.click(within(footer).getByRole("button", { name: tr("sessionImport.nextPage") }));
  await user.click(await screen.findByRole("button", { name: tr("sessionImport.comparison.retry") }));
  await screen.findByText(tr("sessionImport.comparison.position", { n: 3 }));
  const requests = calls("session_import_source_comparison").map(([, args]) => args.request);
  expect(requests[1].cursor).toBe("signed-next:2");
  expect(requests[2]).toEqual(requests[1]);
});
