// Chrome serializes this fixed function into the isolated main frame. No imports or eval inside it.
export function observeDocument(snapshotId, retainRefs = true, executionIdentity) {
  if (document.visibilityState !== "visible" || !/^https?:$/.test(location.protocol)) throw new Error("not visible");
  if (retainRefs && globalThis.__grokComputerUseSnapshot?.operation) throw new Error("document busy");
  const execution = globalThis.__grokComputerUseExecution;
  if (execution?.protocol !== 1 || typeof execution.admit !== "function") throw new Error("executionUnavailable");
  execution.admit(executionIdentity);
  const deadline = performance.now() + 200;
  let truncated = false; let work = 0;
  const budget = () => {
    if (++work > 16384 || performance.now() >= deadline) { truncated = true; return false; }
    return true;
  };
  // Cache inherited exclusions. Never repeatedly walk unbounded ancestor chains or use closest().
  const inherited = new WeakMap();
  const stateOf = element => {
    const path = []; let ancestor = element;
    while (ancestor && !inherited.has(ancestor)) {
      if (path.length >= 128 || !budget()) { truncated = true; return { hidden: true }; }
      path.push(ancestor); ancestor = ancestor.parentElement;
    }
    let state = ancestor ? inherited.get(ancestor) : { hidden: false, textarea: false };
    while (path.length) {
      if (!budget()) return { hidden: true };
      const current = path.pop();
      let hidden = state.hidden;
      if (!hidden) {
        const style = getComputedStyle(current);
        hidden = current.matches("[hidden],[inert],[aria-hidden='true'],input[type='password'],script,style,noscript,iframe,object,embed")
          || style.display === "none" || style.visibility === "hidden" || style.visibility === "collapse" || style.opacity === "0";
      }
      state = { hidden, textarea: state.textarea || current.tagName === "TEXTAREA" };
      inherited.set(current, state);
    }
    return state;
  };
  const visible = element => {
    if (!element || stateOf(element).hidden || !budget()) return false;
    const rect = element.getBoundingClientRect();
    return rect.width > 0 && rect.height > 0 && rect.bottom > 0 && rect.right > 0 && rect.top < innerHeight && rect.left < innerWidth;
  };
  const textOf = element => {
    let label = ""; let child = element.firstChild; let count = 0;
    while (child && count++ < 64 && label.length < 256 && budget()) {
      if (child.nodeType === Node.TEXT_NODE) label += child.textContent.slice(0, 256 - label.length) + " ";
      child = child.nextSibling;
    }
    if (child) truncated = true;
    return label.slice(0, 256).trim();
  };
  const identityAttributes = ["id", "name", "type", "role", "href", "target", "download", "form", "formaction",
    "formmethod", "formtarget", "action", "method", "contenteditable", "readonly", "disabled", "hidden", "inert", "aria-hidden", "aria-disabled"];
  const signature = element => {
    const values = identityAttributes.map(key => element.getAttribute(key));
    if (values.some(value => value && value.length > 8192)) return null;
    return JSON.stringify([element.tagName, document.baseURI, element.form?.action, element.form?.method, element.form?.target, ...values]);
  };
  // Same bounded name algorithm for observation and action validation; never read input values.
  const currentName = element => {
    const aria = element.getAttribute("aria-label");
    if (aria) return aria.slice(0, 256);
    if (element.matches("input,textarea")) return "";
    let label = ""; let child = element.firstChild; let count = 0;
    while (child && count++ < 64 && label.length < 256) {
      if (child.nodeType === Node.TEXT_NODE) label += child.textContent.slice(0, 256 - label.length) + " ";
      child = child.nextSibling;
    }
    return label.slice(0, 256).trim();
  };
  const nodes = []; const refs = new Map(); const records = new Map(); const text = [];
  let scanned = 0; let textLength = 0;
  if (!document.body) throw new Error("document unavailable");
  const walker = document.createTreeWalker(document.body, NodeFilter.SHOW_ELEMENT | NodeFilter.SHOW_TEXT);
  let node = walker.currentNode;
  while (node && scanned++ < 4096 && budget()) {
    const element = node.nodeType === Node.ELEMENT_NODE ? node : node.parentElement;
    if (visible(element)) {
      if (node.nodeType === Node.TEXT_NODE && !stateOf(element).textarea) {
        const raw = node.textContent;
        const content = raw.slice(0, 32000 - textLength).trim();
        if (raw.length > 32000 - textLength) truncated = true;
        if (content) {
          const remaining = 32000 - textLength;
          text.push(content.slice(0, remaining)); textLength += Math.min(content.length, remaining) + 1;
          if (textLength >= 32000) { truncated = true; break; }
        }
      } else if (node.nodeType === Node.ELEMENT_NODE && element.matches("button,a[href],input:not([type='hidden']):not([type='password']),textarea,select,[role='button'],[role='link'],[contenteditable='true']")) {
        if (nodes.length >= 64) { truncated = true; }
        else {
          const elementRef = `${snapshotId}-${nodes.length + 1}`;
          const role = element.getAttribute("role") || ({ BUTTON: "button", A: "link", INPUT: "textbox", TEXTAREA: "textbox", SELECT: "combobox" }[element.tagName] || "textbox");
          // Never read value/defaultValue, hidden fields or password contents.
          const name = (element.getAttribute("aria-label") || (element.matches("input,textarea") ? "" : textOf(element))).slice(0, 256);
          nodes.push({ elementRef, role: role.slice(0, 64), name, disabled: element.disabled === true || element.getAttribute("aria-disabled") === "true" });
          refs.set(elementRef, new WeakRef(element));
          records.set(elementRef, { signature: signature(element), name, stale: false, nameChanged: false,
            form: element.form ? new WeakRef(element.form) : null });
        }
      }
    }
    node = walker.nextNode();
  }
  if (node) truncated = true;
  // One snapshot per document. Preview never replaces refs or acquires their lifecycle.
  if (retainRefs) {
    globalThis.__grokComputerUseSnapshot?.retire?.();
    const view = [location.href, innerWidth, innerHeight, scrollX, scrollY, devicePixelRatio,
      globalThis.visualViewport?.scale ?? 1, globalThis.visualViewport?.offsetLeft ?? 0, globalThis.visualViewport?.offsetTop ?? 0];
    const state = { snapshotId, refs, retired: false, used: false, operation: null, completed: new Set(), execution };
    const listeners = [];
    const observer = new MutationObserver(changes => inspectChanges(changes));
    state.retire = () => {
      if (state.retired) return;
      state.retired = true;
      observer.disconnect();
      for (const [target, type, handler] of listeners) target.removeEventListener(type, handler, true);
      listeners.length = 0;
      refs.clear(); records.clear();
      state.operation?.cancel();
    };
    const inspectChanges = changes => {
      const until = performance.now() + 50;
      let work = 0;
      for (const change of changes) {
        for (const [ref, record] of records) {
          if (++work > 4096 || performance.now() >= until) { state.retire(); return; }
          const element = refs.get(ref)?.deref();
          if (!element) { record.stale = true; continue; }
          const form = record.form?.deref();
          if (document.head?.contains(change.target)) record.stale = true;
          if (change.type === "attributes" && (change.target === element || change.target.contains(element) || change.target === form)) {
            // A name may change while Wait watches it, but no old observation may click that new name.
            if (change.attributeName === "aria-label") record.nameChanged = true;
            else record.stale = true;
          }
          if (change.type === "childList") {
            for (const removed of change.removedNodes) {
              if (++work > 4096 || performance.now() >= until) { state.retire(); return; }
              if (removed === element || removed.contains(element) || (form && (removed === form || removed.contains(form)))) record.stale = true;
            }
          }
          if ((change.type === "childList" || change.type === "characterData") && element.contains(change.target)) {
            record.nameChanged = true;
          }
        }
      }
    };
    state.resolve = (ref, allowUsed = false, allowNameChange = false) => {
      inspectChanges(observer.takeRecords());
      const record = records.get(ref); const element = refs.get(ref)?.deref();
      const nowView = [location.href, innerWidth, innerHeight, scrollX, scrollY, devicePixelRatio,
        globalThis.visualViewport?.scale ?? 1, globalThis.visualViewport?.offsetLeft ?? 0, globalThis.visualViewport?.offsetTop ?? 0];
      if (state.retired || !execution.current() || (state.used && !allowUsed) || globalThis.__grokComputerUseSnapshot !== state
        || !record || record.stale || !element?.isConnected || element.ownerDocument !== document
        || !record.signature || signature(element) !== record.signature || document.visibilityState !== "visible"
        || view.some((value, index) => value !== nowView[index])
        || (!allowNameChange && (record.nameChanged || currentName(element) !== record.name))) throw new Error("stale element");
      const until = performance.now() + 50;
      let current = element; let depth = 0;
      while (current) {
        if (++depth > 128 || performance.now() >= until) throw new Error("stale element");
        const style = getComputedStyle(current);
        if (current.matches("[hidden],[inert],[aria-hidden='true'],[aria-disabled='true'],:disabled,input[type='password']")
          || style.display === "none" || style.visibility === "hidden" || style.visibility === "collapse" || style.opacity === "0") {
          throw new Error("stale element");
        }
        current = current.parentElement;
      }
      const rect = element.getBoundingClientRect();
      if (rect.width <= 0 || rect.height <= 0 || rect.bottom <= 0 || rect.right <= 0
        || rect.top >= innerHeight || rect.left >= innerWidth) throw new Error("stale element");
      return { element, name: currentName(element) };
    };
    // Mutation records also catch remove/reinsert and identity changes reverted before dispatch.
    observer.observe(document, { subtree: true, childList: true, characterData: true,
      attributes: true, attributeFilter: [...identityAttributes, "aria-label", "style", "class"] });
    const listen = (target, type) => {
      if (!target) return;
      const handler = event => {
        // Only the executor's synchronous focus() may transfer focus between fields.
        if (type === "blur" && event.target !== globalThis && state.operation?.focusInProgress) return;
        state.retire();
      };
      target.addEventListener(type, handler, { capture: true, passive: true });
      listeners.push([target, type, handler]);
    };
    for (const type of ["scroll", "resize", "blur", "pagehide"]) listen(globalThis, type);
    listen(document, "visibilitychange");
    for (const type of ["resize", "scroll"]) listen(globalThis.visualViewport, type);
    globalThis.__grokComputerUseSnapshot = state;
  }
  return { snapshotId, title: document.title.slice(0, 512), url: location.href, text: text.join("\n").slice(0, 32000),
    nodes, viewportWidth: Math.round(innerWidth), viewportHeight: Math.round(innerHeight), truncated };
}
