#!/usr/bin/env node
/** Read-only bundle hygiene audit, not a signature or runtime-integrity verifier. */
import { createHash } from "node:crypto";
import { createReadStream, lstatSync, readdirSync, readFileSync, readlinkSync, realpathSync } from "node:fs";
import { isAbsolute, join, relative, resolve, sep } from "node:path";
import { computerUseTarget } from "./computer-use-targets.mjs";

const usage = "usage: audit-computer-use-bundle.mjs <root> --target <target> [--seed-only]";
const args = process.argv.slice(2);
let root;
let target;
let seedOnly = false;
try {
  for (let i = 0; i < args.length; i++) {
    if (args[i] === "--target" && !target) target = computerUseTarget(args[++i]);
    else if (args[i] === "--seed-only" && !seedOnly) seedOnly = true;
    else if (!args[i].startsWith("--") && !root) root = resolve(args[i]);
    else throw new Error("unexpected or duplicate argument");
  }
  if (!root || !target) throw new Error("root and explicit target are required");
} catch (error) {
  process.stderr.write(`${usage}\n${error.message}\n`);
  process.exit(2);
}

const denyNames = new Set([
  "cu_probe", "cu_probe.exe", "grok-computer-use-probe", "grok-computer-use-probe.exe",
  "chromium-win64.zip", "auth.json", "secrets.json", ".probe-owner.json",
  ".cu-mutation.lock", ".cu-mutation.lock.owner.json",
]);
const denyDirectories = new Set([".run", ".cache", "fixtures", ".staging"]);
// Keep in lockstep with runtime_chromium::macos_links; never allow arbitrary links.
const framework = "Chromium.app/Contents/Frameworks/Chromium Framework.framework/";
const macLinks = new Map([
  ["Resources", "Versions/Current/Resources"], ["Versions/Current", "130.0.6723.31"],
  ["Libraries", "Versions/Current/Libraries"], ["Chromium Framework", "Versions/Current/Chromium Framework"],
  ["Helpers", "Versions/Current/Helpers"],
].map(([path, destination]) => [framework + path, destination]));
const isMac = target.arch.endsWith("-macos");
const hits = [];
const entries = [];
const relativeName = path => relative(root, path).split(sep).join("/");
function inside(base, path) {
  const rel = relative(base, path);
  return rel !== ".." && !rel.startsWith(`..${sep}`) && !isAbsolute(rel);
}

function allowedMacLink(path, rel) {
  if (!isMac) return null;
  const marker = "chromium/chrome-mac/";
  const offset = rel.indexOf(marker);
  if (offset < 0 || (offset > 0 && rel[offset - 1] !== "/")) return null;
  const expected = macLinks.get(rel.slice(offset + marker.length));
  if (!expected || readlinkSync(path).split(sep).join("/") !== expected) return null;
  const browserRoot = realpathSync(join(root, rel.slice(0, offset + marker.length)));
  const resolved = realpathSync(path);
  return inside(realpathSync(root), browserRoot) && inside(browserRoot, resolved) ? expected : null;
}

function checkManifest(path, rel) {
  if (!((seedOnly && rel === "manifest.json") || rel.endsWith("resources/computer-use/seed/manifest.json"))) return;
  const manifest = JSON.parse(readFileSync(path, "utf8"));
  for (const id of ["js-runtime", "chromium"]) {
    if (manifest.components?.[id]?.arch !== target.arch) hits.push(`${rel} (${id} target mismatch)`);
  }
  if (isMac && manifest.components?.chromium?.relpath === "chromium/chrome-mac") {
    const base = rel.slice(0, -"manifest.json".length) + "chromium/chrome-mac/";
    for (const link of macLinks.keys()) {
      const linkedPath = join(root, base + link);
      try {
        if (!lstatSync(linkedPath).isSymbolicLink() || !allowedMacLink(linkedPath, base + link)) throw new Error();
      } catch { hits.push(`${base}${link} (missing or invalid pinned framework link)`); }
    }
  }
}

async function inspect(path) {
  const rel = relativeName(path);
  try {
    const st = lstatSync(path);
    const lower = rel.toLowerCase();
    const parts = lower.split("/");
    const name = parts.at(-1);
    if (denyNames.has(name) || parts.some(part => denyDirectories.has(part))
      || lower.includes("tools/computer-use-probe/") || lower.includes("h:/aicoding/") || lower.includes("c:/users/")
      || /\.(?:test\.mjs|dump|pem|key|p12|pfx)$/.test(name)
      || (parts.includes("chromium") && /\.(?:zip|tgz|tar\.gz|tar\.xz)$/.test(name))) hits.push(rel);
    if (st.isSymbolicLink()) {
      const destination = allowedMacLink(path, rel);
      if (!destination) hits.push(`${rel} (symlink)`);
      else entries.push({ rel, kind: "link", size: Buffer.byteLength(destination),
        sha256: createHash("sha256").update(destination).digest("hex") });
      return; // Never traverse a link, including a permitted framework alias.
    }
    if (st.isDirectory()) {
      for (const name of readdirSync(path).sort()) await inspect(join(path, name));
      return;
    }
    if (!st.isFile()) { hits.push(`${rel} (special file)`); return; }
    const hash = createHash("sha256");
    for await (const chunk of createReadStream(path)) hash.update(chunk);
    const after = lstatSync(path);
    if (!after.isFile() || after.size !== st.size || after.mtimeMs !== st.mtimeMs) {
      hits.push(`${rel} (changed during audit)`); return;
    }
    entries.push({ rel, kind: "file", size: st.size, sha256: hash.digest("hex") });
    if (st.size < 256 * 1024 && /\.(json|txt|md|js|mjs|toml|nsi)$/i.test(name)) {
      const text = readFileSync(path, "utf8");
      if (text.includes("sk-") && text.includes("sentinel")) hits.push(`${rel} sentinel`);
    }
    checkManifest(path, rel);
  } catch { hits.push(`${rel || "."} (unreadable or invalid)`); }
}

try {
  const stat = lstatSync(root);
  if (stat.isSymbolicLink() || !stat.isDirectory()) hits.push("audit root must be a real directory (no symlink)");
  else await inspect(root);
} catch { hits.push("audit root unavailable"); }

if (!seedOnly) {
  const files = new Set(entries.filter(entry => entry.kind === "file")
    .map(entry => target.arch.endsWith("-windows") ? entry.rel.toLowerCase() : entry.rel));
  const expected = target.arch.endsWith("-windows") ? ["grok-app.exe", "grok.exe"]
    : isMac ? ["Contents/MacOS/grok-app", "Grok.app/Contents/MacOS/grok-app"] : ["grok-app", "usr/bin/grok-app"];
  if (!expected.some(path => files.has(path))) hits.push(`missing main executable for ${target.arch}`);
}
entries.sort((a, b) => a.rel < b.rel ? -1 : a.rel > b.rel ? 1 : 0);
const fingerprint = createHash("sha256");
for (const entry of entries) fingerprint.update(`${entry.rel}\0${entry.kind}\0${entry.size}\0${entry.sha256}\n`);
process.stdout.write(JSON.stringify({ root, target: target.arch, mode: seedOnly ? "seed-hygiene" : "bundle-hygiene",
  files: entries.filter(entry => entry.kind === "file").length,
  links: entries.filter(entry => entry.kind === "link").length,
  bytes: entries.reduce((sum, entry) => sum + entry.size, 0),
  digestKind: "sha256-path-kind-size-content-v1", digest: fingerprint.digest("hex"),
  hits: [...new Set(hits)].sort(),
}, null, 2) + "\n");
process.exitCode = hits.length ? 1 : 0;
