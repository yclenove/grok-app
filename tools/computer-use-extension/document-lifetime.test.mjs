import { test } from "node:test";
import assert from "node:assert/strict";
import { DocumentLifetime, proveDocumentCompletion } from "./document-lifetime.mjs";

const binding = { tabId: "101", documentId: "a".repeat(32), snapshotId: "snapshot", requestId: "request" };
function fixture() {
  const queries = []; let frame = { documentId: binding.documentId, parentFrameId: -1, frameType: "outermost_frame",
    documentLifecycle: "active", errorOccurred: false, url: "https://fixture.invalid/" };
  const tab = { id: 101, incognito: false }; let allowed = true;
  const navigation = { async getFrame(query) { queries.push(query); return frame; } };
  const extension = { isAllowedIncognitoAccess: async () => allowed };
  const lifetime = new DocumentLifetime({ navigation, extension, tabs: { get: async id => { assert.equal(id, 101); return tab; } } });
  return { lifetime, queries, navigation, tab, frame: value => { frame = value; }, permission: value => { allowed = value; },
    current: () => frame };
}

test("claim admission verifies the original active main document and keeps only its private-context bit", async () => {
  const f = fixture(); const context = await f.lifetime.attest(binding);
  assert.deepEqual(context, { incognito: false }); assert(Object.isFrozen(context));
  assert.deepEqual(f.queries, [{ documentId: binding.documentId, tabId: 101, frameId: 0 }]);
  for (const bad of [null, undefined, { ...f.current(), documentId: "b".repeat(32) },
    { ...f.current(), documentLifecycle: "cached" }, { ...f.current(), parentFrameId: 0 },
    { ...f.current(), url: "file:///fixture" }, { ...f.current(), errorOccurred: true }]) {
    f.frame(bad); await assert.rejects(f.lifetime.attest(binding));
  }
});

test("null is unavailable because Chrome also returns it for BFCache; cache and pending deletion stay alive", async () => {
  const f = fixture(); const context = await f.lifetime.attest(binding); const active = f.current();
  for (const lifecycle of ["cached", "prerender", "pending_deletion"]) {
    f.frame({ ...active, documentLifecycle: lifecycle }); assert.equal(await f.lifetime.state(binding, context), "retained");
  }
  for (const bad of [undefined, {}, { ...active, documentId: "other" }]) {
    f.frame(bad); await assert.rejects(f.lifetime.state(binding, context));
  }
  f.frame(null); assert.equal(await f.lifetime.state(binding, context), "unavailable");
  assert(f.queries.slice(1).every(query => Object.keys(query).length === 1 && query.documentId === binding.documentId));
  f.navigation.getFrame = async () => { throw new Error("API unavailable"); };
  await assert.rejects(f.lifetime.state(binding, context));
});

test("revoked incognito permission before or during a missing-document query never proves destruction", async () => {
  const f = fixture(); f.tab.incognito = true; const context = await f.lifetime.attest(binding);
  f.permission(false); f.frame(null);
  const before = f.queries.length; await assert.rejects(f.lifetime.state(binding, context)); assert.equal(f.queries.length, before);
  await assert.rejects(f.lifetime.attest(binding));
  f.permission(true); f.navigation.getFrame = async () => { f.permission(false); return null; };
  await assert.rejects(f.lifetime.state(binding, context));
});

test("native failure, a missing snapshot and null navigation metadata cannot release occupancy", async () => {
  const f = fixture(); const context = await f.lifetime.attest(binding);
  const args = { lifetime: f.lifetime, binding, context, retireExecution: true,
    scripting: { executeScript: async () => { throw new Error("native unavailable"); } } };
  assert.equal(await proveDocumentCompletion(args), false);
  args.scripting.executeScript = async () => [{ frameId: 0, documentId: binding.documentId, result: { status: "snapshot_missing" } }];
  assert.equal(await proveDocumentCompletion(args), false);
  args.scripting.executeScript = async () => { f.frame(null); throw new Error("document destroyed during injection"); };
  assert.equal(await proveDocumentCompletion(args), false);
});

test("cached and inaccessible documents do not inject; an original active cancellation promise is always joined", async () => {
  const f = fixture(); const context = await f.lifetime.attest(binding); const active = f.current(); let injections = 0;
  let release; const held = new Promise(resolve => { release = resolve; });
  const args = { lifetime: f.lifetime, binding, context, retireExecution: true,
    scripting: { executeScript: async request => { injections++; assert.deepEqual(request.args, ["snapshot", "request", true]); return held; } } };
  f.frame({ ...active, documentLifecycle: "cached" }); assert.equal(await proveDocumentCompletion(args), false);
  f.frame(null); assert.equal(await proveDocumentCompletion(args), false); assert.equal(injections, 0);
  f.frame(active); let done = false; const running = proveDocumentCompletion(args).then(value => { done = true; return value; });
  await new Promise(resolve => setImmediate(resolve)); assert.equal(injections, 1); f.frame(null);
  await new Promise(resolve => setImmediate(resolve)); assert.equal(done, false);
  release([{ frameId: 0, documentId: binding.documentId, result: { status: "settled" } }]); assert.equal(await running, true);
});

test("unattested legacy records cannot infer destruction but may use a matching original cancellation receipt", async () => {
  const f = fixture(); f.frame(null);
  const args = { lifetime: f.lifetime, binding, context: null, retireExecution: true,
    scripting: { executeScript: async () => { throw new Error("missing document"); } } };
  assert.equal(await proveDocumentCompletion(args), false); assert.equal(f.queries.length, 0);
  args.scripting.executeScript = async () => [{ frameId: 0, documentId: binding.documentId, result: { status: "settled" } }];
  assert.equal(await proveDocumentCompletion(args), true); assert.equal(f.queries.length, 0);
});
