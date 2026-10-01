// Failure evidence only: never grant permission, retry actions, or claim CDP cancellation.
const routes = new Set([
  "/cu/pairing-challenge", "/cu/pairing-confirm", "/cu/extension-status", "/cu/extension-disconnect",
  "/cu/browser-restart", "/cu/tab-offer", "/cu/tab-unoffer", "/cu/extension-poll", "/cu/extension-result",
  "/cu/extension-actions/negotiate", "/cu/extension-actions/poll", "/cu/extension-actions/claim", "/cu/extension-actions/result",
  "/cu/extension-completion/claim", "/cu/extension-completion/host-retirement", "/cu/extension-completion/retirement",
]);
const reasons = new Set(["http", "network", "timeout", "cancelled", "body_read", "body_limit", "decode", "json",
  "response_identity", "response_protocol", "retire_status", "retire_share", "retire_observation", "retire_action", "retire_transport"]);
const states = new Set(["", "paired", "unpaired", "shared", "unshared", "connectionFailed", "shareUnavailable",
  "invalidAddress", "invalidCode", "challengeChanged", "pairingRejected", "busy", "cancelled"]);
const statusCode = value => Number.isInteger(value) && value >= 100 && value <= 599 ? value : null;
const tail = value => Array.isArray(value) ? value.slice(-24) : [];

export async function boundedRead(task, deadlineMs = 1000) {
  if (!task) return { status: "unavailable" };
  if (!Number.isFinite(deadlineMs) || deadlineMs <= 0 || deadlineMs > 2000) throw new RangeError("invalid diagnostic deadline");
  let timer;
  const pending = Promise.resolve().then(task).then(
    value => ({ status: "ok", value }), () => ({ status: "failed" }),
  );
  try {
    return await Promise.race([pending, new Promise(resolve => {
      timer = setTimeout(() => resolve({ status: "timed_out" }), deadlineMs);
    })]);
  } finally { clearTimeout(timer); }
}

function traffic(value) {
  const clean = rows => tail(rows).filter(row => routes.has(row?.route)).map(row => ({
    route: row.route, status: statusCode(row.status), failed: row.failed === true,
  }));
  return {
    recent: clean(value?.recent), rejected: clean(value?.rejected),
    connection: tail(value?.connection).filter(row => routes.has(row?.route) && reasons.has(row?.reason)).map(row => ({
      route: row.route, reason: row.reason, status: statusCode(row.status),
      elapsedMs: Number.isSafeInteger(row.elapsedMs) && row.elapsedMs >= 0 ? row.elapsedMs : null,
    })),
  };
}

// Runs only against the exact synthetic fixture, in Chrome's isolated world.
export async function readFixtureVisibility(fixtureUrl) {
  const [tab] = await chrome.tabs.query({ active: true, lastFocusedWindow: true });
  if (tab?.url !== fixtureUrl || !Number.isSafeInteger(tab?.id)) return { fixture: false };
  try {
    const results = await chrome.scripting.executeScript({
      target: { tabId: tab.id, frameIds: [0] }, world: "ISOLATED",
      func: () => ({ visible: document.visibilityState === "visible" }),
    });
    return { fixture: true, visible: results[0]?.result?.visible === true, scriptRejected: false };
  } catch { return { fixture: true, scriptRejected: true }; }
}

export async function reportPairingFailure({ check, error, context, popup, extensionId, fixtureUrl,
  requests = [], lifecycle = [], emit = line => console.error(line), deadlineMs = 1000 }) {
  // Browser error messages/call logs can contain entered pairing credentials.
  const name = ["AssertionError", "Error", "TimeoutError", "TypeError", "RangeError"].includes(error?.name) ? error.name : "Error";
  emit("pairing-live failed at " + check + ": " + name);
  emit(JSON.stringify({ requests: tail(requests).filter(row => routes.has(row?.path))
    .map(row => ({ path: row.path, status: statusCode(row.status) })), lifecycle }));
  let worker, liveWorkers = null, openPages = null;
  try {
    const workers = context?.serviceWorkers().filter(candidate => candidate.url() === `chrome-extension://${extensionId}/sw.js`) ?? [];
    liveWorkers = workers.length;
    // Ambiguous identity is not permission to inspect the first worker.
    if (workers.length === 1) worker = workers[0];
    openPages = context?.pages().filter(page => !page.isClosed()).length ?? 0;
  } catch { /* Context can disappear during the failure; retain the primary stage. */ }
  emit(JSON.stringify({ liveWorkers, openPages }));
  const reads = {
    workerTraffic: worker && (async () => traffic(await worker.evaluate(async () => ({
      recent: (globalThis.__cuProbeHttp ?? []).slice(-24),
      rejected: (globalThis.__cuProbeHttp ?? []).filter(row => row.failed || row.status >= 400).slice(-24),
      connection: (await import("./connection-diagnostics.mjs")).readConnectionDiagnostics(),
    })))),
    popupState: popup && (async () => {
      const value = await popup.evaluate(() => ({
        state: document.querySelector("#status")?.getAttribute("data-state"),
        tabState: document.querySelector("#tab-status")?.getAttribute("data-state"),
      }));
      return { state: states.has(value?.state) ? value.state : null,
        tabState: states.has(value?.tabState) ? value.tabState : null };
    }),
    fixture: check === "explicit-share-current-tab" && worker && (async () => {
      const value = await worker.evaluate(readFixtureVisibility, fixtureUrl);
      return { fixture: value?.fixture === true, visible: value?.visible === true,
        scriptRejected: value?.scriptRejected === true };
    }),
  };
  // One concurrent diagnostic window. A timeout does NOT mean the worker exited or
  // its operation was cancelled; the owner still performs native process cleanup.
  const result = Object.fromEntries(await Promise.all(Object.entries(reads).map(async ([key, read]) =>
    [key, await boundedRead(read, deadlineMs)])));
  emit(JSON.stringify({ diagnostics: result }));
  return result;
}
