/** Search-result navigation shared by the reader buttons and letter shortcuts. */
export function nextReaderHit(
  ids: readonly string[],
  current: string | null,
  direction: -1 | 1,
): string | null {
  if (ids.length === 0) return null;
  const index = current == null ? -1 : ids.indexOf(current);
  const next = index < 0
    ? direction === 1 ? 0 : ids.length - 1
    : (index + direction + ids.length) % ids.length;
  return ids[next] ?? null;
}

type ReaderKeyEvent = Pick<KeyboardEvent,
  "key" | "keyCode" | "target" | "defaultPrevented" | "isComposing" |
  "ctrlKey" | "altKey" | "metaKey" | "shiftKey"
>;

const TYPING_OR_TYPEAHEAD =
  "input, textarea, select, [contenteditable]:not([contenteditable='false']), [role='textbox'], [role='combobox'], [role='listbox'], [role='option']";

/** Never consume editing, IME, or app/OS key chords as reader navigation. */
export function readerShortcutDirection(event: ReaderKeyEvent): -1 | 1 | null {
  if (event.defaultPrevented || event.isComposing || event.keyCode === 229 ||
      event.ctrlKey || event.altKey || event.metaKey || event.shiftKey) return null;
  if (event.target instanceof Element && event.target.closest(TYPING_OR_TYPEAHEAD)) return null;
  return event.key === "n" ? 1 : event.key === "p" ? -1 : null;
}
