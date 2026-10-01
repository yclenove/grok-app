import { useEffect, useState } from "react";
import * as api from "@/lib/api/computerUse";

/** Presentation age only; never cancels a native operation or retries an action. */
const PREVIEW_FRESH_MS = 3000;

/** UI frames never become model observations; hidden panels stop all polling. */
export function useComputerPreview(sessionId: string | null, runId: string | null, targetId: string, active: boolean) {
  const [captured, setCaptured] = useState<{ key: string; frame: api.ComputerObservation } | null>(null);
  const key = JSON.stringify([sessionId, runId, targetId]);
  const [failure, setFailure] = useState<{ key: string; error: string } | null>(null);
  const [stale, setStale] = useState(true);
  useEffect(() => {
    if (!sessionId || !runId || !targetId || !active) return;
    let disposed = false;
    let revision = 0;
    let timer: ReturnType<typeof setTimeout> | undefined;
    let freshnessTimer: ReturnType<typeof setTimeout> | undefined;
    const opts = { sessionId, runId };
    async function capture(epoch: number) {
      const started = performance.now();
      try {
        const value = await api.computerObserve(opts);
        if (!disposed && epoch === revision && !document.hidden) {
          if (value) setCaptured({ key, frame: value });
          setFailure(null);
          clearTimeout(freshnessTimer);
          // Receipt time cannot establish when a slow native request captured
          // its pixels. Use the request's monotonic start as the age bound.
          const remaining = PREVIEW_FRESH_MS - (performance.now() - started);
          setStale(value === null || remaining <= 0);
          if (value && remaining > 0) {
            freshnessTimer = setTimeout(() => {
              if (!disposed && epoch === revision) setStale(true);
            }, remaining);
          }
        }
      } catch (e) {
        if (!disposed && epoch === revision) { setFailure({ key, error: String(e) }); setStale(true); }
      } finally {
        if (!disposed && epoch === revision && !document.hidden) {
          timer = setTimeout(() => void capture(epoch), 1000);
        }
      }
    }
    async function visibilityChanged() {
      const epoch = ++revision;
      clearTimeout(timer);
      clearTimeout(freshnessTimer);
      // A missing visibility acknowledgement must not leave old pixels live.
      setStale(true);
      const visible = !document.hidden;
      try {
        await api.computerSetPreview({ ...opts, visible });
        if (!disposed && epoch === revision && visible) await capture(epoch);
      } catch (e) {
        if (!disposed && epoch === revision) { setFailure({ key, error: String(e) }); setStale(true); }
      }
    }
    void visibilityChanged();
    document.addEventListener("visibilitychange", visibilityChanged);
    return () => {
      disposed = true;
      revision += 1;
      clearTimeout(timer);
      clearTimeout(freshnessTimer);
      document.removeEventListener("visibilitychange", visibilityChanged);
      void api.computerSetPreview({ ...opts, visible: false }).catch(() => {});
    };
  }, [sessionId, runId, targetId, key, active]);
  return {
    frame: captured?.key === key ? captured.frame : null,
    error: failure?.key === key ? failure.error : null,
    stale: stale || !active || captured?.key !== key,
  };
}
