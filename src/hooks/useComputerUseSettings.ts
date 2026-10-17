import { useCallback, useEffect, useRef, useState } from "react";
import * as api from "@/lib/api/computerUse";
import { isDesktopHost } from "@/lib/api/host";

type Resource<T> = { data: T | null; loading: boolean; error: boolean };
/** Independent read states keep a failed engine probe from hiding stored grants. */
function useResource<T>(load: () => Promise<T>, enabled: boolean) {
  const [state, setState] = useState<Resource<T>>({ data: null, loading: enabled, error: false });
  const generation = useRef(0);
  const pending = useRef(false);
  const refresh = useCallback(async () => {
    if (!enabled || pending.current) return;
    pending.current = true;
    const id = ++generation.current;
    setState(current => ({ ...current, loading: true, error: false }));
    try {
      const data = await load();
      if (generation.current === id) setState({ data, loading: false, error: false });
    } catch {
      if (generation.current === id) setState(current => ({ ...current, loading: false, error: true }));
    } finally {
      if (generation.current === id) pending.current = false;
    }
  }, [enabled, load]);
  useEffect(() => {
    if (enabled) void refresh();
    else setState({ data: null, loading: false, error: false });
    return () => { generation.current++; pending.current = false; };
  }, [enabled, refresh]);
  return { ...state, refresh };
}

export function useComputerUseSettings() {
  const desktop = isDesktopHost();
  const settings = useResource(api.cuSettingsGet, desktop);
  const approvals = useResource(api.cuApprovalsList, desktop);
  const doctor = useResource(api.cuDoctor, desktop && !!settings.data?.enabled);
  const [busy, setBusy] = useState(false);
  const [saveError, setSaveError] = useState(false);
  const inflight = useRef(false);
  const mounted = useRef(true);
  useEffect(() => { mounted.current = true; return () => { mounted.current = false; }; }, []);

  const probe = doctor.refresh;
  const report = doctor.data;
  useEffect(() => {
    if (!settings.data?.enabled) return;
    const focus = () => { if (document.visibilityState === "visible") void probe(); };
    window.addEventListener("focus", focus);
    // macOS permission changes take effect while this page is open. Failed
    // probes use the explicit retry; they are not silently retried forever.
    const timer = report?.capabilities.platform === "macos" && !report.ready
      ? window.setInterval(focus, 3000) : undefined;
    return () => { window.removeEventListener("focus", focus); window.clearInterval(timer); };
  }, [settings.data?.enabled, probe, report]);

  async function mutate(action: () => Promise<void>) {
    if (inflight.current || !desktop) return false;
    inflight.current = true;
    setBusy(true);
    setSaveError(false);
    let success = false;
    try { await action(); success = true; }
    catch { if (mounted.current) setSaveError(true); }
    finally {
      // Revocation can persist even when notifying the engine fails. Reload
      // both reads on errors too, so the displayed state is authoritative.
      if (mounted.current) {
        await Promise.allSettled([settings.refresh(), approvals.refresh()]);
        if (mounted.current) setBusy(false);
      }
      inflight.current = false;
    }
    return success;
  }
  return { desktop, settings, approvals, doctor, busy, saveError, mutate, clearError: () => setSaveError(false) };
}
