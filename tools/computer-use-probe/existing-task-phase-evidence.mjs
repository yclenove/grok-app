// Private, owner-validated fixture only. No browser calls, payloads or RPC heartbeats.
import { closeSync, lstatSync, openSync, realpathSync, writeFileSync, writeSync } from "node:fs";
import path from "node:path";
import { sanitizeConnectionEvidence } from "./existing-task-connection-evidence.mjs";
import { nativeEvaluationFailure } from "./extension-native-worker.mjs";

const phases = new Set([
  "owner-validated", "context-launch", "native-version-ready", "native-install-once",
  "native-owned-context-attach", "popup-create", "source-native-session",
  "popup-navigate", "version-query", "native-source-worker-ready",
  "native-source-target-match", "native-source-worker-attach", "trusted-source-api",
  "source-popup-unpaired", "complete", "source-instrument", "owned-fixture-open",
  "pair-fill", "pair-click", "pair-ui-ready", "source-popup-unconfirmed-pair-rejected",
  "source-popup-original-pair-confirmed", "share-focus", "share-ui-ready", "share-click",
  "share-host-ready", "share-confirmed", "unshare-ui-ready", "unshare-click",
  "unshare-host-ready", "unshare-confirmed", "original-native-attachment-disposed",
  "native-source-registration-and-install-match", "native-original-worker-physically-stopped",
  "old-start-order-has-no-running-source-worker", "real-source-popup-wakes-worker-without-authority",
  "original-observation-and-browser-witness", "original-job-before-entire-context-close",
  "entire-original-context-close", "original-job-browser-members-physically-exited",
  "production-http-maintenance-original-lease", "owned-profile-validation",
  "source-api-instrument", "browser-session-ready", "owned-pages-open",
  "reused-active-tab-id-witness", "old-grant-before-replacement-pair",
  "original-host-new-pairing-challenge", "source-popup-share-new-documents",
  "stop-original-run-through-original-mcp",
  "old-run-stop-retains-replacement-browser-shares-and-tabs", "replacement-real-pair-and-shares-no-authority",
  "stop-response-decoded", "original-run-stopped", "replacement-shares-retained", "replacement-pair-retained",
  "terminal-original-call-fenced-and-joined", "terminal-zero-authority-shares-retained",
  "terminal-observe-text-denied", "terminal-observe-image-denied", "terminal-list-denied",
  "terminal-input-denied", "terminal-authority-retained", "terminal-documents-retained", "terminal-pair-retained",
  "publication-original-execution-poll-entered", "publication-original-execution-poll-suspended",
  "owned-fixture-a-create", "owned-fixture-a-navigate", "owned-fixture-a-focus", "owned-fixture-a-tab-query",
  "owned-fixture-b-create", "owned-fixture-b-navigate", "owned-fixture-b-focus", "owned-fixture-b-tab-query",
  "owned-fixture-c-create", "owned-fixture-c-navigate", "owned-fixture-c-focus", "owned-fixture-c-tab-query",
  "original-chat-stop-queued-prompt-fenced-and-joined", "original-chat-stop-stdin-fence",
  "chat-stop-zero-authority-shares-retained", "chat-stop-old-delivery-cannot-revive",
  "original-recovery-start", "original-recovery-registry-held",
  "original-recovery-concurrent-continue", "original-recovery-release-and-join",
  "original-recovery-host-interruption", "original-recovery-terminal-release-and-join", "original-recovery-old-delivery-replay",
  "old-original-observation", "original-manager-pause", "original-manager-resume",
  "original-address-duplicate-rejected", "pre-pause-ref-denied", "original-group-member-b", "original-run-stop",
  "recovered-group-b-open-request", "recovered-group-b-open-response", "recovered-group-b-old-ref-denied",
  "recovered-group-b-observe-request", "recovered-group-b-observe-response", "recovered-group-b-observe-text",
  "original-action-observe", "original-action-start", "original-action-host-interruption",
  "original-action-bind-SharedTabs", "original-action-bind-PairingClient", "original-action-bind-ActionCompletions",
  "original-action-release-and-join", "original-action-replay", "original-action-new-continue", "original-action-final-stop",
  "original-capture-source-module-arm", "original-capture-bind-SharedTabs", "original-capture-bind-PairingClient",
  "original-browser-extension-action", "original-explicit-screenshot-request",
  "original-capture-host-interruption", "original-capture-release-and-join", "original-capture-replay",
  "original-capture-new-continue", "original-capture-final-stop",
]);
const stages = new Set([
  "owned-chrome-launch", "source-mv3-start", "source-mv3-instrument", "owned-fixture-open",
  "original-app-confirm-required", "real-unshare-reshare-original-picker",
  "original-grant-fresh-observation-no-input", "entire-original-browser-exited-with-original-mcp-alive",
  "original-browser-exit-natural-lease-retires-shares", "replacement-browser-new-session-and-document",
  "terminal-original-call-fenced-and-joined", "terminal-zero-authority-shares-retained",
  "original-owned-discovery-unshare-cannot-return-stale-receipt",
  "old-run-stop-retains-replacement-browser-shares-and-tabs", "replacement-real-pair-and-shares-no-authority",
  "original-chat-stop-queued-prompt-fenced-and-joined", "chat-stop-zero-authority-shares-retained",
  "chat-stop-old-delivery-cannot-revive",
  "original-host-pause-denies-model-dispatch", "original-recovery-native-return-held",
  "concurrent-continue-retains-one-original-worker", "original-host-resume-fresh-semantic-observation-once",
  "resumed-old-refs-denied-fresh-click-deduped", "resumed-original-group-switch-retains-unselected-isolation",
  "resumed-stop-keeps-real-shares-and-tabs",
  "original-recovery-interrupted-without-authority", "original-recovery-late-return-rejected-and-joined",
  "original-recovery-old-delivery-and-shares-retained",
  "original-action-native-return-held", "original-action-interrupted-without-authority",
  "original-action-late-return-rejected-and-joined", "original-action-old-delivery-and-shares-retained",
  "original-action-new-explicit-continue", "original-action-final-stop-keeps-shares",
  "original-action-source-module-arm",
  "original-capture-source-module-arm", "original-capture-owned-action-grant",
  "original-capture-native-return-held", "original-capture-interrupted-without-authority",
  "original-capture-late-return-rejected-and-joined", "original-capture-old-delivery-and-shares-retained",
  "original-capture-new-explicit-continue", "original-capture-final-stop-keeps-shares",
]);
const fixed = (value, allowed) => allowed.has(value) ? value : "other";
const count = value => Number.isSafeInteger(value) && value >= 0 ? value : null;

