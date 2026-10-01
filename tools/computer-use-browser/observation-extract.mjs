import { randomUUID } from "node:crypto";
import { createObservationState, disposeObservationState } from "./observation-state.mjs";
import {
  capturePageObservationIdentity,
  invalidatePageObservation,
  pageObservationIdentityMatches,
  replacePageObservation,
} from "./page-state.mjs";
import {
  WORKER_COMPLETION_NOT_STARTED,
  WORKER_COMPLETION_UNKNOWN,
  workerException,
} from "./worker-errors.mjs";

export const OBSERVATION_LIMITS = Object.freeze({
  nodes: 64,
  candidates: 512,
  roleChars: 64,
  nameChars: 256,
  titleChars: 512,
  urlChars: 2048,
  ariaChars: 32_000,
  jsonBytes: 128 * 1024,
  pngB64Chars: 400_000,
  maxPixels: 1920 * 1080,
});

export const PNG_SIGNATURE = Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]);

export function inspectPng(bytes) {
  const buf = Buffer.isBuffer(bytes) ? bytes : Buffer.from(bytes);
  if (buf.length < 33) return { ok: false, empty: true, reason: "too_small" };
  if (!buf.subarray(0, 8).equals(PNG_SIGNATURE)) {
    return { ok: false, empty: true, reason: "bad_signature" };
  }
  if (buf.toString("ascii", 12, 16) !== "IHDR") {
    return { ok: false, empty: true, reason: "no_ihdr" };
  }
  const width = buf.readUInt32BE(16);
  const height = buf.readUInt32BE(20);
  if (width < 1 || height < 1) {
    return { ok: false, empty: true, reason: "empty", width, height };
  }
  let idat = false;
  let nonzero = false;
  let offset = 8;
  while (offset + 12 <= buf.length) {
    const len = buf.readUInt32BE(offset);
    const type = buf.toString("ascii", offset + 4, offset + 8);
    if (offset + 12 + len > buf.length) break;
    const data = buf.subarray(offset + 8, offset + 8 + len);
    if (type === "IDAT") {
      idat = true;
      for (const byte of data) {
        if (byte !== 0) {
          nonzero = true;
          break;
        }
      }
    }
    offset += 12 + len;
    if (type === "IEND") break;
  }
  if (!idat || !nonzero) {
    return { ok: false, empty: true, reason: "empty_pixels", width, height };
  }
  return { ok: true, empty: false, width, height };
}

export function modelObservationView(observation) {
  const view = { ...observation };
  delete view.pngBase64;
  delete view.pageId;
  delete view.profile;
  return view;
}

const INTERACTIVE_ROLES = Object.freeze([
  "button",
  "link",
  "textbox",
  "checkbox",
  "radio",
  "combobox",
  "option",
  "menuitem",
  "menuitemcheckbox",
  "menuitemradio",
  "tab",
  "switch",
  "slider",
  "spinbutton",
]);
const INTERACTIVE_ROLE_SET = new Set(INTERACTIVE_ROLES);

const INTERACTIVE_SELECTOR = [
  "button",
  "a[href]",
  "input:not([type='hidden'])",
  "textarea",
  "select",
  "option",
  ...INTERACTIVE_ROLES.map((role) => `[role='${role}' i]`),
  "[tabindex]:not([tabindex='-1'])",
  "[contenteditable]:not([contenteditable='false' i])",
].join(",");

function boundedText(value, limit, { trim = false, lower = false } = {}) {
  const original = typeof value === "string" ? value : String(value ?? "");
  let text = original.replace(/[\u0000-\u0008\u000b\u000c\u000e-\u001f\u007f]/g, " ");
  if (trim) text = text.trim();
  if (lower) text = text.toLowerCase();
  const truncated = text !== original || text.length > limit;
  return { text: text.slice(0, limit), truncated };
}

export function safeDisplayUrl(raw) {
  const value = String(raw || "");
  if (value === "about:blank") {
    return { url: value, hasQuery: false, truncated: false };
  }
  try {
    const parsed = new URL(value);
    if (parsed.protocol !== "http:" && parsed.protocol !== "https:") {
      return { url: "", hasQuery: false, truncated: true };
    }
    const hasQuery = Boolean(parsed.search);
    parsed.username = "";
    parsed.password = "";
    parsed.search = "";
    parsed.hash = "";
    const bounded = boundedText(parsed.toString(), OBSERVATION_LIMITS.urlChars);
    return { url: bounded.text, hasQuery, truncated: bounded.truncated };
  } catch {
    return { url: "", hasQuery: false, truncated: true };
  }
}

