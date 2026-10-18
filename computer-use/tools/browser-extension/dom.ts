import { type DomNode, type DomOperation, type DomResult, type DomSnapshot, type NodeValue, type Rect, type Role, error } from "./shared";

const roles: Record<string, Role> = {
  button: "button", link: "link", checkbox: "check_box", radio: "radio", switch: "switch",
  textbox: "text_field", searchbox: "search_field", combobox: "combo_box", slider: "slider",
  spinbutton: "stepper", progressbar: "progress_bar", list: "list", listitem: "list_item",
  table: "table", grid: "table", row: "row", cell: "cell", gridcell: "cell", columnheader: "column",
  tree: "tree", treeitem: "tree_item", tab: "tab", tablist: "tab_list", scrollbar: "scroll_bar",
  img: "image", heading: "heading", dialog: "dialog", alertdialog: "dialog", menu: "menu",
  menuitem: "menu_item", menubar: "menu_bar", toolbar: "toolbar", document: "document", group: "group",
};
const tags: Record<string, Role> = {
  HTML: "document", BODY: "web_area", BUTTON: "button", A: "link", TEXTAREA: "text_area",
  SELECT: "combo_box", OPTION: "list_item", UL: "list", OL: "list", LI: "list_item", TABLE: "table",
  TR: "row", TD: "cell", TH: "column", IMG: "image", CANVAS: "unknown", DIALOG: "dialog",
  H1: "heading", H2: "heading", H3: "heading", H4: "heading", H5: "heading", H6: "heading",
};
const execution = /terminal|console|web.?ssh|cloud.?shell|remote.?desktop|container.?exec|debug|repl|xterm|monaco|code.?mirror|execution|终端|控制台|遠端桌面|远程桌面/i;
const executionSurface = (value: string) => execution.test(value.normalize("NFKC"));
const ignored = new Set(["SCRIPT", "STYLE", "NOSCRIPT", "TEMPLATE", "HEAD"]);
const text = (value: string | null | undefined) => (value ?? "").trim().slice(0, 500);
interface Context { x: number; y: number; path: string; documents: Document[]; frames: HTMLIFrameElement[] }
interface Target { element: Element; context: Context; stamp: string; ancestors: Node[] }

function ancestors(el: Element): Node[] {
  const result: Node[] = [];
  let parent: Node | null = el.parentNode;
  while (parent && result.length < 128) {
    result.push(parent);
    parent = parent.parentNode ?? ("host" in parent ? (parent as ShadowRoot).host : null);
  }
  return result;
}

