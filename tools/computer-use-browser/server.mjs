#!/usr/bin/env node
/**
 * Managed Playwright worker. Isolated persistent profile; one owner per profile.
 * Loopback only. Not a root runtime dependency.
 */
import http from "node:http";
import {
  existsSync,
  mkdirSync,
  realpathSync,
  rmSync,
} from "node:fs";
import { dirname, join, resolve, relative } from "node:path";
import { fileURLToPath } from "node:url";
import { clientAllowed } from "./loopback.mjs";
import { sanitizeDownloadFilename } from "./filename.mjs";
import { captureManagedObservation } from "./observation-extract.mjs";
import {
  capturePageStateIdentity,
  currentPageObservation,
  invalidatePageObservation,
  publishPageState,
  pageStateIdentityMatches,
  registerPageState as registerPage,
  runPageMutation,
} from "./page-state.mjs";
import { preflightTypedAct, prepareTypedAct } from "./typed-act.mjs";
import { disposeObservationState, withObservationLease } from "./observation-state.mjs";
import { downloadViaHandle, prepareDownloadTarget } from "./download.mjs";
import { beginAction, captureRecoveryObservation, finishAction, unlockUnknownIfIdle } from "./worker-ledger.mjs";
import { browserPidLookup } from "./browser-pids.mjs";
import { pinnedChromeLaunchPath } from "./immutable-chrome.mjs";
import { prepareManagedPreferences, profileName, runOwner } from "./profile.mjs";
import { RunOperations } from "./run-operations.mjs";
import { InputCleanup } from "./input-cleanup.mjs";
import { ContextClose, waitForCleanup } from "./context-close.mjs";
import { navigatePage } from "./navigation.mjs";
import {
  WORKER_COMPLETION_NOT_STARTED,
  WORKER_COMPLETION_UNKNOWN,
  defaultWorkerCode,
  workerErrorBody,
  workerException,
  workerExceptionSpec,
} from "./worker-errors.mjs";

const ROOT = dirname(fileURLToPath(import.meta.url));
const PORT = Number(process.env.GROK_CU_BROWSER_PORT || 0);
const TOKEN = process.env.GROK_CU_BROWSER_TOKEN || "";
const MAX_BODY = 1024 * 1024;
const PROFILE_ROOT = process.env.GROK_CU_BROWSER_PROFILE_ROOT
  ? process.env.GROK_CU_BROWSER_PROFILE_ROOT
  : join(ROOT, ".run", "profiles");
const STAGING_ROOT = resolve(
  process.env.GROK_CU_BROWSER_STAGING_ROOT || join(PROFILE_ROOT, ".staging"),
);
const MAX_DOWNLOAD = 8 * 1024 * 1024;

if (!TOKEN) {
  process.stderr.write("GROK_CU_BROWSER_TOKEN required\n");
  process.exit(2);
}

function chromePath() {
  if (process.env.GROK_CU_CHROME && existsSync(process.env.GROK_CU_CHROME)) {
    return process.env.GROK_CU_CHROME;
  }
  if (process.env.GROK_CU_ALLOW_SYSTEM_CHROME === "1") {
    const candidates = [
      "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe",
      "C:\\Program Files (x86)\\Google\\Chrome\\Application\\chrome.exe",
      "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
      "/usr/bin/google-chrome-stable",
      "/usr/bin/google-chrome",
      "/usr/bin/chromium-browser",
      "/usr/bin/chromium",
    ];
    return candidates.find((p) => existsSync(p)) || null;
  }
  return null;
}

async function loadPlaywright() {
  return import("playwright-core");
}

function json(res, code, obj, operation) {
  if (code < 400) operation?.check();
  const payload = code >= 400 ? workerErrorBody(code, obj) : obj;
  const body = JSON.stringify(payload);
  res.writeHead(code, {
    "content-type": "application/json",
    "content-length": Buffer.byteLength(body),
  });
  res.end(body);
}

