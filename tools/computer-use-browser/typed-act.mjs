import { inspectHandle } from "./observation-extract.mjs";
import { navigatePage } from "./navigation.mjs";
import { dragDestination, dragObservedTargets, observedDragPoint } from "./typed-drag.mjs";
import {
  assertObservationIdentity,
  resolveObservationTarget,
} from "./observation-state.mjs";
import {
  WORKER_COMPLETION_NOT_STARTED,
  workerException,
} from "./worker-errors.mjs";

export const TEXT_MAX_CHARS = 4000;
export const KEY_MAX_LEN = 32;
export const SCROLL_DELTA_MIN = -2400;
export const SCROLL_DELTA_MAX = 2400;
export const BASIC_ACT_KINDS = Object.freeze([
  "click",
  "set_value",
  "type_text",
  "select",
  "key",
]);
export const SPACE_ACT_KINDS = Object.freeze(["scroll", "drag"]);

const ELEMENT_KINDS = new Set([
  "click",
  "set_value",
  "fill",
  "type_text",
  "type",
  "select",
]);
const SNAPSHOT_KINDS = new Set([...ELEMENT_KINDS, "key", "scroll", "drag", "wait"]);
const DUMMY_REFS = new Set(["dummy", "page", "none", "null", "undefined"]);

const KEY_MAP = Object.freeze({
  enter: "Enter",
  return: "Enter",
  tab: "Tab",
  escape: "Escape",
  esc: "Escape",
  space: " ",
  down: "ArrowDown",
  up: "ArrowUp",
  left: "ArrowLeft",
  right: "ArrowRight",
  backspace: "Backspace",
  delete: "Delete",
  home: "Home",
  end: "End",
  pageup: "PageUp",
  page_up: "PageUp",
  pagedown: "PageDown",
  page_down: "PageDown",
});

export function isDummyElementRef(value) {
  if (value == null) return false;
  const text = String(value).trim();
  if (!text) return true;
  return DUMMY_REFS.has(text.toLowerCase());
}

export function normalizeActKind(kind) {
  const value = String(kind || "").trim().toLowerCase();
  if (value === "fill") return "set_value";
  if (value === "type") return "type_text";
  return value;
}

function actText(body) {
  if (typeof body.text === "string") return body.text;
  if (typeof body.value === "string") return body.value;
  return null;
}