function password(el: Element): boolean {
  return (el.tagName === "INPUT" && (el as HTMLInputElement).type === "password") ||
    /password|passwd|passphrase|密码|密碼/i.test(`${el.id} ${el.getAttribute("name")} ${el.getAttribute("autocomplete")} ${el.getAttribute("aria-label")}`);
}
function role(el: Element): Role {
  const aria = el.getAttribute("role")?.trim().split(/\s+/)[0];
  if (aria) return roles[aria] ?? "unknown";
  if (el.tagName === "INPUT") {
    const type = (el as HTMLInputElement).type;
    return ({ checkbox: "check_box", radio: "radio", button: "button", submit: "button", reset: "button", search: "search_field", range: "slider", number: "stepper" } as Record<string, Role>)[type] ?? "text_field";
  }
  return tags[el.tagName] ?? (el.children.length === 0 && safeText(el) ? "static_text" : "group");
}
// Bounded label text never reads form values/default textarea contents or scripts.
function safeText(root: Element): string {
  let result = "";
  let work = 0;
  const walk = (node: Node, depth: number) => {
    if (++work > 128 || depth > 32 || result.length >= 500) return;
    if (node.nodeType === 3) { result += node.textContent?.slice(0, 500 - result.length) ?? ""; return; }
    if (node.nodeType === 1 && (["INPUT", "TEXTAREA"].includes((node as Element).tagName) || ignored.has((node as Element).tagName) || password(node as Element))) return;
    for (const child of node.childNodes) { walk(child, depth + 1); if (work >= 128 || result.length >= 500) break; }
  };
  walk(root, 0);
  return text(result);
}
function name(el: Element): string | null {
  if (el.hasAttribute("aria-label")) return text(el.getAttribute("aria-label")) || null;
  const refs = el.getAttribute("aria-labelledby")?.split(/\s+/).slice(0, 16);
  if (refs) {
    const root = el.getRootNode() as Document | ShadowRoot;
    return text(refs.map(id => { const label = root.getElementById(id); return label ? safeText(label) : ""; }).join(" ")) || null;
  }
  const labels = (el as HTMLInputElement).labels;
  if (labels?.length) return text(Array.from(labels).slice(0, 16).map(safeText).join(" ")) || null;
  const attr = el.getAttribute("alt") || el.getAttribute("title") || el.getAttribute("placeholder");
  if (attr) return text(attr);
  if (["INPUT", "TEXTAREA"].includes(el.tagName) || password(el)) return null;
  // Do not copy aggregate ancestor text: it duplicates the tree and can echo secrets.
  let own = "";
  let work = 0;
  for (const child of el.childNodes) { if (++work > 128 || own.length >= 500) break; if (child.nodeType === 3) own += child.textContent?.slice(0, 500 - own.length) ?? ""; }
  return text(own) || null;
}
function states(el: Element): string[] {
  const result: string[] = [];
  const disabled = el.matches(":disabled") || el.closest('[inert], [aria-disabled="true"]');
  if (!disabled) result.push("enabled");
  if (el === (el.getRootNode() as Document | ShadowRoot).activeElement) result.push("focused");
  if ((el as HTMLElement).tabIndex >= 0) result.push("focusable");
  if (["INPUT", "TEXTAREA"].includes(el.tagName) && !(el as HTMLInputElement).readOnly) result.push("editable", "settable");
  if (el.tagName === "SELECT") result.push("settable");
  if (password(el)) result.push("password");
  for (const [attr, state] of [["aria-selected", "selected"], ["aria-checked", "checked"], ["aria-expanded", "expanded"], ["aria-busy", "busy"], ["aria-modal", "modal"], ["aria-required", "required"]]) {
    if (el.getAttribute(attr) === "true") result.push(state);
  }
  if ((el as HTMLInputElement).checked && !result.includes("checked")) result.push("checked");
  if ((el as HTMLOptionElement).selected && !result.includes("selected")) result.push("selected");
  if (el.tagName === "OPTION" || el.hasAttribute("aria-selected")) result.push("selectable");
  if ((el as HTMLInputElement).required && !result.includes("required")) result.push("required");
  if (el.hasAttribute("aria-expanded")) result.push("expandable");
  if (el.getAttribute("aria-checked") === "mixed" || (el as HTMLInputElement).indeterminate) result.push("mixed");
  return result;
}
function value(el: Element): NodeValue | null {
  if (password(el)) return { text: "•••" }; // Deliberately do not access the getter.
  if (el.tagName === "INPUT" && (el as HTMLInputElement).type === "hidden") return null;
  if (el.tagName === "INPUT") {
    const input = el as HTMLInputElement;
    if (["checkbox", "radio"].includes(input.type)) return { bool: input.checked };
    if (["range", "number"].includes(input.type) && Number.isFinite(input.valueAsNumber)) return { number: input.valueAsNumber };
  }
  if (["INPUT", "TEXTAREA", "SELECT"].includes(el.tagName)) return { text: (el as HTMLInputElement).value.slice(0, 500) };
  return null;
}
function frame(el: Element, context: Context, scale: number): Rect | null {
  const box = el.getBoundingClientRect();
  const values = [box.x, box.y, box.width, box.height, context.x, context.y, scale];
  if (!values.every(Number.isFinite) || scale <= 0) return null;
  return { x: (box.x + context.x) * scale, y: (box.y + context.y) * scale, w: box.width * scale, h: box.height * scale };
}
function stamp(el: Element, context: Context, scale: number, includeValue = true): string {
  // Local only: never serialize this fingerprint, which includes unredacted field values.
  return JSON.stringify([role(el), name(el), states(el), includeValue ? value(el) : null, frame(el, context, scale), context.path,
    context.documents.map(doc => doc.URL), context.frames.map(frame => [frame.getBoundingClientRect().toJSON(), frame.clientLeft, frame.clientTop]),
    el.getAttribute("href"), el.getAttribute("type"), el.getAttribute("formaction"), el.getAttribute("target"),
    (el as HTMLButtonElement).form?.action, (el as HTMLButtonElement).form?.target]);
}

