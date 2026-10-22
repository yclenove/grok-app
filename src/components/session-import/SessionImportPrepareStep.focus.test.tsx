/** @vitest-environment jsdom */
import "@testing-library/jest-dom/vitest";
import { act, cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, expect, test, vi } from "vitest";
import type { ComponentProps } from "react";
import { createT } from "@/i18n";
import { installDialogFocus } from "@/lib/a11yFocus";
import type { PrepareBatchItem, PrepareBatchStatus } from "@/lib/api/sessionMigration";
import { SessionImportPrepareStep } from "./SessionImportPrepareStep";

afterEach(cleanup);
type Props = ComponentProps<typeof SessionImportPrepareStep>;
const status: PrepareBatchStatus = {
  version: 1, batchId: "focus-batch", scanId: "focus-scan", epoch: 1,
  selectionId: "focus-selection", selectionStateVersion: 1,
  state: "completed-with-issues", stateVersion: 2, total: 1200, prepared: 1199,
  failed: 1, skipped: 0, active: 0, cancelRequested: false,
  createdAt: "2026-10-01T00:00:00Z", updatedAt: "2026-10-01T00:02:00Z",
};
const issue: PrepareBatchItem = {
  candidateId: "last-issue", title: "Last issue", provider: "generic-file",
  sourceRevision: "synthetic-revision", state: "failed", attempt: 1, previewId: null,
  error: { code: "SOURCE_BUSY", message: "synthetic lock", retry: "retry", scope: "candidate" },
};

function show(overrides: Partial<Props> = {}) {
  const actions = {
    onCancel: vi.fn(), onRetry: vi.fn(), onContinue: vi.fn(), onBack: vi.fn(),
    onLoadMore: vi.fn(), onRetryItems: vi.fn(),
  };
  let props: Props = {
    tr: createT("en"), status, items: [], nextCursor: "next", itemsError: false,
    busy: false, active: false, ...actions, ...overrides,
  };
  const view = render(<SessionImportPrepareStep {...props} />);
  return {
    actions,
    update(change: Partial<Props> = {}) {
      props = { ...props, ...change };
      view.rerender(<SessionImportPrepareStep {...props} />);
    },
  };
}

function loseDisabledFocus() {
  // jsdom keeps disabled buttons focused; model the browser's lost focus.
  act(() => {
    document.body.setAttribute("tabindex", "-1");
    document.body.focus();
    document.body.removeAttribute("tabindex");
  });
}

test("recovers removed load-more focus without activating another action", async () => {
  const user = userEvent.setup();
  const { update, actions } = show();
  act(() => screen.getByRole("button", { name: "Load more" }).focus());
  await user.keyboard("{Enter}");
  update({ items: [issue] });
  expect(screen.queryByRole("button", { name: "Load more" })).not.toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Retry remaining" })).toHaveFocus();
  await user.tab();
  expect(screen.getByRole("button", { name: "Back" })).toHaveFocus();
  expect(actions.onLoadMore).toHaveBeenCalledOnce();
  for (const [name, action] of Object.entries(actions)) {
    if (name !== "onLoadMore") expect(action).not.toHaveBeenCalled();
  }
});

test.each([
  ["more issues remain", false], ["the last issue arrives", true],
] as const)("recovers busy pagination focus after %s", (_label, last) => {
  const { update, actions } = show();
  act(() => screen.getByRole("button", { name: "Load more" }).focus());
  update({ busy: true });
  loseDisabledFocus();
  update({ items: last ? [issue] : [] });
  expect(document.body).toHaveFocus();
  update({ busy: false });
  expect(screen.getByRole("button", { name: last ? "Retry remaining" : "Load more" })).toHaveFocus();
  for (const action of Object.values(actions)) expect(action).not.toHaveBeenCalled();
});

test("recovers removed results-retry focus to Back", () => {
  const { update } = show({ itemsError: true, status: { ...status, state: "completed", failed: 0, prepared: 1200 } });
  act(() => screen.getByRole("button", { name: "Retry loading results" }).focus());
  update({ itemsError: false });
  expect(screen.getByRole("button", { name: "Back" })).toHaveFocus();
});

test("does not acquire focus without a previously focused footer control", () => {
  const { update } = show();
  update({ items: [issue] });
  expect(document.body).toHaveFocus();
});

test("does not restore after a deliberate blur of the enabled control", () => {
  const { update } = show();
  const more = screen.getByRole("button", { name: "Load more" });
  act(() => more.focus());
  act(() => more.blur());
  update({ items: [issue] });
  expect(document.body).toHaveFocus();
});

test("does not steal focus from a deliberate move outside the footer", () => {
  const { update } = show();
  const outside = document.createElement("input");
  document.body.append(outside);
  try {
    act(() => screen.getByRole("button", { name: "Load more" }).focus());
    update({ busy: true });
    act(() => outside.focus());
    update({ busy: false, items: [issue] });
    expect(outside).toHaveFocus();
    act(() => outside.blur());
    update();
    expect(document.body).toHaveFocus();
  } finally {
    outside.remove();
  }
});

test("drops the previous batch focus claim on a batch change", () => {
  const { update } = show();
  act(() => screen.getByRole("button", { name: "Load more" }).focus());
  update({ busy: true });
  loseDisabledFocus();
  update({ status: { ...status, batchId: "another-batch" }, busy: true });
  update({ busy: false, items: [issue] });
  expect(document.body).toHaveFocus();
});

test("does not reclaim focus under or after a newer dialog", () => {
  const { update } = show();
  act(() => screen.getByRole("button", { name: "Load more" }).focus());
  const overlay = document.createElement("div");
  overlay.setAttribute("role", "dialog");
  document.body.append(overlay);
  const release = installDialogFocus(() => overlay, { initialFocus: "none", restoreFocus: false });
  try {
    update({ items: [issue] });
    expect(document.body).toHaveFocus();
  } finally {
    release();
    overlay.remove();
  }
  update();
  expect(document.body).toHaveFocus();
});
