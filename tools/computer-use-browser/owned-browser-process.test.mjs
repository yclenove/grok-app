import assert from "node:assert/strict";
import test from "node:test";
import { spawn } from "node:child_process";
import { once } from "node:events";
import { realpathSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { discoverOwnedBrowserPid, ownedBrowserPid, windowsArguments } from "./owned-browser-process.mjs";

test("Windows process argument parsing preserves spaces, empty arguments and quoted slashes", () => {
  assert.deepEqual(windowsArguments('"C:\\Program Files\\Chrome\\chrome.exe" "--user-data-dir=C:\\my profile" --headless=new'),
    ["C:\\Program Files\\Chrome\\chrome.exe", "--user-data-dir=C:\\my profile", "--headless=new"]);
  assert.deepEqual(windowsArguments('a "" b'), ["a", "", "b"]);
  assert.deepEqual(windowsArguments(String.raw`a "trailing\\" "embedded\"quote"`), ["a", "trailing\\", 'embedded"quote']);
  assert.deepEqual(windowsArguments(' \t a\tb '), ["a", "b"]);
  assert.deepEqual(windowsArguments(String.raw`a\ b`), ["a\\", "b"]);
  assert.equal(windowsArguments('"unterminated'), null);
  assert.equal(windowsArguments(null), null);
  assert.equal(windowsArguments("a\0b"), null);
});

for (const platform of ["win32", "linux"]) {
  test(`${platform}: process identity needs exact parent, executable and unique owned profile`, () => {
    const executablePath = platform === "win32" ? "C:\\private\\chrome.exe" : "/private/chrome";
    const userDataDir = platform === "win32" ? "C:\\my profiles\\two" : "/my profiles/two";
    const launch = { parentPid: 51001, executablePath, userDataDir, platform };
    const row = { pid: 51002, parentPid: 51001, executablePath,
      args: [executablePath, "--headless=new", `--user-data-dir=${userDataDir}`] };
    const decoy = { ...row, pid: 51003, args: [executablePath, `--user-data-dir=${userDataDir}-other`] };
    assert.equal(ownedBrowserPid([decoy, row], launch), row.pid);
    assert.equal(ownedBrowserPid([row, { ...row, pid: 51004 }], launch), null);
    for (const changed of [{ parentPid: 51000 }, { executablePath: `${executablePath}-other` },
      { args: [...row.args, "--type=renderer"] }, { args: [...row.args, `--user-data-dir=${userDataDir}`] },
      { args: [...row.args, "--user-data-dir", `${userDataDir}-other`] },
      { args: [executablePath, `prefix--user-data-dir=${userDataDir}`] }, { args: null },
      ...[0, -1, 1.5, "51002", 2 ** 32, launch.parentPid].map(pid => ({ pid }))]) {
      assert.equal(ownedBrowserPid([{ ...row, ...changed }], launch), null);
    }
    assert.equal(ownedBrowserPid([row], { ...launch, userDataDir: "relative" }), null);
  });
}

test("native owned-process discovery preserves Unicode and spaced profile arguments", {
  skip: !["win32", "linux"].includes(process.platform),
}, async () => {
  const executablePath = realpathSync(process.execPath);
  const userDataDir = join(realpathSync(tmpdir()), "grok-cu-pid-中文-தமிழ் profile");
  const children = [userDataDir, `${userDataDir}-other`].map(profile => spawn(executablePath,
    ["-e", "setInterval(() => {}, 1000)", "--", `--user-data-dir=${profile}`],
    { stdio: "ignore", windowsHide: true }));
  try {
    await Promise.all(children.map(child => once(child, "spawn")));
    assert.equal(await discoverOwnedBrowserPid({ executablePath, userDataDir }), children[0].pid);
    assert.equal(await discoverOwnedBrowserPid({ executablePath, userDataDir: `${userDataDir}-other` }), children[1].pid);
  } finally {
    await Promise.all(children.map(async child => {
      if (child.exitCode !== null || child.signalCode !== null) return;
      const exited = once(child, "exit");
      child.kill();
      await exited;
    }));
  }
});