function runStaging(owner) {
  return join(STAGING_ROOT, "computer-use-staging", runOwner(owner));
}

function underRunStaging(p, owner) {
  try {
    const root = runStaging(owner);
    if (!existsSync(root) || !existsSync(String(p || ""))) return false;
    const full = realpathSync(String(p));
    const canonicalRoot = realpathSync(root);
    const rel = relative(canonicalRoot, full);
    return Boolean(rel) && !rel.startsWith("..") && !rel.startsWith("/") && !rel.startsWith("\\");
  } catch {
    return false;
  }
}

function urlAllowed(raw) {
  const u = String(raw || "").trim();
  if (u.toLowerCase() === "about:blank") return true;
  let parsed;
  try {
    parsed = new URL(u);
  } catch {
    return false;
  }
  if (parsed.protocol !== "http:" && parsed.protocol !== "https:") return false;
  if (parsed.username || parsed.password) return false;
  const host = parsed.hostname.replace(/\.$/, "");
  if (
    host === "169.254.169.254" ||
    host === "100.100.100.200" ||
    host === "metadata.google.internal" ||
    host.endsWith(".metadata.google.internal")
  ) {
    return false;
  }
  if (parsed.protocol === "http:" && host !== "127.0.0.1" && host !== "localhost") {
    return false;
  }
  return true;
}

/** @type {Map<string, BrowserSlot>} */
const owners = new Map();
const operations = new RunOperations();
const openingProfiles = new Set();
const cancellingRuns = new Map();
let shutdownPromise = null;
const RUN_ROUTES = new Set(["/open", "/tabs", "/new-tab", "/frames", "/goto", "/upload", "/observe", "/act", "/download", "/close"]);

function forgetSlot(profile, slot) {
  if (owners.get(profile) === slot) owners.delete(profile);
}

async function retireRunObservations(owner) {
  const retired = [];
  for (const slot of owners.values()) {
    if (slot.owner !== owner) continue;
    for (const page of slot.pages.values()) {
      const observation = currentPageObservation(page);
      invalidatePageObservation(page);
      retired.push(disposeObservationState(observation));
    }
  }
  await Promise.all(retired);
}

/**
 * @typedef {object} PageState
 * @property {string} id
 * @property {import("playwright-core").Page} page
 * @property {number} generation
 * @property {string} lastUrl
 * @property {object | null} observation
 * @property {boolean} published
 * @property {boolean} closed
 * @property {boolean} identityExhausted
 * @property {number} frameRevision
 */

/**
 * @typedef {object} BrowserSlot
 * @property {string} owner
 * @property {import("playwright-core").BrowserContext} context
 * @property {string} dir
 * @property {Map<string, PageState>} pages
 * @property {WeakMap<import("playwright-core").Page, string>} pageIds
 * @property {Map<string, { fingerprint: string, state: "pending" | "done" | "rejected" | "unknown", result?: object, error?: string, code?: string, controller: AbortController }>} actions
 */

function createSlot(owner, context, dir) {
  const slot = {
    owner,
    context,
    dir,
    pages: new Map(),
    pageIds: new WeakMap(),
    actions: new Map(),
    inFlight: null,
    writeBlockedUntilObserve: false,
    readBrowserPid: null,
    inputCleanup: new InputCleanup(context),
    closure: new ContextClose(context, () => operations.holdResourceCleanup(owner, context)),
  };
  context.on("page", (page) => registerPage(slot, page));
  for (const page of context.pages()) registerPage(slot, page);
  return slot;
}

async function pageResult(state) {
  publishPageState(state);
  let popup = false;
  try {
    popup = Boolean(await state.page.opener());
  } catch {
    popup = false;
  }
  return {
    pageId: state.id,
    pageGeneration: state.generation,
    url: state.page.url(),
    popup,
  };
}

