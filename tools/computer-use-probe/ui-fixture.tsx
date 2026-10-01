// Development-only visual fixture: real UI, mocked IPC. Never imports App entrypoints.
import { createRoot } from "react-dom/client";
import { mockIPC } from "@tauri-apps/api/mocks";
import { isLocale, loadLocaleCatalog } from "../../src/i18n";
import type { ComputerRuntimeStatus, ComputerStatus } from "../../src/lib/api/computerUse";
import type { ComputerSurface } from "../../src/lib/computer-use/surface";
import { ComputerPanel } from "../../src/components/computer-use/ComputerPanel";
import { ComputerTaskCardLive } from "../../src/components/computer-use/ComputerTaskCardLive";
import { ComputerUseSettings } from "../../src/components/computer-use/ComputerUseSettings";
import { ExtensionsPanel } from "../../src/components/ExtensionsPanel";
import { ComputerResizeFixture } from "./ui-resize-fixture";
import { DEFAULT_LAYOUT, LAYOUT_STORAGE_KEY } from "../../src/lib/layout";
import "../../src/styles/tokens.css";
import "../../src/styles/skins.css";
import "../../src/styles/tailwind.css";
import "../../src/styles/app.css";

const params = new URLSearchParams(location.search);
const localeParam = params.get("locale");
const locale = localeParam !== null && isLocale(localeParam) ? localeParam : "en";
const mode = params.get("state") ?? "running";
const page = params.get("page") ?? "panel";
const surface = (params.get("surface") ?? "desktop") as ComputerSurface;
document.documentElement.dataset.theme = params.get("theme") ?? "dark";
document.documentElement.lang = locale;
await loadLocaleCatalog(locale);
const root = document.getElementById("fixture")!;
root.dataset.page = page;
if (page === "resize" && localStorage.getItem(LAYOUT_STORAGE_KEY) === null) {
  localStorage.setItem(LAYOUT_STORAGE_KEY, JSON.stringify({
    ...DEFAULT_LAYOUT, asideWidth: 500,
  }));
}
if (params.get("text") === "200") {
  for (const [key, size] of [["xs", 24], ["sm", 26], ["md", 28], ["lg", 32]]) {
    root.style.setProperty(`--text-${key}`, `${size}px`);
  }
}
const ready = mode === "ready" || mode === "off";
const targetName = params.get("long") ? "Research document · 文档与资料 · Sehr langer Fenstertitel / Browser / Project notes" : "Research notes · 文档";
let status: ComputerStatus = {
  runId: ready ? null : "fixture-run", targetId: ready ? null : "fixture-target",
  targetName: ready ? null : targetName, enabled: !ready,
  featureEnabled: mode !== "off", paused: mode === "paused", stopState: ready ? "stopped" : "running",
  backend: params.get("backend") ?? "fixture", notes: [], traces: [], recovery: null, timings: null, targetAlive: !ready, mcpCatalog: null,
};
if (mode === "stopping") status.stopState = "stop_requested";
if (mode === "cleanup" || mode === "cleanup-native-running") {
  status = { ...status, enabled: mode === "cleanup-native-running",
    stopState: mode === "cleanup-native-running" ? "running" : "stopped", mcpCatalog: {
    desiredGeneration: 2, appliedGeneration: 1, desiredPresent: false,
    pending: true, cleanupPending: true, lastError: "Fixture tool cleanup timed out",
  } };
}
const portrait = params.get("portrait") === "1";
const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="${portrait ? 360 : 1000}" height="${portrait ? 900 : 560}" viewBox="0 0 1000 560"><rect width="1000" height="560" fill="#f3f4f5"/><rect width="1000" height="48" fill="#e2e4e7"/><circle cx="24" cy="24" r="5" fill="#91979f"/><rect x="170" y="92" width="660" height="410" rx="8" fill="white"/><text x="220" y="148" font-family="sans-serif" font-size="26" fill="#252a31">Research notes</text><text x="220" y="184" font-family="sans-serif" font-size="16" fill="#657080">UI fixture - not a captured private window</text><path d="M220 224h430 M220 260h515 M220 296h460 M220 332h485" stroke="#e0e3e6" stroke-width="12"/><rect x="220" y="380" width="160" height="44" rx="7" fill="#27313d"/><text x="250" y="408" font-family="sans-serif" font-size="16" fill="white">Save draft</text></svg>`;
let statusReads = 0;
let targetListReads = 0;
let rejectTargetList: (() => void) | null = null;
let resolveExpiredTargetList: (() => void) | null = null;
let resolveTargetList: (() => void) | null = null;
async function listTargets<T>(rows: T[]): Promise<T[]> {
  if (!params.has("targetRefresh")) return rows;
  targetListReads += 1;
  if (targetListReads === 2) return new Promise((resolve, reject) => {
    rejectTargetList = () => reject(new Error("fixture target discovery failed"));
    resolveExpiredTargetList = () => resolve(rows);
  });
  if (targetListReads === 3) return new Promise(resolve => {
    resolveTargetList = () => resolve(rows);
  });
  return rows;
}
const calls: string[] = [];
let rejectStopReply: (() => void) | null = null;
let previewReads = 0;
let pendingPreviewStarted: number | null = null;
let resolvePreview: (() => void) | null = null;
let settingsReads = 0;
let resolveExpiredSettings: (() => void) | null = null;
let resolveSettingsRetry: (() => void) | null = null;
let runtimeMutationStarted = false;
let runtimeReadbacks = 0;
let resolveRuntimeMutation: (() => void) | null = null;
let resolveExpiredRuntime: (() => void) | null = null;
function settingsRead<T>(kind: string, value: T): Promise<T> {
  if (params.get("settingsRead") !== kind) return Promise.resolve(value);
  settingsReads += 1;
  if (settingsReads === 1) return new Promise(resolve => { resolveExpiredSettings = () => resolve(value); });
  if (settingsReads === 2) return new Promise(resolve => { resolveSettingsRetry = () => resolve(value); });
  return Promise.resolve(value);
}
function runtimeRead(value: ComputerRuntimeStatus): Promise<ComputerRuntimeStatus> {
  if (!params.has("settingsMutation") || !runtimeMutationStarted) return settingsRead("runtime", value);
  runtimeReadbacks += 1;
  if (runtimeReadbacks === 1) return new Promise(resolve => {
    resolveExpiredRuntime = () => resolve({ ...value, canRepair: false,
      issues: [{ code: "hash_mismatch", component: "expired-fixture", action: "repair" }] });
  });
  return Promise.resolve(value);
}
mockIPC(async (command, payload: any) => {
  calls.push(command);
  if (command === "skills_list") return { skills: [], skillRoots: [] };
  if (command === "inspect_mcp") return { servers: [] };
  if (command === "plugins_list") return { plugins: [] };
  if (command === "computer_use_status") {
    if (mode === "initial-unknown") throw new Error("fixture initially offline");
    if (mode === "unknown" && statusReads++ > 0) throw new Error("fixture offline");
    return page === "settings" ? settingsRead("feature", status) : status;
  }
  if (command === "computer_use_list_targets" || command === "computer_use_list_shared_tabs") {
    if (params.get("backend") === "none" && surface === "desktop") return [];
    return listTargets([{ targetId: "fixture-target", title: targetName, appName: "Fixture", kind: "window" }]);
  }
  if (command === "side_browser_list") return listTargets([{ label: "fixture-webview", url: "https://example.test/research" }]);
  if (command === "computer_use_authorize_surface") {
    const request = payload.request;
    status = { ...status, runId: "fixture-run", targetId: request.targetId, targetName,
      stopState: "running", enabled: true, targetAlive: true };
    return { ...request, runId: status.runId, target: { targetId: request.targetId, title: targetName, appName: "Fixture", kind: "window" } };
  }
  if (command === "computer_use_observe") {
    if (mode === "error") throw new Error("fixture capture failed");
    const frame = { snapshotId: `fixture-frame-${++previewReads}`, geometryRevision: 1,
      previewDataUrl: "data:image/svg+xml;base64," + btoa(svg) };
    if (mode === "preview-reply-lost" && previewReads === 2) {
      pendingPreviewStarted = performance.now();
      return new Promise(resolve => { resolvePreview = () => resolve(frame); });
    }
    return frame;
  }
  if (command === "computer_use_pause" || command === "computer_use_takeover") status = { ...status, paused: true };
  if (command === "computer_use_resume") status = { ...status, paused: false, enabled: false, recovery: "user" };
  if (command === "computer_use_stop") {
    status = { ...status, enabled: false, stopState: "stopped" };
    if (mode === "stop-reply-lost" || mode === "stop-cleanup-reply-lost") {
      if (mode === "stop-cleanup-reply-lost") status = { ...status, mcpCatalog: {
        desiredGeneration: 2, appliedGeneration: 1, desiredPresent: false,
        pending: true, cleanupPending: true, lastError: null,
      } };
      return new Promise<void>((_, reject) => {
        rejectStopReply = () => reject(new Error("late fixture Stop reply"));
      });
    }
  }
  if (params.has("settingsMutation") && ["computer_use_runtime_repair", "computer_use_runtime_rollback"].includes(command)) {
    runtimeMutationStarted = true;
    return new Promise<void>(resolve => { resolveRuntimeMutation = resolve; });
  }
  if (command === "computer_use_runtime_status") return runtimeRead({
    issues: params.get("runtime") === "missing" ? [
      "js-runtime", "browser-worker", "chromium", "playwright-runtime",
      "playwright-core-archive", "fixture-component-with-an-intentionally-long-name-to-check-wrapping",
    ].map(component => ({ code: "missing_file", component, action: "repair" })) : [],
    canRepair: true, canRollback: true,
  });
  if (command === "computer_use_export_bundle") return "C:/fixture/support.zip";
  if (command === "computer_use_begin_pairing") return { nonce: "fixture", instanceId: "fixture",
    endpoint: "http://127.0.0.1:12345", installedExtensionId: "fixture-extension",
    verificationCode: "ABCDE-12345-ABCDE-12345", expiresAtMs: Date.now() + 300000 };
  return null;
}, { shouldMockEvents: true });
(window as any).__cuUiFixture = {
  runtimeMutationStarted: () => runtimeMutationStarted, runtimeReadbacks: () => runtimeReadbacks,
  resolveRuntimeMutation: () => resolveRuntimeMutation?.(), resolveExpiredRuntime: () => resolveExpiredRuntime?.(),
  settingsReads: () => settingsReads,
  resolveExpiredSettings: () => resolveExpiredSettings?.(), resolveSettingsRetry: () => resolveSettingsRetry?.(),
  calls, rejectStopReply: () => rejectStopReply?.(), resolvePreview: () => resolvePreview?.(),
  targetListReads: () => targetListReads,
  rejectTargetList: () => rejectTargetList?.(), resolveTargetList: () => resolveTargetList?.(),
  resolveExpiredTargetList: () => resolveExpiredTargetList?.(),
  previewAgeMs: () => pendingPreviewStarted === null ? null : performance.now() - pendingPreviewStarted,
};
createRoot(root).render(page === "resize" ? <ComputerResizeFixture locale={locale} />
  : page === "settings" && params.get("shell") === "extensions"
    ? <ExtensionsPanel locale={locale} activeTab="computer" onTabChange={() => {}} />
  : page === "settings" ? <ComputerUseSettings locale={locale} />
  : page === "card" ? <ComputerTaskCardLive locale={locale} sessionId="fixture-chat" />
    : <ComputerPanel locale={locale} sessionId="fixture-chat"
      runId={mode === "initial-unknown" && params.get("knownRun") !== "0" ? "fixture-run" : null} surface={surface} />);
