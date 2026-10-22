import { IconChevronDown, IconChevronUp, IconClose } from "@/components/icons";
import { createT } from "@/i18n";
import type { QueuedSend, QueueMoveDirection } from "@/lib/sendQueue";

type Props = {
  queue: readonly QueuedSend[];
  index: number;
  busy: boolean;
  guidingItemId: string | null;
  canGuide: boolean;
  tr: ReturnType<typeof createT>;
  onMove: (id: string, direction: QueueMoveDirection) => void;
  onEdit: (item: QueuedSend) => void;
  onGuide: (item: QueuedSend) => void | Promise<void>;
  onRemove: (id: string) => void;
  onReview?: (item: QueuedSend) => void;
};

/** Host-owned prompts expose review, never composer-local edit/remove/send actions. */
export function SendQueueItemActions(p: Props) {
  const item = p.queue[p.index];
  if (!item) return null;
  if (item.source === "external") {
    const key = item.externalState === "pending" ? "composer.queueHostPending"
      : item.externalState === "held" ? "composer.queueHostHeld" : p.onReview ? "composer.queueHostQueued" : "composer.queueHostManaged";
    const review = item.externalState === "pending" || item.externalState === "held";
    return <><span className="composer__queue-src composer__queue-host-state" title={review ? p.tr("composer.queueHostReview") : undefined}
      aria-label={review ? `${p.tr(key)}. ${p.tr("composer.queueHostReview")}` : undefined}>{p.tr(key)}</span>
      {p.onReview && <button type="button" className="composer__queue-edit" onClick={() => p.onReview?.(item)}>{p.tr("composer.queueReviewAction")}</button>}</>;
  }
  const upBlocked = p.busy || p.index === 0 || p.queue[p.index - 1]?.source === "external";
  const downBlocked = p.busy || p.index >= p.queue.length - 1 || p.queue[p.index + 1]?.source === "external";
  const guideLabel = p.guidingItemId !== null
    ? p.tr("composer.queueGuiding")
    : p.canGuide ? p.tr("composer.queueGuide") : p.tr("composer.queueSendNow");
  return <>
    <div className="composer__queue-move">
      <button type="button" className="composer__queue-move-btn" data-testid="queue-move-up"
        aria-label={p.tr("composer.queueMoveUp")} title={p.tr("composer.queueMoveUp")}
        disabled={upBlocked} onClick={() => p.onMove(item.id, "up")}>
        <IconChevronUp size={12} aria-hidden />
      </button>
      <button type="button" className="composer__queue-move-btn" data-testid="queue-move-down"
        aria-label={p.tr("composer.queueMoveDown")} title={p.tr("composer.queueMoveDown")}
        disabled={downBlocked} onClick={() => p.onMove(item.id, "down")}>
        <IconChevronDown size={12} aria-hidden />
      </button>
    </div>
    <button type="button" className="composer__queue-edit" data-testid="queue-edit"
      aria-label={p.tr("composer.queueEdit")} title={p.tr("composer.queueEdit")}
      disabled={p.busy} onClick={() => p.onEdit(item)}>{p.tr("composer.queueEdit")}</button>
    <button type="button" className="composer__queue-guide" data-testid="queue-guide"
      aria-label={guideLabel} title={guideLabel} disabled={p.busy || p.guidingItemId !== null}
      onClick={() => void p.onGuide(item)}>{guideLabel}</button>
    <button type="button" className="composer__queue-remove" aria-label={p.tr("composer.queueRemove")}
      disabled={p.busy} onClick={() => p.onRemove(item.id)}><IconClose size={12} /></button>
  </>;
}
