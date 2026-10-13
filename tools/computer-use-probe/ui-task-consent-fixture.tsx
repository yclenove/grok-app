// Product components + Tauri Channel API; all Host decisions are synthetic.
import { createRoot } from "react-dom/client";
import { mockIPC } from "@tauri-apps/api/mocks";
import type { Channel } from "@tauri-apps/api/core";
import { isLocale, loadLocaleCatalog } from "../../src/i18n";
import type { ComputerTaskView, ComputerTaskKey, ComputerTaskExecutionAddress, ComputerTaskUpload, ComputerTaskUploadCompletion } from "../../src/lib/api/computerTask";
import type { ComputerTaskEvent, ComputerTaskSubscription } from "../../src/lib/api/computerTaskEvents";
import { ComputerPanel } from "../../src/components/computer-use/ComputerPanel";
import { ComputerTaskCardLive } from "../../src/components/computer-use/ComputerTaskCardLive";
import "../../src/styles/tokens.css";
import "../../src/styles/skins.css";
import "../../src/styles/tailwind.css";
import "../../src/styles/app.css";

const params = new URLSearchParams(location.search);
const language = params.get("locale");
const locale = language && isLocale(language) ? language : "en";
await loadLocaleCatalog(locale);
document.documentElement.dataset.theme = params.get("theme") ?? "dark";
document.documentElement.lang = locale;
const root = document.getElementById("fixture")!;
root.dataset.page = params.get("page") ?? "panel";
if (params.get("text") === "200") {
  for (const [key, size] of [["xs", 24], ["sm", 26], ["md", 28], ["lg", 32]] as const) {
    root.style.setProperty(`--text-${key}`, `${size}px`);
  }
}
const sessionId = "synthetic-ui-task-chat";
const key: ComputerTaskKey = { connectionId: "synthetic-ui-connection", taskId: "synthetic-ui-task", requestId: "synthetic-ui-request", revision: 1 };
const address: ComputerTaskExecutionAddress = { key, runId: "synthetic-ui-run", generation: "18446744073709551610" };
let generation = BigInt(address.generation);
let mode = params.get("state") ?? "pending";
const long = params.has("long");
const requestedSurface = params.get("surface");
const surface = requestedSurface === "managed-browser" ? "managed-browser"
  : requestedSurface === "existing-tabs" ? "existing-tabs"
    : requestedSurface === "app-webview" ? "app-webview" : "desktop";