function requirePage(got, body) {
  const pageId = String(body.pageId || "");
  const pageGeneration = Number(body.pageGeneration);
  if (!pageId || pageId.length > 128) {
    return { error: { code: 400, error: "pageId required" } };
  }
  if (!Number.isSafeInteger(pageGeneration) || pageGeneration < 1) {
    return { error: { code: 400, error: "pageGeneration must be a positive integer" } };
  }
  const state = got.slot.pages.get(pageId);
  if (!state || state.page.isClosed()) {
    return { error: { code: 404, error: "page is closed or unknown" } };
  }
  if (state.generation !== pageGeneration) {
    return {
      error: {
        code: 409,
        error: "stale page generation; observe the page again",
        pageId,
        pageGeneration: state.generation,
      },
    };
  }
  return { state };
}

function checkedPage(got, body) {
  const selected = requirePage(got, body);
  if (selected.error) {
    throw workerException(
      selected.error.code,
      selected.error.errorCode || defaultWorkerCode(selected.error.code),
      selected.error.error,
      WORKER_COMPLETION_NOT_STARTED,
      selected.error.pageGeneration,
    );
  }
  return selected.state;
}

function actionId(body) {
  const value = String(body.actionId || "");
  if (!value || value.length > 128 || !/^[A-Za-z0-9._:-]+$/.test(value)) {
    throw workerException(400, "invalid_action_id", "valid actionId required");
  }
  return value;
}

async function runAction(got, body, operation, parentSignal) {
  got.slot.inputCleanup.check();
  if (got.slot.writeBlockedUntilObserve) {
    throw workerException(
      409,
      "unknown_quarantine",
      "previous action outcome is unknown; observe before continuing",
      WORKER_COMPLETION_UNKNOWN,
    );
  }
  const id = actionId(body);
  const started = beginAction(got.slot, body, id);
  if (started.replay) return started.replay;
  const { entry } = started;
  try {
    const signal = AbortSignal.any([entry.controller.signal, parentSignal]);
    if (signal.aborted) throw workerException(409, "run_cancelled", "worker operation was cancelled");
    const result = await operation(signal);
    if (signal.aborted) throw workerException(409, "run_cancelled", "worker operation was cancelled", WORKER_COMPLETION_UNKNOWN);
    finishAction(got.slot, id, { state: "done", result });
    return result;
  } catch (error) {
    const spec = workerExceptionSpec(error);
    if (spec.completion === WORKER_COMPLETION_NOT_STARTED) {
      finishAction(got.slot, id, {
        state: "rejected",
        error: spec.message,
        code: spec.code,
      });
    } else {
      finishAction(got.slot, id, {
        state: "unknown",
        error: spec.message,
        code: spec.code,
      });
      got.slot.writeBlockedUntilObserve = true;
      for (const page of got.slot.pages.values()) {
        invalidatePageObservation(page);
      }
    }
    throw workerException(
      spec.status,
      spec.code,
      spec.message,
      spec.completion,
      spec.currentPageGeneration,
    );
  }
}

function requireOwner(body, { allowClosing = false } = {}) {
  let profile;
  let owner;
  try {
    profile = profileName(body.profile);
    owner = runOwner(body.owner);
  } catch (error) {
    return { error: { code: 400, error: error.message } };
  }
  const slot = owners.get(profile);
  if (!slot) return { error: { code: 404, error: "profile not open" } };
  if (slot.owner !== owner) {
    return { error: { code: 409, error: "profile owned by another run" } };
  }
  if (!allowClosing) slot.closure.check();
  return { profile, owner, slot };
}

function forbiddenBrowserAct(body) {
  const kind = String(body.kind || "").toLowerCase();
  if (
    [
      "evaluate",
      "eval",
      "cdp",
      "js",
      "script",
      "run_code",
      "browser_run_code_unsafe",
    ].includes(kind)
  ) {
    return "evaluate/cdp is forbidden";
  }
  for (const key of ["script", "expression", "evaluate", "cdp", "function", "js"]) {
    if (body[key] != null) return `${key} is forbidden`;
  }
  return null;
}

