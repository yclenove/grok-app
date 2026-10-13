import test from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, copyFileSync, readFileSync, writeFileSync, rmSync, symlinkSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { checkExtension, repo } from "./check-computer-use-extension.mjs";
function fixture(t) {
  const root = mkdtempSync(path.join(tmpdir(), "grok-cu-extension-audit-"));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const list = "src-tauri/src/computer_use/extension_bundle/files.rs";
  const source = readFileSync(path.join(repo, list), "utf8");
  const names = [...source.matchAll(/embed!\("([^"]+)"\)/g)].map(match => match[1]);
  for (const name of [list, ...names.map(name => "tools/computer-use-extension/" + name)]) {
    mkdirSync(path.dirname(path.join(root, name)), { recursive: true }); copyFileSync(path.join(repo, name), path.join(root, name));
  }
  return { root, extension: path.join(root, "tools/computer-use-extension"), list: path.join(root, list) };
}
test("fixed embedded contents include the whole dependency graph and all 15 locales", () => {
  const report = checkExtension(); assert.equal(report.files, 48); assert.equal(report.version, "1.1.0");
  assert.equal(report.extensionId, "bgegbabkegkanjbmjbeaockdijnbkjgi"); assert.equal(report.sha256.length, 64);
});
test("new production source cannot be silently left out of the App executable", t => {
  const f = fixture(t); writeFileSync(path.join(f.extension, "forgotten.mjs"), "export const value = 1;");
  assert.throws(() => checkExtension(f.root), /production source missing/);
});
test("a dependency outside the production list is rejected", t => {
  for (const source of ['import "./missing.mjs";', 'import { missing } from "./missing.mjs";', 'import("./missing.mjs");']) {
    const f = fixture(t); writeFileSync(path.join(f.extension, "sw.js"), source);
    assert.throws(() => checkExtension(f.root), /unbundled dependency/);
  }
});
test("tests cannot be embedded even if they exist on disk", t => {
  const f = fixture(t);
  writeFileSync(f.list, readFileSync(f.list, "utf8").replace('embed!("sw.js")', 'embed!("sw.test.mjs")'));
  writeFileSync(path.join(f.extension, "sw.test.mjs"), "test fixture only");
  assert.throws(() => checkExtension(f.root), /non-production bundle/);
});
test("manifest identity and authority cannot drift from the supported extension", t => {
  for (const patch of [{ key: Buffer.from("foreign key").toString("base64") }, { host_permissions: ["<all_urls>"] }]) {
    const f = fixture(t); const manifest = path.join(f.extension, "manifest.json");
    writeFileSync(manifest, JSON.stringify({ ...JSON.parse(readFileSync(manifest, "utf8")), ...patch }));
    assert.throws(() => checkExtension(f.root));
  }
});

test("an older browser cannot be advertised as supporting trusted local storage", t => {
  const f = fixture(t); const manifest = path.join(f.extension, "manifest.json");
  writeFileSync(manifest, JSON.stringify({ ...JSON.parse(readFileSync(manifest, "utf8")), minimum_chrome_version: "112" }));
  assert.throws(() => checkExtension(f.root), /trusted local storage requires/);
});
test("a linked locale folder cannot enter the embedded allowlist", t => {
  const f = fixture(t); const locales = path.join(f.extension, "_locales"); const original = path.join(locales, "en");
  const foreign = path.join(f.root, "foreign"); mkdirSync(foreign); copyFileSync(path.join(original, "messages.json"), path.join(foreign, "messages.json"));
  rmSync(original, { recursive: true }); symlinkSync(foreign, original, process.platform === "win32" ? "junction" : "dir");
  assert.throws(() => checkExtension(f.root), /cannot follow links/);
});
