/** @vitest-environment jsdom */
import { afterEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, renderHook } from "@testing-library/react";
import { createT } from "@/i18n";
import { makeQueuedSend, type QueuedSend } from "@/lib/sendQueue";
import { useQueueEditDialog } from "./useQueueEditDialog";
afterEach(() => { cleanup(); vi.useRealTimers(); });
const local = (id: string) => makeQueuedSend({ id, storedDisplay: id, attachments: [], goalMode: false });
const external = (id: string): QueuedSend => ({ ...local(id), source: "external" });
function props(queue: QueuedSend[]) { return { tr: createT("en"), showToast: vi.fn(), activeQueue: queue,
  updateItem: vi.fn(), pauseFlush: vi.fn(), releaseFlushHold: vi.fn(), clearQueue: vi.fn() }; }
describe("useQueueEditDialog Host ownership", () => {
  it("refuses to open a local editor for a Host payload without pausing sends", () => {
    const p = props([external("host")]); const { result } = renderHook(() => useQueueEditDialog(p));
    act(() => result.current.openEdit(p.activeQueue[0]!));
    expect(result.current.editItemId).toBeNull(); expect(p.pauseFlush).not.toHaveBeenCalled();
    expect(p.updateItem).not.toHaveBeenCalled();
  });
  it("cannot save if a stale edited item is now Host-owned", () => {
    vi.useFakeTimers(); const p = props([local("same")]);
    const { result, rerender } = renderHook((input) => useQueueEditDialog(input), { initialProps: p });
    act(() => result.current.openEdit(p.activeQueue[0]!));
    act(() => result.current.setEditText("replacement"));
    rerender({ ...p, activeQueue: [external("same")] });
    act(() => result.current.saveEdit());
    expect(p.updateItem).not.toHaveBeenCalled(); expect(result.current.editItemId).toBeNull();
  });
  it("does not offer clear for Host-only rows and counts only local rows in mixed queues", () => {
    const p = props([external("host")]); const { result, rerender } = renderHook((input) => useQueueEditDialog(input), { initialProps: p });
    act(() => result.current.requestClear()); expect(result.current.clearOpen).toBe(false);
    rerender({ ...p, activeQueue: [external("host"), local("local")] });
    act(() => result.current.requestClear()); expect(result.current.clearOpen).toBe(true);
    expect(result.current.clearPlan.count).toBe(1); expect(result.current.clearPlan.next[0]?.source).toBe("external");
  });
});