function bearerOk(req) {
  return String(req.headers.authorization || "") === `Bearer ${TOKEN}`;
}

const server = http.createServer(async (req, res) => {
  const ra = req.socket.remoteAddress;
  if (!clientAllowed(ra)) {
    json(res, 403, { error: "loopback only" });
    return;
  }
  if (!bearerOk(req)) {
    json(res, 401, { error: "unauthorized" });
    return;
  }
  let requestOperation;
  try {
    const chunks = [];
    let total = 0;
    try {
      for await (const c of req) {
        total += c.length;
        if (total > MAX_BODY) {
          json(res, 413, { error: "body too large" });
          return;
        }
        chunks.push(c);
      }
    } catch {
      throw workerException(400, "invalid_request_body", "request body could not be read completely");
    }
    let body;
    try {
      body = JSON.parse(Buffer.concat(chunks).toString("utf8") || "{}");
    } catch {
      throw workerException(400, "invalid_json", "valid JSON body required");
    }
    const url = new URL(req.url || "/", "http://127.0.0.1");
    if (!body || typeof body !== "object" || Array.isArray(body)) {
      throw workerException(400, "invalid_request", "object body required");
    }
    if (RUN_ROUTES.has(url.pathname)) requestOperation = operations.begin(body.owner, body.runRevision);
    if (["/pause-run", "/resume-run", "/run-status"].includes(url.pathname)) {
      if (String(req.headers["x-grok-cu-host"] || "") !== "1") {
        throw workerException(403, "forbidden", "run lifecycle is a Host operation");
      }
      let state;
      if (url.pathname === "/pause-run") {
        operations.pause(body.owner, body.runRevision);
        const cleanup = operations.holdCleanup(body.owner);
        try { await retireRunObservations(body.owner); }
        finally { cleanup.finish(); }
        state = operations.status(body.owner, body.runRevision);
      } else if (url.pathname === "/resume-run") {
        state = operations.resume(body.owner, body.runRevision, body.nextRunRevision);
      } else {
        state = operations.status(body.owner, body.runRevision);
      }
      json(res, 200, { ok: true, ...state });
      return;
    }
    if (url.pathname === "/health") {
      json(res, 200, {
        ok: true,
        protocol: 1,
        build: "computer-use-browser",
        runtime: process.version,
        sourceHash: process.env.GROK_CU_WORKER_SOURCE_SHA256 || "",
        playwrightVersion: process.env.GROK_CU_PLAYWRIGHT_VERSION || "",
        browserVersion: process.env.GROK_CU_BROWSER_VERSION || "",
        page: { observe: true, observeOptions: 1, act: true, waitCondition: 1, navigate: true },
        runLifecycle: 1,
        playwright: true,
        chrome: chromePath(),
        openProfiles: [...owners.keys()],
      });
      return;
    }
    if (url.pathname === "/open") {
      let profile;
      let owner;
      try {
        profile = profileName(body.profile);
        owner = runOwner(body.owner);
      } catch (error) {
        json(res, 400, { error: error.message });
        return;
      }
      const existing = owners.get(profile);
      if (existing && existing.owner !== owner) {
        json(res, 409, { error: "profile owned by another run" });
        return;
      }
      if (existing && existing.owner === owner) {
        existing.closure.check();
        const primary = existing.pages.values().next().value;
        if (!primary) {
          json(res, 409, { error: "profile has no live page" });
          return;
        }
        json(res, 200, {
          ok: true,
          profile,
          owner,
          reused: true,
          targetId: `pw:${profile}`,
          dir: existing.dir,
          ...(await pageResult(primary)),
        }, requestOperation);
        return;
      }
      if (openingProfiles.has(profile)) throw workerException(409, "profile_opening", "profile is currently opening");
      openingProfiles.add(profile);
      let slot;
      try {
        const sourceExe = chromePath();
        if (!sourceExe) {
          json(res, 501, { error: "chrome not found" });
          return;
        }
        // Execute a copy when the binary is the hash-locked pack. Chromium
        // otherwise creates debug.log and Dictionaries beside chrome.exe.
        const exe = pinnedChromeLaunchPath(sourceExe, resolve(PROFILE_ROOT, ".chrome-exec"));
        const pw = await loadPlaywright();
        requestOperation.check();
        const dir = join(PROFILE_ROOT, profile);
        mkdirSync(dir, { recursive: true });
        prepareManagedPreferences(dir);
        const context = await pw.chromium.launchPersistentContext(dir, {
          executablePath: exe,
          headless: true,
          acceptDownloads: true,
          // Unified headless and a profile-owned log avoid writes beside the
          // immutable Chrome binary with Playwright 1.48 on Windows.
          args: ["--disable-gpu", "--disable-dev-shm-usage", "--disable-crash-reporter",
            "--headless=new", `--log-file=${join(dir, "chrome-debug.log")}`],
        });
        // Own the context before the next await, including a failed newPage.
        slot = createSlot(owner, context, dir);
        slot.readBrowserPid = browserPidLookup(context, { executablePath: exe, userDataDir: dir });
        owners.set(profile, slot);
        await slot.readBrowserPid();
        requestOperation.check();
        const page = context.pages()[0] || (await context.newPage());
        const primary = registerPage(slot, page);
        json(res, 200, {
          ok: true, profile, owner, reused: false,
          targetId: `pw:${profile}`, dir, ...(await pageResult(primary)),
        }, requestOperation);
      } catch (error) {
        if (slot) {
          // Preserve uncertain ownership until closure is actually confirmed.
          await slot.closure.close();
          forgetSlot(profile, slot);
        }
        throw error;
      } finally {
        openingProfiles.delete(profile);
      }
      return;
    }
    if (url.pathname === "/tabs") {
      const got = requireOwner(body);
      if (got.error) {
        json(res, got.error.code, { error: got.error.error });
        return;
      }
      const pages = await Promise.all(
        got.slot.context.pages().map((page) => pageResult(registerPage(got.slot, page))),
      );
      json(res, 200, { ok: true, profile: got.profile, pages }, requestOperation);
      return;
    }
    if (url.pathname === "/new-tab") {
      const got = requireOwner(body);
      if (got.error) {
        json(res, got.error.code, { error: got.error.error });
        return;
      }
      if (!body.kind) body.kind = "new_tab";
      const result = await runAction(got, body, async (signal) => {
        if (signal.aborted) throw new Error("cancelled");
        const page = await got.slot.context.newPage();
        return { ok: true, profile: got.profile, ...(await pageResult(registerPage(got.slot, page))) };
      }, requestOperation.signal);
      json(res, 200, result, requestOperation);
      return;
    }
    if (url.pathname === "/frames") {
      const got = requireOwner(body);
      if (got.error) {
        json(res, got.error.code, { error: got.error.error });
        return;
      }
      const selected = requirePage(got, body);
      if (selected.error) {
        json(res, selected.error.code, selected.error);
        return;
      }
      const frames = selected.state.page.frames().map((f) => f.url());
      json(res, 200, {
        ok: true,
        profile: got.profile,
        ...(await pageResult(selected.state)),
        frames,
      }, requestOperation);
      return;
    }
    if (url.pathname === "/clear") {
      if (String(req.headers["x-grok-cu-host"] || "") !== "1") {
        json(res, 403, { error: "clear profile is a Host operation" });
        return;
      }
      let profile;
      try {
        profile = profileName(body.profile);
      } catch (error) {
        json(res, 400, { error: error.message });
        return;
      }
      if (openingProfiles.has(profile)) throw workerException(409, "profile_opening", "profile is currently opening");
      const slot = owners.get(profile);
      if (slot) {
        requestOperation = operations.holdCleanup(slot.owner);
        for (const action of slot.actions.values()) action.controller.abort();
        await slot.closure.close();
        forgetSlot(profile, slot);
      }
      if (openingProfiles.has(profile) || owners.has(profile)) {
        throw workerException(409, "profile_changed", "profile ownership changed during cleanup");
      }
      const dir = join(PROFILE_ROOT, profile);
      rmSync(dir, { recursive: true, force: true });
      json(res, 200, { ok: true, cleared: profile });
      return;
    }
    if (url.pathname === "/goto") {
      const got = requireOwner(body);
      if (got.error) {
        json(res, got.error.code, { error: got.error.error });
        return;
      }
      const href = String(body.url || "");
      if (!urlAllowed(href)) {
        json(res, 400, { error: "url must be loopback http or https without credentials" });
        return;
      }
      if (!body.kind) body.kind = "navigate";
      const result = await runAction(got, body, async (signal) => {
        const selected = checkedPage(got, body);
        return runPageMutation(selected, async () => {
          if (signal.aborted) throw new Error("cancelled");
          await navigatePage(selected.page, { url: href, signal });
          if (signal.aborted) throw new Error("cancelled");
          const landed = selected.page.url();
          if (!urlAllowed(landed)) {
            throw Object.assign(new Error("redirect landed on a forbidden origin"), {
              statusCode: 400,
            });
          }
          return { ok: true, profile: got.profile, ...(await pageResult(selected)) };
        });
      }, requestOperation.signal);
      json(res, 200, result, requestOperation);
      return;
    }
    if (url.pathname === "/upload") {
      const got = requireOwner(body);
      if (got.error) {
        json(res, got.error.code, { error: got.error.error });
        return;
      }
      if (body.selector) {
        json(res, 400, { error: "selector locators are not a production worker route" });
        return;
      }
      if (!body.kind) body.kind = "upload";
      const filePath = String(body.path || "");
      if (!underRunStaging(filePath, got.owner)) {
        json(res, 400, { error: "upload must be a Host-authorized staging file" });
        return;
      }
      const result = await runAction(got, body, async (signal) => {
        const selected = checkedPage(got, body);
        const observation = currentPageObservation(selected);
        return withObservationLease(observation, body, async () => {
          const target = prepareDownloadTarget(observation, body);
          return runPageMutation(selected, async () => {
            if (signal.aborted) throw new Error("cancelled");
            await target.handle.setInputFiles(filePath);
            if (signal.aborted) throw new Error("cancelled");
            return { ok: true, profile: got.profile, ...(await pageResult(selected)) };
          }, observation);
        });
      }, requestOperation.signal);
      json(res, 200, result, requestOperation);
      return;
    }
    if (url.pathname === "/observe") {
      if ((body.preview !== undefined && typeof body.preview !== "boolean")
        || (body.screenshot !== undefined && typeof body.screenshot !== "boolean")) {
        json(res, 400, { error: "preview and screenshot must be booleans" });
        return;
      }
      const got = requireOwner(body);
      if (got.error) {
        json(res, got.error.code, { error: got.error.error });
        return;
      }
      const selected = checkedPage(got, body);
      const recovery = captureRecoveryObservation(got.slot);
      const observation = await captureManagedObservation(selected, {
        preview: body.preview === true, screenshot: body.screenshot !== false,
        signal: requestOperation.signal,
      });
      if (body.preview !== true) unlockUnknownIfIdle(got.slot, recovery);
      json(res, 200, observation, requestOperation);
      return;
    }
    if (url.pathname === "/act") {
      const got = requireOwner(body);
      if (got.error) {
        json(res, got.error.code, { error: got.error.error });
        return;
      }
      const blocked = forbiddenBrowserAct(body);
      if (blocked) {
        json(res, 400, { error: blocked });
        return;
      }
      const kind = String(body.kind || "click");
      try {
        const result = await runAction(got, body, async (signal) => {
          const selected = checkedPage(got, body);
          const observation = currentPageObservation(selected);
          // Validate the wire schema before admission so a malformed request
          // remains a 400, not a misleading stale-observation conflict.
          const observed = !["reload", "navigate"].includes(preflightTypedAct(kind, body));
          const perform = async () => {
            const identity = capturePageStateIdentity(selected);
            const prepared = await prepareTypedAct({
              page: selected.page,
              observation,
              kind,
              body,
              signal,
              checkIdentity: () => {
                if (!pageStateIdentityMatches(selected, identity)) {
                  throw workerException(409, "stale_drag_document", "original drag document changed");
                }
              },
            });
            const operation = async () => {
              await prepared.dispatch();
              if (signal.aborted) throw new Error("cancelled");
              return {
                ok: true,
                profile: got.profile,
                kind: prepared.kind,
                ...(await pageResult(selected)),
              };
            };
            // Wait is read-only but still participates in cancellation, action
            // deduplication and exclusive admission. Keep its model handles live.
            return prepared.kind === "wait" ? operation()
              : runPageMutation(selected, operation, observed ? observation : undefined);
          };
          return observed ? withObservationLease(observation, body, perform) : perform();
        }, requestOperation.signal);
        json(res, 200, result, requestOperation);
      } catch (e) {
        if (workerExceptionSpec(e).code === "input_cleanup_pending" && requestOperation) {
          got.slot.inputCleanup.retain(requestOperation);
          requestOperation = undefined;
        }
        throw e instanceof Error ? e : new Error(String(e));
      }
      return;
    }
    if (url.pathname === "/download") {
      if (body.path) {
        json(res, 400, { error: "model-supplied filesystem path is not trusted" });
        return;
      }
      const got = requireOwner(body);
      if (got.error) {
        json(res, got.error.code, { error: got.error.error });
        return;
      }
      if (!body.kind) body.kind = "download";
      const filename = sanitizeDownloadFilename(body.filename);
      if (!filename) {
        json(res, 400, { error: "invalid download filename" });
        return;
      }
      const dir = runStaging(got.owner);
      mkdirSync(dir, { recursive: true });
      const dest = join(dir, filename);
      const result = await runAction(got, body, async (signal) => {
        const selected = checkedPage(got, body);
        const observation = currentPageObservation(selected);
        return withObservationLease(observation, body, async () => {
          const target = prepareDownloadTarget(observation, body);
          return runPageMutation(selected, async () => {
            const bytes = await downloadViaHandle({
              page: selected.page,
              target,
              destPath: dest,
              maxBytes: MAX_DOWNLOAD,
              urlAllowed,
              signal,
            });
            if (signal.aborted) throw new Error("cancelled");
            return {
              ok: true,
              ...(await pageResult(selected)),
              path: dest,
              bytes,
            };
          }, observation);
        });
      }, requestOperation.signal);
      json(res, 200, result, requestOperation);
      return;
    }
    if (url.pathname === "/pids") {
      if (String(req.headers["x-grok-cu-host"] || "") !== "1") {
        json(res, 403, { error: "pids is a Host operation" });
        return;
      }
      const identities = await Promise.all([...owners.entries()].map(async ([profile, slot]) =>
        ({ profile, slot, pid: await slot.readBrowserPid() })));
      const browserPids = identities.filter(row => row.pid && owners.get(row.profile) === row.slot)
        .map(row => row.pid);
      json(res, 200, { ok: true, workerPid: process.pid, browserPids });
      return;
    }
    if (url.pathname === "/shutdown") {
      // The supervisor allows two seconds for a graceful reply. A pending
      // response must leave the worker alive for reconciliation/owned teardown.
      await waitForCleanup(shutdown(), 1500);
      json(res, 200, { ok: true, shutdown: true });
      setImmediate(() => { server.close(); process.exit(0); });
      return;
    }
    if (url.pathname === "/cancel-run") {
      if (String(req.headers["x-grok-cu-host"] || "") !== "1") {
        json(res, 403, { error: "cancel run is a Host operation" });
        return;
      }
      let owner;
      try {
        owner = runOwner(body.owner);
      } catch (error) {
        json(res, 400, { error: error.message });
        return;
      }
      const closedProfiles = await waitForCleanup(cancelRun(owner));
      json(res, 200, { ok: true, owner, closedProfiles });
      return;
    }
    if (url.pathname === "/close") {
      const got = requireOwner(body, { allowClosing: true });
      if (got.error) {
        json(res, got.error.code, { error: got.error.error });
        return;
      }
      for (const action of got.slot.actions.values()) action.controller.abort();
      await waitForCleanup(got.slot.closure.close().then(() => forgetSlot(got.profile, got.slot)));
      json(res, 200, { ok: true, closed: got.profile }, requestOperation);
      return;
    }
    json(res, 404, { error: "not found" });
  } catch (e) {
    // A disconnected caller cannot receive an envelope. Physical cleanup keeps
    // its resource lease beyond this response; socket closure is not completion.
    if (req.aborted || res.destroyed) return;
    const error = workerExceptionSpec(e);
    json(res, error.status, { error });
  } finally {
    requestOperation?.finish();
  }
});

