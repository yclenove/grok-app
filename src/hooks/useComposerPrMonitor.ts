import * as api from "@/lib/api";
import { useMemo } from "react";
import type { Project } from "@/lib/app/sidebarModels";
import type { SessionSnapshot } from "@/lib/session";
import { prListRows } from "@/lib/prMonitor";
import { buildPrHubDeepLink } from "@/lib/prHubDeepLink";
import type { createT, Locale } from "@/i18n";
import { usePrMonitor } from "@/hooks/usePrMonitor";

/** Session-owned watches over the repo's PRs, independent of layout state. */
export function useComposerPrMonitor({
  activeProject, session, locale, showToast, tr,
}: {
  activeProject: Project | null;
  session: Pick<SessionSnapshot, "sessionId">;
  locale: Locale;
  showToast: (message: string, duration: number) => void;
  tr: ReturnType<typeof createT>;
}) {
  const prMonitor = usePrMonitor({
    enabled: api.isDesktopHost() && !!activeProject?.path,
    projectPath: activeProject?.path ?? null,
    sessionId: session.sessionId ?? null,
    locale,
    notify: (message, tone) => showToast(message, tone === "error" ? 5000 : 3200),
  });
  const prRows = useMemo(
    () => prListRows(prMonitor.prs, prMonitor.watches, { busy: prMonitor.busy, tr }),
    [prMonitor.prs, prMonitor.watches, prMonitor.busy, tr],
  );
  const openPrHubForNumber = (prNumber: number) => {
    if (typeof window === "undefined") return;
    const hash = buildPrHubDeepLink({ prNumber });
    if (window.location.hash !== hash) window.location.hash = hash;
  };
  return { prMonitor, prRows, openPrHubForNumber };
}
