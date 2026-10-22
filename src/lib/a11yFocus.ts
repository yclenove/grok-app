/**
 * Small focus helpers for dialogs / permission bars (T15).
 * No React dependency — unit-testable.
 */

const FOCUSABLE_SEL = [
  "a[href]",
  "button:not([disabled])",
  "textarea:not([disabled])",
  "input:not([disabled]):not([type='hidden'])",
  "select:not([disabled])",
  "[tabindex]:not([tabindex='-1'])",
].join(",");

function isConcealed(el: HTMLElement): boolean {
  if (el.getAttribute("aria-hidden") === "true") return true;
  if (el.hasAttribute("hidden") || el.hasAttribute("inert")) return true;
  const style =
    typeof window !== "undefined" ? window.getComputedStyle(el) : null;
  return !!style && (style.visibility === "hidden" || style.display === "none");
}

/**
 * Visible, enabled focus targets inside `root`.
 * A control under a hidden, inert, or aria-hidden ancestor is not a candidate.
 * This does not use offsetParent: that is null for valid fixed controls and
 * for every control in jsdom.
 */
export function listFocusable(root: ParentNode | null | undefined): HTMLElement[] {
  if (!root || typeof (root as Element).querySelectorAll !== "function") {
    return [];
  }
  const rootEl = root as Element;
  const nodes = Array.from(rootEl.querySelectorAll<HTMLElement>(FOCUSABLE_SEL));
  return nodes.filter((el) => {
    if (el.hasAttribute("disabled")) return false;
    let node: HTMLElement | null = el;
    while (node) {
      if (isConcealed(node)) return false;
      if (node === rootEl) break;
      const parent: HTMLElement | null = node.parentElement;
      if (!parent || parent === node) break;
      if (typeof rootEl.contains === "function" && !rootEl.contains(parent)) break;
      node = parent;
    }
    return true;
  });
}

/** Focus the first focusable control; returns it or null. */
export function focusFirst(
  root: ParentNode | null | undefined,
): HTMLElement | null {
  const list = listFocusable(root);
  const el = list[0] ?? null;
  el?.focus();
  return el;
}

/**
 * Keep Tab / Shift+Tab cycling inside `root` (basic focus trap).
 * Call from keydown when the dialog is open.
 */
export function trapTabKey(
  e: { key: string; shiftKey: boolean; preventDefault: () => void },
  root: ParentNode | null | undefined,
): void {
  if (e.key !== "Tab") return;
  const list = listFocusable(root);
  if (list.length === 0) {
    e.preventDefault();
    return;
  }
  const first = list[0]!;
  const last = list[list.length - 1]!;
  const active =
    typeof document !== "undefined"
      ? (document.activeElement as HTMLElement | null)
      : null;

  if (e.shiftKey) {
    if (!active || active === first || !rootContains(root, active)) {
      e.preventDefault();
      last.focus();
    }
  } else if (!active || active === last || !rootContains(root, active)) {
    e.preventDefault();
    first.focus();
  }
}

function rootContains(
  root: ParentNode | null | undefined,
  el: Node | null,
): boolean {
  if (!root || !el) return false;
  if (root === el) return true;
  return typeof (root as Node).contains === "function"
    ? (root as Node).contains(el)
    : false;
}

/** Prefer primary allow / solid button when focusing a permission bar. */
export function preferPermissionFocus(
  root: ParentNode | null | undefined,
): HTMLElement | null {
  if (!root || typeof (root as Element).querySelector !== "function") {
    return focusFirst(root);
  }
  const allow = (root as Element).querySelector<HTMLElement>(
    ".perm-bar__btn--allow, .perm-bar__btn--session, .btn--primary, .btn--solid",
  );
  if (allow && !allow.hasAttribute("disabled")) {
    allow.focus();
    return allow;
  }
  return focusFirst(root);
}

/**
 * True when the event target is an input surface where plain letter keys
 * should type (not trigger sidebar j/k or similar chrome shortcuts).
 */
