// Actual product notice/store/styles; IPC replies and native cover are synthetic.
import { createRoot } from "react-dom/client";
import { mockIPC } from "@tauri-apps/api/mocks";
import { createT, isLocale, loadLocaleCatalog } from "../../src/i18n";
import { EmbeddedBrowserCloseNotice } from "../../src/components/EmbeddedBrowserCloseNotice";
import { embeddedBrowserLifecycle as life } from "../../src/lib/embeddedBrowserLifecycle";
import { nativeWebviewCoverDepth } from "../../src/lib/nativeWebviewCover";
import "../../src/styles/tokens.css";
import "../../src/styles/skins.css";
import "../../src/styles/tailwind.css";
import "../../src/styles/app.css";

const params = new URLSearchParams(location.search);
const language = params.get("locale");
const locale = language && isLocale(language) ? language : "en";
await loadLocaleCatalog(locale);
document.documentElement.lang = locale;
document.documentElement.dataset.theme = params.get("theme") ?? "dark";
if (params.get("text") === "200") {
  for (const [key, size] of [["xs", 24], ["sm", 26], ["md", 28], ["lg", 32]] as const) {
    document.documentElement.style.setProperty(`--text-${key}`, `${size}px`);
  }
  document.documentElement.style.setProperty("--modal-title-size", "32px");
}
const tr = createT(locale);
const label = "synthetic-browser-close-owned-page";
const title = "Owned research page / " + "very-long-page-name-without-separators".repeat(5);
const calls: { command: string; label: unknown }[] = [];
let initialFailure = true;
let complete: ((success: boolean) => void) | undefined;
mockIPC(async (command, raw) => {
  if (command !== "side_browser_close" || !raw || Array.isArray(raw)
    || raw instanceof ArrayBuffer || raw instanceof Uint8Array) {
    throw new Error(`Unexpected synthetic IPC: ${command}`);
  }
  calls.push({ command, label: raw.label });
  if (raw.label !== label) throw new Error("Synthetic close must preserve the original label");
  if (initialFailure) {
    initialFailure = false;
    throw new Error("synthetic original Wry callback still pending: " + "diagnostic/".repeat(60));
  }
  return new Promise<void>((resolve, reject) => {
    if (complete) throw new Error("Synthetic fixture received concurrent native close calls");
    complete = success => {
      complete = undefined;
      if (success) resolve();
      else reject(new Error("synthetic retry still pending; original owner retained"));
    };
  });
});
async function fail() {
  const lease = life.retain(label, title);
  lease.release();
  await life.retryClose(label).catch(() => undefined);
}
const fixture = {
  labels: {
    title: tr("side.tab.browser"), failure: tr("resources.browserCloseFailed"), retry: tr("resources.browserCloseRetry"),
    closing: tr("resources.browserClosing"), later: tr("resources.browserCloseLater"),
    details: tr("resources.browserCloseDetails"), close: tr("common.close"),
  },
  inspect: () => ({ calls: [...calls], state: life.getState(label), failures: life.getFailures(),
    coverDepth: nativeWebviewCoverDepth(), pending: !!complete }),
  finish(success: boolean) {
    if (!complete) throw new Error("No original pending close reply");
    complete(success);
  },
};
(window as unknown as { __browserCloseUi: typeof fixture }).__browserCloseUi = fixture;
createRoot(document.getElementById("fixture")!).render(<>
  <button className="btn btn--ghost" onClick={() => { void fail(); }}>Create synthetic close failure</button>
  <EmbeddedBrowserCloseNotice locale={locale} />
</>);