export function preflightTypedAct(kind, body = {}) {
  const normalized = normalizeActKind(kind);
  if (!body || typeof body !== "object" || Array.isArray(body)) {
    throw workerException(400, "invalid_request", "act body required");
  }
  if (!Number.isSafeInteger(body.pageGeneration) || body.pageGeneration < 1) {
    throw workerException(
      400,
      "invalid_page_generation",
      "pageGeneration must be a positive safe integer",
    );
  }
  const snapshotId = String(body.snapshotId || "");
  if (SNAPSHOT_KINDS.has(normalized) && !snapshotId) {
    throw workerException(400, "snapshot_required", "snapshotId required");
  }
  if (ELEMENT_KINDS.has(normalized)) {
    if (body.elementRef == null || String(body.elementRef).trim() === "") {
      throw workerException(400, "element_ref_required", "elementRef required");
    }
    if (isDummyElementRef(body.elementRef)) {
      throw workerException(400, "dummy_element_ref", "dummy elementRef is not allowed");
    }
  }
  if (normalized === "key") {
    if (body.elementRef != null && String(body.elementRef).trim() !== "") {
      if (isDummyElementRef(body.elementRef)) {
        throw workerException(400, "dummy_element_ref", "dummy elementRef is not allowed");
      }
    }
    const key = String(body.key || "");
    if (!key.trim() || key.length > KEY_MAX_LEN) {
      throw workerException(400, "invalid_key", "key required");
    }
    if (!KEY_MAP[key.trim().toLowerCase()]) {
      throw workerException(400, "invalid_key", "key is not in the allowed set");
    }
  }
  if (normalized === "set_value" || normalized === "type_text") {
    const text = actText(body);
    if (typeof text !== "string") {
      throw workerException(400, "invalid_text", "text required");
    }
    if ([...text].length > TEXT_MAX_CHARS || text.includes("\0")) {
      throw workerException(400, "invalid_text", "invalid text length or NUL");
    }
  }
  if (normalized === "select") {
    const value = body.value ?? body.text;
    if (value == null || String(value) === "") {
      throw workerException(400, "invalid_select_value", "select value required");
    }
  }
  if (normalized === "scroll" || normalized === "drag") {
    if (body.elementRef != null && String(body.elementRef).trim() !== "") {
      if (isDummyElementRef(body.elementRef)) {
        throw workerException(400, "dummy_element_ref", "dummy elementRef is not allowed");
      }
    }
  }
  if (normalized === "scroll") {
    const delta = body.delta ?? body.dy;
    if (!Number.isInteger(delta) || delta < SCROLL_DELTA_MIN || delta > SCROLL_DELTA_MAX) {
      throw workerException(400, "invalid_scroll_delta", "integer delta required");
    }
  }
  if (normalized === "drag") {
    const destRef = dragDestination(body).elementRef;
    if (destRef != null && String(destRef).trim() !== "" && isDummyElementRef(destRef)) {
      throw workerException(400, "dummy_element_ref", "dummy elementRef is not allowed");
    }
    if (body.elementRef == null || String(body.elementRef).trim() === "") {
      throw workerException(400, "element_ref_required", "elementRef required");
    }
  }
  if (normalized === "wait") {
    if (body.elementRef == null || String(body.elementRef).trim() === "") {
      throw workerException(400, "element_ref_required", "elementRef required");
    }
    if (isDummyElementRef(body.elementRef)) {
      throw workerException(400, "dummy_element_ref", "dummy elementRef is not allowed");
    }
    const name = body.nameEquals;
    if (typeof name !== "string" || !name.trim() || name.length > 256) {
      throw workerException(400, "invalid_wait_condition", "nameEquals required");
    }
    if (body.timeoutMs !== undefined) {
      const ms = Number(body.timeoutMs);
      if (!Number.isInteger(ms) || ms < 1 || ms > 10_000) {
        throw workerException(400, "invalid_timeout", "timeoutMs must be between 1 and 10000");
      }
    }
  }
  if (normalized === "reload" || normalized === "navigate") {
    if (body.elementRef != null && String(body.elementRef).trim() !== "") {
      throw workerException(
        400,
        "element_ref_forbidden",
        "navigate/reload must not use an elementRef",
      );
    }
  }
  if (normalized === "navigate") {
    const raw = String(body.url || "").trim();
    if (!raw) {
      throw workerException(400, "invalid_navigate_url", "navigate url required");
    }
  }
  if (
    !ELEMENT_KINDS.has(normalized) &&
    normalized !== "key" &&
    normalized !== "scroll" &&
    normalized !== "drag" &&
    normalized !== "wait" &&
    normalized !== "reload" &&
    normalized !== "navigate"
  ) {
    throw workerException(400, "unsupported_act", `unsupported act ${normalized}`);
  }
  return normalized;
}

function abortableDelay(ms, signal) {
  return new Promise((resolve, reject) => {
    if (!signal) {
      setTimeout(resolve, ms);
      return;
    }
    if (signal.aborted) {
      reject(new Error("cancelled"));
      return;
    }
    const timer = setTimeout(done, ms);
    function done() {
      signal.removeEventListener("abort", aborted);
      resolve();
    }
    function aborted() {
      clearTimeout(timer);
      reject(new Error("cancelled"));
    }
    signal.addEventListener("abort", aborted, { once: true });
  });
}

async function assertHandleLive(target) {
  if (!target || target.handle == null) {
    throw workerException(
      409,
      "stale_element_ref",
      "element reference is not in the current snapshot",
      WORKER_COMPLETION_NOT_STARTED,
    );
  }
  let raw;
  try {
    const connected = await target.handle.evaluate((element) =>
      Boolean(element && element.isConnected),
    );
    if (!connected) {
      throw workerException(
        409,
        "stale_element_ref",
        "element is no longer connected",
        WORKER_COMPLETION_NOT_STARTED,
      );
    }
    raw = await inspectHandle(target.handle);
  } catch (error) {
    if (error && error.workerCode) throw error;
    throw workerException(
      409,
      "stale_element_ref",
      "element handle is not current",
      WORKER_COMPLETION_NOT_STARTED,
    );
  }
  if (raw.connected === false) {
    throw workerException(409, "stale_element_ref", "element is no longer connected");
  }
  if (target.signature && raw.signature !== target.signature) {
    throw workerException(
      409,
      "stale_element_signature",
      "element signature changed; observe the page again",
      WORKER_COMPLETION_NOT_STARTED,
    );
  }
  return raw;
}

