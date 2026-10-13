// Read-only check of the fixed embedded bundle and its production dependency graph.
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFileSync, lstatSync, readdirSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";

export const repo = fileURLToPath(new URL("../", import.meta.url));
export function checkExtension(root = repo) {
  const extension = path.join(root, "tools/computer-use-extension");
  const source = readFileSync(path.join(root, "src-tauri/src/computer_use/extension_bundle/files.rs"), "utf8");
  const files = [...source.matchAll(/embed!\("([^"]+)"\)/g)].map(match => match[1]);
  assert(files.length > 0 && new Set(files).size === files.length, "explicit, unique embedded allowlist required");
  assert.deepEqual(files, [...files].sort(), "stable bundle order required");
  const contents = new Map();
  for (const name of files) {
    assert(!name.includes("..") && !path.isAbsolute(name) && !/test|fixtures|\.run|\.cache/.test(name), "non-production bundle entry");
    let directory = extension;
    for (const part of name.split("/")) {
      assert(/^[a-zA-Z0-9_.-]+$/.test(part)); directory = path.join(directory, part);
      assert(!lstatSync(directory).isSymbolicLink(), "bundle cannot follow links");
    }
    assert(lstatSync(directory).isFile()); contents.set(name, readFileSync(directory));
  }
  for (const name of readdirSync(extension)) {
    if ((name.endsWith(".mjs") && !name.endsWith(".test.mjs")) || ["sw.js", "popup.js", "popup.html", "popup.css", "manifest.json", "INSTALL.md", "LICENSE"].includes(name)) {
      assert(contents.has(name), `production source missing from bundle: ${name}`);
    }
  }
  for (const [name, content] of contents) {
    const text = content.toString("utf8");
    const references = name.endsWith(".html") ? [...text.matchAll(/(?:src|href)="([^"]+)"/g)].map(match => match[1])
      : /\.(?:m?js)$/.test(name) ? [...text.matchAll(/(?:from\s*|import\s*(?:\(\s*)?)["']([^"']+)["']/g)].map(match => match[1]) : [];
    for (const reference of references) {
      const target = path.posix.normalize(path.posix.join(path.posix.dirname(name), reference));
      assert(contents.has(target), `unbundled dependency ${name}: ${reference}`);
    }
  }
  const manifest = JSON.parse(contents.get("manifest.json").toString("utf8"));
  const id = [...createHash("sha256").update(Buffer.from(manifest.key, "base64")).digest().subarray(0, 16)]
    .map(byte => String.fromCharCode(97 + (byte >> 4), 97 + (byte & 15))).join("");
  assert.equal(id, "bgegbabkegkanjbmjbeaockdijnbkjgi");
  assert.equal(manifest.manifest_version, 3); assert.equal(manifest.default_locale, "en");
  assert.equal(manifest.minimum_chrome_version, "140", "trusted local storage requires a compatible browser");
  assert(/^\d+\.\d+\.\d+$/.test(manifest.version));
  assert.deepEqual(manifest.host_permissions, ["http://127.0.0.1/*"]);
  assert.deepEqual(manifest.permissions, ["storage", "activeTab", "webNavigation", "scripting"]);
  assert(contents.has(manifest.background.service_worker) && contents.has(manifest.action.default_popup));
  const locales = files.filter(name => name.startsWith("_locales/")); assert.equal(locales.length, 15);
  const keys = Object.keys(JSON.parse(contents.get("_locales/en/messages.json").toString("utf8"))).sort();
  for (const name of locales) {
    const catalog = JSON.parse(contents.get(name).toString("utf8"));
    assert.deepEqual(Object.keys(catalog).sort(), keys);
    assert(Object.values(catalog).every(value => typeof value.message === "string" && value.message.trim()));
  }
  const hash = createHash("sha256");
  for (const [name, content] of contents) hash.update(`${name}\0${content.length}\0${createHash("sha256").update(content).digest("hex")}\n`);
  return { version: manifest.version, extensionId: id, files: files.length, bytes: [...contents.values()].reduce((sum, value) => sum + value.length, 0), sha256: hash.digest("hex") };
}
if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try { process.stdout.write(JSON.stringify(checkExtension(), null, 2) + "\n"); }
  catch (error) { process.stderr.write(error.message + "\n"); process.exitCode = 1; }
}
