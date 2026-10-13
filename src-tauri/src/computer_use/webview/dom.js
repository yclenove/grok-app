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
  const textField = element => element instanceof HTMLTextAreaElement || (element instanceof HTMLInputElement
    && ["text", "search", "url", "tel", "email"].includes(element.type));
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
    "formmethod", "formenctype", "formtarget", "disabled", "readonly", "contenteditable", "aria-label", "aria-labelledby", "title",
    "accept", "multiple", "capture"];
  const waitIdentityAttributes = ["id", "name", "type", "role"];
  const signature = element => {
    const values = [document.baseURI, element.form?.action, element.form?.method, ...identityAttributes.map(name => element.getAttribute(name))];
    if (values.some(value => value && value.length > 4096)) return null;
    return JSON.stringify([element.tagName, ...values]);
  };
  const waitSignature = element => {
    const values = waitIdentityAttributes.map(name => element.getAttribute(name));
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
  const directDownload = element => {
    if (!(element instanceof HTMLAnchorElement) || !element.hasAttribute("download")) return null;
    const raw = element.getAttribute("href");
    if (!raw || raw.length > 4096) return null;
    try {
      const url = new URL(raw, document.baseURI);
      if (!["http:", "https:"].includes(url.protocol) || url.username || url.password || url.href.length > 4096) return null;
      return url.href;
    } catch { return null; }
  };
  const actionsOf = element => {
    if (disabled(element)) return [];
    const actions = [];
    const downloadLink = element instanceof HTMLAnchorElement && element.hasAttribute("download");
    if (downloadLink) {
      if (directDownload(element)) actions.push("download");
    } else if (element instanceof HTMLInputElement && element.type === "file") {
      actions.push("upload");
    } else if (element instanceof HTMLElement && element.matches("button,a[href],input:not([type='hidden']):not([type='password']),select,[role='button'],[role='link']")) actions.push("click");
    if (!element.readOnly && (element.matches("textarea,input[type='text'],input:not([type]),input[type='search'],input[type='email'],input[type='url'],input[type='tel'],input[type='number']")
      || element.isContentEditable)) actions.push("set_value");
    if (!element.readOnly && textField(element)) actions.push("type_text");
    const style = getComputedStyle(element);
    if (element.scrollHeight > element.clientHeight + 1
      && (element === document.scrollingElement || /^(auto|scroll|overlay)$/.test(style.overflowY))) actions.push("scroll");
    if (actions.length) actions.push("wait");
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
                if (retain) records.set(ref, { element: new WeakRef(element), signature: identity,
                  waitSignature: waitSignature(element), name, rect, actions,
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
      state.current = (ref, action) => {
        inspect(observer.takeRecords());
        const record = records.get(ref); const element = record?.element.deref();
        if (state.retired || !record || record.stale || !visible(element) || disabled(element)
          || signature(element) !== record.signature || nameOf(element) !== record.name
          || viewport().some((value, i) => value !== frame[i])
          || geometry(element).some((value, i) => Math.abs(value - record.rect[i]) > 0.5)
          || !record.actions.includes(action) || !actionsOf(element).includes(action)) return null;
        return element;
      };
      state.resolve = (ref, action) => state.used ? null : state.current(ref, action);
      state.waitCheck = (ref, expected) => {
        inspect(observer.takeRecords());
        const record = records.get(ref); const element = record?.element.deref();
        if (state.retired || state.used || !record || !element || !visible(element)
          || element.ownerDocument !== document || waitSignature(element) !== record.waitSignature
          || location.href !== frame[0] || !record.actions.includes("wait")) return null;
        return nameOf(element) === expected;
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

  if (command.kind === "wait_check") {
    if (typeof command.elementRef !== "string" || !command.elementRef
      || typeof command.nameEquals !== "string" || !command.nameEquals.trim()
      || command.nameEquals.length > 256) return fail("invalid_command");
    const state = globalThis[key];
    if (!state || state.owner !== command.owner || state.snapshot !== command.snapshot) return fail("stale_snapshot");
    const matched = state.waitCheck(command.elementRef, command.nameEquals);
    if (matched === null) return fail("stale_element");
    return { ok: true, matched };
  }
  if (command.kind === "download_prepare" || command.kind === "download_dispatch") {
    if (typeof command.elementRef !== "string" || !command.elementRef) return fail("invalid_command");
    const state = globalThis[key];
    if (!state || state.owner !== command.owner || state.snapshot !== command.snapshot) return fail("stale_snapshot");
    const element = state.resolve(command.elementRef, "download");
    const url = element && directDownload(element);
    if (!element || !url) return fail("stale_element");
    if (command.kind === "download_prepare") return { ok: true, url };
    state.used = true;
    // DOM attributes cross script worlds; page globals do not. Bypass the
    // manual-save polyfill only for this retained link's synchronous click.
    // This is no authority: native instance/URL admission remains on the Host.
    const marker = "data-grok-native-download";
    const previous = element.getAttribute(marker);
    element.setAttribute(marker, "1");
    try { HTMLElement.prototype.click.call(element); }
    finally {
      if (previous === null) element.removeAttribute(marker);
      else element.setAttribute(marker, previous);
    }
    return { ok: true };
  }
  if (command.kind === "upload_prepare" || command.kind === "upload_dispatch") {
    if (typeof command.elementRef !== "string" || !command.elementRef) return fail("invalid_command");
    const state = globalThis[key];
    if (!state || state.owner !== command.owner || state.snapshot !== command.snapshot) return fail("stale_snapshot");
    const element = state.resolve(command.elementRef, "upload");
    if (!(element instanceof HTMLInputElement) || element.type !== "file") return fail("stale_element");
    if (command.kind === "upload_prepare") return { ok: true };
    // Only the Host supplies bounded immutable content. No local path, URL,
    // selector or executable source is accepted by this fixed command.
    if (typeof command.filename !== "string" || !command.filename || [...command.filename].length > 80
      || /[\\/\0:\r\n]/.test(command.filename) || command.filename.includes("..")
      || typeof command.mime !== "string" || command.mime.length > 127
      || !/^[a-zA-Z0-9!#$&^_.+-]+\/[a-zA-Z0-9!#$&^_.+-]+$/.test(command.mime)
      || !Number.isSafeInteger(command.bytes) || command.bytes < 0 || command.bytes > 8 * 1024 * 1024
      || typeof command.data !== "string" || command.data.length !== 4 * Math.ceil(command.bytes / 3)) return fail("invalid_upload");
    let decoded;
    try { decoded = atob(command.data); } catch { return fail("invalid_upload"); }
    if (decoded.length !== command.bytes) return fail("invalid_upload");
    const bytes = new Uint8Array(decoded.length);
    for (let i = 0; i < decoded.length; i++) bytes[i] = decoded.charCodeAt(i);
    const file = new File([bytes], command.filename, { type: command.mime });
    const transfer = new DataTransfer();
    transfer.items.add(file);
    state.used = true;
    const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "files")?.set;
    if (!setter) return fail("unsupported_upload");
    setter.call(element, transfer.files);
    element.dispatchEvent(new Event("input", { bubbles: true, composed: true }));
    element.dispatchEvent(new Event("change", { bubbles: true, composed: true }));
    if (state.current(command.elementRef, "upload") !== element || element.files.length !== 1
      || element.files[0] !== file) return fail("upload_postcondition_failed");
    return { ok: true, verified: true };
  }
  if (command.kind !== "preflight" && command.kind !== "act") return fail("invalid_command");
  if (["set_value", "type_text"].includes(command.action) && (typeof command.text !== "string"
    || [...command.text].length > 4000 || command.text.includes("\0"))) return fail("invalid_text");
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
  } else if (command.action === "type_text") {
    if (!textField(element) || element.readOnly) return fail("unsupported_type_text");
    HTMLElement.prototype.focus.call(element, { preventScroll: true });
    const current = () => state.current(command.elementRef, command.action) === element
      && document.activeElement === element;
    if (!current()) return fail("focus_changed");
    const before = element.value;
    if (before.length > 16000) return fail("field_too_large");
    const start = element.selectionStart ?? before.length;
    const end = element.selectionEnd ?? before.length;
    const wanted = before.slice(0, start) + command.text + before.slice(end);
    if (element.maxLength >= 0 && wanted.length > element.maxLength) return fail("field_limit");
    const accepted = element.dispatchEvent(new InputEvent("beforeinput", {
      bubbles: true, composed: true, cancelable: true, inputType: "insertText", data: command.text,
    }));
    if (!accepted || !current() || element.value !== before
      || (element.selectionStart ?? before.length) !== start
      || (element.selectionEnd ?? before.length) !== end) return fail("input_changed_during_dispatch");
    const prototype = element instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(prototype, "value").set.call(element, wanted);
    element.setSelectionRange(start + command.text.length, start + command.text.length);
    element.dispatchEvent(new InputEvent("input", {
      bubbles: true, composed: true, inputType: "insertText", data: command.text,
    }));
    if (!current() || element.value !== wanted
      || element.selectionStart !== start + command.text.length
      || element.selectionEnd !== start + command.text.length) return fail("input_postcondition_failed");
    return { ok: true, verified: true };
  } else if (command.action === "scroll") {
    element.scrollTop += command.dy;
  } else return fail("unsupported_action");
  return { ok: true };
}
