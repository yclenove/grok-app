/** Cursor stack for one job detail window. Forward motion drops cursors past the
 * current page, so the length stays at deepestPage + 1 instead of growing once
 * per click. */
export function nextJobCursorHistory(
  cursors: ReadonlyArray<string | null>,
  page: number,
  cursor: string,
): Array<string | null> {
  return [...cursors.slice(0, page + 1), cursor];
}