function clickOptions(body) {
  const options = {};
  const button = String(body.button || "left").toLowerCase();
  if (button === "right" || button === "middle" || button === "left") {
    options.button = button;
  }
  if (Number(body.count) === 2) options.clickCount = 2;
  return options;
}

function mapKey(raw) {
  const mapped = KEY_MAP[String(raw || "").trim().toLowerCase()];
  if (!mapped) {
    throw workerException(400, "invalid_key", "key is not in the allowed set");
  }
  return mapped;
}

function tagOf(raw) {
  return String(raw?.signature || "").split("|")[0] || "";
}

async function typeAppend(handle, text) {
  await handle.focus();
  try {
    await handle.press("End");
  } catch {
    /* some controls ignore End */
  }
  if (typeof handle.type === "function") {
    await handle.type(text, { delay: 10 });
    return;
  }
  await handle.pressSequentially(text, { delay: 10 });
}

async function scrollBy(handle, page, delta) {
  if (handle) {
    await handle.evaluate((element, dy) => {
      if (typeof element.scrollBy === "function") element.scrollBy(0, dy);
      else element.scrollTop = (Number(element.scrollTop) || 0) + dy;
    }, delta);
    return;
  }
  if (typeof page.evaluate === "function") {
    await page.evaluate((dy) => {
      window.scrollBy(0, dy);
    }, delta);
    return;
  }
  await page.mouse.wheel(0, delta);
}

async function executeTypedAct({ page, handle, live, kind, body, signal, observation, observedTarget }) {
  if (kind === "click") {
    await handle.click(clickOptions(body));
    return;
  }
  if (kind === "set_value") {
    if (tagOf(live) === "select" || tagOf(live) === "option") {
      throw workerException(
        400,
        "set_value_not_supported_on_select",
        "use select for select/option",
      );
    }
    await handle.fill(actText(body));
    return;
  }
  if (kind === "type_text") {
    if (tagOf(live) === "select" || tagOf(live) === "option") {
      throw workerException(
        400,
        "type_text_not_supported_on_select",
        "use select for select/option",
      );
    }
    await typeAppend(handle, actText(body));
    return;
  }
  if (kind === "select") {
    const tag = tagOf(live);
    if (tag !== "select" && tag !== "option") {
      throw workerException(
        400,
        "select_target_invalid",
        "select only operates select/option",
      );
    }
    const value = String(body.value ?? body.text ?? "");
    try {
      if (tag === "option") {
        await handle.evaluate((element, selected) => {
          const option = element;
          const select = option.closest("select");
          if (!select) throw new Error("option has no select");
          option.selected = true;
          select.value = selected;
          select.dispatchEvent(new Event("input", { bubbles: true }));
          select.dispatchEvent(new Event("change", { bubbles: true }));
        }, value);
      } else {
        await handle.selectOption(value);
      }
    } catch (error) {
      if (error && error.workerCode) throw error;
      throw workerException(
        400,
        "select_failed",
        "selectOption failed without fill fallback",
      );
    }
    return;
  }
  if (kind === "key") {
    const mapped = mapKey(body.key);
    if (handle) await handle.press(mapped);
    else await page.keyboard.press(mapped);
    return;
  }
  if (kind === "scroll") {
    const delta = Number(body.delta ?? body.dy);
    await scrollBy(handle, page, delta);
    return;
  }
  if (kind === "wait") {
    const want = String(body.nameEquals);
    const timeout = Math.min(Number(body.timeoutMs ?? 2000), 10_000);
    const deadline = performance.now() + timeout;
    for (;;) {
      if (signal?.aborted) throw new Error("cancelled");
      assertObservationIdentity(observation, body);
      const raw = await assertHandleLive(observedTarget);
      // Re-check after the async DOM read: navigation, re-observe or mutation
      // must not turn a retired reference into a successful condition.
      assertObservationIdentity(observation, body);
      if (signal?.aborted) throw new Error("cancelled");
      if (raw.hidden || raw.inert || raw.ariaHidden) {
        throw workerException(409, "wait_target_hidden", "wait target is no longer visible");
      }
      if (performance.now() >= deadline) {
        throw workerException(
          400,
          "wait_timeout",
          "wait condition was not met before timeout",
          WORKER_COMPLETION_NOT_STARTED,
        );
      }
      if (String(raw.name || "") === want) return;
      await abortableDelay(Math.min(50, Math.max(1, deadline - performance.now())), signal);
    }
  }
  if (kind === "reload") {
    if (typeof page.reload !== "function") {
      throw workerException(400, "unsupported_act", "reload is unavailable");
    }
    await navigatePage(page, { kind: "reload", signal });
    return;
  }
  if (kind === "navigate") {
    throw workerException(
      400,
      "unsupported_act",
      "navigate uses the /goto worker route",
    );
  }
  throw workerException(400, "unsupported_act", `unsupported act ${kind}`);
}

