import { useCallback, useEffect, useRef, useState } from "react";
import * as api from "@/lib/api";
import { parseExternalQueueSnapshot, type ExternalQueueSnapshot } from "@/lib/externalQueueSnapshot";
import type { ExternalQueuePush, ExternalQueueTake } from "@/lib/sendQueue";

export type ExternalQueueSync = {
  enabled: boolean; ready: boolean; loading: boolean; error: boolean; refresh: () => void;
};
type Options = {
  acceptExternal: boolean; sessionKey: string;
  onPush: (value: ExternalQueuePush) => void;
  onTake: (value: ExternalQueueTake) => void;
  onSnapshot: (snapshot: ExternalQueueSnapshot) => void;
};
const POLL_MS = 5_000;
const COALESCE_MS = 100;
/** Observe durable Host ownership. No dispatch, cancel, claim-release or retry-send capability. */
export function useExternalSendQueue(options: Options) {
  const enabled = options.acceptExternal && api.hasHost();
  const callbacks = useRef(options); callbacks.current = options;
  const [state, setState] = useState({ ready: false, loading: false, error: false });
  const stateRef = useRef(state);
  const enabledRef = useRef(enabled); enabledRef.current = enabled;
  const refreshRef = useRef<() => void>(() => {});
  const refresh = useCallback(() => refreshRef.current(), []);
  const canFlush = useCallback(() =>
    !enabledRef.current || (stateRef.current.ready && !stateRef.current.error), []);

  useEffect(() => {
    let cancelled = false;
    const update = (patch: Partial<typeof state>) => {
      if (cancelled) return;
      stateRef.current = { ...stateRef.current, ...patch };
      setState(stateRef.current);
    };
    update({ ready: false, loading: enabled, error: false });
    if (!enabled) return;
    let generation = 0;
    let registered = false;
    let inFlight = false;
    let dirty = false;
    let attempt = 0;
    let unPush: (() => void) | undefined;
    let unTake: (() => void) | undefined;
    let unReconnect: (() => void) | undefined;
    let timer: ReturnType<typeof setTimeout> | undefined;
    let retryTimer: ReturnType<typeof setTimeout> | undefined;
    let watchdog: ReturnType<typeof setTimeout> | undefined;
    let registrationWatchdog: ReturnType<typeof setTimeout> | undefined;
    const disposeListeners = () => {
      unPush?.(); unPush = undefined;
      unTake?.(); unTake = undefined;
      unReconnect?.(); unReconnect = undefined;
    };
    const schedule = (delay: number) => {
      if (cancelled) return;
      clearTimeout(timer);
      timer = setTimeout(() => {
        timer = undefined;
        if (document.visibilityState === "hidden") schedule(POLL_MS);
        else void readSnapshot();
      }, delay);
    };
    const readSnapshot = async () => {
      if (cancelled || !registered) return;
      if (inFlight) { dirty = true; return; }
      inFlight = true; dirty = false;
      const observed = generation;
      update({ ready: false, loading: true });
      watchdog = setTimeout(() => {
        update({ ready: false, loading: false, error: true });
        // IPC cannot be cancelled. Keep single flight until it actually settles.
      }, POLL_MS);
      try {
        const snapshot = parseExternalQueueSnapshot(await api.sessionQueueSnapshot());
        if (cancelled) return;
        if (observed !== generation) { dirty = true; return; }
        callbacks.current.onSnapshot(snapshot);
        update({ ready: true, error: false });
      } catch {
        update({ ready: false, error: true });
      } finally {
        clearTimeout(watchdog); watchdog = undefined; inFlight = false;
        if (!cancelled) {
          update({ loading: false });
          schedule(dirty ? COALESCE_MS : POLL_MS);
        }
      }
    };
    const request = () => {
      if (cancelled) return;
      generation += 1;
      update({ ready: false });
      if (inFlight) dirty = true;
      else if (registered) { clearTimeout(timer); void readSnapshot(); }
    };
    const changed = () => {
      generation += 1;
      update({ ready: false });
      if (inFlight) dirty = true;
      else if (registered) schedule(COALESCE_MS);
    };
    const register = async () => {
      registrationWatchdog = setTimeout(() => update({ error: true, loading: false }), POLL_MS);
      try {
        const push = await api.listen<ExternalQueuePush>("session://send_queue", value => {
          if (cancelled) return;
          changed(); callbacks.current.onPush(value);
        });
        if (cancelled) { push(); return; }
        unPush = push;
        const take = await api.listen<ExternalQueueTake>("session://send_queue_take", value => {
          if (cancelled) return;
          changed(); callbacks.current.onTake(value);
        });
        if (cancelled) { take(); return; }
        unTake = take;
        if (api.isMirrorClient()) {
          const reconnect = await api.listen("mirror://reconnected", request);
          if (cancelled) { reconnect(); return; }
          unReconnect = reconnect;
        }
        registered = true;
        void readSnapshot();
      } catch {
        disposeListeners();
        if (!cancelled) {
          update({ ready: false, error: true, loading: false });
          retryTimer = setTimeout(() => void register(), Math.min(POLL_MS, 250 * 2 ** Math.min(attempt++, 4)));
        }
      } finally {
        clearTimeout(registrationWatchdog); registrationWatchdog = undefined;
      }
    };
    const onVisible = () => { if (document.visibilityState !== "hidden") request(); };
    refreshRef.current = request;
    window.addEventListener("focus", request);
    document.addEventListener("visibilitychange", onVisible);
    void register();
    return () => {
      cancelled = true; refreshRef.current = () => {};
      clearTimeout(timer); clearTimeout(retryTimer); clearTimeout(watchdog); clearTimeout(registrationWatchdog);
      window.removeEventListener("focus", request);
      document.removeEventListener("visibilitychange", onVisible);
      disposeListeners();
    };
  }, [enabled]);
  const previousKey = useRef(options.sessionKey);
  useEffect(() => {
    if (previousKey.current !== options.sessionKey) {
      previousKey.current = options.sessionKey; refresh();
    }
  }, [options.sessionKey, refresh]);
  return { enabled, ...state, refresh, canFlush };
}
