const MAX_PAGES = 8;
const EVENTS_PER_PAGE = 100;
export const MAX_READER_EVENTS = MAX_PAGES * EVENTS_PER_PAGE;
export const MAX_READER_CHARS = 2 * 1024 * 1024;

/** Keep the newest loaded events inside 8 pages / 2 MiB. */
export function retainRecentEvents(list: unknown[]): unknown[] {
  let out = list.length > MAX_READER_EVENTS ? list.slice(list.length - MAX_READER_EVENTS) : list;
  const utf8 = new TextEncoder();
  let size = 0;
  for (let i = out.length - 1; i >= 0; i--) {
    size += utf8.encode(JSON.stringify(out[i] ?? "")).length;
    if (size > MAX_READER_CHARS) {
      // The reader display independently truncates long bodies. Keep the
      // newest event addressable even when that one record exceeds this
      // retained-state budget; returning [] would make a successful jump land
      // on no DOM target at all.
      return out.slice(Math.min(i + 1, out.length - 1));
    }
  }
  return out;
}
