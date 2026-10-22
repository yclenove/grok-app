import { useCallback, useEffect, useRef, useState } from "react";
import {
  sessionImportPrepareCancel,
  sessionImportPrepareGet,
  sessionImportPrepareItems,
  sessionImportPrepareRetry,
  sessionImportPrepareStart,
  type PrepareBatchItem,
  type PrepareBatchStatus,
  type PrepareOverride,
} from "@/lib/api/sessionMigration";

function active(status: PrepareBatchStatus | null): boolean {
  return !!status && ["queued", "preparing", "cancelling"].includes(status.state);
}

function terminal(status: PrepareBatchStatus | null): boolean {
  return !!status && !active(status);
}

type PrepareCommand = "start" | "cancel" | "retry" | "discard";

export function useSessionImportPrepare(onError: (message: string) => void) {
  const [batch, setBatch] = useState<PrepareBatchStatus | null>(null);
  const [items, setItems] = useState<PrepareBatchItem[]>([]);
  const [nextCursor, setNextCursor] = useState<string | null>(null);
  const [commandBusy, setCommandBusy] = useState(false);
  const [itemsBusy, setItemsBusy] = useState(false);
  const [itemsError, setItemsError] = useState(false);
  const [observationVersion, setObservationVersion] = useState(0);
  const generation = useRef(0);
  const commandRef = useRef<number | null>(null);
  const commandKindRef = useRef<PrepareCommand | null>(null);
  const stopRequestedRef = useRef<number | null>(null);
  const batchRef = useRef<PrepareBatchStatus | null>(null);
  const nextCursorRef = useRef<string | null>(null);
  const terminalLoadRef = useRef<string | null>(null);
  const itemsLoadingRef = useRef(false);
  const failedPageRef = useRef<{ cursor: string | null; append: boolean } | null>(null);

  // Invalidating a request must also restart its observers, even when a failed
  // command leaves batch id/state unchanged. A ref alone cannot restart effects.
  const invalidateObservation = useCallback(() => {
    const request = ++generation.current;
    setObservationVersion(request);
    if (itemsLoadingRef.current) terminalLoadRef.current = null;
    itemsLoadingRef.current = false;
    setItemsBusy(false);
    return request;
  }, []);

  const beginCommand = useCallback((kind: PrepareCommand) => {
    const request = invalidateObservation();
    commandRef.current = request;
    commandKindRef.current = kind;
    stopRequestedRef.current = null;
    setCommandBusy(true);
    return request;
  }, [invalidateObservation]);

  const finishCommand = useCallback((request: number) => {
    if (request !== generation.current || commandRef.current !== request) return;
    commandRef.current = null;
    commandKindRef.current = null;
    stopRequestedRef.current = null;
    setCommandBusy(false);
  }, []);

  useEffect(() => () => {
    generation.current += 1;
  }, []);

  const replaceStatus = useCallback((next: PrepareBatchStatus) => {
    batchRef.current = next;
    setBatch(next);
  }, []);

  const applyOwnedStatus = useCallback((next: PrepareBatchStatus, batchId: string) => {
    const current = batchRef.current;
    if (!current || current.batchId !== batchId || next.batchId !== batchId) return false;
    if (current.stateVersion > next.stateVersion) return false;
    replaceStatus(next);
    return true;
  }, [replaceStatus]);

  const resetItems = useCallback(() => {
    itemsLoadingRef.current = false;
    setItemsBusy(false);
    setItemsError(false);
    failedPageRef.current = null;
    terminalLoadRef.current = null;
    nextCursorRef.current = null;
    setItems([]);
    setNextCursor(null);
  }, []);

  const adopt = useCallback(
    (next: PrepareBatchStatus) => {
      const current = batchRef.current;
      if (current?.batchId === next.batchId) {
        if (current.stateVersion > next.stateVersion) return;
      } else {
        invalidateObservation();
        commandRef.current = null;
        commandKindRef.current = null;
        stopRequestedRef.current = null;
        setCommandBusy(false);
        resetItems();
      }
      replaceStatus(next);
    },
    [invalidateObservation, replaceStatus, resetItems],
  );

  const stopOwnedBatch = useCallback(async (request: number) => {
    const current = batchRef.current;
    if (request !== generation.current || commandRef.current !== request || !current
      || (current.state !== "queued" && current.state !== "preparing")) return;
    const next = await sessionImportPrepareCancel(current.batchId);
    if (request === generation.current) applyOwnedStatus(next, current.batchId);
  }, [applyOwnedStatus]);

  const start = useCallback(
    async (
      scanId: string,
      epoch: number,
      selectionId: string,
      overrides: PrepareOverride[],
      expectedStateVersion?: number,
    ) => {
      const request = beginCommand("start");
      resetItems();
      try {
        const previous = batchRef.current;
        if (previous && active(previous)) {
          const cancelled = await sessionImportPrepareCancel(previous.batchId);
          if (request !== generation.current) return null;
          if (stopRequestedRef.current === request) {
            applyOwnedStatus(cancelled, previous.batchId);
            return null;
          }
        }
        if (request !== generation.current) return null;
        const next = await sessionImportPrepareStart(scanId, epoch, selectionId, overrides);
        if (request !== generation.current) {
          await sessionImportPrepareCancel(next.batchId).catch(() => null);
          return null;
        }
        if (expectedStateVersion !== undefined && (next.scanId !== scanId || next.epoch !== epoch ||
          next.selectionId !== selectionId || next.selectionStateVersion !== expectedStateVersion)) {
          await sessionImportPrepareCancel(next.batchId).catch(() => null);
          throw new Error("PLAN_STALE: preparation acknowledged a different selection snapshot");
        }
        replaceStatus(next);
        if (stopRequestedRef.current === request) {
          await stopOwnedBatch(request);
          return null;
        }
        return next;
      } catch (error) {
        if (request === generation.current) onError(String(error));
        return null;
      } finally {
        finishCommand(request);
      }
    },
    [applyOwnedStatus, beginCommand, finishCommand, onError, replaceStatus, resetItems, stopOwnedBatch],
  );

  useEffect(() => {
    if (!batch || !active(batch) || commandBusy) return;
    const request = generation.current;
    const batchId = batch.batchId;
    let stopped = false;
    let timer = 0;
    let failureDelay = 500;
    const schedule = (delay: number) => {
      if (!stopped && request === generation.current) {
        timer = window.setTimeout(() => void poll(), delay);
      }
    };
    const poll = async () => {
      try {
        const next = await sessionImportPrepareGet(batchId);
        if (stopped || request !== generation.current) return;
        applyOwnedStatus(next, batchId);
        failureDelay = 500;
        if (batchRef.current?.batchId === batchId && active(batchRef.current)) schedule(250);
      } catch (error) {
        if (!stopped && request === generation.current) {
          onError(String(error));
          schedule(failureDelay);
          failureDelay = Math.min(failureDelay * 2, 4_000);
        }
      }
    };
    schedule(100);
    return () => {
      stopped = true;
      window.clearTimeout(timer);
    };
  }, [batch?.batchId, batch?.state, observationVersion, commandBusy, applyOwnedStatus, onError]);

  const loadItemsPage = useCallback(
    async (cursor: string | null, append: boolean) => {
      const current = batchRef.current;
      if (!current || commandRef.current !== null || (append && !cursor) || itemsLoadingRef.current) return false;
      const request = generation.current;
      const batchId = current.batchId;
      itemsLoadingRef.current = true;
      setItemsBusy(true);
      setItemsError(false);
      failedPageRef.current = null;
      try {
        const page = await sessionImportPrepareItems(
          batchId,
          cursor,
          50,
        );
        if (request !== generation.current || batchRef.current?.batchId !== batchId) return false;
        setItems((previous) => {
          if (!append) return page.items;
          const seen = new Set(previous.map((item) => item.candidateId));
          return [...previous, ...page.items.filter((item) => !seen.has(item.candidateId))];
        });
        nextCursorRef.current = page.nextCursor;
        setNextCursor(page.nextCursor);
        return true;
      } catch (error) {
        if (request === generation.current) {
          failedPageRef.current = { cursor, append };
          setItemsError(true);
          onError(String(error));
        }
        return false;
      } finally {
        if (request === generation.current && batchRef.current?.batchId === batchId) {
          itemsLoadingRef.current = false;
          setItemsBusy(false);
        }
      }
    },
    [onError],
  );

  const loadFirstItems = useCallback(
    () => loadItemsPage(null, false),
    [loadItemsPage],
  );

  const loadMoreItems = useCallback(
    () => loadItemsPage(nextCursorRef.current, true),
    [loadItemsPage],
  );

  const retryItems = useCallback(
    () => {
      const failed = failedPageRef.current;
      return failed ? loadItemsPage(failed.cursor, failed.append) : Promise.resolve(false);
    },
    [loadItemsPage],
  );

  useEffect(() => {
    if (!batch || !terminal(batch) || commandBusy) return;
    const key = `${batch.batchId}:${batch.stateVersion}`;
    if (terminalLoadRef.current === key) return;
    terminalLoadRef.current = key;
    void loadFirstItems();
  }, [batch?.batchId, batch?.state, batch?.stateVersion, observationVersion, commandBusy, loadFirstItems]);

  const cancel = useCallback(async () => {
    const current = batchRef.current;
    if (!current || commandRef.current !== null
      || (current.state !== "queued" && current.state !== "preparing")) return null;
    const request = beginCommand("cancel");
    const batchId = current.batchId;
    try {
      const next = await sessionImportPrepareCancel(batchId);
      if (request !== generation.current || !applyOwnedStatus(next, batchId)) return null;
      return next;
    } catch (error) {
      if (request === generation.current) onError(String(error));
      return null;
    } finally {
      finishCommand(request);
    }
  }, [applyOwnedStatus, beginCommand, finishCommand, onError]);

  const stop = useCallback(async () => {
    const request = commandRef.current;
    if (request !== null) {
      // Close is a lifecycle request: let the owner accept its result and stop
      // it, rather than invalidating the owner or clearing busy prematurely.
      if (commandKindRef.current === "start" || commandKindRef.current === "retry") {
        stopRequestedRef.current = request;
      }
      return null;
    }
    return cancel();
  }, [cancel]);

  const retry = useCallback(async () => {
    const current = batchRef.current;
    if (!current || commandRef.current !== null) return null;
    const request = beginCommand("retry");
    const batchId = current.batchId;
    resetItems();
    try {
      const next = await sessionImportPrepareRetry(batchId);
      if (request !== generation.current) return null;
      const accepted = applyOwnedStatus(next, batchId);
      if (stopRequestedRef.current === request) {
        await stopOwnedBatch(request);
        return null;
      }
      return accepted ? next : null;
    } catch (error) {
      if (request === generation.current) onError(String(error));
      return null;
    } finally {
      finishCommand(request);
    }
  }, [applyOwnedStatus, beginCommand, finishCommand, onError, resetItems, stopOwnedBatch]);

  const clearLocal = useCallback(() => {
    batchRef.current = null;
    setBatch(null);
    resetItems();
    commandRef.current = null;
    commandKindRef.current = null;
    stopRequestedRef.current = null;
    setCommandBusy(false);
  }, [resetItems]);

  const clear = useCallback(() => {
    invalidateObservation();
    clearLocal();
  }, [clearLocal, invalidateObservation]);

  const discard = useCallback(async () => {
    const current = batchRef.current;
    const request = beginCommand("discard");
    if (!current) {
      clearLocal();
      return true;
    }
    try {
      if (active(current)) {
        await sessionImportPrepareCancel(current.batchId);
      }
      if (request !== generation.current) return false;
      clearLocal();
      return true;
    } catch (error) {
      if (request === generation.current) onError(String(error));
      return false;
    } finally {
      finishCommand(request);
    }
  }, [beginCommand, clearLocal, finishCommand, onError]);

  const isCurrent = useCallback(
    (batchId: string) => batchRef.current?.batchId === batchId,
    [],
  );

  const hasBatch = useCallback(() => batchRef.current !== null, []);
  return {
    batch,
    items,
    nextCursor,
    busy: commandBusy || itemsBusy,
    itemsError,
    active: active(batch),
    start,
    adopt,
    cancel,
    stop,
    retry,
    loadMore: loadMoreItems,
    retryItems,
    clear,
    discard,
    isCurrent,
    hasBatch,
  };
}
