import test from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, writeFileSync, rmSync, symlinkSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const script = fileURLToPath(new URL("./audit-computer-use-bundle.mjs", import.meta.url));
const targets = ["x86_64-windows", "aarch64-macos", "x86_64-macos", "x86_64-linux"];
function fixture(t) {
  const root = mkdtempSync(join(tmpdir(), "grok-cu-audit-test-"));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const put = (name, data = "fixture") => {
    const path = join(root, name); mkdirSync(dirname(path), { recursive: true });
    writeFileSync(path, data); return path;
  };
  return { root, put };
}
function audit(root, target, flags = []) {
  const args = [script, root, ...flags];
  if (target !== undefined) args.push("--target", target);
  const result = spawnSync(process.execPath, args, { encoding: "utf8", timeout: 10000 });
  assert(!result.error, String(result.error));
  return { ...result, report: result.stdout.trim() ? JSON.parse(result.stdout) : null };
}

test("audit requires an explicit supported target", t => {
  const f = fixture(t); f.put("grok-app.exe");
  for (const target of [undefined, "arm-linux", "darwin"]) {
    assert.equal(audit(f.root, target).status, 2);
  }
});

test("each target recognizes only its own main executable layout", t => {
  for (const target of targets) {
    const f = fixture(t);
    const app = target.endsWith("-windows") ? "grok-app.exe"
      : target.endsWith("-macos") ? "Contents/MacOS/grok-app" : "usr/bin/grok-app";
    f.put(app);
    const result = audit(f.root, target);
    assert.equal(result.status, 0, result.stdout + result.stderr);
    assert.equal(result.report.target, target);
    const wrong = target.endsWith("-windows") ? "x86_64-linux" : "x86_64-windows";
    assert.equal(audit(f.root, wrong).status, 1);
  }
});

test("root-level probe, cache, staging and secret paths are denied", t => {
  const f = fixture(t);
  for (const name of [".run/evidence.json", ".cache/item", "fixtures/page.html", ".staging/item",
    "cu_probe", "grok-computer-use-probe", "auth.json", "signing.pem", "worker.test.mjs"]) f.put(name);
  const { status, report } = audit(f.root, "x86_64-windows", ["--seed-only"]);
  assert.equal(status, 1);
  for (const name of [".run", ".cache", "fixtures", ".staging", "cu_probe", "auth.json", "signing.pem"]) {
    assert(report.hits.some(hit => hit.includes(name)), name);
  }
});

test("the Windows portable Grok.exe layout is accepted without lowercasing its fingerprint", t => {
  const f = fixture(t); f.put("Grok.exe");
  const result = audit(f.root, "x86_64-windows");
  assert.equal(result.status, 0, result.stdout + result.stderr);
});

test("build mutation locks and private owner metadata cannot ship", t => {
  const f = fixture(t); f.put("Grok.exe");
  for (const name of [".cu-mutation.lock", ".cu-mutation.lock.owner.json"]) {
    f.put(`resources/computer-use/${name}`);
  }
  const result = audit(f.root, "x86_64-windows");
  assert.equal(result.status, 1);
  assert.deepEqual(result.report.hits, [
    "resources/computer-use/.cu-mutation.lock",
    "resources/computer-use/.cu-mutation.lock.owner.json",
  ]);
});

test("directory links cannot escape or create traversal cycles", t => {
  const f = fixture(t); const outside = fixture(t);
  outside.put("private.txt", "not part of bundle"); f.put("inside.txt");
  symlinkSync(outside.root, join(f.root, "escape"), process.platform === "win32" ? "junction" : "dir");
  symlinkSync(f.root, join(f.root, "cycle"), process.platform === "win32" ? "junction" : "dir");
  const { status, report } = audit(f.root, "x86_64-windows", ["--seed-only"]);
  assert.equal(status, 1); assert.equal(report.files, 1);
  assert(report.hits.some(hit => hit.includes("escape") && hit.includes("symlink")));
  assert(report.hits.some(hit => hit.includes("cycle") && hit.includes("symlink")));
});

test("a linked audit root is rejected before traversal", t => {
  const f = fixture(t); const outside = fixture(t); outside.put("file.txt");
  const link = join(f.root, "linked-root");
  symlinkSync(outside.root, link, process.platform === "win32" ? "junction" : "dir");
  const { status, report } = audit(link, "x86_64-linux", ["--seed-only"]);
  assert.equal(status, 1); assert.equal(report.files, 0);
});

