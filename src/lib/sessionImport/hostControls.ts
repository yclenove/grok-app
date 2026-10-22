/** Wizard Host interaction policy. Pointer and keyboard share these gates. */

export function visibleImportErrorText(
  error: string | null | undefined,
): string | null {
  if (typeof error !== "string" || error.length === 0) return null;
  return error;
}

const ACTIVATION_OWNING =
  "button, input, textarea, select, summary, a[href], [role='button'], [role='combobox'], [role='listbox'], [role='option'], [contenteditable]:not([contenteditable='false'])";

/** True when Enter/Space already belongs to the focused control. */
export function activationOwningTarget(target: EventTarget | null): boolean {
  if (!(target instanceof Element)) return false;
  return target.closest(ACTIVATION_OWNING) != null;
}

export type ContentKeyContext = {
  busy: boolean;
  composing: boolean;
  inTextField: boolean;
  onControl: boolean;
  hasNextPage: boolean;
  canPreviousPage: boolean;
  hasFocusedCandidate: boolean;
};

export type ContentKeyCommand =
  | "loadMore"
  | "previousPage"
  | "toggleFocused"
  | "peekFocused"
  | "focusFilter"
  | "none";

export function contentKeyCommand(
  key: string,
  ctx: ContentKeyContext,
): ContentKeyCommand {
  if (ctx.inTextField || ctx.onControl || ctx.composing) return "none";
  if (ctx.busy) return "none";
  if (key === "ArrowRight" && ctx.hasNextPage) return "loadMore";
  if (key === "ArrowLeft" && ctx.canPreviousPage) return "previousPage";
  if (key === " " && ctx.hasFocusedCandidate) return "toggleFocused";
  if (key === "Enter" && ctx.hasFocusedCandidate) return "peekFocused";
  if (key === "/") return "focusFilter";
  return "none";
}

export function mappingKeyCommand(
  key: string,
  ctx: { busy: boolean; composing: boolean; onControl: boolean },
): "commit" | "none" {
  if (ctx.onControl || ctx.composing || ctx.busy) return "none";
  if (key === "Enter") return "commit";
  return "none";
}
