import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { EventEmitter, once } from "node:events";
import { join } from "node:path";
import test from "node:test";
import { browserPidLookup, captureBrowserPid, childPids } from "./browser-pids.mjs";

test("child PID discovery works with the private worker's minimal Windows PATH", async () => {
  const previousPath = process.env.PATH;
  const child = spawn(process.execPath, ["-e", "setInterval(() => {}, 1000)"],
    { stdio: "ignore", windowsHide: true });
  try {
    await once(child, "spawn");
    if (process.platform === "win32") process.env.PATH = join(process.env.SystemRoot, "System32");
    const observed = childPids(process.pid);
    assert(observed.includes(child.pid), "PID lookup lost the owned child in the sanitized environment");
    assert.deepEqual(observed, [child.pid], "do not report the short-lived discovery helper itself");
  } finally {
    if (previousPath === undefined) delete process.env.PATH;
    else process.env.PATH = previousPath;
    const exited = once(child, "exit");
    child.kill();
    await exited;
  }
});

test("invalid parent PID never performs process discovery", () => {
  for (const parent of [0, -1, NaN, Infinity, "1", 1.5]) assert.deepEqual(childPids(parent), []);
});

test("browser PID belongs to the exact context, not the worker's first child", async () => {
  const calls = [];
  function context(pid) {
    return { browser: () => ({ newBrowserCDPSession: async () => ({
      send: async method => {
        calls.push([pid, method]);
        return { processInfo: [{ type: "renderer", id: 71 }, { type: "browser", id: pid }] };
      },
      detach: async () => calls.push([pid, "detach"]),
    }) }) };
  }
  assert.equal(await captureBrowserPid(context(51001)), 51001);
  assert.equal(await captureBrowserPid(context(51002)), 51002);
  assert.deepEqual(calls, [
    [51001, "SystemInfo.getProcessInfo"], [51001, "detach"],
    [51002, "SystemInfo.getProcessInfo"], [51002, "detach"],
  ]);
});

test("missing, ambiguous or failed context identity never adopts another child PID", async () => {
  const decoy = spawn(process.execPath, ["-e", "setInterval(() => {}, 1000)"],
    { stdio: "ignore", windowsHide: true });
  try {
    await once(decoy, "spawn");
    assert.equal(await captureBrowserPid({ browser: () => null }), null);
    for (const processInfo of [undefined, [], [{ type: "renderer", id: decoy.pid }],
      [{ type: "browser", id: 12 }, { type: "browser", id: 13 }],
      ...[0, -1, 1.5, "12", NaN, Infinity, 2 ** 32, process.pid].map(id => [{ type: "browser", id }])]) {
      let detached = false;
      const context = { browser: () => ({ newBrowserCDPSession: async () => ({
        send: async () => ({ processInfo }), detach: async () => { detached = true; },
      }) }) };
      assert.equal(await captureBrowserPid(context), null);
      assert.equal(detached, true);
    }
    let detached = false;
    assert.equal(await captureBrowserPid({ browser: () => ({ newBrowserCDPSession: async () => ({
      send: async () => { throw new Error("disconnected"); },
      detach: async () => { detached = true; },
    }) }) }), null);
    assert.equal(detached, true);
  } finally {
    const exited = once(decoy, "exit");
    decoy.kill();
    await exited;
  }
});

test("a late diagnostic attach is bounded and still detaches without replacing identity", async () => {
  let attach;
  let detached = false;
  const context = { browser: () => ({ newBrowserCDPSession: () => new Promise(resolve => { attach = resolve; }) }) };
  assert.equal(await captureBrowserPid(context), null);
  attach({
    send: async () => ({ processInfo: [{ type: "browser", id: 51003 }] }),
    detach: async () => { detached = true; },
  });
  await new Promise(resolve => setImmediate(resolve));
  assert.equal(detached, true);
});

test("unavailable identity can recover by an exact read, without parallel discovery or retired-context adoption", async () => {
  const context = new EventEmitter();
  let finish, calls = 0;
  context.browser = () => ({ newBrowserCDPSession: async () => ({
    send: () => { calls += 1; return new Promise(resolve => { finish = resolve; }); },
    detach: async () => {},
  }) });
  const read = browserPidLookup(context, {});
  const first = read(), duplicate = read();
  assert.equal(first, duplicate);
  await new Promise(resolve => setImmediate(resolve));
  finish({ processInfo: [] });
  assert.equal(await first, null);
  assert.equal(calls, 1);
  const retry = read();
  await new Promise(resolve => setImmediate(resolve));
  finish({ processInfo: [{ type: "browser", id: 51008 }] });
  assert.equal(await retry, 51008);
  assert.equal(await read(), 51008);
  assert.equal(calls, 2, "a known original identity is cached, not rediscovered");
  context.emit("close");
  assert.equal(await read(), 51008, "captured diagnostic survives until its owning slot is retired");

  const retired = new EventEmitter();
  retired.browser = context.browser;
  const lateRead = browserPidLookup(retired, {});
  const late = lateRead();
  await new Promise(resolve => setImmediate(resolve));
  retired.emit("close");
  finish({ processInfo: [{ type: "browser", id: 51009 }] });
  assert.equal(await late, null);
  assert.equal(await lateRead(), null, "no new lookup may attach after context retirement");
  assert.equal(calls, 3);
});