export function normalizeInteractiveDescriptor(raw, handle) {
  const candidate = raw && typeof raw === "object" ? raw : {};
  const roleValue = boundedText(candidate.role || "generic", OBSERVATION_LIMITS.roleChars, {
    trim: true,
    lower: true,
  });
  let role = roleValue.text;
  let roleLossy = roleValue.truncated;
  if (!role || !/^[a-z][a-z0-9_-]*$/.test(role)) {
    role = "generic";
    roleLossy = true;
  }
  const focusable = candidate.focusable === true;
  if (
    candidate.hidden === true ||
    candidate.inert === true ||
    candidate.ariaHidden === true ||
    (!INTERACTIVE_ROLE_SET.has(role) && !focusable)
  ) {
    return null;
  }
  const nameValue = boundedText(candidate.name, OBSERVATION_LIMITS.nameChars);
  const signature = boundedText(candidate.signature, 512).text;
  const publicNode = Object.freeze({
    role,
    name: nameValue.text,
    disabled: candidate.disabled === true,
    truncated: roleLossy || nameValue.truncated,
  });
  return {
    publicNode,
    target: Object.freeze({ handle, signature }),
  };
}

export async function inspectHandle(handle) {
  return handle.evaluate((element) => {
    const attribute = (name) => element.getAttribute(name) || "";
    const tag = String(element.tagName || "").toLowerCase();
    const type = attribute("type").toLowerCase();
    let role = attribute("role").trim().split(/\s+/)[0].toLowerCase();
    if (!role) {
      if (tag === "button") role = "button";
      else if (tag === "a" && attribute("href")) role = "link";
      else if (tag === "textarea" || attribute("contenteditable") === "true") role = "textbox";
      else if (tag === "select") role = "combobox";
      else if (tag === "option") role = "option";
      else if (tag === "input" && ["checkbox", "radio"].includes(type)) role = type;
      else if (tag === "input" && ["button", "submit", "reset"].includes(type)) role = "button";
      else if (tag === "input") role = "textbox";
      else role = "generic";
    }
    let name = attribute("aria-label") || attribute("alt") || attribute("placeholder");
    if (!name && element.labels && element.labels.length) {
      name = Array.from(element.labels)
        .map((label) => label.textContent || "")
        .join(" ");
    }
    if (!name) name = attribute("title") || element.textContent || "";
    const disabled =
      Boolean(element.disabled) || attribute("aria-disabled").trim().toLowerCase() === "true";
    const visibilityTarget = tag === "option" ? element.closest("select") || element : element;
    const ariaHidden = Boolean(element.closest("[aria-hidden='true' i]"));
    const inert = Boolean(element.closest("[inert]"));
    const hiddenByAttribute = Boolean(element.closest("[hidden]"));
    let rendered = true;
    try {
      rendered =
        typeof visibilityTarget.checkVisibility === "function"
          ? visibilityTarget.checkVisibility({ checkOpacity: true, checkVisibilityCSS: true })
          : Boolean(visibilityTarget.getClientRects().length);
    } catch {
      rendered = false;
    }
    const focusable =
      Number.isInteger(element.tabIndex) && element.tabIndex >= 0
        ? true
        : Boolean(element.isContentEditable);
    return {
      role,
      name,
      connected: Boolean(element.isConnected),
      disabled,
      focusable,
      hidden: hiddenByAttribute || !rendered,
      inert,
      ariaHidden,
      signature: [tag, type, role, attribute("id"), attribute("name")].join("|"),
    };
  });
}

function observationChanged(state) {
  throw workerException(
    409,
    "observation_changed_during_capture",
    "page changed while it was being observed; observe again",
    WORKER_COMPLETION_NOT_STARTED,
    state.generation,
  );
}

async function collectTargets(state, allocated) {
  const locator = state.page.locator(INTERACTIVE_SELECTOR);
  let count;
  try {
    count = await locator.count();
  } catch {
    observationChanged(state);
  }
  if (!Number.isSafeInteger(count) || count < 0) observationChanged(state);
  const scan = Math.min(count, OBSERVATION_LIMITS.candidates);
  const rows = [];
  const handles = [];
  let truncated = count > scan;
  for (let index = 0; index < scan; index += 1) {
    let handle;
    let raw;
    try {
      handle = await locator.nth(index).elementHandle();
      if (!handle) observationChanged(state);
      allocated.add(handle);
      raw = await inspectHandle(handle);
    } catch {
      observationChanged(state);
    }
    const normalized = normalizeInteractiveDescriptor(raw, handle);
    if (!normalized) continue;
    if (rows.length >= OBSERVATION_LIMITS.nodes) {
      truncated = true;
      continue;
    }
    rows.push({ target: normalized.target, ...normalized.publicNode });
    handles.push(handle);
  }
  try {
    for (const handle of handles) {
      if (!(await handle.evaluate((element) => Boolean(element && element.isConnected)))) {
        observationChanged(state);
      }
    }
  } catch {
    observationChanged(state);
  }
  return { rows, truncated };
}

