/* global fetch */
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { once } from "node:events";
import process from "node:process";
import { createInterface } from "node:readline";
import { test } from "node:test";
import { fileURLToPath, URL } from "node:url";
import { parseFixturePort, startAppWebviewFixture } from "./app-webview-fixture.mjs";

test("App WebView fixture serves localhost and IPv4 from the same state", async t => {
  const fixture = await startAppWebviewFixture();
  t.after(() => fixture.close());

  const [pageA, pageB, initial] = await Promise.all([
    fetch(fixture.pageA).then(response => response.text()),
    fetch(fixture.ipv4PageB).then(response => response.text()),
    fetch(fixture.state).then(response => response.json()),
  ]);
  assert.match(pageA, /<title>内嵌页 A - Computer Use 验收<\/title>/);
  assert.match(pageB, /<title>内嵌页 B - Computer Use 验收<\/title>/);
  assert.deepEqual(initial, {
    a: { label: "A", note: "未保存", count: 0, events: [] },
    b: { label: "B", note: "未保存", count: 0, events: [] },
  });

  const effect = { action: "save", note: "甲页已保存", count: 0 };
  const applied = await fetch(new URL("/effect/a", fixture.ipv4State), {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(effect),
  });
  assert.equal(applied.status, 204);
  const shared = await fetch(fixture.state).then(response => response.json());
  assert.equal(shared.a.note, effect.note);
  assert.equal(shared.a.events.length, 1);
  assert.deepEqual(shared.b, initial.b);
});

test("fixture port parser rejects ambiguous or out-of-range input", () => {
  assert.equal(parseFixturePort(), 0);
  assert.equal(parseFixturePort("14569"), 14569);
  for (const value of ["-1", "1x", "65536", "", " 12"])
    assert.throws(() => parseFixturePort(value), /valid TCP port/);
});

async function startFixtureCli(t) {
  const entry = fileURLToPath(new URL("./app-webview-fixture.mjs", import.meta.url));
  const child = spawn(process.execPath, [entry], {
    env: { ...process.env, GROK_CU_WEBVIEW_FIXTURE_PORT: "0" },
    stdio: ["pipe", "pipe", "pipe"],
    windowsHide: true,
  });
  let stderr = "";
  child.stderr.setEncoding("utf8");
  child.stderr.on("data", chunk => { stderr += chunk; });
  t.after(async () => {
    if (child.exitCode !== null) return;
    const exited = once(child, "exit");
    child.kill();
    await exited;
  });
  const lines = createInterface({ input: child.stdout });
  const [line] = await once(lines, "line");
  lines.close();
  const description = JSON.parse(line);
  assert.match(description.pageA, /^http:\/\/localhost:\d+\/a$/);
  assert.match(description.ipv4State, /^http:\/\/127\.0\.0\.1:\d+\/state$/);
  assert.equal((await fetch(description.state)).status, 200);
  return { child, description, stderr: () => stderr };
}

test("fixture CLI publishes both loopback forms and exits on quit", { timeout: 10_000 }, async t => {
  const { child, stderr } = await startFixtureCli(t);
  const exited = once(child, "exit");
  child.stdin.end("quit\n");
  const [code, signal] = await exited;
  assert.equal(signal, null);
  assert.equal(code, 0, stderr());
});

test("fixture CLI closes both listeners normally when its original stdin ends", { timeout: 10_000 }, async t => {
  const { child, description, stderr } = await startFixtureCli(t);
  const exited = once(child, "exit");
  child.stdin.end();
  const [code, signal] = await exited;
  assert.equal(signal, null);
  assert.equal(code, 0, stderr());
  await assert.rejects(fetch(description.ipv4PageA), { name: "TypeError" });
  await assert.rejects(fetch(description.pageA), { name: "TypeError" });
});