export async function prepareTypedAct({ page, observation, kind, body, signal, checkIdentity }) {
  const normalized = preflightTypedAct(kind, body);
  if (signal?.aborted) throw new Error("cancelled");
  const wantsElement =
    ELEMENT_KINDS.has(normalized) ||
    normalized === "drag" ||
    normalized === "wait" ||
    ((normalized === "key" || normalized === "scroll") &&
      body.elementRef != null &&
      String(body.elementRef).trim() !== "");
  let handle = null;
  let observedTarget = null;
  let destHandle = null;
  let destTarget = null;
  let destPoint = null;
  let live = null;
  if (wantsElement) {
    const target = resolveObservationTarget(observation, {
      pageGeneration: body.pageGeneration,
      snapshotId: body.snapshotId,
      elementRef: body.elementRef,
    });
    observedTarget = target;
    live = await assertHandleLive(target);
    const tag = tagOf(live);
    if (normalized === "select" && tag !== "select" && tag !== "option") {
      throw workerException(
        400,
        "select_target_invalid",
        "select only operates select/option",
      );
    }
    if (
      (normalized === "set_value" || normalized === "type_text") &&
      (tag === "select" || tag === "option")
    ) {
      throw workerException(
        400,
        `${normalized}_not_supported_on_select`,
        "use select for select/option",
      );
    }
    handle = target.handle;
  } else if (normalized === "reload" || normalized === "navigate") {
    /* navigation identity is pageGeneration + actionId; no DOM snapshot */
  } else {
    assertObservationIdentity(observation, {
      pageGeneration: body.pageGeneration,
      snapshotId: body.snapshotId,
    });
  }
  if (normalized === "drag") {
    const destination = dragDestination(body);
    if (destination.elementRef !== undefined) {
      destTarget = resolveObservationTarget(observation, {
        pageGeneration: body.pageGeneration,
        snapshotId: body.snapshotId,
        elementRef: destination.elementRef,
      });
      await assertHandleLive(destTarget);
      destHandle = destTarget.handle;
    } else {
      destPoint = observedDragPoint(observation, body, destination);
    }
  }
  return {
    kind: normalized,
    async dispatch() {
      if (signal?.aborted) throw new Error("cancelled");
      if (normalized === "drag") {
        return dragObservedTargets({ page, source: handle, destination: destHandle, point: destPoint,
          signal, checkIdentity: checkIdentity ?? (() => assertObservationIdentity(observation, body)),
          checkTargets: async () => {
            for (const target of [observedTarget, destTarget].filter(Boolean)) {
              const current = await assertHandleLive(target);
              if (current.hidden || current.inert || current.ariaHidden || current.disabled) {
                throw workerException(409, "drag_target_invalid", "drag target is not available");
              }
            }
          },
        });
      }
      await executeTypedAct({
        page,
        handle,
        live,
        kind: normalized,
        body,
        signal,
        observation,
        observedTarget,
      });
    },
  };
}
