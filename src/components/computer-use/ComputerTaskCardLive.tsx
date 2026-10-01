import { useEffect, useState } from "react";
import type { Locale } from "@/i18n";
import { computerStatus, type ComputerStatus } from "@/lib/api/computerUse";
import { COMPUTER_STATUS_STALE_MS, computerTaskView } from "@/lib/computer-use/taskCard";
import { ComputerTaskCard } from "./ComputerTaskCard";

export function ComputerTaskCardLive(props: { locale: Locale; sessionId: string | null }) {
  return <SessionTaskCard key={props.sessionId} {...props} />;
}

function SessionTaskCard({
  locale,
  sessionId,
}: {
  locale: Locale;
  sessionId: string | null;
}) {
  const [status, setStatus] = useState<ComputerStatus | null>(null);
  const [fresh, setFresh] = useState(true);
  useEffect(() => {
    if (!sessionId) return;
    let cancelled = false;
    let timer: ReturnType<typeof setTimeout>;
    let watchdog: ReturnType<typeof setTimeout>;
    async function poll() {
      watchdog = setTimeout(() => { if (!cancelled) setFresh(false); }, COMPUTER_STATUS_STALE_MS);
      try {
        const value = await computerStatus({ sessionId, runId: null });
        if (!cancelled) { setStatus(value); setFresh(true); }
      } catch {
        // Keep the exact last-known run stoppable; never keep a green state.
        if (!cancelled) setFresh(false);
      } finally {
        clearTimeout(watchdog);
        if (!cancelled) timer = setTimeout(() => void poll(), 800);
      }
    }
    void poll();
    return () => { cancelled = true; clearTimeout(timer); clearTimeout(watchdog); };
  }, [sessionId]);
  const view = status ? computerTaskView(status) : null;
  if (!view || !status?.runId) return null;
  return (
    <ComputerTaskCard
      key={status.runId}
      locale={locale}
      sessionId={sessionId}
      runId={status.runId}
      targetName={view.targetName}
      backend={view.backend}
      state={fresh ? view.state : "unknown"}
      stopConfirmed={fresh && status.stopState === "stopped"}
      failure={view.failure}
    />
  );
}
