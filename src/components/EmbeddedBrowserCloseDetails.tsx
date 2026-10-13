import { createT, type Locale } from "@/i18n";
import "@/styles/embedded-browser-close.css";

/** Keep recovery diagnostics available without crowding the actionable message. */
export function EmbeddedBrowserCloseDetails({ locale, error }: { locale: Locale; error: string | null }) {
  const tr = createT(locale);
  return (
    <details className="embedded-browser-close__details">
      <summary>{tr("resources.browserCloseDetails")}</summary>
      <p>{error}</p>
    </details>
  );
}
