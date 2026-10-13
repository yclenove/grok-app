import { useCallback, useEffect, useRef, useState, useSyncExternalStore } from "react";
import { embeddedBrowserLifecycle } from "@/lib/embeddedBrowserLifecycle";

export function useEmbeddedBrowserRecovery(label: string) {
  const [bootRevision, setBootRevision] = useState(0);
  const retryRef = useRef<{ label: string; operation: Promise<void> } | null>(null);
  const mountedLabel = useRef<{ label: string } | null>(null);
  useEffect(() => {
    mountedLabel.current = { label };
    return () => { mountedLabel.current = null; };
  }, [label]);
  const getSnapshot = useCallback(() => embeddedBrowserLifecycle.getState(label), [label]);
  const closeState = useSyncExternalStore(
    embeddedBrowserLifecycle.subscribe, getSnapshot, embeddedBrowserLifecycle.getServerState,
  );
  const retryClose = useCallback(() => {
    if (retryRef.current?.label === label) return;
    const originalLabel = label;
    const originalMount = mountedLabel.current;
    const operation = embeddedBrowserLifecycle.retryClose(originalLabel);
    retryRef.current = { label, operation };
    void operation.then(() => {
      if (originalMount && mountedLabel.current === originalMount) setBootRevision(value => value + 1);
    }, () => undefined).finally(() => {
      if (retryRef.current?.operation === operation) retryRef.current = null;
    });
  }, [label]);
  return { closeState, bootRevision, retryClose };
}
