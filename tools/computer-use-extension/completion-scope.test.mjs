import { test } from "node:test";
import assert from "node:assert/strict";
import { inspect } from "node:util";
import { randomUUID } from "node:crypto";
import { CompletionScope, copyCompletionProof } from "./completion-scope.mjs";

function proof() {
  return { binding: { protocol: 2, requestId: randomUUID(), connection: { instanceId: randomUUID(), connectionNonce: randomUUID(), generation: 1 },
    session: "owner", runId: "run", tabId: "101", documentId: "0".repeat(32), documentGeneration: 7, grantGeneration: 1, snapshotId: randomUUID() },
  completionKey: "a".repeat(64) };
}
const endpoint = "http://127.0.0.1:32100";

test("completion scope copies bounded full binding and never exposes its key in public state", async () => {
  const original = proof(); const expected = structuredClone(original); const calls = [];
  const scope = new CompletionScope({ endpoint, proof: original, fetch: async (url, init) => {
    calls.push({ url, init });
    return Response.json(url.endsWith("/status") ? { ok: true, status: { phase: "claimed", cancelRequested: true } } : { ok: true });
  } });
  original.completionKey = "f".repeat(64); original.binding.connection.generation++;
  assert.equal(JSON.stringify(scope), "{}"); assert(!inspect(scope).includes(expected.completionKey));
  assert.deepEqual(await scope.status(), { phase: "claimed", cancelRequested: true });
  await scope.settle(); await scope.settle();
  assert.equal(calls.length, 2);
  for (const { url, init } of calls) {
    assert.equal(new URL(url).origin, endpoint); assert(!url.includes(expected.completionKey));
    assert.deepEqual(JSON.parse(init.body), expected);
    assert.deepEqual(init.headers, { "content-type": "application/json" });
    assert.equal(init.credentials, "omit"); assert.equal(init.redirect, "error"); assert.equal(init.cache, "no-store");
  }
  assert.deepEqual(await scope.status(), { phase: "settled", cancelRequested: true });
  assert.equal(calls.length, 2);
});

test("invalid binding, unbounded inputs, wrong endpoints and extra authority are rejected before fetch", () => {
  for (const mutate of [p => { p.sessionKey = "secret"; }, p => { p.binding.script = "source"; },
    p => { p.binding.connection.extra = true; }, p => { p.binding.protocol = 1; },
    p => { p.binding.connection.generation = 2 ** 53; }, p => { p.binding.documentGeneration = 0; },
    p => { p.binding.session = "界".repeat(100); }, p => { p.binding.documentId = "x".repeat(129); },
    p => { p.binding.runId = "run\0"; }, p => { p.binding.snapshotId = "-".repeat(36); },
    p => { p.completionKey = "G".repeat(64); }]) {
    const value = proof(); mutate(value);
    assert.throws(() => copyCompletionProof(value), /invalidCompletion/);
  }
  for (const url of ["http://localhost:32100", endpoint + "/path", "https://127.0.0.1:32100"]) {
    assert.throws(() => new CompletionScope({ endpoint: url, proof: proof() }), /invalidAddress/);
  }
});

test("responses are bounded and strict and failures cannot expose reply or exception secrets", async () => {
  for (const response of [() => Response.json({ ok: true, status: { phase: "claimed", cancelRequested: false, sessionKey: "secret" } }),
    () => Response.json({ ok: true, status: { phase: "unknown", cancelRequested: false } }),
    () => Response.json({ ok: true, status: { phase: "claimed", cancelRequested: 0 } }),
    () => new Response("secret".repeat(300)), () => new Response("secret", { status: 403 }),
    () => { throw new Error("private credential"); }]) {
    const scope = new CompletionScope({ endpoint, proof: proof(), fetch: response });
    await assert.rejects(scope.status(), { message: "completionUnavailable" });
  }
});

test("lost settlement response can be resent without restoring other credentials", async () => {
  let calls = 0; let release;
  const scope = new CompletionScope({ endpoint, proof: proof(), fetch: async url => {
    assert(url.endsWith("/settle"));
    if (++calls === 1) { await new Promise(resolve => { release = resolve; }); throw new Error("reply lost"); }
    return Response.json({ ok: true });
  } });
  const first = scope.settle(); const duplicate = scope.settle();
  assert.equal(first, duplicate); release(); await assert.rejects(first, /completionUnavailable/);
  await scope.settle(); assert.equal(calls, 2);
});

test("aborting a status request joins its body reader and does not acknowledge settlement", async () => {
  let entered; let aborted = false; let releaseRead; let cancelEntered; let releaseCancel;
  const started = new Promise(resolve => { entered = resolve; });
  const cancelStarted = new Promise(resolve => { cancelEntered = resolve; });
  const scope = new CompletionScope({ endpoint, proof: proof(), fetch: async (_url, init) => {
    init.signal.addEventListener("abort", () => { aborted = true; releaseRead({ done: true }); }, { once: true });
    return { ok: true, body: { getReader: () => ({
      read: () => { entered(); return new Promise(resolve => { releaseRead = resolve; }); },
      cancel: () => { cancelEntered(); return new Promise(resolve => { releaseCancel = resolve; }); },
    }) } };
  } });
  const controller = new AbortController(); let finished = false;
  const reading = scope.status(controller.signal).finally(() => { finished = true; });
  const rejected = assert.rejects(reading, /completionUnavailable/);
  await started; controller.abort(); await cancelStarted;
  assert(aborted); assert(!finished, "cleanup cannot detach the owned body reader");
  releaseCancel(); await rejected;
  await assert.rejects(scope.status(controller.signal), /completionUnavailable/);
});
