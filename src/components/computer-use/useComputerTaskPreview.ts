import { useEffect, useRef, useState } from "react";
import { computerTaskPreview, type ComputerTaskExecutionAddress, type ComputerTaskPreview } from "@/lib/api/computerTask";
import { sameComputerTaskKey } from "@/lib/computer-use/taskActions";

const PREVIEW_FRESH_MS = 3000;

/** Exact task UI pixels only. Hidden/unapproved/paused panels never capture;
 * even across effect changes there is at most one outstanding request. */
export function useComputerTaskPreview(sessionId: string | null, address: ComputerTaskExecutionAddress | null, owner: string | null, active: boolean) {
  const key = JSON.stringify([sessionId, owner, address?.key, address?.runId, address?.generation]);
  const [captured, setCaptured] = useState<{ key: string; frame: ComputerTaskPreview; fresh: boolean } | null>(null);
  const [failure, setFailure] = useState<string | null>(null);
  const outstanding = useRef<Promise<unknown> | null>(null);
  const connectionId = address?.key.connectionId;
  const taskId = address?.key.taskId;
  const requestId = address?.key.requestId;
  const revision = address?.key.revision;
  const runId = address?.runId;
  const generation = address?.generation;
  useEffect(() => {
    if (!active || !sessionId || !connectionId || !taskId || !requestId || revision === undefined || !runId || !generation) return;
    const original = { key: { connectionId, taskId, requestId, revision }, runId, generation };
    let disposed = false;
    let visibilityEpoch = 0;
    let timer: ReturnType<typeof setTimeout> | undefined;
    let freshTimer: ReturnType<typeof setTimeout> | undefined;
    async function capture(epoch: number) {
      if (disposed || document.hidden || epoch !== visibilityEpoch) return;
      if (outstanding.current) {
        timer = setTimeout(() => void capture(epoch), 1000);
        return;
      }
      const started = performance.now();
      const call = computerTaskPreview(sessionId!, original);
      outstanding.current = call;
      try {
        const frame = await call;
        if (disposed || epoch !== visibilityEpoch || document.hidden) return;
        if (frame.address.runId !== runId || frame.address.generation !== generation || !sameComputerTaskKey(frame.address.key, original.key)) throw new Error("preview identity changed");
        const remaining = PREVIEW_FRESH_MS - (performance.now() - started);
        setCaptured({ key, frame, fresh: remaining > 0 });
        setFailure(null);
        clearTimeout(freshTimer);
        if (remaining > 0) freshTimer = setTimeout(() => {
          if (!disposed && epoch === visibilityEpoch) setCaptured((current) => current?.key === key ? { ...current, fresh: false } : current);
        }, remaining);
      } catch {
        if (!disposed && epoch === visibilityEpoch) {
          setFailure(key);
          setCaptured((current) => current?.key === key ? { ...current, fresh: false } : current);
        }
      } finally {
        if (outstanding.current === call) outstanding.current = null;
        if (!disposed && epoch === visibilityEpoch && !document.hidden) timer = setTimeout(() => void capture(epoch), 1000);
      }
    }
    function visibilityChanged() {
      clearTimeout(timer); clearTimeout(freshTimer);
      const epoch = ++visibilityEpoch;
      setCaptured(null);
      if (!document.hidden) void capture(epoch);
    }
    visibilityChanged();
    document.addEventListener("visibilitychange", visibilityChanged);
    return () => {
      disposed = true;
      clearTimeout(timer); clearTimeout(freshTimer);
      document.removeEventListener("visibilitychange", visibilityChanged);
    };
  }, [sessionId, key, active, connectionId, taskId, requestId, revision, runId, generation]);
  return {
    frame: active && captured?.key === key ? captured.frame : null,
    stale: !active || captured?.key !== key || !captured.fresh,
    error: active && failure === key,
  };
}
