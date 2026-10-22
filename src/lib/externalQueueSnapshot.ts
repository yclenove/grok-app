import type { QueuedSend } from "./sendQueue";

export type ExternalQueueState = "queued" | "pending" | "held";
export type ExternalQueueRow = {
  sessionId: string; itemId: string; prompt: string; createdAt: number; state: ExternalQueueState;
};
export type ExternalQueueSnapshot = { version: 1; items: ExternalQueueRow[] };
const unavailable = () => new Error("SESSION_QUEUE_UNAVAILABLE");
function record(value: unknown): value is Record<string, unknown> {
  return !!value && typeof value === "object" && !Array.isArray(value);
}
function identity(value: unknown): value is string {
  return typeof value === "string" && value.length > 0 && value === value.trim();
}
/** Reject an incomplete/future response atomically; never turn invalid data into an empty queue. */
export function parseExternalQueueSnapshot(value: unknown): ExternalQueueSnapshot {
  if (!record(value) || value.version !== 1 || !Array.isArray(value.items)
      || value.items.length > 200 || Object.keys(value).some(k => !["version", "items"].includes(k))) throw unavailable();
  const ids = new Set<string>();
  const items: ExternalQueueRow[] = value.items.map(row => {
    if (!record(row) || Object.keys(row).length !== 5
        || !identity(row.sessionId) || row.sessionId === "__draft__" || row.sessionId.startsWith("imp:")
        || !identity(row.itemId) || ids.has(row.itemId)
        || typeof row.prompt !== "string" || !row.prompt.trim()
        || typeof row.createdAt !== "number" || !Number.isSafeInteger(row.createdAt)
        || typeof row.state !== "string" || !["queued", "pending", "held"].includes(row.state)) throw unavailable();
    ids.add(row.itemId);
    return { sessionId: row.sessionId, itemId: row.itemId, prompt: row.prompt,
      createdAt: row.createdAt, state: row.state as ExternalQueueState };
  });
  return { version: 1, items };
}
/** Replace only Host-owned slots in authoritative FIFO order; local payloads/order remain untouched. */
export function reconcileExternalQueueSnapshot(
  current: Record<string, QueuedSend[]>, snapshot: ExternalQueueSnapshot,
): Record<string, QueuedSend[]> {
  const groups = new Map<string, QueuedSend[]>();
  for (const row of snapshot.items) {
    const prior = Object.hasOwn(current, row.sessionId)
      ? current[row.sessionId]?.find(x => x.source === "external" && x.id === row.itemId) : undefined;
    const next = prior && prior.storedDisplay === row.prompt && prior.createdAt === row.createdAt
      && prior.externalState === row.state ? prior : {
        id: row.itemId, storedDisplay: row.prompt, attachments: [], goalMode: false,
        createdAt: row.createdAt, source: "external" as const, externalState: row.state,
      };
    const group = groups.get(row.sessionId) ?? []; group.push(next); groups.set(row.sessionId, group);
  }
  let changed = false;
  const entries: Array<[string, QueuedSend[]]> = [];
  for (const key of new Set([...Object.keys(current), ...groups.keys()])) {
    const before = Object.hasOwn(current, key) ? current[key]! : [];
    const host = groups.get(key) ?? []; let index = 0;
    const next: QueuedSend[] = [];
    for (const item of before) {
      if (item.source !== "external") next.push(item);
      else if (index < host.length) next.push(host[index++]!);
    }
    next.push(...host.slice(index));
    const same = next.length === before.length && next.every((item, i) => item === before[i]);
    if (!same) changed = true;
    if (next.length) entries.push([key, same ? before : next]);
    else if (Object.hasOwn(current, key)) changed = true;
  }
  return changed ? Object.fromEntries(entries) : current;
}
