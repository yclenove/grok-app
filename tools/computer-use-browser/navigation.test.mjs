import test from "node:test";
import assert from "node:assert/strict";
import { errors } from "playwright-core";
import { navigatePage, NAVIGATION_TIMEOUT_MS } from "./navigation.mjs";
import { workerExceptionSpec } from "./worker-errors.mjs";

test("goto and reload use the same finite native navigation deadline", async () => {
  const calls = [];
  const result = {};
  const page = {
    goto: async (...args) => { calls.push(["goto", ...args]); return result; },
    reload: async (...args) => { calls.push(["reload", ...args]); return result; },
  };
  assert.equal(await navigatePage(page, { url: "https://example.test/" }), result);
  assert.equal(await navigatePage(page, { kind: "reload" }), result);
  const options = { waitUntil: "domcontentloaded", timeout: NAVIGATION_TIMEOUT_MS };
  assert.equal(options.timeout, 8000);
  assert.deepEqual(calls, [["goto", "https://example.test/", options], ["reload", options]]);
});

test("actual native timeout is unknown, sanitized and never retried", async () => {
  for (const kind of ["goto", "reload"]) {
    let dispatched = 0;
    const page = { [kind]: async () => {
      dispatched += 1;
      throw new errors.TimeoutError("https://example.test/?token=PRIVATE #password C:/profile");
    } };
    await assert.rejects(navigatePage(page, { kind, url: "https://example.test/" }), error => {
      assert.deepEqual(workerExceptionSpec(error), { status: 504, code: "navigation_timeout",
        completion: "unknown",
        message: "page navigation did not finish before its deadline; observe before continuing" });
      return true;
    });
    assert.equal(dispatched, 1);
  }
});

test("untrusted timeout-like failures retain their original unknown classification", async () => {
  for (const error of [Object.assign(new Error("PRIVATE"), { name: "TimeoutError" }),
    { name: "TimeoutError", statusCode: 504, completion: "not_started" }]) {
    await assert.rejects(navigatePage({ goto: async () => { throw error; } }), caught => caught === error);
    assert.equal(workerExceptionSpec(error).code, "worker_internal");
    assert.equal(workerExceptionSpec(error).completion, "unknown");
  }
});

test("cancellation before native admission cannot dispatch navigation", async () => {
  const controller = new AbortController();
  controller.abort();
  let dispatched = 0;
  await assert.rejects(navigatePage({ goto: async () => { dispatched += 1; } }, { signal: controller.signal }), error => {
    assert.equal(workerExceptionSpec(error).completion, "not_started");
    assert.equal(workerExceptionSpec(error).code, "run_cancelled");
    return true;
  });
  assert.equal(dispatched, 0);
});

test("late cancellation stays unknown after native success or failure", async () => {
  for (const fail of [false, true]) {
    const controller = new AbortController();
    let dispatched = 0;
    await assert.rejects(navigatePage({ goto: async () => {
      dispatched += 1;
      controller.abort();
      if (fail) throw new errors.TimeoutError("PRIVATE");
    } }, { signal: controller.signal }), error => {
      assert.equal(workerExceptionSpec(error).completion, "unknown");
      assert.equal(workerExceptionSpec(error).code, "run_cancelled");
      return true;
    });
    assert.equal(dispatched, 1);
  }
});

test("unknown navigation kinds never dispatch a page method", async () => {
  await assert.rejects(navigatePage({}, { kind: "evaluate" }), TypeError);
});