export function isTypingTarget(el: EventTarget | null | undefined): boolean {
  if (!el || typeof (el as HTMLElement).tagName !== "string") return false;
  const node = el as HTMLElement;
  const tag = node.tagName.toLowerCase();
  if (tag === "input" || tag === "textarea" || tag === "select") return true;
  if (node.isContentEditable) return true;
  // Nested editable (e.g. inside a contenteditable child).
  if (typeof node.closest === "function") {
    if (node.closest("input, textarea, select, [contenteditable='true']")) {
      return true;
    }
  }
  return false;
}

export type InstallDialogFocusOptions = {
  /** Escape closes / cancels. */
  onEscape?: () => void;
  /** Use capture-phase keydown (default true — wins over body handlers). */
  capture?: boolean;
  /**
   * Initial focus strategy after paint:
   * - `"first"` (default) — {@link focusFirst}
   * - `"none"` — caller focuses (e.g. confirm primary / prompt input)
   * - getter — focus that element when present, else first
   */
  initialFocus?: "first" | "none" | (() => HTMLElement | null | undefined);
  /** Restore on cleanup (default true); a getter can exempt deliberate Tab/outside focus moves. */
  restoreFocus?: boolean | (() => boolean);
  /** Override the Tab trap for a scoped popup that returns to its trigger's tab order. */
  onTab?: (event: KeyboardEvent) => void;
};

type DialogFocusScope = {
  getRoot: () => ParentNode | null | undefined;
  returnFocus: HTMLElement | null;
  lastFocused: HTMLElement | null;
};

// Dialogs can be independent portals and use different event phases. A DOM
// propagation stop alone cannot keep older document listeners from running.
const dialogFocusScopes = new WeakMap<Document, DialogFocusScope[]>();

/** Recheck this inside delayed focus work so a newer modal keeps ownership. */
export function isDialogFocusAllowed(target: Node): boolean {
  const scopes = target.ownerDocument && dialogFocusScopes.get(target.ownerDocument);
  const top = scopes?.[scopes.length - 1];
  return !top || rootContains(top.getRoot(), target);
}

/**
 * Wire a basic dialog focus lifecycle for an open panel:
 * remember previous focus → optional initial focus → Tab trap → Escape.
 * Only the most recently opened scope owns keyboard / initial focus. Closing
 * a covered scope must not steal focus from the still-open panel above it.
 *
 * Returns cleanup (remove listeners + restore focus). Call from `useEffect`
 * when the dialog is open.
 */
