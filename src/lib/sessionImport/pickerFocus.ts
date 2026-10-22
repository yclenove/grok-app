import { isDialogFocusAllowed } from "@/lib/a11yFocus";

/**
 * Owns the import control that opened a native picker.
 * Disabling that control drops DOM focus. Restoration waits until this same
 * claim has settled and the control is enabled again. A newer claim, a
 * disconnected node, a real focus move, or a newer dialog drops the claim.
 * A false result means this operation no longer owns the outcome, so the
 * claim is dropped without focusing. An owned success may change the source
 * choice and still restore this same surface.
 */

type PickerClaim = {
  id: number;
  target: HTMLElement;
  settled: boolean;
};

let claim: PickerClaim | null = null;
let nextId = 0;

function focusIsLost(target: HTMLElement): boolean {
  const doc = target.ownerDocument;
  const active = doc.activeElement;
  return !active || active === doc.body || active === doc.documentElement || active === target;
}

function controlDisabled(target: HTMLElement): boolean {
  if (target.hasAttribute("disabled") || target.getAttribute("aria-disabled") === "true") return true;
  return "disabled" in target && (target as HTMLButtonElement).disabled === true;
}

function drop(current: PickerClaim): void {
  if (claim === current) claim = null;
}

/** Run `work` for this trigger. A later claim makes this completion a no-op. */
export function ownPickerTrigger(
  target: HTMLElement,
  work: () => void | Promise<unknown>,
): Promise<void> {
  const id = ++nextId;
  claim = { id, target, settled: false };
  let pending: Promise<unknown>;
  try {
    pending = Promise.resolve(work());
  } catch (cause) {
    pending = Promise.reject(cause);
  }
  // False is an ownership result, not a generation comparison. The same
  // successful call may bump sourceGeneration via setChoice and still return
  // true. Rejection stays restorable because the hook reports that itself.
  let stale = false;
  const tracked = pending.then((result) => {
    stale = result === false;
  });
  return tracked.finally(() => {
    const current = claim;
    if (current?.id !== id) return;
    current.settled = true;
    if (stale) {
      drop(current);
      return;
    }
    restorePickerTrigger();
  });
}

/**
 * Move focus back to the settled trigger once, or drop the claim.
 * A still-disabled control keeps the claim for the commit that re-enables it.
 */
export function restorePickerTrigger(): boolean {
  const current = claim;
  if (!current?.settled) return false;
  const target = current.target;
  if (!target.isConnected) {
    drop(current);
    return false;
  }
  if (!focusIsLost(target)) {
    drop(current);
    return false;
  }
  if (!isDialogFocusAllowed(target)) {
    drop(current);
    return false;
  }
  if (controlDisabled(target)) return false;
  try {
    target.focus({ preventScroll: true });
  } catch {
    drop(current);
    return false;
  }
  const restored = target.ownerDocument.activeElement === target;
  drop(current);
  return restored;
}

/** Drop a claim whose trigger was removed. A connected newer claim stays. */
export function releaseDisconnectedPickerTrigger(): void {
  if (claim && !claim.target.isConnected) claim = null;
}

/** Keep a surface's generic lost-focus fallback behind its owned picker. */
export function hasPickerTriggerWithin(surface: HTMLElement): boolean {
  return !!claim && claim.target.isConnected && surface.contains(claim.target);
}
