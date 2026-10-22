/** @vitest-environment jsdom */
import "@/test/jsdomStubs";
import { cleanup, fireEvent, render } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { PlanReviewPanel, type PlanReviewPanelLabels } from "./PlanReviewPanel";
import { PlanStatusBar, type PlanStatusBarLabels } from "./PlanStatusBar";
import { PlanReviseModal } from "./workbench-modals/PlanReviseModal";

const reviewLabels = Object.fromEntries([
  "ready", "waiting", "progress", "done", "empty", "approve", "changes", "dismiss", "steps", "fraction",
  "expandDetails", "collapseDetails", "current", "edit", "cancelEdit", "requestWithDraft", "approveDirtyHint",
  "draftPlaceholder", "draftAria", "discardTitle", "discardMessage", "discardConfirm", "discardCancel",
  "draftEmpty", "draftTooLong", "close",
].map((key) => [key, key])) as PlanReviewPanelLabels;
const barLabels = Object.fromEntries([
  "goal", "planMode", "progress", "review", "done", "resume", "fraction", "current", "approve", "changes",
  "dismiss", "expand", "clearGoal", "aria",
].map((key) => [key, key])) as PlanStatusBarLabels;
const plan = { visible: true, waiting: false, title: "计划", body: "原始中文计划", entries: [], rpcId: 0, requestGeneration: "old" };
afterEach(cleanup);

test("the sticky plan bar keeps actions visible but disables all reply paths while busy", () => {
  const approve = vi.fn(); const changes = vi.fn(); const dismiss = vi.fn();
  const view = render(<PlanStatusBar goalMode={false} mode="plan" planVisible planWaiting={false} planRpcId={0}
    entries={[]} labels={barLabels} busy onApprove={approve} onRequestChanges={changes} onDismiss={dismiss} />);
  for (const name of ["approve", "changes", "dismiss"]) {
    const button = view.getByRole("button", { name }) as HTMLButtonElement;
    expect(button.disabled).toBe(true); fireEvent.click(button);
  }
  expect(approve).not.toHaveBeenCalled(); expect(changes).not.toHaveBeenCalled(); expect(dismiss).not.toHaveBeenCalled();
});

test("the plan editor retains submitted text until an ACK changes the gate, and resets on a new same-RPC generation", () => {
  const changes = vi.fn();
  const view = render(<PlanReviewPanel plan={plan} labels={reviewLabels} onRequestChanges={changes} />);
  fireEvent.click(view.getByTestId("plan-review-edit"));
  fireEvent.change(view.getByRole("textbox"), { target: { value: "修改后的中文计划" } });
  fireEvent.click(view.getByTestId("plan-review-request-with-draft"));
  expect((view.getByRole("textbox") as HTMLTextAreaElement).value).toBe("修改后的中文计划");
  view.rerender(<PlanReviewPanel plan={{ ...plan, busy: true }} labels={reviewLabels} onRequestChanges={changes} />);
  expect((view.getByRole("textbox") as HTMLTextAreaElement).disabled).toBe(true);
  view.rerender(<PlanReviewPanel plan={{ ...plan, busy: false }} labels={reviewLabels} onRequestChanges={changes} />);
  expect((view.getByRole("textbox") as HTMLTextAreaElement).value).toBe("修改后的中文计划");
  view.rerender(<PlanReviewPanel plan={{ ...plan, requestGeneration: "new" }} labels={reviewLabels} onRequestChanges={changes} />);
  expect(view.queryByRole("textbox")).toBeNull();
  fireEvent.click(view.getByTestId("plan-review-edit"));
  expect((view.getByRole("textbox") as HTMLTextAreaElement).value).toBe(plan.body);
});

test("the Chinese revision modal disables editing, submit and cancellation during a reply", () => {
  const close = vi.fn(); const submit = vi.fn(); const change = vi.fn();
  const view = render(<PlanReviseModal locale="zh" open note="保留意见" busy onClose={close} onSubmit={submit} onNoteChange={change} />);
  expect((view.getByTestId("plan-revise-note") as HTMLTextAreaElement).disabled).toBe(true);
  const button = view.getByTestId("plan-revise-submit") as HTMLButtonElement;
  expect(button.disabled).toBe(true); fireEvent.click(button); fireEvent.keyDown(document, { key: "Escape" });
  expect(submit).not.toHaveBeenCalled(); expect(close).not.toHaveBeenCalled();
});
