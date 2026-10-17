/** Session-scoped confirm and handoff cards. Switching chats does not drop them. */

import { useCallback, useEffect, useState } from "react";
import {
  listenComputerUseConfirm,
  listenComputerUseConfirmSettled,
  listenComputerUseHandoff,
  listenComputerUseHandoffSettled,
} from "@/lib/api/computerUse";
import {
  bindSensitive,
  type ConfirmCard,
  type ConfirmEnd,
  type ConfirmPhase,
  type HandoffCard,
  type HandoffEnd,
  type HandoffPhase,
} from "@/lib/computer-use/confirmState";

function useLiveCards<T extends { requestId: number }, P extends string>(
  listenPending: (handler: (card: T) => void) => Promise<() => void>,
  listenSettled: (handler: (requestId: number, end: P) => void) => Promise<() => void>,
) {
  const [cards, setCards] = useState<T[]>([]);
  const [phases, setPhases] = useState<Record<number, P>>({});

  useEffect(() => {
    let stop = false;
    const unlisten: Array<() => void> = [];
    void (async () => {
      const stopPending = await listenPending((card) => {
        if (stop) return;
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
      if (stop) {
        stopPending();
        stopSettled();
        return;
      }
      unlisten.push(stopPending, stopSettled);
    })();
    return () => {
      stop = true;
      for (const fn of unlisten) fn();
    };
  }, [listenPending, listenSettled]);

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