export function installDialogFocus(
  getRoot: () => ParentNode | null | undefined,
  opts: InstallDialogFocusOptions = {},
): () => void {
  const {
    onEscape,
    capture = true,
    initialFocus = "first",
    restoreFocus = true,
    onTab,
  } = opts;

  const doc = typeof document !== "undefined" ? document : null;
  const scopes = (doc && dialogFocusScopes.get(doc)) || [];
  if (doc) dialogFocusScopes.set(doc, scopes);
  // React clears refs during the mutation phase, before passive cleanup. Keep
  // the last panel so return targets in a detached portal can still be linked.
  let lastRoot = getRoot();
  const currentRoot = () => {
    const root = getRoot();
    if (root) lastRoot = root;
    return root;
  };
  const active = (doc?.activeElement as HTMLElement | null) ?? null;
  // Async preparation may disable its trigger before this panel mounts. The
  // browser then reports body, not the control to which closing should return.
  // Only use the parent's remembered control when focus is actually lost.
  const parent = scopes[scopes.length - 1];
  const focusLost = !active || active === doc?.body || active === doc?.documentElement;
  const scope: DialogFocusScope = {
    getRoot: currentRoot,
    returnFocus: focusLost ? parent?.lastFocused ?? active : active,
    lastFocused: rootContains(currentRoot(), active) ? active : null,
  };
  scopes.push(scope);
  let disposed = false;
  const isTop = () => !disposed && scopes[scopes.length - 1] === scope;

  let focusTimer: number | undefined;
  if (initialFocus !== "none" && typeof window !== "undefined") {
    focusTimer = window.setTimeout(() => {
      if (!isTop() || !currentRoot()) return;
      if (typeof initialFocus === "function") {
        const el = initialFocus();
        if (el && typeof el.focus === "function") {
          el.focus();
          return;
        }
      }
      focusFirst(currentRoot());
    }, 0) as unknown as number;
  }

  const onKey = (e: KeyboardEvent) => {
    if (
      !isTop() || !currentRoot() || e.defaultPrevented ||
      e.isComposing || e.keyCode === 229
    ) {
      return;
    }
    if (e.key === "Escape" && onEscape) {
      e.preventDefault();
      e.stopPropagation();
      onEscape();
      return;
    }
    if (e.key === "Tab" && onTab) onTab(e);
    else trapTabKey(e, currentRoot());
  };

  const onFocus = () => {
    const focused = (doc?.activeElement as HTMLElement | null) ?? null;
    if (isTop() && rootContains(currentRoot(), focused)) scope.lastFocused = focused;
  };
  doc?.addEventListener("keydown", onKey, capture);
  doc?.addEventListener("focusin", onFocus, true);

  return () => {
    if (disposed) return;
    const wasTop = isTop();
    disposed = true;
    if (focusTimer != null && typeof window !== "undefined") {
      window.clearTimeout(focusTimer);
    }
    doc?.removeEventListener("keydown", onKey, capture);
    doc?.removeEventListener("focusin", onFocus, true);
    scopes.splice(scopes.indexOf(scope), 1);
    // A parent may disappear while its child remains open (navigation / async
    // completion). Carry the original trigger forward even if refs are gone.
    const closingRoot = currentRoot() ?? lastRoot;
    for (const remaining of scopes) {
      if (rootContains(closingRoot, remaining.returnFocus)) {
        remaining.returnFocus = scope.returnFocus;
      }
    }
    if (doc && scopes.length === 0) dialogFocusScopes.delete(doc);
    if (!wasTop || !restoreFocus) return;

    const remainingRoot = scopes[scopes.length - 1]?.getRoot();
    const target = scope.returnFocus;
    try {
      if (typeof restoreFocus === "function" && !restoreFocus()) return;
      if (
        target && target.isConnected !== false &&
        !target.hasAttribute("disabled") && typeof target.focus === "function" &&
        (!remainingRoot || rootContains(remainingRoot, target))
      ) {
        target.focus();
      } else if (remainingRoot) {
        focusFirst(remainingRoot);
      }
    } catch {
      /* A removed/invalid restore target must not break dialog cleanup. */
    }
  };
}

/**
 * Move selection among a linear list of focusable controls (settings nav,
 * radiogroup, etc.). Clamps at ends. Returns the focused element or null.
 */
export function focusRelative(
  list: readonly HTMLElement[],
  current: HTMLElement | null | undefined,
  dir: "next" | "prev",
): HTMLElement | null {
  if (list.length === 0) return null;
  const idx = current ? list.indexOf(current) : -1;
  let nextIdx: number;
  if (dir === "next") {
    nextIdx = idx < 0 ? 0 : Math.min(list.length - 1, idx + 1);
  } else {
    nextIdx = idx < 0 ? list.length - 1 : Math.max(0, idx - 1);
  }
  const el = list[nextIdx] ?? null;
  el?.focus();
  return el;
}

/**
 * Index helper for arrow-key nav in pure tests / components.
 * Same clamp semantics as {@link focusRelative}.
 */
export function nextIndex(
  length: number,
  current: number,
  dir: "next" | "prev",
): number {
  if (length <= 0) return -1;
  if (current < 0 || current >= length) {
    return dir === "next" ? 0 : length - 1;
  }
  if (dir === "next") return Math.min(length - 1, current + 1);
  return Math.max(0, current - 1);
}
