import { useSyncExternalStore } from "react";
import { createT, type Locale } from "@/i18n";
import { embeddedBrowserLifecycle } from "@/lib/embeddedBrowserLifecycle";
import { GlassModal } from "@/components/GlassModal";
import { Spinner } from "@/components/ui/spinner";
import { EmbeddedBrowserCloseDetails } from "@/components/EmbeddedBrowserCloseDetails";

/** A close failure remains actionable after its tab has already unmounted. */
export function EmbeddedBrowserCloseNotice({ locale }: { locale: Locale }) {
  const tr = createT(locale);
  const failures = useSyncExternalStore(
    embeddedBrowserLifecycle.subscribe,
    embeddedBrowserLifecycle.getFailures,
    embeddedBrowserLifecycle.getServerFailures,
  );
  const current = failures[0];
  if (!current) return null;
  const busy = current.phase === "closing";
  const later = () => embeddedBrowserLifecycle.deferFailure(current.label, current.attempt);
  return (
    <GlassModal
      open title={tr("side.tab.browser")} size="sm" wrapBody className="embedded-browser-close"
      onClose={later} closeLabel={tr("common.close")}
      footer={<>
        <button type="button" className="btn btn--ghost" onClick={later}>
          {tr("resources.browserCloseLater")}
        </button>
        <button type="button" className="btn btn--primary" disabled={busy} aria-busy={busy}
          onClick={() => { void embeddedBrowserLifecycle.retryClose(current.label).catch(() => undefined); }}>
          {busy ? <Spinner size={14} /> : null}
          {tr(busy ? "resources.browserClosing" : "resources.browserCloseRetry")}
        </button>
      </>}
    >
      <p className="embedded-browser-close__failure" role="status">{tr("resources.browserCloseFailed")}</p>
      <p className="embedded-browser-close__page">{current.title}</p>
      <p>{tr("resources.browserCloseRetained")}</p>
      <EmbeddedBrowserCloseDetails locale={locale} error={current.error} />
    </GlassModal>
  );
}
