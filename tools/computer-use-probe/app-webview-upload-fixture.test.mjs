/* global fetch */
import assert from "node:assert/strict";
import { Buffer } from "node:buffer";
import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { once } from "node:events";
import process from "node:process";
import { createInterface } from "node:readline";
import { test } from "node:test";
import { fileURLToPath, URL } from "node:url";
import { MAX_FIXTURE_UPLOAD_BYTES, startAppWebviewUploadFixture } from "./app-webview-upload-fixture.mjs";

async function setup(t) {
  const fixture = await startAppWebviewUploadFixture();
  t.after(() => fixture.close());
  const response = await fetch(fixture.page);
  assert.equal(response.status, 200);
  assert.equal(response.headers.get("access-control-allow-origin"), null);
  const html = await response.text();
  assert.match(html, /type="file"/);
  assert.match(html, /选择文件不会自动提交/);
  const match = /"(\/upload\/[a-f0-9]{48})"/.exec(html);
  assert.ok(match);
  return { fixture, upload: new URL(match[1], fixture.page) };
}

function send(fixture, upload, bytes, headers = {}) {
  return fetch(upload, {
    method: "PUT",
    headers: {
      origin: new URL(fixture.page).origin,
      "content-type": "application/octet-stream",
      "x-fixture-filename": encodeURIComponent("中文验收.bin"),
      ...headers,
    },
    body: bytes,
  });
}

test("upload oracle hashes exact binary and empty bytes without deduplicating effects", async t => {
  const { fixture, upload } = await setup(t);
  assert.deepEqual(await fetch(fixture.state).then(r => r.json()), { uploads: [] });
  const bytes = Buffer.from([0, 13, 10, 255, 128, 65, 0]);
  for (const [index, body] of [bytes, bytes, Buffer.alloc(0)].entries()) {
    const response = await send(fixture, upload, body);
    assert.equal(response.status, 200);
    const receipt = await response.json();
    assert.equal(receipt.sequence, index + 1);
    assert.equal(receipt.bytes, body.length);
    assert.equal(receipt.sha256, createHash("sha256").update(body).digest("hex"));
    assert.equal(receipt.filename, "中文验收.bin");
    assert.equal(receipt.contentType, "application/octet-stream");
  }
  const state = await fetch(fixture.state).then(r => r.json());
  assert.equal(state.uploads.length, 3);
  assert.equal(state.uploads[0].sha256, state.uploads[1].sha256);
});

test("upload oracle rejects wrong origin, path names and payload overflow without effects", async t => {
  const { fixture, upload } = await setup(t);
  assert.equal((await send(fixture, upload, Buffer.from("x"), { origin: "https://example.invalid" })).status, 403);
  for (const filename of ["", "../secret", "a\\b", "x\n", "x".repeat(241)]) {
    assert.equal((await send(fixture, upload, Buffer.from("x"), { "x-fixture-filename": encodeURIComponent(filename) })).status, 400);
  }
  assert.equal((await send(fixture, upload, Buffer.alloc(MAX_FIXTURE_UPLOAD_BYTES + 1))).status, 413);
  assert.deepEqual(await fetch(fixture.state).then(r => r.json()), { uploads: [] });
  assert.equal((await send(fixture, upload, Buffer.alloc(MAX_FIXTURE_UPLOAD_BYTES))).status, 200);
});

test("upload oracle caps retained receipts and can close idempotently", async t => {
  const { fixture, upload } = await setup(t);
  for (let i = 0; i < 16; i++) assert.equal((await send(fixture, upload, Buffer.alloc(0))).status, 200);
  assert.equal((await send(fixture, upload, Buffer.alloc(0))).status, 403);
  assert.equal((await fetch(fixture.state).then(r => r.json())).uploads.length, 16);
  await fixture.close();
  await fixture.close();
});

test("simultaneous uploads cannot overflow the receipt cap", async t => {
  const { fixture, upload } = await setup(t);
  const responses = await Promise.all(Array.from({ length: 24 }, () => send(fixture, upload, Buffer.from("bounded"))));
  assert.equal(responses.filter(response => response.status === 200).length, 16);
  assert.equal(responses.filter(response => response.status === 403).length, 8);
  assert.equal((await fetch(fixture.state).then(r => r.json())).uploads.length, 16);
});

test("upload fixture CLI has a normal quit and owner-pipe EOF teardown", async t => {
  const entry = fileURLToPath(new URL("./app-webview-upload-fixture.mjs", import.meta.url));
  for (const input of ["quit\n", ""]) {
    const child = spawn(process.execPath, [entry], {
      env: { ...process.env, GROK_CU_UPLOAD_FIXTURE_PORT: "0" },
      stdio: ["pipe", "pipe", "pipe"], windowsHide: true,
    });
    const exited = once(child, "exit");
    let stderr = "";
    child.stderr.setEncoding("utf8");
    child.stderr.on("data", chunk => { stderr += chunk; });
    t.after(async () => {
      if (child.exitCode !== null) return;
      child.stdin.end("quit\n");
      await exited;
    });
    const lines = createInterface({ input: child.stdout });
    const [line] = await once(lines, "line");
    lines.close();
    const description = JSON.parse(line);
    assert.match(description.page, /^http:\/\/127\.0\.0\.1:\d+\/$/);
    assert.deepEqual(await fetch(description.state).then(r => r.json()), { uploads: [] });
    child.stdin.end(input);
    const [code, signal] = await exited;
    assert.equal(signal, null);
    assert.equal(code, 0, stderr);
  }
});