function originOf(raw) {
  try {
    const parsed = new URL(String(raw || ""));
    return parsed.origin === "null" ? "" : parsed.origin;
  } catch {
    return "";
  }
}

function summarizeFrames(page, mainUrl) {
  let frames;
  try {
    frames = page.frames();
  } catch {
    return {
      mainFrameOnly: true,
      sameOriginOmitted: 0,
      crossOriginOmitted: 0,
      unavailable: true,
    };
  }
  const main = page.mainFrame();
  const mainOrigin = originOf(mainUrl);
  let sameOriginOmitted = 0;
  let crossOriginOmitted = 0;
  for (const frame of Array.isArray(frames) ? frames : []) {
    if (frame === main) continue;
    let frameOrigin = "";
    try {
      frameOrigin = originOf(frame.url());
    } catch {
      // Unknown child frames remain outside this main-frame-only observation.
    }
    if (!frameOrigin || frameOrigin === mainOrigin) sameOriginOmitted += 1;
    else crossOriginOmitted += 1;
  }
  return { mainFrameOnly: true, sameOriginOmitted, crossOriginOmitted };
}

function responseWithinLimit(response) {
  const { pngBase64, ...text } = response;
  let bytes = Buffer.byteLength(JSON.stringify(text));
  if (bytes <= OBSERVATION_LIMITS.jsonBytes) return response;
  const overflow = bytes - OBSERVATION_LIMITS.jsonBytes + 512;
  response.aria = response.aria.slice(0, Math.max(0, response.aria.length - overflow));
  response.truncated = true;
  const { pngBase64: _png, ...again } = response;
  bytes = Buffer.byteLength(JSON.stringify(again));
  if (bytes > OBSERVATION_LIMITS.jsonBytes) {
    throw workerException(
      413,
      "observation_too_large",
      "managed browser observation exceeds its transport limit",
      WORKER_COMPLETION_NOT_STARTED,
      response.pageGeneration,
    );
  }
  return response;
}

async function capturePng(page) {
  if (typeof page.screenshot !== "function") {
    return { omitted: "not_captured" };
  }
  const viewport = typeof page.viewportSize === "function" ? page.viewportSize() : page.viewport;
  if (
    viewport &&
    Number(viewport.width) * Number(viewport.height) > OBSERVATION_LIMITS.maxPixels
  ) {
    return { omitted: "size_limit" };
  }
  let buffer;
  try {
    buffer = await page.screenshot({ type: "png", scale: "css" });
  } catch {
    return { omitted: "not_captured" };
  }
  const inspected = inspectPng(buffer);
  if (!inspected.ok) {
    return { omitted: inspected.reason === "empty_pixels" ? "empty" : "not_captured" };
  }
  const pngBase64 = Buffer.from(buffer).toString("base64");
  if (pngBase64.length > OBSERVATION_LIMITS.pngB64Chars) {
    return {
      omitted: "size_limit",
      width: inspected.width,
      height: inspected.height,
    };
  }
  return {
    width: inspected.width,
    height: inspected.height,
    pngBase64,
    contentId: `img-${randomUUID()}`,
  };
}

function formatAccessibilitySnapshot(node, depth = 0) {
  if (!node || typeof node !== "object") return "";
  const role = String(node.role || "generic");
  const name = String(node.name || "").trim();
  const line = `${"  ".repeat(depth)}- ${role}${name ? ` "${name}"` : ""}`;
  const children = Array.isArray(node.children) ? node.children : [];
  const rest = children
    .map((child) => formatAccessibilitySnapshot(child, depth + 1))
    .filter(Boolean);
  return [line, ...rest].join("\n");
}

function checkCaptureCancellation(signal, completion = WORKER_COMPLETION_UNKNOWN) {
  if (signal?.aborted) throw workerException(409, "run_cancelled", "capture was cancelled", completion);
}

