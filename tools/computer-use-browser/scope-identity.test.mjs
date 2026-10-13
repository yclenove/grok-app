import test from "node:test";
import assert from "node:assert/strict";
import { randomUUID } from "node:crypto";
import { managedScopeIdentity } from "./scope-identity.mjs";

function fixture() {
  const forbidden = () => { throw new Error("content or input access is forbidden"); };
  const page = { isClosed: () => false, url: forbidden, title: forbidden,
    evaluate: forbidden, screenshot: forbidden, click: forbidden };
  const state = { id: "page-1", page, published: true, closed: false,
    identityExhausted: false, generation: 1 };
  const slot = { contextInstanceId: randomUUID(), pages: new Map([[state.id, state]]),
    context: { pages: () => [page] }, closure: { check() {} } };
  const got = { slot, owner: "owner", profile: "profile" };
  const worker = randomUUID();
  return { got, state, page, identity: () => managedScopeIdentity(got, state.id, worker) };
}

test("managed witness only reads original instance metadata; document changes are separate", () => {
  const f = fixture(), first = f.identity();
  f.state.generation += 1;
  assert.deepEqual(f.identity(), first);
  assert.deepEqual(Object.keys(first).sort(),
    ["ok", "owner", "profile", "workerInstanceId", "contextInstanceId", "pageId"].sort());
  f.got.slot.contextInstanceId = randomUUID();
  assert.notDeepEqual(f.identity(), first);
});
for (const reason of ["closed", "exhausted", "unpublished", "native-closed", "context-replaced", "missing", "closing"]) {
  test(`managed witness rejects ${reason} without reading content`, () => {
    const f = fixture();
    if (reason === "closed") f.state.closed = true;
    if (reason === "exhausted") f.state.identityExhausted = true;
    if (reason === "unpublished") f.state.published = false;
    if (reason === "native-closed") f.page.isClosed = () => true;
    if (reason === "context-replaced") f.got.slot.context.pages = () => [];
    if (reason === "missing") f.got.slot.pages.clear();
    if (reason === "closing") f.got.slot.closure.check = () => { throw new Error("closing"); };
    assert.throws(f.identity);
  });
}
test("managed witness rejects unbounded/non-string page ids", () => {
  const f = fixture();
  for (const id of [null, 1, "", "x".repeat(129)]) {
    assert.throws(() => managedScopeIdentity(f.got, id, randomUUID()));
  }
});
