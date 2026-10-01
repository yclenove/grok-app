// Isolated browser fixture only. No real native command, extension or app entrypoint.
import { createRoot } from "react-dom/client";
import { mockIPC } from "@tauri-apps/api/mocks";
import { createT, isLocale, loadLocaleCatalog, type MessageKey } from "../../src/i18n";
import { ComputerGnomeHelper } from "../../src/components/computer-use/ComputerGnomeHelper";
import type { HelperAction, HelperState, HelperStatus } from "../../src/lib/api/computerGnomeHelper";
import "../../src/styles/tokens.css";
import "../../src/styles/skins.css";
import "../../src/styles/tailwind.css";
import "../../src/styles/app.css";

const params = new URLSearchParams(location.search);
const requestedLocale = params.get("locale");
const locale = requestedLocale && isLocale(requestedLocale) ? requestedLocale : "en";
await loadLocaleCatalog(locale);
const tr = createT(locale);
document.documentElement.lang = locale;
document.documentElement.dataset.theme = params.get("theme") ?? "dark";
// Set text tokens at the document root, including portalled confirmation content.
if (params.get("text") === "200") for (const [key, size] of [["xs", 24], ["sm", 26], ["md", 28], ["lg", 32]]) {
  document.documentElement.style.setProperty(`--text-${key}`, `${size}px`);
}
const state = (params.get("state") ?? "missing") as HelperState;
const actions: Partial<Record<HelperState, HelperAction[]>> = {
  missing: ["install"], repair_required: ["repair", "disable"],
  ready: ["disable"], disabled: ["enable", "repair", "disable"],
  restart_required: ["repair", "disable"], global_disabled: ["disable"],
  blocked: ["disable"], unconfirmed: ["disable"],
};
const stateKeys: Record<HelperState, MessageKey> = {
  missing: "cu.helper.missing", repair_required: "cu.helper.repairRequired", ready: "cu.helper.ready",
  disabled: "cu.helper.disabled", restart_required: "cu.helper.restartRequired", global_disabled: "cu.helper.globalDisabled",
  blocked: "cu.helper.blocked", unconfirmed: "cu.helper.unconfirmed", conflict: "cu.helper.conflict",
  unsupported: "cu.helper.unsupported", unavailable: "cu.helper.unavailable",
};
let status: HelperStatus = {
  state, actions: actions[state] ?? [], available: state !== "unavailable", busy: false,
  featureEnabled: params.get("feature") === "on", shellVersion: "46.0", installation: "current",
};
if (status.featureEnabled) status.actions = [];
const calls: { command: string; action?: HelperAction }[] = [];
let settle: ((reject: boolean) => void) | null = null;
let readFailure = false;
mockIPC(async (command, payload) => {
  const action = (payload as { action?: HelperAction } | undefined)?.action;
  calls.push({ command, action });
  if (command === "computer_use_helper_status") {
    if (readFailure) throw new Error("deliberate fixture read failure");
    return status;
  }
  if (command === "computer_use_helper_action") return new Promise((resolve, reject) => {
    settle = failed => {
      settle = null;
      if (failed) { reject(new Error("deliberate fixture lost mutation reply")); return; }
      const next = action === "enable" ? "ready" : action === "disable" ? "disabled" : "restart_required";
      status = { ...status, state: next, actions: actions[next] ?? [] };
      resolve(status);
    };
  });
  // GlassModal's native WebView cover is also inert in this browser-only fixture.
  if (command === "side_browser_set_cover") return null;
  throw new Error(`Unexpected fixture IPC: ${command}`);
}, { shouldMockEvents: true });
const labels = {
  install: tr("cu.helper.install"), repair: tr("cu.helper.repair"), enable: tr("cu.helper.enable"), disable: tr("cu.helper.disable"),
  cancel: tr("common.cancel"), confirm: tr("common.confirm"), refresh: tr("cu.helper.refresh"), close: tr("common.close"),
  failed: tr("cu.helper.failed"), busy: tr("cu.panel.busy"), state: tr(stateKeys[state]),
  restart: tr("cu.helper.restartRequired"), ready: tr("cu.helper.ready"), disabled: tr("cu.helper.disabled"),
};
const fixture = {
  calls, labels, state, availableActions: status.actions,
  settle: (failed = false) => { if (!settle) throw new Error("No original mutation to settle"); settle(failed); },
  failReads: (failed: boolean) => { readFailure = failed; },
};
declare global { interface Window { __cuHelperFixture: typeof fixture } }
window.__cuHelperFixture = fixture;
createRoot(document.getElementById("fixture")!).render(
  <section className="settings-card cu-settings">
    <ComputerGnomeHelper locale={locale} featureEnabled={status.featureEnabled} />
  </section>,
);
