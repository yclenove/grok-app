/** Normalize imported event JSON for the reader (flatten or nested `base`). */

export const DISPLAY_TEXT_CAP = 4000;

type Rec = Record<string, unknown>;

function asRec(v: unknown): Rec | null {
  return v !== null && typeof v === "object" ? (v as Rec) : null;
}

export function importedEventId(raw: unknown): string | null {
  const o = asRec(raw);
  if (!o) return null;
  if (typeof o.id === "string" && o.id) return o.id;
  if (typeof o.eventId === "string" && o.eventId) return o.eventId;
  const base = asRec(o.base);
  if (base && typeof base.id === "string" && base.id) return base.id;
  return null;
}

export type EventPart =
  | { kind: "text"; text: string; fullText?: string }
  | { kind: "asset"; assetId: string; alt?: string }
  | { kind: "unsupported"; sourceType: string }
  | { kind: "truncated" }
  | { kind: "incomplete" }
  | { kind: "tool-input-payload"; blobId: string }
  | { kind: "system"; text: string; fullText?: string };

export type EventDisplay = {
  id: string | null;
  kind: string;
  role?: string;
  heading: string;
  parts: EventPart[];
  flags: {
    unsupported: boolean;
    truncated: boolean;
    incomplete: boolean;
    displayClipped: boolean;
    system: boolean;
  };
};

/** Bound DOM text without splitting a UTF-16 surrogate pair between pages. */
export function boundedTextEnd(text: string, start = 0): number {
  let end = Math.min(text.length, start + DISPLAY_TEXT_CAP);
  if (end < text.length && text.charCodeAt(end - 1) >= 0xd800 &&
      text.charCodeAt(end - 1) <= 0xdbff && text.charCodeAt(end) >= 0xdc00 &&
      text.charCodeAt(end) <= 0xdfff) end -= 1;
  return end;
}

function aliasedString(value: Rec, current: string, legacy: string): string {
  const first = value[current];
  if (typeof first === "string" && first) return first;
  return typeof value[legacy] === "string" ? value[legacy] as string : "";
}

function pushCapped(
  parts: EventPart[],
  kind: "text" | "system",
  text: string,
  flags: EventDisplay["flags"],
) {
  if (text.length > DISPLAY_TEXT_CAP) {
    flags.displayClipped = true;
    parts.push({ kind, text: text.slice(0, boundedTextEnd(text)), fullText: text });
  } else {
    parts.push({ kind, text });
  }
}

function collectBlocks(o: Rec): Rec[] {
  const raw = Array.isArray(o.displayParts)
    ? o.displayParts
    : Array.isArray(o.blocks)
    ? o.blocks
    : Array.isArray(o.summary)
      ? o.summary
      : [];
  return raw.map(asRec).filter((b): b is Rec => b !== null);
}

export function importedEventDisplay(raw: unknown): EventDisplay {
  const o = asRec(raw) ?? {};
  const id = importedEventId(o);
  const kind = typeof o.kind === "string" ? o.kind : "message";
  const role = typeof o.role === "string" ? o.role : undefined;
  const blocks = collectBlocks(o);
  const preview = Array.isArray(o.displayParts);
  const flags = {
    unsupported: kind === "unsupported",
    truncated: (!preview && o.truncated === true) ||
      (o.complete === false && !blocks.some((block) => block.kind === "asset")),
    displayClipped: o.displayLimited === true || (preview && o.truncated === true),
    incomplete: o.complete === false && blocks.some((block) => block.kind === "asset"),
    system: role === "system",
  };
  const parts: EventPart[] = [];

  if (flags.unsupported) {
    const sourceType = aliasedString(o, "source_type", "sourceType") || kind;
    parts.push({ kind: "unsupported", sourceType });
  }

  for (const br of blocks) {
    const bk = typeof br.kind === "string" ? br.kind : "";
    if (bk === "asset") {
      parts.push({
        kind: "asset",
        assetId: aliasedString(br, "asset_id", "assetId"),
        alt: typeof br.alt === "string" ? br.alt : undefined,
      });
      continue;
    }
    if (bk === "opaque") {
      flags.unsupported = true;
      parts.push({
        kind: "unsupported",
        sourceType: aliasedString(br, "source_type", "sourceType") || "opaque",
      });
      continue;
    }
    const text = typeof br.text === "string" ? br.text : "";
    if (!text) continue;
    pushCapped(parts, flags.system ? "system" : "text", text, flags);
  }

  const hasBody = parts.some((p) => p.kind === "text" || p.kind === "system");
  if (!hasBody && blocks.length === 0) {
    if (typeof o.text === "string" && o.text) {
      pushCapped(parts, flags.system ? "system" : "text", o.text, flags);
    } else if (typeof o.name === "string" && o.name) {
      pushCapped(parts, "text", o.name, flags);
    }
  }

  if (kind === "tool-call") {
    const input = asRec(o.input);
    if (input?.encoding === "json" && "value" in input) {
      const text = JSON.stringify(input.value, null, 2);
      if (text !== undefined) pushCapped(parts, "text", text, flags);
    } else if (input?.encoding === "payload") {
      const payload = asRec(input.payload);
      parts.push({ kind: "tool-input-payload", blobId: payload ? aliasedString(payload, "blobId", "blob_id") : "" });
    }
  }

  if (flags.truncated && !parts.some((p) => p.kind === "truncated")) {
    parts.push({ kind: "truncated" });
  }
  if (flags.incomplete) parts.push({ kind: "incomplete" });

  return { id, kind, role, heading: role ?? kind, parts, flags };
}

export function eventPlainText(view: EventDisplay): string {
  return view.parts
    .filter((p): p is Extract<EventPart, { kind: "text" | "system" }> => p.kind === "text" || p.kind === "system")
    .map((p) => p.text)
    .join("\n");
}

export function eventHasIssue(view: EventDisplay, assets: Record<string, { state: string }> = {}): boolean {
  return view.flags.unsupported || view.flags.truncated || view.flags.incomplete || view.parts.some((part) =>
    part.kind === "asset" && assets[part.assetId] &&
    assets[part.assetId].state !== "stored" && assets[part.assetId].state !== "loading");
}

/**
 * Pre-compaction branch named by a compaction event on the page: the last
 * covered event is the leaf the conversation stood at before it was compacted.
 */
export function compactionHistoryLeaf(events: readonly unknown[]): { leaf: string; count: number } | null {
  for (const raw of events) {
    const o = asRec(raw);
    if (!o || o.kind !== "compaction" || !Array.isArray(o.covered_event_ids)) continue;
    const ids = o.covered_event_ids.filter((id): id is string => typeof id === "string" && id.length > 0);
    if (ids.length) return { leaf: ids[ids.length - 1], count: ids.length };
  }
  return null;
}
