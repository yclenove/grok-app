import assert from "node:assert/strict";
import { test } from "node:test";
import { setTimeout as delay } from "node:timers/promises";
import { runInNewContext } from "node:vm";
import { boundedRead, readFixtureVisibility, reportPairingFailure } from "./pairing-diagnostics.mjs";

const extensionId = "fixture-extension";
const fixtureUrl = "http://127.0.0.1:12345/fixture";
const never = () => new Promise(() => {});
const worker = evaluate => ({ url: () => `chrome-extension://${extensionId}/sw.js`, evaluate });
const context = workers => ({ serviceWorkers: () => workers, pages: () => [] });
const base = () => ({ check: "explicit-share-current-tab", error: new Error("SECRET"), extensionId,
  fixtureUrl, deadlineMs: 20, emit: () => {} });

test("bounded diagnostic reads distinguish unavailable, failed, timed out and success", async () => {
  assert.deepEqual(await boundedRead(null), { status: "unavailable" });
  assert.deepEqual(await boundedRead(() => { throw new Error("SECRET"); }), { status: "failed" });
  assert.deepEqual(await boundedRead(async () => 7), { status: "ok", value: 7 });
  assert.deepEqual(await boundedRead(never, 15), { status: "timed_out" });
  for (const deadline of [0, -1, Infinity, NaN, 2001]) {
    await assert.rejects(boundedRead(never, deadline), RangeError);
  }
});

test("primary failure is emitted synchronously before three hung reads share one window", async () => {
  const lines = [], started = [];
  const pending = reportPairingFailure({ ...base(), emit: line => lines.push(line),
    context: context([worker(() => { started.push("worker"); return never(); })]),
    popup: { evaluate: () => { started.push("popup"); return never(); } },
  });
  assert.equal(lines[0], "pairing-live failed at explicit-share-current-tab: Error");
  assert.equal(started.length, 0, "diagnostics must not run before the primary record");
  await Promise.resolve();
  assert.equal(started.length, 3, "all reads start without awaiting a previous diagnostic");
  const result = await pending;
  assert.deepEqual(Object.values(result).map(value => value.status), ["timed_out", "timed_out", "timed_out"]);
  assert(!lines.join("\n").includes("SECRET"));
});

test("timed-out reads consume late rejection without reporting it as another failure", async () => {
  let reject;
  const result = await boundedRead(() => new Promise((_, fail) => { reject = fail; }), 10);
  assert.equal(result.status, "timed_out");
  reject(new Error("SECRET late failure"));
  await delay(0);
});

test("only the unique exact production worker can supply diagnostic evidence", async () => {
  let calls = 0;
  const unrelated = { url: () => "chrome-extension://other/sw.js", evaluate: () => assert.fail("unrelated worker") };
  const exact = worker(async () => { calls++; return {}; });
  const result = await reportPairingFailure({ ...base(), context: context([unrelated, exact]) });
  assert.equal(calls, 2);
  assert.equal(result.workerTraffic.status, "ok");
  const ambiguous = await reportPairingFailure({ ...base(), context: context([exact, exact]) });
  assert.equal(ambiguous.workerTraffic.status, "unavailable");
  assert.equal(calls, 2);
});

test("closed or missing contexts retain the primary failure without raw browser errors", async () => {
  const lines = [];
  for (const ctx of [null, { serviceWorkers() { throw new Error("SECRET browser details"); } }]) {
    const result = await reportPairingFailure({ ...base(), error: { name: "SECRET" },
      emit: line => lines.push(line), context: ctx,
      popup: { evaluate: async () => { throw new Error("SECRET popup details"); } },
    });
    assert.equal(result.workerTraffic.status, "unavailable");
    assert.equal(result.popupState.status, "failed");
  }
  assert(!lines.join("\n").includes("SECRET"));
});

test("diagnostic evidence keeps only bounded allowlisted routes, reasons and popup states", async () => {
  const lines = [];
  const good = { route: "/cu/extension-status", status: 403, failed: true,
    reason: "retire_status", elapsedMs: 123, body: "SECRET", authorization: "SECRET" };
  const bad = { route: "/cu/SECRET", status: 403, failed: true, reason: "SECRET" };
  const result = await reportPairingFailure({ ...base(), check: "heartbeat-survives-without-popup",
    emit: line => lines.push(line), requests: [{ path: "/cu/SECRET", status: 403 },
      { path: good.route, status: 403, body: "SECRET" }],
    context: context([worker(async () => ({ recent: [...Array(50).fill(good), bad],
      rejected: [good, bad], connection: [good, bad] }))]),
    popup: { evaluate: async () => ({ state: "paired", tabState: "SECRET", text: "SECRET" }) },
  });
  assert.equal(result.workerTraffic.value.recent.length, 23);
  assert.deepEqual(result.workerTraffic.value.connection, [{
    route: good.route, status: 403, reason: "retire_status", elapsedMs: 123,
  }]);
  assert.deepEqual(result.popupState.value, { state: "paired", tabState: null });
  assert.equal(result.fixture.status, "unavailable");
  assert(!lines.join("\n").includes("SECRET"));
});

test("visibility diagnostic never injects into another active tab with the same title", async () => {
  let injections = 0;
  const read = runInNewContext(`(${readFixtureVisibility.toString()})`, { chrome: {
    tabs: { query: async () => [{ id: 1, title: "Computer Use Fixture", url: "https://example.test/private" }] },
    scripting: { executeScript: () => { injections++; return []; } },
  } });
  assert.equal((await read(fixtureUrl)).fixture, false);
  assert.equal(injections, 0);
});

test("visibility diagnostic targets only the fixture main frame in an isolated world", async () => {
  let target;
  const read = runInNewContext(`(${readFixtureVisibility.toString()})`, { chrome: {
    tabs: { query: async () => [{ id: 42, url: fixtureUrl }] },
    scripting: { executeScript: async options => { target = options; return [{ result: { visible: true } }]; } },
  } });
  const value = await read(fixtureUrl);
  assert.equal(value.fixture, true);
  assert.equal(value.visible, true);
  assert.equal(target.target.tabId, 42);
  assert.equal(target.target.frameIds.join(","), "0");
  assert.equal(target.world, "ISOLATED");
});

test("visibility diagnostic redacts scripting errors", async () => {
  const read = runInNewContext(`(${readFixtureVisibility.toString()})`, { chrome: {
    tabs: { query: async () => [{ id: 42, url: fixtureUrl }] },
    scripting: { executeScript: async () => { throw new Error("SECRET"); } },
  } });
  const value = await read(fixtureUrl);
  assert.equal(value.scriptRejected, true);
  assert(!JSON.stringify(value).includes("SECRET"));
});
