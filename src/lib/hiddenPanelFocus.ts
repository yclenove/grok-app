/**
 * Return keyboard focus when a persist-mounted panel is hidden.
 * Call with preventScroll so restoring cannot pan an overflow ancestor.
 */

export function restoreFocusFromHiddenPanel(
  panel: ParentNode | null | undefined,
  restoreTo?: { focus: (opts?: { preventScroll?: boolean }) => void } | null,
): boolean {
  if (typeof document === "undefined") return false;
  const active = document.activeElement;
  if (!(active instanceof HTMLElement)) return false;
  if (!panel || !panel.contains(active)) return false;
  if (restoreTo && typeof restoreTo.focus === "function") {
    restoreTo.focus({ preventScroll: true });
    // A hidden responsive duplicate cannot accept focus. Do not leave focus
    // parked inside the panel that is about to become inert.
    if (document.activeElement === active) active.blur();
  } else {
    active.blur();
  }
  return true;
}

export function visibleBottomTerminalToggle(
  root: ParentNode | Document | null = typeof document !== "undefined"
    ? document
    : null,
): HTMLElement | null {
  if (!root) return null;
  const nodes = root.querySelectorAll<HTMLElement>(
    '[data-testid="bottom-terminal-toggle"]',
  );
  for (const el of nodes) {
    if (el.getClientRects().length > 0) return el;
  }
  // jsdom has no layout and reports no rectangles for otherwise focusable
  // controls. The fallback also covers the short pre-paint window in-app;
  // restoreFocusFromHiddenPanel verifies that focus actually moved.
  return nodes.item(0);
}
