/** Session-scoped confirm and handoff cards. Switching chats does not drop them. */

import { useCallback, useEffect, useRef, useState } from "react";
import {
  listenComputerUseConfirm,
  listenComputerUseConfirmSettled,
  listenComputerUseHandoff,
  listenComputerUseHandoffSettled,
} from "@/lib/api/computerUse";
import {
  listenComputerUseSessionClosed,
  listenComputerUseTurnEnded,
} from "@/lib/api/computerUseShot";
import {
  bindSensitive,
  evictSensitive,
  type ConfirmCard,
  type ConfirmEnd,
  type ConfirmPhase,
  type HandoffCard,
  type HandoffEnd,
  type HandoffPhase,
} from "@/lib/computer-use/confirmState";
import { sharedTimeline } from "@/lib/computer-use/store";

function useLiveCards<T extends { requestId: number; sessionId: string }, P extends string>(
  listenPending: (handler: (card: T) => void) => Promise<() => void>,
  listenSettled: (handler: (requestId: number, end: P) => void) => Promise<() => void>,
) {
  const [cards, setCards] = useState<T[]>([]);
  const [phases, setPhases] = useState<Record<number, P>>({});
  const sessions = useRef(new Map<number, string>());
  const stopped = useRef(sharedTimeline.view("").stopped);

  const evict = useCallback((sessionId: string | null) => {
    const drop = new Set<number>();
    for (const [id, sid] of sessions.current) {
      if (sessionId === null || sid === sessionId) {
        drop.add(id);
        sessions.current.delete(id);
      }
    }
    setCards((current) =>
      evictSensitive(current, sessionId === null ? { all: true } : { sessionId }),
    );
    setPhases((current) => {
      if (drop.size === 0) return current;
      const next = { ...current };
      for (const id of drop) delete next[id];
      return next;
    });
  }, []);

  useEffect(() => {
    let stop = false;
    const unlisten: Array<() => void> = [];
    void (async () => {
      const stopPending = await listenPending((card) => {
        if (stop || stopped.current) return;
        sessions.current.set(card.requestId, card.sessionId);
        setCards((current) => bindSensitive(current, card));
        setPhases((current) =>
          current[card.requestId] ? current : { ...current, [card.requestId]: "pending" as P },
        );
      });
      const stopSettled = await listenSettled((requestId, end) => {
        if (stop) return;
        setPhases((current) => {
          const phase = current[requestId] ?? ("pending" as P);
          if (phase !== "pending" && phase !== "submitting") return current;
          return { ...current, [requestId]: end };
        });
      });
      const stopClosed = await listenComputerUseSessionClosed((sessionId) => {
        if (!stop) evict(sessionId);
      });
      const stopTurn = await listenComputerUseTurnEnded((sessionId) => {
        if (!stop) evict(sessionId);
      });
      if (stop) {
        stopPending();
        stopSettled();
        stopClosed();
        stopTurn();
        return;
      }
      unlisten.push(stopPending, stopSettled, stopClosed, stopTurn);
    })();
    const unsubscribe = sharedTimeline.subscribe(() => {
      const now = sharedTimeline.view("").stopped;
      stopped.current = now;
      if (now) evict(null);
    });
    return () => {
      stop = true;
      unsubscribe();
      for (const fn of unlisten) fn();
    };
  }, [listenPending, listenSettled, evict]);

  const setPhase = useCallback((requestId: number, phase: P) => {
    setPhases((current) => {
      const previous = current[requestId] ?? ("pending" as P);
      if (previous !== "pending" && previous !== "submitting") return current;
      return { ...current, [requestId]: phase };
    });
  }, []);

  return { cards, phases, setPhase };
}

export function useComputerUseConfirm() {
  return useLiveCards<ConfirmCard, ConfirmPhase | ConfirmEnd>(
    listenComputerUseConfirm,
    listenComputerUseConfirmSettled,
  );
}

export function useComputerUseHandoff() {
  return useLiveCards<HandoffCard, HandoffPhase | HandoffEnd>(
    listenComputerUseHandoff,
    listenComputerUseHandoffSettled,
  );
}