export async function captureManagedObservation(state, { preview = false, screenshot = true, signal } = {}) {
  checkCaptureCancellation(signal, WORKER_COMPLETION_NOT_STARTED);
  if (state.captureInFlight) {
    throw workerException(409, "observation_busy", "another capture is still in progress",
      WORKER_COMPLETION_NOT_STARTED);
  }
  state.captureInFlight = true;
  const allocated = new Set();
  const retained = new Set();
  const ownership = { observation: null, published: false, identity: null };
  try {
    let response;
    try {
      response = await captureObservation(state, { preview, screenshot, allocated, retained, ownership, signal });
    } finally {
      await Promise.allSettled([...allocated].filter(handle => !retained.has(handle))
        .map(handle => Promise.resolve().then(() => handle.dispose?.())));
    }
    // Even disposal of an omitted handle yields to other actions/lifecycle
    // events. Revalidate after the LAST await, not only after image capture.
    checkCaptureCancellation(signal);
    if (!pageObservationIdentityMatches(state, ownership.identity) || state.page.isClosed()
      || (ownership.published && state.observation !== ownership.observation)) observationChanged(state);
    return response;
  } catch (error) {
    if (ownership.observation) {
      if (state.observation === ownership.observation) invalidatePageObservation(state);
      await disposeObservationState(ownership.observation);
    }
    throw error;
  } finally {
    state.captureInFlight = false;
  }
}

async function captureObservation(state, { preview, screenshot, allocated, retained, ownership, signal }) {
  const identity = capturePageObservationIdentity(state);
  ownership.identity = identity;
  const page = state.page;
  const initialUrl = String(page.url());
  let title = "";
  let aria = "";
  let ariaUnavailable = false;
  let popup = false;
  try {
    title = await page.title();
  } catch {
    title = "";
  }
  checkCaptureCancellation(signal);
  try {
    if (typeof page.locator("body").ariaSnapshot === "function") {
      aria = await page.locator("body").ariaSnapshot();
    } else {
      const snap = await page.accessibility.snapshot({ interestingOnly: true });
      aria = formatAccessibilitySnapshot(snap);
    }
  } catch {
    ariaUnavailable = true;
  }
  checkCaptureCancellation(signal);
  try {
    popup = Boolean(await page.opener());
  } catch {
    popup = false;
  }
  checkCaptureCancellation(signal);
  const targets = await collectTargets(state, allocated);
  checkCaptureCancellation(signal);
  const finalUrl = String(page.url());
  if (
    initialUrl !== finalUrl ||
    !pageObservationIdentityMatches(state, identity) ||
    state.page.isClosed()
  ) {
    observationChanged(state);
  }

  const titleValue = boundedText(title, OBSERVATION_LIMITS.titleChars);
  const ariaValue = boundedText(aria, OBSERVATION_LIMITS.ariaChars);
  const display = safeDisplayUrl(finalUrl);
  const png = screenshot ? await capturePng(page) : { omitted: "not_requested" };
  checkCaptureCancellation(signal);
  if (!pageObservationIdentityMatches(state, identity) || state.page.isClosed()) {
    observationChanged(state);
  }
  const hasImage = Boolean(png.pngBase64);
  const observation = createObservationState(identity.pageGeneration, targets.rows,
    hasImage ? { width: png.width, height: png.height } : undefined);
  ownership.observation = observation;
  // Ownership transfers only after construction succeeds. Omitted/truncated
  // handles stay with the capture wrapper; private model handles belong here.
  for (const row of targets.rows) {
    if (!row.truncated) retained.add(row.target.handle);
  }
  const response = {
    ok: true,
    pageId: identity.pageId,
    pageGeneration: identity.pageGeneration,
    snapshotId: observation.snapshotId,
    title: titleValue.text,
    url: display.url,
    hasQuery: display.hasQuery,
    popup,
    aria: ariaValue.text,
    ariaUnavailable,
    nodes: observation.nodes,
    frameObservation: summarizeFrames(page, finalUrl),
    truncated:
      targets.truncated ||
      titleValue.truncated ||
      ariaValue.truncated ||
      display.truncated ||
      png.omitted === "size_limit",
    textOnly: !hasImage,
    image: hasImage
      ? {
          width: png.width,
          height: png.height,
          contentId: png.contentId,
        }
      : undefined,
  };
  if (hasImage) response.pngBase64 = png.pngBase64;
  else if (png.omitted) response.imageOmittedReason = png.omitted;
  responseWithinLimit(response);
  if (!pageObservationIdentityMatches(state, identity)) observationChanged(state);
  if (preview) {
    // Public preview nodes are not action authority. Their private handles are
    // retired here; the model's current snapshot stays intact.
    await disposeObservationState(observation);
  } else {
    const previous = state.observation;
    replacePageObservation(state, observation);
    ownership.published = true;
    // Backpressure: a slow disposal cannot accumulate an unbounded sequence of
    // retired captures. Existing actions release their pins in finally.
    await disposeObservationState(previous);
  }
  if (!pageObservationIdentityMatches(state, identity) || state.page.isClosed()) observationChanged(state);
  checkCaptureCancellation(signal);
  return response;
}
