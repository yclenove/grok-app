/** Session-scoped approval cards. Switching chats does not drop a pending card. */

import { useEffect, useState } from "react";
import {
  listenComputerUseApproval,
  listenComputerUseSettled,
} from "@/lib/api/computerUse";
import {
  applySettled,
  bindCard,
  emptyDesk,
  type Desk,
} from "@/lib/computer-use/approvalDesk";

export function useComputerUseApprovals(): Desk {
  const [desk, setDesk] = useState(emptyDesk);

  useEffect(() => {
    let stop = false;
    const unlisten: Array<() => void> = [];
    void (async () => {
      const stopPending = await listenComputerUseApproval((card) => {
        if (!stop) setDesk((current) => bindCard(current, card));
      });
      const stopSettled = await listenComputerUseSettled((requestId) => {
        if (!stop) setDesk((current) => applySettled(current, requestId));
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
  }, []);

  return desk;
}
