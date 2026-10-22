import { useEffect, useRef, useState } from "react";
import { isDesktopHost } from "@/lib/api/host";
import { NativeSoakQueues, startNativeSoakTelemetry } from "@/lib/nativeSoakTelemetry";

/** Desktop-only, native-authorized diagnostics. Never routes through the
 * mirror transport. A false/failed probe allocates no timers, RAF or counters.
 */
export function useNativeSoakTelemetry(): NativeSoakQueues {
  const [queues] = useState(() => new NativeSoakQueues());
  const probe = useRef<Promise<typeof import("@tauri-apps/api/core") | null> | null>(null);
  useEffect(() => {
    if (!isDesktopHost()) return;
    let cancelled = false;
    let stop: (() => void) | undefined;
    // Reuse the same probe during StrictMode effect replay; stale replies do
    // not activate the obsolete effect or create a second sampler.
    probe.current ??= import("@tauri-apps/api/core").then(async (core) =>
      await core.invoke<boolean>("native_soak_telemetry_enabled") === true ? core : null,
    ).catch(() => null);
    void probe.current.then((core) => {
      if (cancelled || !core || !isDesktopHost()) return;
      stop = startNativeSoakTelemetry(queues, (sample) =>
        core.invoke<void>("native_soak_telemetry_sample", { sample }),
      );
    });
    return () => { cancelled = true; stop?.(); };
  }, [queues]);
  return queues;
}