/** One bounded traversal and opaque handle table. Construction never reads the tree. */
export class DomTree {
  private targets = new Map<string, Target>();
  private snapshot: DomSnapshot | null = null;
  private documents = new Map<Document, string>();
  private observers: MutationObserver[] = [];
  private dirty = false;
  constructor(private readonly win: Window, private readonly onDocument: (doc: Document) => void,
    private readonly onFrame: (frame: Element) => void) {}

  /** Drops all content and handles immediately; no mutation or asynchronous work. */
  clear(): void {
    for (const observer of this.observers) observer.disconnect();
    this.observers = []; this.dirty = false;
    this.targets.clear(); this.documents.clear(); this.snapshot = null;
  }

  private watch(root: Document | ShadowRoot): void {
    const doc = root.nodeType === 9 ? root as Document : root.ownerDocument!;
    const Observer = (doc.defaultView as unknown as typeof globalThis).MutationObserver;
    const observer = new Observer(() => { this.dirty = true; });
    observer.observe(root, { subtree: true, childList: true, characterData: true, attributes: true });
    this.observers.push(observer);
  }
  private mutated(): boolean {
    if (this.observers.some(observer => observer.takeRecords().length > 0)) this.dirty = true;
    return this.dirty;
  }

  /** Synchronous, bounded to 5000 nodes, depth 64 and two seconds. No logging. */
  observe(): DomSnapshot {
    this.clear();
    const started = performance.now();
    const scale = this.win.devicePixelRatio;
    const id = crypto.randomUUID();
    let count = 0;
    let incomplete: DomSnapshot["incomplete"] = null;
    let handoff = false;
    const partial = (reason: NonNullable<DomSnapshot["incomplete"]>["reason"]) => {
      if (!incomplete || reason === "deadline") incomplete = { reason, read_nodes: count, unread_nodes: null };
      handoff = true;
    };
    const register = (doc: Document) => {
      if (this.documents.has(doc)) return;
      this.documents.set(doc, doc.URL);
      this.watch(doc);
      this.onDocument(doc);
      if (executionSurface(`${doc.title} ${doc.URL}`)) handoff = true;
    };
    const walk = (el: Element, context: Context, depth: number): DomNode | null => {
      if (performance.now() - started > 2000) { partial("deadline"); return null; }
      if (count >= 5000) { partial("node_cap"); return null; }
      if (depth > 64) { partial("depth_cap"); return null; }
      if (ignored.has(el.tagName)) return null;
      count++;
      const node: DomNode = { id: `n${count}`, role: role(el), native_role: (el.getAttribute("role") ?? el.tagName.toLowerCase()).slice(0, 128),
        name: name(el), value: value(el), states: states(el), frame: frame(el, context, scale), children: [] };
      if (!node.frame) partial("partial_error");
      const style = el.ownerDocument.defaultView?.getComputedStyle(el);
      if (el.hasAttribute("hidden") || style?.display === "none" || style?.visibility === "hidden" ||
        (node.frame && (node.frame.x + node.frame.w <= 0 || node.frame.y + node.frame.h <= 0 || node.frame.x >= this.win.innerWidth * scale || node.frame.y >= this.win.innerHeight * scale))) node.states.push("offscreen");
      if (executionSurface(`${el.id} ${el.getAttribute("class")} ${el.getAttribute("aria-label")} ${el.getAttribute("role")}`)) handoff = true;
      if (["CANVAS", "OBJECT", "EMBED"].includes(el.tagName)) handoff = true;
      if (node.role === "unknown" && ((el as HTMLElement).tabIndex >= 0 || (el as HTMLElement).isContentEditable)) handoff = true;
      this.targets.set(node.id, { element: el, context, stamp: stamp(el, context, scale), ancestors: ancestors(el) });
      const append = (child: Element, ctx = context) => {
        try { const result = walk(child, ctx, depth + 1); if (result) node.children.push(result); }
        catch { partial("partial_error"); }
      };
      if (el.tagName === "IFRAME" || el.tagName === "FRAME") {
        this.onFrame(el);
        const child = (el as HTMLIFrameElement).contentDocument;
        const transformed = [el, ...ancestors(el).filter(n => n.nodeType === 1) as Element[]].some(ancestor => {
          const css = ancestor.ownerDocument.defaultView?.getComputedStyle(ancestor);
          return css?.transform && css.transform !== "none" || css?.zoom && css.zoom !== "1" && css.zoom !== "normal";
        });
        if (!child?.documentElement || transformed) partial("partial_error");
        else {
          register(child);
          const box = el.getBoundingClientRect();
          append(child.documentElement, { x: context.x + box.left + el.clientLeft, y: context.y + box.top + el.clientTop,
            path: `${context.path}/${node.id}`, documents: [...context.documents, child], frames: [...context.frames, el as HTMLIFrameElement] });
        }
      } else {
        // Open shadow roots and light DOM are both retained; closed roots are opaque.
        for (const child of el.children) { append(child); if (count >= 5000 || performance.now() - started > 2000) { partial(count >= 5000 ? "node_cap" : "deadline"); break; } }
        if (el.shadowRoot) { this.watch(el.shadowRoot); for (const child of el.shadowRoot.children) append(child); }
        else if (el.localName.includes("-")) { node.role = "unknown"; partial("partial_error"); }
      }
      return node;
    };
    register(this.win.document);
    let root: DomNode | null = null;
    try { root = walk(this.win.document.documentElement, { x: 0, y: 0, path: "top", documents: [this.win.document], frames: [] }, 0); }
    catch { partial("partial_error"); }
    if (!root) { partial("partial_error"); root = { id: "unread", role: "unknown", native_role: "unread", name: null, value: null, states: [], frame: null, children: [] }; }
    if (incomplete) (incomplete as NonNullable<DomSnapshot["incomplete"]>).read_nodes = count;
    if (this.win.visualViewport && (this.win.visualViewport.scale !== 1 || this.win.visualViewport.offsetLeft !== 0 || this.win.visualViewport.offsetTop !== 0)) partial("partial_error");
    this.snapshot = { id, root, frameSpace: "tab_viewport_physical", geometry: { width: this.win.innerWidth, height: this.win.innerHeight, scale }, incomplete, handoff };
    return this.snapshot;
  }

