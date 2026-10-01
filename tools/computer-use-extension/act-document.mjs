// Fixed isolated-world executor. Host transport must own dispatch/cancellation before exposing it.
// DOM actions are semantic operations, not trusted OS/keyboard input or arbitrary page evaluation.
export async function actDocument(command) {
  const result = (status, detail) => ({ status, detail });
  const keys = (value, allowed, required = allowed) => value && typeof value === "object" && !Array.isArray(value)
    && Object.keys(value).every(key => allowed.includes(key)) && required.every(key => Object.hasOwn(value, key));
  const uuid = value => typeof value === "string" && /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i.test(value);
  if (!keys(command, ["operationId", "snapshotId", "elementRef", "action", "parameters", "deadlineMs"])
    || !uuid(command.operationId) || !uuid(command.snapshotId) || typeof command.elementRef !== "string"
    || command.elementRef.length > 128 || !Number.isSafeInteger(command.deadlineMs)
    || command.deadlineMs <= Date.now() || command.deadlineMs > Date.now() + 11000) return result("rejected", "invalid_command");
  const { action, parameters: params } = command;
  if (!["click", "set_value", "type_text", "scroll", "wait"].includes(action)) return result("rejected", "unsupported_action");
  if (action === "click" && (!keys(params, ["button", "count"], [])
    || (params.button !== undefined && params.button !== "left") || (params.count !== undefined && params.count !== 1))) {
    return result("rejected", "unsupported_click");
  }
  if (["set_value", "type_text"].includes(action) && (!keys(params, ["text"])
    || typeof params.text !== "string" || [...params.text].length > 4000 || params.text.includes("\0"))) {
    return result("rejected", "invalid_text");
  }
  if (action === "scroll" && (!keys(params, ["delta"]) || !Number.isInteger(params.delta)
    || params.delta < -2400 || params.delta > 2400)) return result("rejected", "invalid_scroll");
  if (action === "wait" && (!keys(params, ["nameEquals", "timeoutMs"], ["nameEquals"])
    || typeof params.nameEquals !== "string" || !params.nameEquals.trim() || params.nameEquals.length > 256
    || (params.timeoutMs !== undefined && (!Number.isInteger(params.timeoutMs) || params.timeoutMs < 1 || params.timeoutMs > 10000)))) {
    return result("rejected", "invalid_wait");
  }
  const state = globalThis.__grokComputerUseSnapshot;
  if (!state || !state.execution?.current() || state.snapshotId !== command.snapshotId || state.retired || state.used) {
    return result("rejected", "stale_snapshot");
  }
  if (state.operation) return result("rejected", "document_busy");
  if (state.completed.has(command.operationId)) return result("rejected", "duplicate_operation");
  if (state.completed.size >= 64) { state.retire(); return result("rejected", "snapshot_budget"); }
  let element;
  try { ({ element } = state.resolve(command.elementRef, false, action === "wait")); }
  catch { return result("rejected", "stale_element"); }
  const field = element instanceof HTMLTextAreaElement || (element instanceof HTMLInputElement
    && ["text", "search", "url", "tel", "email"].includes(element.type));
  if (["set_value", "type_text"].includes(action) && (!field || element.readOnly)) return result("rejected", "not_editable");
  if (action === "click") {
    if (!(element instanceof HTMLElement)) return result("rejected", "unsupported_click");
    const safeUrl = raw => {
      try { const url = new URL(raw, document.baseURI); return /^https?:$/.test(url.protocol) && !url.username && !url.password; }
      catch { return false; }
    };
    const sameTab = target => ["", "_self"].includes((target || document.querySelector("base[target]")?.target || "").toLowerCase());
    if (element instanceof HTMLAnchorElement && (element.hasAttribute("download")
      || !sameTab(element.target) || !safeUrl(element.href))) {
      return result("rejected", "unsupported_link");
    }
    if (element instanceof HTMLInputElement && element.type === "file") return result("rejected", "unsupported_click");
    const form = element.form;
    if (form && (!sameTab(element.getAttribute("formtarget") ?? form.target)
      || !safeUrl(element.getAttribute("formaction") || form.action))) {
      return result("rejected", "unsupported_form");
    }
    const rect = element.getBoundingClientRect();
    const x = (Math.max(0, rect.left) + Math.min(innerWidth, rect.right)) / 2;
    const y = (Math.max(0, rect.top) + Math.min(innerHeight, rect.bottom)) / 2;
    const hit = document.elementFromPoint(x, y);
    if (!hit || (hit !== element && !element.contains(hit))) return result("rejected", "element_occluded");
  }
  if (action === "scroll" && !["auto", "scroll"].includes(getComputedStyle(element).overflowY)) {
    return result("rejected", "not_scrollable");
  }
  const controller = new AbortController();
  let settle;
  const operation = { id: command.operationId, cancel: () => controller.abort(),
    finished: new Promise(resolve => { settle = resolve; }) };
  state.operation = operation;
  state.completed.add(command.operationId);
  const until = performance.now() + Math.min(command.deadlineMs - Date.now(), action === "wait" ? params.timeoutMs ?? 2000 : 10000);
  let dispatched = false;
  const live = () => {
    if (controller.signal.aborted || performance.now() >= until || Date.now() >= command.deadlineMs) throw new Error("cancelled");
    return state.resolve(command.elementRef, true, action === "wait");
  };
  const delay = ms => new Promise(resolve => {
    const finish = () => { clearTimeout(timer); controller.signal.removeEventListener("abort", finish); resolve(); };
    const timer = setTimeout(finish, ms);
    controller.signal.addEventListener("abort", finish, { once: true });
    if (controller.signal.aborted) finish();
  });
  try {
    live();
    if (action === "wait") {
      for (;;) {
        if (live().name === params.nameEquals) return result("verified", "condition_matched");
        await delay(Math.max(1, Math.min(50, until - performance.now())));
      }
    }
    // Consume before the first event/click: even reentrant page handlers cannot repeat this snapshot.
    state.used = true;
    if (action === "click") {
      live(); dispatched = true;
      HTMLElement.prototype.click.call(element);
      return result("applied", "semantic_click_dispatched");
    }
    if (action === "scroll") {
      const before = element.scrollTop;
      const expected = Math.min(Math.max(0, element.scrollHeight - element.clientHeight), Math.max(0, before + params.delta));
      live(); dispatched = true;
      // `instant` avoids leaving a smooth-scroll animation alive after a cancellation acknowledgement.
      Element.prototype.scrollTo.call(element, { top: expected, left: element.scrollLeft, behavior: "instant" });
      const after = element.scrollTop;
      if (Math.abs(after - expected) > 1) return result("unknown", "scroll_postcondition_failed");
      return result("verified", params.delta === 0 ? "scroll_unchanged"
        : Math.abs(after - before) <= 1 ? "scroll_boundary" : "scroll_position_changed");
    }
    dispatched = true;
    operation.focusInProgress = true;
    try { HTMLElement.prototype.focus.call(element, { preventScroll: true }); }
    finally { operation.focusInProgress = false; }
    live();
    if (document.activeElement !== element) return result("unknown", "focus_changed");
    const before = element.value;
    if (action === "type_text" && before.length > 16000) return result("unknown", "field_too_large");
    const start = action === "type_text" ? element.selectionStart ?? before.length : 0;
    const end = action === "type_text" ? element.selectionEnd ?? before.length : before.length;
    const wanted = before.slice(0, start) + params.text + before.slice(end);
    if (element.maxLength >= 0 && wanted.length > element.maxLength) return result("unknown", "field_limit");
    const inputType = action === "set_value" ? "insertReplacementText" : "insertText";
    const accepted = element.dispatchEvent(new InputEvent("beforeinput", {
      bubbles: true, composed: true, cancelable: true, inputType, data: params.text,
    }));
    live();
    if (!accepted) return result("unknown", "input_prevented");
    if (document.activeElement !== element || element.value !== before
      || (action === "type_text" && ((element.selectionStart ?? before.length) !== start || (element.selectionEnd ?? before.length) !== end))) {
      return result("unknown", "input_changed_during_dispatch");
    }
    const prototype = element instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(prototype, "value").set.call(element, wanted);
    if (element.selectionStart !== null) element.setSelectionRange(start + params.text.length, start + params.text.length);
    element.dispatchEvent(new InputEvent("input", { bubbles: true, composed: true, inputType, data: params.text }));
    live();
    // SetValue commits a complete value; TypeText leaves the active edit uncommitted, like text entry.
    if (action === "set_value") element.dispatchEvent(new Event("change", { bubbles: true }));
    live();
    if (controller.signal.aborted || !element.isConnected || element.ownerDocument !== document
      || document.activeElement !== element || element.value !== wanted) return result("unknown", "input_postcondition_failed");
    return result("verified", "field_value_changed");
  } catch {
    return result(dispatched ? "unknown" : "rejected", controller.signal.aborted ? "cancelled" : "stale_or_deadline");
  } finally {
    if (state.operation === operation) state.operation = null;
    if (action !== "wait") state.retire();
    settle();
  }
}

// A cancellation request fences the snapshot even when a delayed action script has not entered yet.
// Its caller must still await the original executeScript Promise; this alone is not Host quiescence.
export async function cancelDocumentOperation(snapshotId, operationId, retireExecution = false) {
  const state = globalThis.__grokComputerUseSnapshot;
  if (!state || state.snapshotId !== snapshotId) return { status: "snapshot_missing" };
  const operation = state.operation;
  if (operation && operation.id !== operationId) return { status: "operation_mismatch" };
  if (retireExecution) {
    if (typeof state.execution?.close !== "function") return { status: "execution_missing" };
    state.execution.close();
  }
  state.retire();
  if (operation) await operation.finished;
  return { status: "settled" };
}
