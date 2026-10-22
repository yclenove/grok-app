/** @vitest-environment jsdom */
import { afterEach, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { createT, type Locale } from "@/i18n";
import { SendQueueSyncNotice } from "./SendQueueSyncNotice";
import { SendQueueItemActions } from "./SendQueueItemActions";
import { makeQueuedSend } from "@/lib/sendQueue";
afterEach(cleanup);
const locales: Locale[] = ["en","de","es","fil","fr","id","it","ja","ko","pt-BR","ru","ta","uk","zh","zh-TW"];
const sync = () => ({ enabled: true, ready: false, error: false, loading: true, refresh: vi.fn() });
it("hides when disabled or verified and shows initial loading without a send action", () => {
  const p = { sync: sync(), tr: createT("en") }; const v = render(<SendQueueSyncNotice {...p} />);
  expect(screen.getByRole("status").textContent).toBe(p.tr("composer.queueSyncLoading"));
  expect(screen.queryAllByRole("button")).toHaveLength(0);
  v.rerender(<SendQueueSyncNotice {...p} sync={{ ...p.sync, enabled: false }} />); expect(v.container.textContent).toBe("");
  v.rerender(<SendQueueSyncNotice {...p} sync={{ ...p.sync, ready: true }} />); expect(v.container.textContent).toBe("");
});
it.each(locales)("renders translated stale-state protection and status-only retry in %s", locale => {
  const s = { ...sync(), loading: false, error: true }; const tr = createT(locale);
  const v = render(<SendQueueSyncNotice sync={s} tr={tr} />);
  expect(screen.getByRole("alert").textContent).toContain(tr("composer.queueSyncError"));
  fireEvent.click(screen.getByRole("button", { name: tr("composer.queueSyncRefresh") })); expect(s.refresh).toHaveBeenCalledTimes(1);
  v.rerender(<SendQueueSyncNotice sync={{ ...s, loading: true }} tr={tr} />);
  const retry = screen.getByRole("button") as HTMLButtonElement; expect(retry.disabled).toBe(true);
  fireEvent.click(retry); expect(s.refresh).toHaveBeenCalledTimes(1);
});
it.each(locales)("shows Pending and Held as read-only with a review warning in %s", locale => {
  const tr = createT(locale);
  const item = { ...makeQueuedSend({ id: "host", storedDisplay: "prompt", attachments: [], goalMode: false }), source: "external" as const, externalState: "pending" as const };
  const p = { queue: [item], index: 0, busy: false, guidingItemId: null, canGuide: true, tr, onMove: vi.fn(), onEdit: vi.fn(), onGuide: vi.fn(), onRemove: vi.fn() };
  const v = render(<SendQueueItemActions {...p} />);
  expect(screen.getByText(tr("composer.queueHostPending")).getAttribute("title")).toBe(tr("composer.queueHostReview"));
  v.rerender(<SendQueueItemActions {...p} queue={[{ ...item, externalState: "held" }]} />);
  expect(screen.getByText(tr("composer.queueHostHeld")).getAttribute("aria-label")).toContain(tr("composer.queueHostReview"));
  expect(screen.queryAllByRole("button")).toHaveLength(0);
});