  /** Consumes the snapshot before any event; never retries a target by name. */
  perform(op: Exclude<DomOperation, { type: "observe" }>, live: () => boolean): DomResult {
    const state = this.snapshot;
    if (!state || state.id !== op.snapshotId) return error("stale_element");
    if (state.handoff || state.incomplete) return error("handoff_required");
    const target = this.targets.get(op.nodeId);
    if (!target) return error("stale_element");
    const { element: el, context } = target;
    const current = () => live() && !this.mutated() && el.isConnected && context.documents.includes(el.ownerDocument) &&
      context.frames.every((frame, i) => frame.contentDocument === context.documents[i + 1]) &&
      ancestors(el).length === target.ancestors.length && ancestors(el).every((ancestor, i) => ancestor === target.ancestors[i]) &&
      [...this.documents].every(([doc, url]) => doc.URL === url) &&
      this.win.innerWidth === state.geometry.width && this.win.innerHeight === state.geometry.height &&
      this.win.devicePixelRatio === state.geometry.scale && stamp(el, context, state.geometry.scale) === target.stamp;
    if (!current()) return error("stale_element");
    const targetStates = states(el);
    const box = el.getBoundingClientRect();
    const style = el.ownerDocument.defaultView?.getComputedStyle(el);
    if (!targetStates.includes("enabled") || el.closest("[hidden], [inert]") || style?.display === "none" || style?.visibility === "hidden" ||
      box.width <= 0 || box.height <= 0 || box.right <= 0 || box.bottom <= 0 || box.left >= el.ownerDocument.defaultView!.innerWidth || box.top >= el.ownerDocument.defaultView!.innerHeight) return error("element_not_actionable");
    const field = ["INPUT", "TEXTAREA"].includes(el.tagName) && (el.tagName === "TEXTAREA" ||
      ["text", "password", "search", "email", "url", "tel", "number"].includes((el as HTMLInputElement).type));
    if (op.type === "set_value" && (!field || (el as HTMLInputElement).readOnly)) return error("not_settable");
    if (op.type === "select") {
      if (el.tagName !== "SELECT") return error("element_not_actionable");
      const options = Array.from((el as HTMLSelectElement).options).filter(o => o.value === op.value);
      if (options.length !== 1 || options[0].disabled || (options[0].parentElement?.tagName === "OPTGROUP" &&
        (options[0].parentElement as HTMLOptGroupElement).disabled)) return error("element_not_actionable");
    }
    if (op.type === "click" && (!["button", "link", "check_box", "radio", "switch", "tab", "menu_item", "list_item"].includes(role(el)) || (el as HTMLInputElement).type === "file")) return error("element_not_actionable");
    if (op.type === "scroll" && !/(auto|scroll)/.test(`${el.ownerDocument.defaultView?.getComputedStyle(el).overflow} ${el.ownerDocument.defaultView?.getComputedStyle(el).overflowY} ${el.ownerDocument.defaultView?.getComputedStyle(el).overflowX}`)) return error("element_not_actionable");
    // Re-scan before dispatch: mixed execution interfaces may have appeared since observation.
    const fresh = this.observe();
    if (fresh.handoff || fresh.incomplete) return error("handoff_required");
    // observe rebuilt the handle table, but the original exact element and fingerprint remain the target.
    if (!current()) return error("stale_element");
    const identity = stamp(el, context, state.geometry.scale, false);
    // Consume handles but retain mutation/document guards across synchronous page handlers.
    this.targets.clear(); this.snapshot = null;
    const afterEvent = () => live() && !this.mutated() && el.isConnected &&
      context.frames.every((frame, i) => frame.contentDocument === context.documents[i + 1]) &&
      context.documents.every(doc => doc.URL === this.documents.get(doc)) && stamp(el, context, state.geometry.scale, false) === identity;
    let started = false;
    try {
      if (!live()) return error("app_denied");
      started = true;
      const view = el.ownerDocument.defaultView!;
      if (op.type === "click") (el as HTMLElement).click();
      else if (op.type === "scroll") el.scrollTo({ left: el.scrollLeft + op.dx, top: el.scrollTop + op.dy, behavior: "instant" });
      else {
        const field = el as HTMLInputElement | HTMLTextAreaElement | HTMLSelectElement;
        const prototype = el.tagName === "INPUT" ? (view as unknown as typeof globalThis).HTMLInputElement.prototype :
          el.tagName === "TEXTAREA" ? (view as unknown as typeof globalThis).HTMLTextAreaElement.prototype : (view as unknown as typeof globalThis).HTMLSelectElement.prototype;
        Object.getOwnPropertyDescriptor(prototype, "value")?.set?.call(field, op.value);
        field.dispatchEvent(new (view as unknown as typeof globalThis).Event("input", { bubbles: true, composed: true }));
        if (!afterEvent()) return { kind: "action", outcome: "unknown" };
        field.dispatchEvent(new (view as unknown as typeof globalThis).Event("change", { bubbles: true }));
        if (field.value !== op.value) return { kind: "action", outcome: "unknown" };
      }
      return { kind: "action", outcome: live() && el.isConnected ? "done" : "unknown" };
    } catch { return started ? { kind: "action", outcome: "unknown" } : error("driver_error"); }
    finally { this.clear(); }
  }
}
