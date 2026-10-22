import { importedEventId } from "./eventView";

export const MAX_READER_HITS = 200;
export const MAX_RENDERED_EVENTS = 160;

export function capReaderHits<T>(items: T[]): { items: T[]; windowed: boolean } {
  if (items.length <= MAX_READER_HITS) return { items, windowed: false };
  return {
    items: items.slice(items.length - MAX_READER_HITS),
    windowed: true,
  };
}

export function windowReaderEvents(
  events: unknown[],
  anchorEventId?: string | null,
): { items: unknown[]; start: number; end: number; windowed: boolean } {
  if (events.length <= MAX_RENDERED_EVENTS) {
    return { items: events, start: 0, end: events.length, windowed: false };
  }
  const anchor = anchorEventId
    ? events.findIndex((event) => importedEventId(event) === anchorEventId)
    : -1;
  const preferred = anchor >= 0 ? anchor - Math.floor(MAX_RENDERED_EVENTS / 3) : events.length;
  const start = Math.max(0, Math.min(preferred, events.length - MAX_RENDERED_EVENTS));
  const end = start + MAX_RENDERED_EVENTS;
  return { items: events.slice(start, end), start, end, windowed: true };
}
