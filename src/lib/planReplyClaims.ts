import type { SessionPlanState } from "./planSession";

export type PlanReplyOwner = { sessionId: string; plan: SessionPlanState };

/** Generations distinguish reused RPC ids. Legacy gates require object identity;
 * progress without a gate belongs to its stable plan tool, when present. */
export function samePlanReplyTarget(current: SessionPlanState | undefined, expected: SessionPlanState): boolean {
  if (!current || current.rpcId !== expected.rpcId) return false;
  if (expected.rpcId != null) {
    if (current.requestGeneration || expected.requestGeneration) {
      return !!expected.requestGeneration && current.requestGeneration === expected.requestGeneration;
    }
    return current === expected;
  }
  if (current.chromeRevision || expected.chromeRevision) {
    return !!expected.chromeRevision && current.chromeRevision === expected.chromeRevision;
  }
  return current === expected;
}

class PlanReplyClaims {
  #owners = new Set<PlanReplyOwner>();
  #listeners = new Set<() => void>();
  #settlementListeners = new Set<(owner: PlanReplyOwner, next: SessionPlanState) => void>();
  #revision = 0;
  subscribe = (listener: () => void) => {
    this.#listeners.add(listener);
    return () => { this.#listeners.delete(listener); };
  };
  snapshot = () => this.#revision;
  subscribeSettlements = (listener: (owner: PlanReplyOwner, next: SessionPlanState) => void) => {
    this.#settlementListeners.add(listener);
    return () => { this.#settlementListeners.delete(listener); };
  };
  settle(owner: PlanReplyOwner, next: SessionPlanState): void {
    for (const listener of this.#settlementListeners) listener(owner, next);
  }
  has(owner: PlanReplyOwner): boolean {
    return [...this.#owners].some((pending) => pending.sessionId === owner.sessionId && samePlanReplyTarget(owner.plan, pending.plan));
  }
  claim(owner: PlanReplyOwner): boolean {
    if (this.has(owner)) return false;
    this.#owners.add(owner); this.#changed(); return true;
  }
  release(owner: PlanReplyOwner): void {
    if (this.#owners.delete(owner)) this.#changed();
  }
  #changed() {
    this.#revision++;
    for (const listener of this.#listeners) listener();
  }
}

const stores = new WeakMap<Map<string, SessionPlanState>, PlanReplyClaims>();
/** A composer remount shares in-flight claims and completion notifications. */
export function getPlanReplyClaims(cache: Map<string, SessionPlanState>): PlanReplyClaims {
  let store = stores.get(cache);
  if (!store) { store = new PlanReplyClaims(); stores.set(cache, store); }
  return store;
}