let existingStage = params.get("existingStage") ?? "pair";
let pairingConfirmed = false;
let existingLoads = 0;
let existingReads = 0;
let completeExisting: (() => void) | undefined;
let holdNextExistingRead = false;
let completeLoad: (() => void) | undefined;
let holdNextExistingLoad = params.has("holdLoad");
let pickerSequence = 0;
let shareSequence = 1;
let showCard = true;
const pairingExpiresAtMs = Date.now() + 60_000;
let completeExport: (() => void) | undefined;
let exportCalls = 0;
const extensionInfo = { version: "1.1.0", extensionId: "bgegbabkegkanjbmjbeaockdijnbkjgi", minimumBrowserVersion: "112", sha256: "a".repeat(64), fileCount: 44 };
const managedReply = params.get("managedReply") ?? "success";
let uploadPhase: ComputerTaskUpload["phase"] | null = params.has("upload") ? "awaiting_file" : null;
let completeUpload: ((result: ComputerTaskUploadCompletion) => void) | undefined;
let uploadRetired = false;
const uploadAction = "synthetic-file-action";
function uploadCompletion(kind: "verified" | "rejected" | "unknown"): ComputerTaskUploadCompletion {
  return { outcome: { actionId: uploadAction, runId: address.runId, kind, executed: kind === "verified",
    reason: kind === "rejected" ? "user cancelled file selection" : null }, bytes: kind === "verified" ? 0 : null };
}
let managedCalls = 0;
let completeManaged: ((success: boolean) => void) | undefined;
const purpose = long ? "Arrange the selected research editor and reference viewer for the current task; keep unrelated documents and browser tabs outside the allowed scope." : "Arrange my research editor";
const view: ComputerTaskView = {
  sessionId, connectionId: key.connectionId, taskId: key.taskId, featureEnabled: false,
  request: { key, phase: "pending", purpose, scopeIntent: long ? "Only the selected fixture scope. Do not access another app, an unshared tab or a different chat." : "Only the selected fixture", surfaces: [surface], capabilities: ["observe", "input"], remainingMs: 60_000 },
  grants: [], executions: [], pickerSurfaces: [surface],
};
function applyMode() {
  const phase = mode === "preparing" ? "preparing" : ["expired", "denied", "cancelled", "completed"].includes(mode) ? mode : ["pending", "unavailable"].includes(mode) ? "pending" : "approved";
  view.request!.phase = phase as NonNullable<ComputerTaskView["request"]>["phase"];
  const executionPhase = mode === "error" ? "unknown" : mode;
  if (["running", "paused", "resuming", "target_closed", "stop_requested", "stopped", "unknown", "error"].includes(mode)) {
    view.featureEnabled = true;
    view.grants = [{ key, runId: address.runId, members: surface === "existing-tabs" ? [
      { targetId: "synthetic-shared-tab", surface, displayName: "Shared fixture tab" },
    ] : surface === "managed-browser" ? [
      { targetId: "synthetic-owned-tab", surface, displayName: "Task browser" },
    ] : surface === "app-webview" ? [
      { targetId: "synthetic-app-page-a", surface, displayName: "App page A" },
      { targetId: "synthetic-app-page-b", surface, displayName: "App page B" },
    ] : [
      { targetId: "synthetic-editor", surface: "desktop", displayName: "Research editor" },
      { targetId: "synthetic-viewer", surface: "desktop", displayName: "Reference viewer" },
    ], capabilities: ["observe", "input"], executable: mode === "running", suspended: mode === "paused" || mode === "resuming" }];
    view.executions = [{ ...address, generation: generation.toString(), phase: mode === "running" ? "idle" : executionPhase as ComputerTaskView["executions"][number]["phase"], target: view.grants[0].members[0], targetName: "Research editor", backend: "synthetic-ui", cleanupPending: mode === "stop_requested", resumeAvailable: mode === "paused", hasFreshObservation: mode === "running", lastFailure: mode === "error" ? "outcome_unknown" : null }];
  } else { view.featureEnabled = false; view.grants = []; view.executions = []; }
  if (view.executions[0] && mode === "running" && uploadPhase) view.executions[0].upload = {
    requestId: "synthetic-file-request", actionId: uploadAction, phase: uploadPhase,
    targetName: long ? "Owned fixture upload page with a deliberately long title for narrow layout verification" : "Owned upload page", fieldName: "Attachment",
  };
  if (mode === "unavailable") view.pickerSurfaces = [];
}
applyMode();
const streams = new Map<string, { owner: ComputerTaskSubscription; channel: Channel<ComputerTaskEvent>; sequence: number }>();
const calls: string[] = [];
const pickerCalls: { command: string; payload: unknown }[] = [];
function publish() {
  for (const stream of streams.values()) stream.channel.onmessage({ ...stream.owner, kind: "view", sequence: String(++stream.sequence), view: structuredClone(view) });
}
const svg = '<svg xmlns="http://www.w3.org/2000/svg" width="1000" height="560"><rect width="1000" height="560" fill="#e8ebef"/><rect x="160" y="90" width="680" height="380" rx="12" fill="white"/><text x="220" y="160" font-size="30" font-family="sans-serif" fill="#303840">Research notes</text><path d="M220 220h460M220 270h520M220 320h410" stroke="#c7d0db" stroke-width="14"/></svg>';
mockIPC(async (command, raw) => {
  if (Array.isArray(raw) || raw instanceof ArrayBuffer || raw instanceof Uint8Array) throw new Error("Synthetic UI expects structured arguments");
  const payload = raw ?? {};
  calls.push(command);
  if (command === "computer_use_task_choose_upload" || command === "computer_use_task_cancel_upload") {
    const exact = { ...address, generation: generation.toString() };
    if (payload.sessionId !== sessionId || JSON.stringify(payload.address) !== JSON.stringify(exact)
      || payload.uploadRequestId !== "synthetic-file-request" || mode !== "running" || !uploadPhase) throw new Error("Synthetic upload requires original intent");
    pickerCalls.push({ command, payload: structuredClone(payload) });
    if (command === "computer_use_task_cancel_upload") {
      uploadRetired = true; uploadPhase = null; applyMode(); publish();
      return uploadCompletion("rejected");
    }
    if (completeUpload || uploadPhase !== "awaiting_file") throw new Error("Duplicate synthetic file picker");
    uploadPhase = "selecting"; applyMode(); publish();
    return new Promise<ComputerTaskUploadCompletion>(resolve => { completeUpload = resolve; });
  }
  if (command.startsWith("computer_use_task_picker_")) pickerCalls.push({ command, payload: structuredClone(payload) });
  if (command === "computer_use_task_subscribe") {
    const owner = { sessionId, connectionId: key.connectionId, subscriptionId: `synthetic-ui-stream-${streams.size}`, hostInstanceId: "synthetic-ui-host" };
    streams.set(owner.subscriptionId, { owner, channel: payload.events as Channel<ComputerTaskEvent>, sequence: 0 });
    queueMicrotask(publish);
    return owner;
  }
  if (command === "computer_use_task_unsubscribe") return streams.delete(String(payload.subscriptionId));
  if (command === "computer_use_task_acknowledge") return;
  if (command === "computer_use_extension_info") return extensionInfo;
  if (command === "computer_use_extension_export") {
    exportCalls++;
    if (params.get("extensionReply") === "fail-first" && exportCalls === 1) throw new Error("Synthetic disk full");
    if (params.get("extensionReply") === "hold") await new Promise<void>(resolve => { completeExport = resolve; });
    return { ...extensionInfo, path: "H:/Synthetic App data/browser-extension/owned-bundle" };
  }
  if (command === "computer_use_extension_open") return;
  if (command === "computer_use_task_picker_begin") return { sessionId, key, pickerId: `synthetic-ui-picker-${++pickerSequence}`, maxSelection: 2, surface };
  if (command === "computer_use_task_picker_preparation") {
    existingReads++;
    const pickerId = payload.pickerId;
    if (surface !== "existing-tabs" || payload.sessionId !== sessionId || pickerId !== `synthetic-ui-picker-${pickerSequence}`
      || JSON.stringify(payload.key) !== JSON.stringify(key)) throw new Error("Synthetic preparation requires original picker address");
    if (existingStage === "hold" || holdNextExistingRead) {
      holdNextExistingRead = false;
      await new Promise<void>(resolve => { completeExisting = resolve; });
    }
    if (existingStage === "fail" && existingReads === 1) throw new Error("Synthetic preparation unavailable");
    const ready = ["shared", "race"].includes(existingStage);
    return { sessionId, key, pickerId, maxSelection: 2, surface, ready,
      pairingConfirmed, availableTabs: ready ? 1 : 0, borrowedTabs: existingStage === "busy" ? 2 : 0, incompatibleTabs: existingStage === "busy" ? 1 : 0,
      pairing: ["pair", "fail"].includes(existingStage) ? { nonce: "synthetic-ui-nonce", instanceId: "synthetic-instance", verificationCode: "123456", endpoint: "http://127.0.0.1:32100", installedExtensionId: "synthetic-extension", expiresAtMs: pairingExpiresAtMs } : null };
  }
  if (command === "computer_use_task_picker_pairing_confirm") {
    if (surface !== "existing-tabs" || payload.sessionId !== sessionId || payload.pickerId !== `synthetic-ui-picker-${pickerSequence}`
      || JSON.stringify(payload.key) !== JSON.stringify(key) || payload.nonce !== "synthetic-ui-nonce") throw new Error("Synthetic Pair confirm requires original address and nonce");
    pairingConfirmed = true; return;
  }
  if (command === "computer_use_task_picker_load") {
    existingLoads++;
    const pickerId = payload.pickerId;
    if (holdNextExistingLoad) { holdNextExistingLoad = false; await new Promise<void>(resolve => { completeLoad = resolve; }); }
    const existingCandidates = existingStage === "unshared" || (existingStage === "race" && existingLoads === 1) ? [] : [
      { candidateId: `synthetic-shared-tab@${shareSequence === 1 ? "original" : "reshared"}-doc`, title: "Shared fixture tab", appName: "Chrome", kind: "tab", surface: "existing-tabs" },
    ];
    const candidates = surface === "existing-tabs" ? existingCandidates : surface === "app-webview" ? [
      { candidateId: "synthetic-app-page-a", title: "App page A", appName: "Grok", kind: "webview", surface },
      { candidateId: "synthetic-app-page-b", title: "App page B", appName: "Grok", kind: "webview", surface },
    ] : [
      { candidateId: "synthetic-editor", title: "Research editor", appName: "Owned fixture", kind: "window", surface: "desktop" },
      { candidateId: "synthetic-viewer", title: "Reference viewer", appName: "Owned fixture", kind: "window", surface: "desktop" },
    ];
    return { sessionId, key, pickerId, maxSelection: 2, surface, truncated: false, candidates };
  }
  if (command === "computer_use_task_picker_cancel") return true;
  if (command === "computer_use_task_picker_approve") {
    mode = "running"; applyMode(); publish();
    return { requestId: key.requestId, revision: key.revision, runId: address.runId, decisionClass: "synthetic-host-not-a-human-or-production-grant" };
  }
  if (command === "computer_use_task_managed_approve") {
    if (surface !== "managed-browser" || mode !== "pending" || payload.sessionId !== sessionId
      || JSON.stringify(payload.key) !== JSON.stringify(key)) throw new Error("Synthetic managed approval requires original full address");
    managedCalls++;
    if (managedReply === "hold") {
      await new Promise<void>((resolve, reject) => {
        completeManaged = (success) => { completeManaged = undefined; success ? resolve() : reject(new Error("Synthetic delayed preparation failed")); };
      });
      if (mode !== "pending") throw new Error("Synthetic managed preparation fenced by Stop/cancel");
    }
    if (managedReply === "fail-first" && managedCalls === 1) throw new Error("Synthetic preparation unavailable");
    if (managedReply !== "receipt") { mode = "running"; applyMode(); publish(); }
    return { requestId: key.requestId, revision: key.revision, runId: address.runId, decisionClass: "synthetic-host-not-a-human-or-production-grant" };
  }
  if (command === "computer_use_task_pause") { generation++; mode = "paused"; applyMode(); publish(); return; }
  if (command === "computer_use_task_resume") {
    if (mode !== "paused") throw new Error("Synthetic resume requires original paused state");
    generation++; mode = "resuming"; applyMode(); publish();
    await new Promise((resolve) => setTimeout(resolve, 180));
    if (mode !== "resuming") throw new Error("Synthetic resume fenced by Stop");
    mode = "running"; applyMode(); publish();
    return { phase: "resumed", address: { ...address, generation: generation.toString() } };
  }
  if (command === "computer_use_task_decide") {
    mode = payload.decision === "stop" ? "stopped" : payload.decision === "deny" ? "denied" : "cancelled";
    applyMode(); publish();
    return { sessionId, key, decision: payload.decision };
  }
  if (command === "computer_use_task_preview") return { address: payload.address, previewDataUrl: "data:image/svg+xml;base64," + btoa(svg), truncated: false };
  throw new Error(`Unexpected synthetic UI IPC: ${command}`);
}, { shouldMockEvents: true });
setInterval(() => { if (mode !== "stale") publish(); }, 1000);
(window as unknown as { __taskUi: { calls: string[]; pickerCalls: typeof pickerCalls; mode: (next: string) => void; completeManaged: (success: boolean) => void; share: () => void; unshare: () => void; completeLoad: () => void; closeCard: () => void; completeExisting: () => void; holdMetadata: () => void; releaseMetadata: () => void; completeExport: () => void } }).__taskUi = {
  calls, pickerCalls, mode(next) { mode = next; applyMode(); publish(); },
  completeManaged(success) { if (!completeManaged) throw new Error("No original managed preparation"); completeManaged(success); },
  share() { shareSequence++; existingStage = "shared"; },
  unshare() { existingStage = "unshared"; },
  completeLoad() { if (!completeLoad) throw new Error("No original held load"); const resolve = completeLoad; completeLoad = undefined; resolve(); },
  closeCard() { showCard = false; renderFixture(); },
  completeExisting() { if (!completeExisting) throw new Error("No original held preparation"); const resolve = completeExisting; completeExisting = undefined; existingStage = "shared"; resolve(); },
  holdMetadata() { holdNextExistingRead = true; },
  releaseMetadata() { if (!completeExisting) throw new Error("No original held metadata"); const resolve = completeExisting; completeExisting = undefined; resolve(); },
  completeExport() { if (!completeExport) throw new Error("No original held export"); const resolve = completeExport; completeExport = undefined; resolve(); },
};
const app = createRoot(root);
// UI-only synthetic completion hook. No native file dialog or user consent.
(window as unknown as { __uploadUi: { finish: (kind: "verified" | "rejected" | "unknown") => void } }).__uploadUi = {
  finish(kind) {
    if (!completeUpload) throw new Error("No original synthetic picker");
    const resolve = completeUpload; completeUpload = undefined;
    uploadPhase = null; applyMode(); publish();
    resolve(uploadCompletion(uploadRetired || mode !== "running" ? "rejected" : kind));
  },
};
function renderFixture() {
  app.render(params.get("page") === "both" ? <>
    {showCard ? <div key="card" className="fixture-card"><ComputerTaskCardLive locale={locale} sessionId={sessionId} /></div> : null}
    <div key="panel" className="fixture-panel"><ComputerPanel locale={locale} sessionId={sessionId} runId={null} /></div>
  </> : params.get("page") === "card" ? <ComputerTaskCardLive locale={locale} sessionId={sessionId} />
    : <ComputerPanel locale={locale} sessionId={sessionId} runId={null} />);
}
renderFixture();