export class JointPhaseEvidence {
  #profile;
  #fd;
  #started = performance.now();
  #rows = 0;
  #last = "owner-validated";
  #overflow = false;

  constructor(profile) {
    // The caller has already checked the original parent owner marker.
    for (const directory of [profile, path.dirname(profile)]) {
      if (!path.isAbsolute(directory) || realpathSync(directory) !== directory
        || !lstatSync(directory).isDirectory() || lstatSync(directory).isSymbolicLink()) {
        throw new Error("private phase evidence requires the original plain profile");
      }
    }
    this.#profile = profile;
    this.#fd = openSync(path.join(profile, "joint-phases.ndjson"), "wx");
    this.note("owner-validated");
  }

  note(phase, replacement = false, connectionEvidence = null) {
    this.#last = fixed(phase, phases);
    if (this.#rows >= 128) { this.#overflow = true; return; }
    // A synchronous local append survives a stalled browser diagnostic or Node teardown.
    writeSync(this.#fd, JSON.stringify({ phase: this.#last, replacement: replacement === true,
      elapsedMs: Math.round(performance.now() - this.#started),
      ...(connectionEvidence ? { connection: sanitizeConnectionEvidence(connectionEvidence) } : {}) }) + "\n");
    this.#rows++;
  }

  primary({ stage, detail, errorClass, errorCode, errorFamily, checks, startupEvidence, connectionEvidence, nativeEvaluation }) {
    const startup = startupEvidence;
    const row = {
      stage: fixed(stage, stages), detail: fixed(detail.replace(/^startup-/, ""), phases),
      lastPhase: this.#last, phaseRows: this.#rows, phaseOverflow: this.#overflow,
      elapsedMs: Math.round(performance.now() - this.#started), checks: count(checks.length),
      errorClass: fixed(errorClass, new Set(["AssertionError", "Error", "TypeError"])),
      errorCode: fixed(errorCode, new Set(["ENOENT", "EACCES", "EPERM", "ERR_ASSERTION"])),
      nativeEvaluation: nativeEvaluationFailure({ nativeFailure: nativeEvaluation?.kind,
        nativeExceptionClass: nativeEvaluation?.exceptionClass }),
      errorFamily: fixed(errorFamily, new Set(["timeout", "execution-context-destroyed", "worker-closed",
        "target-closed", "protocol-error", "navigation-aborted", "navigation-failed",
        "private-rpc-timeout", "postcondition-timeout", "native-worker-evaluation-failed",
        "native-worker-evaluation-timeout", "native-worker-attachment-unavailable"])),
      originalModuleCaptureTimeout: errorFamily === "original-module-capture-timeout",
      startup: { phase: fixed(startup.phase, phases), elapsedMs: count(startup.elapsedMs),
        contextClosed: startup.contextClosed === true, originalWorkerClosed: startup.originalWorkerClosed === true,
        sourceWorkerMatched: startup.sourceWorkerMatched === true,
        initialWorkerCount: count(startup.initialWorkerCount), workerEvents: count(startup.workerEvents) },
      connection: sanitizeConnectionEvidence(connectionEvidence),
    };
    // Exclusive first failure: auxiliary diagnostics cannot replace this record.
    writeFileSync(path.join(this.#profile, "joint-primary-failure.json"), JSON.stringify(row) + "\n", { flag: "wx" });
  }

  close() {
    if (this.#fd !== undefined) { closeSync(this.#fd); this.#fd = undefined; }
  }
}
