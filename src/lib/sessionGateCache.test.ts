import { expect, test } from "vitest";
import { SessionGateCache } from "./sessionGateCache";

test("construction preserves normal Map entries and chaining", () => {
  const cache = new SessionGateCache([["a", 1]]);
  expect(cache.set("b", 2)).toBe(cache);
  expect([...cache]).toEqual([["a", 1], ["b", 2]]);
});

test("finishing an older restore cannot retire the newer one", () => {
  const cache = new SessionGateCache<number>();
  const old = cache.beginRestore("a"); const latest = cache.beginRestore("a");
  old.finish(); expect(old.isCurrent()).toBe(false); expect(latest.isCurrent()).toBe(true);
  latest.finish(); expect(latest.isCurrent()).toBe(false);
});

test("a delete of an absent session still retires its restore without affecting other sessions", () => {
  const cache = new SessionGateCache<number>();
  const a = cache.beginRestore("a"); const b = cache.beginRestore("b");
  expect(cache.delete("a")).toBe(false);
  expect(a.isCurrent()).toBe(false); expect(b.isCurrent()).toBe(true);
});

test("clear retires all active restores even when no gates were cached", () => {
  const cache = new SessionGateCache<number>();
  const a = cache.beginRestore("a"); const b = cache.beginRestore("b");
  cache.clear(); expect(a.isCurrent()).toBe(false); expect(b.isCurrent()).toBe(false);
});

test("gate families sharing a session id keep independent restore leases", () => {
  const questions = new SessionGateCache<number>(); const plans = new SessionGateCache<number>();
  const question = questions.beginRestore("a"); const plan = plans.beginRestore("a");
  questions.delete("a"); expect(question.isCurrent()).toBe(false); expect(plan.isCurrent()).toBe(true);
});