test("same-size content mutations change the audit fingerprint", t => {
  const f = fixture(t); f.put("worker.mjs", "AAAA");
  const before = audit(f.root, "x86_64-windows", ["--seed-only"]);
  f.put("worker.mjs", "BBBB");
  const after = audit(f.root, "x86_64-windows", ["--seed-only"]);
  assert.equal(before.status, 0); assert.equal(after.status, 0);
  assert.notEqual(before.report.digest, after.report.digest);
  assert.equal(after.report.digestKind, "sha256-path-kind-size-content-v1");
});

test("unexpected Chromium archives stay denied on all targets", t => {
  const f = fixture(t); f.put("chromium/chromium-win64.zip"); f.put("chromium/new.zip");
  for (const target of targets) {
    const { status, report } = audit(f.root, target, ["--seed-only"]);
    assert.equal(status, 1);
    assert(report.hits.some(hit => hit.includes("chromium-win64.zip")));
    assert(report.hits.some(hit => hit.includes("new.zip")));
  }
});

test("a seed manifest cannot claim another target", t => {
  const f = fixture(t);
  f.put("manifest.json", JSON.stringify({ schema_version: 2, pack_id: "seed", components: {
    "js-runtime": { arch: "x86_64-windows" }, chromium: { arch: "x86_64-windows" },
  } }));
  assert.equal(audit(f.root, "x86_64-linux", ["--seed-only"]).status, 1);
  assert.equal(audit(f.root, "x86_64-windows", ["--seed-only"]).status, 0);
});

function macFixture(f) {
  const framework = "chromium/chrome-mac/Chromium.app/Contents/Frameworks/Chromium Framework.framework";
  const version = "130.0.6723.31";
  for (const name of ["Resources/data", "Libraries/data", "Helpers/data", "Chromium Framework"]) {
    f.put(`${framework}/Versions/${version}/${name}`);
  }
  const links = [["Versions/Current", version, "dir"], ["Resources", "Versions/Current/Resources", "dir"],
    ["Libraries", "Versions/Current/Libraries", "dir"], ["Helpers", "Versions/Current/Helpers", "dir"],
    ["Chromium Framework", "Versions/Current/Chromium Framework", "file"]];
  for (const [name, destination, type] of links) symlinkSync(destination, join(f.root, framework, name), type);
  f.put("manifest.json", JSON.stringify({ schema_version: 2, pack_id: "seed", components: {
    "js-runtime": { arch: "aarch64-macos" }, chromium: { arch: "aarch64-macos", relpath: "chromium/chrome-mac" },
  } }));
  return { framework, links };
}

test("only the five pinned relative framework links are accepted on macOS", t => {
  const f = fixture(t); const { framework } = macFixture(f);
  const accepted = audit(f.root, "aarch64-macos", ["--seed-only"]);
  assert.equal(accepted.status, 0, accepted.stdout + accepted.stderr);
  assert.equal(accepted.report.links, 5); assert.equal(accepted.report.files, 5);
  assert.equal(audit(f.root, "x86_64-windows", ["--seed-only"]).status, 1);
  symlinkSync("Versions/Current/Resources", join(f.root, framework, "Unlisted"), "dir");
  const rejected = audit(f.root, "aarch64-macos", ["--seed-only"]);
  assert.equal(rejected.status, 1);
  assert(rejected.report.hits.some(hit => hit.includes("Unlisted") && hit.includes("symlink")));
});

test("flattened and retargeted framework links fail closed", t => {
  for (const mode of ["flattened", "retargeted"]) {
    const f = fixture(t); const { framework } = macFixture(f);
    const path = join(f.root, framework, "Resources");
    rmSync(path); // Only this test-created link, never its destination.
    if (mode === "flattened") mkdirSync(path);
    else symlinkSync("Versions/Current/Libraries", path, "dir");
    const rejected = audit(f.root, "aarch64-macos", ["--seed-only"]);
    assert.equal(rejected.status, 1);
    assert(rejected.report.hits.some(hit => hit.includes("invalid pinned framework link")));
  }
});
