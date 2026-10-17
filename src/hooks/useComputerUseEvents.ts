/** Live Computer side-panel timeline, plus the chat approval and handoff desks. */

import { useEffect, useState, useSyncExternalStore } from "react";
import { cuTimeline, listenComputerUseTimeline } from "@/lib/api/computerUsePanel";
import { isDesktopHost } from "@/lib/api/host";
import { cardsForSession as approvalCards } from "@/lib/computer-use/approvalDesk";
import { cardsForSession } from "@/lib/computer-use/confirmState";
import { sharedShotBook, type ShotBook } from "@/lib/computer-use/events";
import {
  sharedTimeline,
  type TimelineNotice,
  type TimelineStore,
} from "@/lib/computer-use/store";
import { useComputerUseApprovals } from "@/hooks/useComputerUseApprovals";
import { useComputerUseConfirm, useComputerUseHandoff } from "@/hooks/useComputerUseSensitive";

function pending<T extends { requestId: number }>(
  cards: T[],
  phases: Record<number, string>,
): T | undefined {
  return cards.find((card) => {
    const phase = phases[card.requestId] ?? "pending";
    return phase === "pending" || phase === "submitting";
  });
}

/** Apply one host notice. A forgotten session also drops its redacted shots. */
export function applyComputerUseNotice(
  store: TimelineStore,
  notice: TimelineNotice,
  book: ShotBook = sharedShotBook,
): void {
  store.apply(notice);
  if (notice.kind === "forgotten") book.dropSession(notice.sessionId);
}

export function useComputerUseEvents(
  sessionId: string | null,
  store: TimelineStore = sharedTimeline,
  book: ShotBook = sharedShotBook,
) {
  const view = useSyncExternalStore(
    store.subscribe,
    () => store.view(sessionId),
    () => store.view(sessionId),
  );
  const approvals = useComputerUseApprovals();
  const confirms = useComputerUseConfirm();
  const handoffs = useComputerUseHandoff();
  const [loadFailed, setLoadFailed] = useState(false);
  const session = sessionId ?? "";
  const approval = approvalCards(approvals, session)[0];
  const confirm = pending(cardsForSession(confirms.cards, session), confirms.phases);
  const handoff = pending(cardsForSession(handoffs.cards, session), handoffs.phases);

  useEffect(() => {
    if (!isDesktopHost()) return;
    let stop = false;
    const unlisten: Array<() => void> = [];
    setLoadFailed(false);
    void (async () => {
      const off = await listenComputerUseTimeline((notice) => {
        if (!stop) applyComputerUseNotice(store, notice, book);
      });
      if (sessionId) {
        try {
          const snapshot = await cuTimeline(sessionId);
          if (!stop) store.replace(snapshot);
        } catch {
          if (!stop) setLoadFailed(true);
        }
      }
      if (stop) {
        off();
        return;
      }
      unlisten.push(off);
    })();
    return () => {
      stop = true;
      for (const fn of unlisten) fn();
    };
  }, [book, sessionId, store]);

  return {
    view,
    waiting: Boolean(approval || confirm),
    waitingTarget: approval?.name || confirm?.name || "",
    handoff: Boolean(handoff),
    loadFailed,
  };
}
