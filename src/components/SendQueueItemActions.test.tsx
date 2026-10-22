/** @vitest-environment jsdom */
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { createT, type Locale } from "@/i18n";
import { makeQueuedSend, type QueuedSend } from "@/lib/sendQueue";
import { SendQueueItemActions } from "./SendQueueItemActions";
afterEach(cleanup);
const local = (id: string) => makeQueuedSend({ id, storedDisplay: id, attachments: [], goalMode: false });
const external = (id: string): QueuedSend => ({ ...local(id), source: "external" });
const locales: Locale[] = ["en", "de", "es", "fil", "fr", "id", "it", "ja", "ko", "pt-BR", "ru", "ta", "uk", "zh", "zh-TW"];
function props(queue: QueuedSend[] = [local("a"), local("b"), local("c")]) {
  return { queue, index: 1, busy: false, guidingItemId: null as string | null, canGuide: false,
    tr: createT("en"), onMove: vi.fn(), onEdit: vi.fn(), onGuide: vi.fn(), onRemove: vi.fn() };
}
describe("SendQueueItemActions", () => {
  it.each(locales)("Host entries are visibly read-only with no resend or pretend-cancel actions in %s", (locale) => {
    const p = { ...props([external("host")]), index: 0, tr: createT(locale) };
    render(<SendQueueItemActions {...p} />);
    expect(screen.getByText(p.tr("composer.queueHostManaged"))).toBeTruthy();
    expect(screen.queryAllByRole("button")).toHaveLength(0);
    expect(p.onMove).not.toHaveBeenCalled(); expect(p.onEdit).not.toHaveBeenCalled();
    expect(p.onGuide).not.toHaveBeenCalled(); expect(p.onRemove).not.toHaveBeenCalled();
  });
  it("retains working move/edit/send/remove controls for local entries", () => {
    const p = props(); render(<SendQueueItemActions {...p} />);
    fireEvent.click(screen.getByRole("button", { name: p.tr("composer.queueMoveUp") }));
    fireEvent.click(screen.getByRole("button", { name: p.tr("composer.queueMoveDown") }));
    fireEvent.click(screen.getByRole("button", { name: p.tr("composer.queueEdit") }));
    fireEvent.click(screen.getByRole("button", { name: p.tr("composer.queueSendNow") }));
    fireEvent.click(screen.getByRole("button", { name: p.tr("composer.queueRemove") }));
    expect(p.onMove.mock.calls).toEqual([["b", "up"], ["b", "down"]]);
    expect(p.onEdit).toHaveBeenCalledWith(p.queue[1]); expect(p.onGuide).toHaveBeenCalledWith(p.queue[1]);
    expect(p.onRemove).toHaveBeenCalledWith("b");
  });
  it("disables local movement across Host entries but preserves other local actions", () => {
    const p = props([external("first"), local("local"), external("last")]);
    render(<SendQueueItemActions {...p} />);
    for (const label of ["composer.queueMoveUp", "composer.queueMoveDown"] as const) {
      const button = screen.getByRole("button", { name: p.tr(label) }) as HTMLButtonElement;
      expect(button.disabled).toBe(true); fireEvent.click(button);
    }
    expect(p.onMove).not.toHaveBeenCalled();
    expect((screen.getByRole("button", { name: p.tr("composer.queueEdit") }) as HTMLButtonElement).disabled).toBe(false);
  });
  it("blocks every local action while editing or guiding that row", () => {
    const p = { ...props(), busy: true }; render(<SendQueueItemActions {...p} />);
    for (const button of screen.getAllByRole("button")) {
      expect((button as HTMLButtonElement).disabled).toBe(true); fireEvent.click(button);
    }
    expect(p.onMove).not.toHaveBeenCalled(); expect(p.onEdit).not.toHaveBeenCalled();
    expect(p.onGuide).not.toHaveBeenCalled(); expect(p.onRemove).not.toHaveBeenCalled();
  });
  it("labels live steering and blocks a second guide while another item is in flight", () => {
    const p = { ...props(), canGuide: true }; const view = render(<SendQueueItemActions {...p} />);
    expect(screen.getByRole("button", { name: p.tr("composer.queueGuide") })).toBeTruthy();
    view.rerender(<SendQueueItemActions {...p} guidingItemId="different" />);
    const button = screen.getByRole("button", { name: p.tr("composer.queueGuiding") }) as HTMLButtonElement;
    expect(button.disabled).toBe(true); fireEvent.click(button); expect(p.onGuide).not.toHaveBeenCalled();
  });
  it("renders no controls for a disappeared row", () => {
    render(<SendQueueItemActions {...props([])} />); expect(screen.queryAllByRole("button")).toHaveLength(0);
  });
});
