import { describe, expect, it } from "vitest";
import { parseExternalQueueSnapshot as parse, reconcileExternalQueueSnapshot as reconcile } from "./externalQueueSnapshot";
import { applyExternalQueuePush, applyExternalQueueTake, getQueueForKey, makeQueuedSend, type ExternalQueuePush, type ExternalQueueTake, type QueuedSend } from "./sendQueue";
const row = (itemId = "a", sessionId = "s") => ({ itemId, sessionId, prompt: " prompt ", createdAt: 1, state: "queued" });
const snapshot = (items = [row()]) => parse({ version: 1, items });
const local = (id: string) => makeQueuedSend({ id, storedDisplay: "[[skill:test]] local", goalMode: true, attachments: [] });
describe("Host queue snapshot boundary", () => {
  it("accepts empty and the full 200-item queue without trimming or sorting", () => {
    expect(snapshot([]).items).toEqual([]);
    const rows = Array.from({ length: 200 }, (_, i) => ({ ...row(String(i), i % 2 ? "b" : "a"), createdAt: 200 - i }));
    expect(snapshot(rows).items).toEqual(rows);
    expect(() => snapshot([...rows, row("overflow")])).toThrow("SESSION_QUEUE_UNAVAILABLE");
  });
  it.each([
    null, [], {}, { version: 2, items: [] }, { version: 1, items: [], secret: "x" },
    { version: 1, items: [row(), row()] },
    ...[ { itemId: "" }, { itemId: " a " }, { sessionId: "__draft__" }, { sessionId: "imp:foreign" },
      { prompt: "" }, { prompt: 1 }, { createdAt: NaN }, { createdAt: Infinity },
      { createdAt: Number.MAX_SAFE_INTEGER + 1 }, { createdAt: 0.1 }, { state: "unknown" },
      { state: { toString: () => "queued" } }, { nonce: "private" },
    ].map(p => ({ version: 1, items: [{ ...row(), ...p }] })),
  ])("rejects an invalid response atomically: %#", value => {
    expect(() => parse(value)).toThrow("SESSION_QUEUE_UNAVAILABLE");
  });
  it("preserves all local object identities and order while repairing Host FIFO", () => {
    const a = local("local-a"), b = local("local-b");
    const initial = reconcile({}, snapshot([row("old"), row("tail")]));
    const mixed = { s: [a, initial.s[0], b, initial.s[1]] };
    const next = reconcile(mixed, snapshot([row("replacement"), row("tail"), row("new")]));
    expect(next.s.map(x => x.id)).toEqual(["local-a", "replacement", "local-b", "tail", "new"]);
    expect(next.s[0]).toBe(a); expect(next.s[2]).toBe(b); expect(next.s[3]).toBe(initial.s[1]);
    expect(reconcile(next, snapshot([row("replacement"), row("tail"), row("new")]))).toBe(next);
    expect(reconcile(next, snapshot([]))).toEqual({ s: [a, b] });
  });
  it("retains state transitions and session isolation", () => {
    const first = reconcile({}, snapshot([row("a", "one"), row("b", "two")]));
    const next = reconcile(first, parse({ version: 1, items: [{ ...row("b", "two"), state: "held" }] }));
    expect(Object.keys(next)).toEqual(["two"]); expect(next.two[0].externalState).toBe("held");
    expect(first.one[0].externalState).toBe("queued");
  });
  it.each(["__proto__", "constructor", "toString"])("handles own session key %s without prototype pollution", key => {
    expect(getQueueForKey({}, key)).toEqual([]);
    const next = reconcile({}, snapshot([row("host", key)]));
    expect(getQueueForKey(next, key)[0].source).toBe("external");
    expect(Object.getPrototypeOf(next)).toBe(Object.prototype);
    expect(Object.prototype).not.toHaveProperty("storedDisplay");
  });
  it("keeps a colliding local id and does not treat an external take as local removal", () => {
    const own = local("a");
    const next = reconcile({ s: [own] }, snapshot());
    expect(next.s).toHaveLength(2); expect(next.s[0]).toBe(own);
    expect(applyExternalQueueTake(next, { sessionId: "s", itemId: "a" }).byKey.s).toEqual([own]);
  });
  it.each([null, undefined, 5, {}, { sessionId: 4, itemId: [] }, { sessionId: "s", itemId: "q", prompt: 4 }])("ignores malformed event %# without throwing", value => {
    const current: Record<string, QueuedSend[]> = { s: [local("safe")] };
    expect(applyExternalQueuePush(current, value as ExternalQueuePush).byKey).toBe(current);
    expect(applyExternalQueueTake(current, value as ExternalQueueTake).byKey).toBe(current);
  });
});