server.listen(PORT, "127.0.0.1", () => {
  const addr = server.address();
  process.stdout.write(`${JSON.stringify({ port: addr.port })}\n`);
});

function cancelRun(owner) {
  const existing = cancellingRuns.get(owner);
  if (existing) return existing;
  const pending = finishCancelRun(owner);
  cancellingRuns.set(owner, pending);
  // A fulfilled native close may later gain its missing close event. A rejected
  // native close stays owned; ContextClose never redispatches it or any action.
  void pending.catch(() => {
    if (cancellingRuns.get(owner) === pending) cancellingRuns.delete(owner);
  });
  return pending;
}

async function finishCancelRun(owner) {
  operations.stop(owner);
  const slots = [...owners.entries()].filter(([, slot]) => slot.owner === owner);
  for (const [, slot] of slots) {
    for (const action of slot.actions.values()) action.controller.abort();
  }
  const closed = await Promise.allSettled(slots.map(async ([profile, slot]) => {
    await slot.closure.close();
    forgetSlot(profile, slot);
  }));
  if (closed.some(result => result.status === "rejected")) {
    throw workerException(500, "run_cleanup_pending", "browser cleanup failed", WORKER_COMPLETION_UNKNOWN);
  }
  // Launches admitted before the fence cannot publish success after Stop.
  // The response deadline does not cancel this cleanup or its physical leases.
  await operations.whenIdle(owner);
  for (const [profile, slot] of owners) {
    if (slot.owner !== owner) continue;
    await slot.closure.close();
    forgetSlot(profile, slot);
  }
  return slots.map(([profile]) => profile);
}

function shutdown() {
  if (shutdownPromise) return shutdownPromise;
  const pending = finishShutdown();
  shutdownPromise = pending;
  void pending.catch(() => {
    if (shutdownPromise === pending) shutdownPromise = null;
  });
  return pending;
}

async function finishShutdown() {
  operations.stopAll();
  for (const slot of owners.values()) {
    for (const action of slot.actions.values()) action.controller.abort();
  }
  const closed = await Promise.allSettled([...owners.values()].map((slot) => slot.closure.close()));
  if (closed.some(result => result.status === "rejected")) {
    throw workerException(500, "run_cleanup_pending", "browser cleanup failed", WORKER_COMPLETION_UNKNOWN);
  }
  await operations.whenAllIdle();
  // Include contexts that were still launching when shutdown fenced the run.
  await Promise.all([...owners.values()].map(slot => slot.closure.close()));
  owners.clear();
}
process.on("SIGINT", async () => {
  await shutdown();
  process.exit(0);
});
process.on("SIGTERM", async () => {
  await shutdown();
  process.exit(0);
});
