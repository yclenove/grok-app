import { describe, expect, it } from "vitest";
import {
  applyClearSendQueuePlan, applyExternalQueuePush, applyExternalQueueTake,
  canReorderSendQueue, claimQueueHead, enqueueSend, makeQueuedSend,
  moveQueuedSend, planClearSendQueue, removeQueuedSend, reorderQueuedSend,
  requeueAtFront, resolveSendQueueStripState, summarizeSendQueue,
  updateQueuedSend, type QueuedSend,
} from "./sendQueue";
const local = (id: string) => makeQueuedSend({ id, storedDisplay: id, attachments: [], goalMode: false });
const external = (id: string): QueuedSend => ({ ...local(id), source: "external" });
const ids = (q: QueuedSend[]) => q.map((x) => x.id);

describe("Host queue ownership", () => {
  it("a Host arrival never enforces a local budget on an already merged draft queue", () => {
    const q = Array.from({ length: 21 }, (_, i) => local(`draft-${i}`));
    const result = applyExternalQueuePush({ s: q }, { sessionId: "s", itemId: "host", prompt: "keep" }, 0);
    expect(result.dropped).toBe(0);
    expect(ids(result.byKey.s!)).toEqual([...ids(q), "host"]);
  });
  it("restoring a Host entry cannot trim existing local entries", () => {
    const q = [local("a"), local("b"), local("c")];
    const result = requeueAtFront(q, external("host"), 1);
    expect(result.dropped).toBe(0);
    expect(ids(result.queue)).toEqual(["host", "a", "b", "c"]);
  });
  it("retains all 200 durable Host items without composer-cap eviction", () => {
    let map: Record<string, QueuedSend[]> = {};
    for (let i = 0; i < 200; i++) {
      const result = applyExternalQueuePush(map, { sessionId: "s", itemId: `host-${i}`, prompt: `follow-up ${i}` });
      expect(result.dropped).toBe(0);
      map = result.byKey;
    }
    expect(ids(map.s!)).toEqual(Array.from({ length: 200 }, (_, i) => `host-${i}`));
    expect(claimQueueHead(map, "s")).toBeNull();
  });
  it("Host arrivals do not evict accepted composer prompts", () => {
    const q = Array.from({ length: 20 }, (_, i) => local(`local-${i}`));
    const r = applyExternalQueuePush({ s: q }, { sessionId: "s", itemId: "host", prompt: "keep" });
    expect(r.dropped).toBe(0);
    expect(ids(r.byKey.s!)).toEqual([...ids(q), "host"]);
    expect(q).toHaveLength(20);
  });
  it("composer overflow only removes the oldest composer entry", () => {
    const q = [external("host-1"), local("oldest"), external("host-2"), local("recent")];
    const r = enqueueSend(q, local("new"), 2);
    expect(r.dropped).toBe(1);
    expect(ids(r.queue)).toEqual(["host-1", "host-2", "recent", "new"]);
    expect(ids(q)).toEqual(["host-1", "oldest", "host-2", "recent"]);
  });
  it("failed local sends restore their head without dropping Host entries", () => {
    const q = [external("host-1"), local("old"), external("host-2"), local("new")];
    const r = requeueAtFront(q, local("retry"), 2);
    expect(r.dropped).toBe(1);
    expect(ids(r.queue)).toEqual(["retry", "host-1", "host-2", "new"]);
  });
  it("a zero local cap still cannot delete Host entries", () => {
    expect(ids(enqueueSend([external("host")], local("discard"), 0).queue)).toEqual(["host"]);
    expect(ids(requeueAtFront([external("host")], local("discard"), 0).queue)).toEqual(["host"]);
  });
  it("clear counts and removes only composer-owned entries", () => {
    const q = [external("host-1"), local("a"), external("host-2"), local("b")];
    const plan = planClearSendQueue(q);
    expect(plan.count).toBe(2);
    expect(plan.logMeta).toEqual({ clearedCount: 2 });
    expect(ids(plan.next)).toEqual(["host-1", "host-2"]);
    expect(ids(applyClearSendQueuePlan({ s: q }, "s", plan).s!)).toEqual(["host-1", "host-2"]);
  });
  it("applying a stale clear plan preserves Host arrivals and does not resurrect taken items", () => {
    const plan = planClearSendQueue([external("taken"), local("a")]);
    const map = { s: [local("a"), external("new-arrival")], other: [external("other")] };
    const result = applyClearSendQueuePlan(map, "s", plan);
    expect(ids(result.s!)).toEqual(["new-arrival"]);
    expect(result.other).toBe(map.other);
  });
  it("Host-only queues offer neither clear nor local retry", () => {
    const q = [external("host-1"), external("host-2")];
    expect(planClearSendQueue(q)).toMatchObject({ count: 0, confirmNeeded: false, next: q });
    expect(resolveSendQueueStripState({ queue: q, flushHold: true })).toMatchObject({ visible: true, count: 2, canClear: false, canReorder: false, showHold: false });
    expect(summarizeSendQueue(q, 1).isFull).toBe(false);
    expect(canReorderSendQueue(q)).toBe(false);
  });
  it("local removal and editing cannot diverge from durable Host payloads", () => {
    const q = [local("a"), external("host"), local("b")];
    expect(removeQueuedSend(q, "host")).toBe(q);
    expect(updateQueuedSend(q, "host", { storedDisplay: "changed" })).toBe(q);
    expect(removeQueuedSend(q, "a")).toEqual(q.slice(1));
  });
  it.each(["up", "down"] as const)("Host entries cannot be moved %s", (direction) => {
    const q = [local("a"), external("host"), local("b")];
    expect(moveQueuedSend(q, "host", direction)).toBe(q);
  });
  it("local reorder cannot cross a Host FIFO boundary", () => {
    const q = [local("a"), external("host"), local("b")];
    expect(moveQueuedSend(q, "a", "down")).toBe(q);
    expect(reorderQueuedSend(q, 0, 2)).toBe(q);
    expect(canReorderSendQueue(q)).toBe(false);
  });
  it("keeps local reorder available within a contiguous composer segment", () => {
    const q = [external("host"), local("a"), local("b")];
    expect(ids(moveQueuedSend(q, "b", "up"))).toEqual(["host", "b", "a"]);
    expect(canReorderSendQueue(q)).toBe(true);
  });
  it("Host take requires an exact session and never removes a composer ID collision", () => {
    const map = { s: [local("same")], other: [external("same")] };
    expect(applyExternalQueueTake(map, { itemId: "same" }).byKey).toBe(map);
    expect(applyExternalQueueTake(map, { sessionId: "s", itemId: "same" }).byKey).toBe(map);
    const taken = applyExternalQueueTake(map, { sessionId: "other", itemId: "same" });
    expect(taken.removed).toBe(true);
    expect(taken.byKey.s).toBe(map.s);
    expect(taken.byKey.other).toBeUndefined();
  });
});
