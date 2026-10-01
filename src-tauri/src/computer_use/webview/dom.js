// Fixed Host kernel. Execute only in the native isolated world. Command data is
// JSON, never a selector or source supplied by the model. References are held
// here as actual DOM objects and cannot be reconstructed from an element's id.
function webViewDom(command) {
  "use strict";
  const key = "__grokAppWebViewDom";
  const fail = reason => ({ ok: false, reason });
  if (command.version !== 1 || typeof command.owner !== "string" || !command.owner
    || typeof command.snapshot !== "string" || !/^wvsnap:[0-9a-f-]{36}$/.test(command.snapshot)) return fail("invalid_command");
  if (!document.body || document.visibilityState !== "visible") return fail("document_unavailable");
  const parent = element => element.parentElement || element.getRootNode()?.host || null;
  const containsComposed = (ancestor, node) => {
    let current = node; let depth = 0;
    while (current && depth++ < 128) {
      if (current === ancestor) return true;
      current = parent(current);
    }
    return false;
  };
  const excluded = "[hidden],[inert],[aria-hidden='true'],input[type='hidden'],input[type='password'],script,style,noscript,iframe,object,embed";
  const editable = element => element.matches("input,textarea,[contenteditable]:not([contenteditable='false'])");
  const visible = element => {
    if (!element?.isConnected || element.ownerDocument !== document) return false;
    let current = element; let depth = 0;
    while (current) {
      if (++depth > 128 || current.matches(excluded)) return false;
      const style = getComputedStyle(current);
      if (style.display === "none" || style.visibility === "hidden" || style.visibility === "collapse" || style.opacity === "0") return false;
      current = parent(current);
    }
    const rect = element.getBoundingClientRect();
    return rect.width > 0 && rect.height > 0 && rect.bottom > 0 && rect.right > 0 && rect.top < innerHeight && rect.left < innerWidth;
  };
  const directText = element => {
    if (editable(element)) return "";
    const walker = document.createTreeWalker(element, NodeFilter.SHOW_TEXT);
    let text = ""; let node; let count = 0;
    while ((node = walker.nextNode()) && count++ < 64 && text.length < 256) {
      let current = node.parentElement; let privateText = false; let depth = 0;
      while (current && current !== element) {
        if (++depth > 128 || editable(current) || current.matches(excluded)) { privateText = true; break; }
        const style = getComputedStyle(current);
        if (style.display === "none" || style.visibility === "hidden" || style.opacity === "0") { privateText = true; break; }
        current = parent(current);
      }
      if (!privateText) text += node.textContent.slice(0, 256 - text.length) + " ";
    }
    return text.slice(0, 256).trim();
  };
  const labelsOf = element => {
    const ids = (element.getAttribute("aria-labelledby") || "").split(/\s+/).filter(Boolean).slice(0, 8);
    const labels = ids.map(id => element.getRootNode().getElementById?.(id)).filter(label => label && visible(label));
    if (!labels.length && element.labels) labels.push(...Array.from(element.labels).slice(0, 8).filter(visible));
    return labels;
  };
  const nameOf = element => {
    const aria = element.getAttribute("aria-label");
    if (aria) return aria.slice(0, 256);
    const label = labelsOf(element).map(directText).join(" ").trim();
    return (label || directText(element) || element.getAttribute("title") || "").slice(0, 256);
  };
  const identityAttributes = ["id", "name", "type", "role", "href", "target", "download", "form", "formaction",
    "formmethod", "formenctype", "formtarget", "disabled", "readonly", "contenteditable", "aria-label", "aria-labelledby", "title"];
  const signature = element => {
    const values = [document.baseURI, element.form?.action, element.form?.method, ...identityAttributes.map(name => element.getAttribute(name))];
    if (values.some(value => value && value.length > 4096)) return null;
    return JSON.stringify([element.tagName, ...values]);
  };
  const geometry = element => {
    const r = element.getBoundingClientRect();
    return [r.x, r.y, r.width, r.height];
  };
  const viewport = () => [location.href, innerWidth, innerHeight, scrollX, scrollY, devicePixelRatio,
    globalThis.visualViewport?.scale ?? 1, globalThis.visualViewport?.offsetLeft ?? 0, globalThis.visualViewport?.offsetTop ?? 0];
  const disabled = element => {
    let current = element; let depth = 0;
    while (current) {
      if (++depth > 128 || current.matches(":disabled,[aria-disabled='true'],[inert]")) return true;
      current = parent(current);
    }
    return false;
  };
  const actionsOf = element => {
    if (disabled(element)) return [];
    const actions = [];
    if (element instanceof HTMLElement && element.matches("button,a[href],input:not([type='hidden']):not([type='password']),select,[role='button'],[role='link']")) actions.push("click");
    if (!element.readOnly && (element.matches("textarea,input[type='text'],input:not([type]),input[type='search'],input[type='email'],input[type='url'],input[type='tel'],input[type='number']")
      || element.isContentEditable)) actions.push("set_value");
    const style = getComputedStyle(element);
    if (element.scrollHeight > element.clientHeight + 1
      && (element === document.scrollingElement || /^(auto|scroll|overlay)$/.test(style.overflowY))) actions.push("scroll");
    return actions;
  };
  const safety = () => {
    const frames = document.querySelectorAll("iframe");
    if (frames.length > 64) return "unsupported_frame_limit";
    for (const frame of frames) {
      try { if (frame.src && new URL(frame.src, location.href).origin !== location.origin) return "unsupported_cross_origin"; }
      catch { return "unsupported_cross_origin"; }
    }
    const downloads = document.querySelectorAll("a[download]");
    if (downloads.length > 128) return "unsupported_download_limit";
    for (const link of downloads) if (/^(https?:|blob:)/.test(link.getAttribute("href") || "")) return "unsupported_download";
    if (document.body.getAttribute("data-permission-ui") === "1") return "unsupported_permission";
    return null;
  };
  const unsupported = safety();
  if (unsupported) return fail(unsupported);

  if (command.kind === "observe") {
    const retain = command.forModel === true;
    if (retain) globalThis[key]?.retire();
    const nodes = []; const records = new Map(); const text = []; const roots = new Set([document]);
    const deadline = performance.now() + 200;
    let visited = 0; let truncated = false; let textLength = 0;
    const walk = (root, depth) => {
      if (depth > 16 || roots.size > 64) { truncated = true; return; }
      const walker = document.createTreeWalker(root, NodeFilter.SHOW_ELEMENT | NodeFilter.SHOW_TEXT);
      let node = walker.currentNode;
      while (node) {
        if (++visited > 4096 || performance.now() >= deadline) { truncated = true; return; }
        const element = node.nodeType === Node.ELEMENT_NODE ? node : node.parentElement;
        if (element && visible(element)) {
          if (node.nodeType === Node.TEXT_NODE) {
            let current = element; let privateText = false; let depth = 0;
            while (current) {
              if (++depth > 128 || editable(current)) { privateText = true; break; }
              current = parent(current);
            }
            if (!privateText && textLength < 32000) {
              const raw = node.textContent.trim();
              const part = raw.slice(0, 32000 - textLength);
              if (part) { text.push(part); textLength += part.length + 1; }
              if (part.length < raw.length) truncated = true;
            }
          } else if (node.nodeType === Node.ELEMENT_NODE) {
            const actions = actionsOf(element); const identity = signature(element);
            if (actions.length && identity) {
              if (nodes.length >= 128) truncated = true;
              else {
                const ref = `${command.snapshot}:${nodes.length + 1}`;
                const name = nameOf(element); const rect = geometry(element);
                const role = element.getAttribute("role") || ({ BUTTON: "button", A: "link", INPUT: "textbox", TEXTAREA: "textbox", SELECT: "combobox" }[element.tagName]
                  || (actions.includes("scroll") ? "region" : "textbox"));
                nodes.push({ elementRef: ref, role: role.slice(0, 64), name, actions, x: rect[0], y: rect[1], width: rect[2], height: rect[3] });
                if (retain) records.set(ref, { element: new WeakRef(element), signature: identity, name, rect, actions,
                  form: element.form ? new WeakRef(element.form) : null, labels: labelsOf(element).map(label => new WeakRef(label)), stale: false });
              }
            }
            if (element.shadowRoot) { roots.add(element.shadowRoot); walk(element.shadowRoot, depth + 1); }
          }
        }
        node = walker.nextNode();
      }
    };
    walk(document.documentElement, 0);
    if (retain) {
      const frame = viewport(); const listeners = [];
      const state = { owner: command.owner, snapshot: command.snapshot, retired: false, used: false };
      const observer = new MutationObserver(changes => inspect(changes));
      state.retire = () => {
        if (state.retired) return;
        state.retired = true; observer.disconnect(); records.clear();
        for (const [target, type, callback] of listeners) target.removeEventListener(type, callback, true);
        listeners.length = 0;
      };
      const inspect = changes => {
        let work = 0; const until = performance.now() + 50;
        for (const change of changes) for (const record of records.values()) {
          if (++work > 4096 || performance.now() >= until) { state.retire(); return; }
          const element = record.element.deref(); const form = record.form?.deref();
          if (!element || !element.isConnected) { record.stale = true; continue; }
          if (document.head?.contains(change.target)) record.stale = true;
          const labels = record.labels.map(label => label.deref()).filter(Boolean);
          if (change.type === "attributes" && (containsComposed(change.target, element) || change.target === form
            || labels.some(label => containsComposed(change.target, label)))) record.stale = true;
          if ((change.type === "childList" || change.type === "characterData")
            && (containsComposed(element, change.target) || labels.some(label => containsComposed(label, change.target)))) record.stale = true;
          if (change.type === "childList") for (const removed of change.removedNodes) {
            if (++work > 4096) { state.retire(); return; }
            if (containsComposed(removed, element) || (form && containsComposed(removed, form))
              || labels.some(label => containsComposed(removed, label))) record.stale = true;
          }
        }
      };
      state.resolve = (ref, action) => {
        inspect(observer.takeRecords());
        const record = records.get(ref); const element = record?.element.deref();
        if (state.retired || state.used || !record || record.stale || !visible(element) || disabled(element)
          || signature(element) !== record.signature || nameOf(element) !== record.name
          || viewport().some((value, i) => value !== frame[i])
          || geometry(element).some((value, i) => Math.abs(value - record.rect[i]) > 0.5)
          || !record.actions.includes(action) || !actionsOf(element).includes(action)) return null;
        return element;
      };
      for (const root of roots) observer.observe(root, { subtree: true, childList: true, characterData: true,
        attributes: true, attributeFilter: [...identityAttributes, "class", "style", "hidden", "inert", "aria-hidden", "aria-disabled"] });
      const listen = (target, type, trustedOnly = false) => {
        if (!target) return;
        const callback = event => { if (!trustedOnly || event.isTrusted) state.retire(); };
        target.addEventListener(type, callback, { capture: true, passive: true });
        listeners.push([target, type, callback]);
      };
      for (const type of ["scroll", "resize", "blur", "pagehide"]) listen(globalThis, type);
      for (const type of ["pointerdown", "keydown"]) listen(globalThis, type, true);
      listen(document, "visibilitychange");
      for (const type of ["resize", "scroll"]) listen(globalThis.visualViewport, type);
      globalThis[key] = state;
    }
    return { ok: true, snapshot: command.snapshot, nodes, text: text.join("\n").slice(0, 32000),
      width: Math.round(innerWidth), height: Math.round(innerHeight), truncated };
  }

  if (command.kind !== "preflight" && command.kind !== "act") return fail("invalid_command");
  if (command.action === "set_value" && (typeof command.text !== "string" || command.text.length > 16000)) return fail("invalid_text");
  if (command.action === "scroll" && (!Number.isSafeInteger(command.dy) || Math.abs(command.dy) > 4096)) return fail("invalid_scroll");
  const state = globalThis[key];
  if (!state || state.owner !== command.owner || state.snapshot !== command.snapshot) return fail("stale_snapshot");
  const element = state.resolve(command.elementRef, command.action);
  if (!element) return fail("stale_element");
  if (command.kind === "preflight") return { ok: true };
  // Consume before dispatch, including when a page handler throws or navigates.
  // A business retry can never invoke the same snapshot a second time.
  state.used = true;
  if (command.action === "click") HTMLElement.prototype.click.call(element);
  else if (command.action === "set_value") {
    if (element instanceof HTMLInputElement || element instanceof HTMLTextAreaElement) {
      const prototype = element instanceof HTMLInputElement ? HTMLInputElement.prototype : HTMLTextAreaElement.prototype;
      Object.getOwnPropertyDescriptor(prototype, "value").set.call(element, command.text);
    } else if (element.isContentEditable) element.textContent = command.text;
    else return fail("unsupported_fill");
    element.dispatchEvent(new Event("input", { bubbles: true, composed: true }));
    element.dispatchEvent(new Event("change", { bubbles: true }));
  } else if (command.action === "scroll") {
    element.scrollTop += command.dy;
  } else return fail("unsupported_action");
  return { ok: true };
}
