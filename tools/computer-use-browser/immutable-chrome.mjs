import { createHash } from "node:crypto";
import {
  copyFileSync,
  existsSync,
  lstatSync,
  mkdirSync,
  readdirSync,
  readFileSync,
  realpathSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { basename, dirname, isAbsolute, join, relative, resolve } from "node:path";

// Chromium 130 writes debug.log and Hunspell Dictionaries beside chrome.exe
// (DIR_MODULE). The pinned pack tree is hash-locked, so a launch must execute
// a copy that lives outside that tree. Hardlinks are not used: a write through
// a shared inode would change the locked bytes.

function contained(parent, child) {
  const rel = relative(parent, child);
  return rel === "" || (!rel.startsWith("..") && !isAbsolute(rel));
}

export function pinnedChromePack(exe) {
  if (!exe || !existsSync(exe)) return null;
  const realExe = realpathSync(resolve(exe));
  const exeStat = lstatSync(realExe);
  if (!exeStat.isFile() || exeStat.isSymbolicLink()) return null;
  const tree = dirname(realExe);
  if (basename(tree).toLowerCase() !== "chrome-win") return null;
  const chromium = dirname(tree);
  if (basename(chromium).toLowerCase() !== "chromium") return null;
  const pack = dirname(chromium);
  const manifest = join(pack, "manifest.json");
  if (!existsSync(manifest)) return null;
  const manifestStat = lstatSync(manifest);
  if (!manifestStat.isFile() || manifestStat.isSymbolicLink()) return null;
  return { exe: realExe, tree, pack };
}

function stampOf(tree) {
  const rows = [];
  const walk = (dir) => {
    for (const name of readdirSync(dir)) {
      const path = join(dir, name);
      const meta = lstatSync(path);
      if (meta.isSymbolicLink()) throw new Error("pinned chrome tree contains a symlink");
      const rel = relative(tree, path).replaceAll("\\", "/");
      if (meta.isDirectory()) walk(path);
      else if (meta.isFile()) rows.push(`${rel}\t${meta.size}\t${Math.trunc(meta.mtimeMs)}`);
      else throw new Error("pinned chrome tree contains a special file");
    }
  };
  walk(tree);
  rows.sort();
  return createHash("sha256").update(rows.join("\n")).digest("hex");
}

function copyTree(src, dest) {
  mkdirSync(dest, { recursive: true });
  for (const name of readdirSync(src)) {
    const from = join(src, name);
    const to = join(dest, name);
    const meta = lstatSync(from);
    if (meta.isSymbolicLink()) throw new Error("pinned chrome tree contains a symlink");
    if (meta.isDirectory()) copyTree(from, to);
    else if (meta.isFile()) copyFileSync(from, to);
    else throw new Error("pinned chrome tree contains a special file");
  }
}

export function pinnedChromeLaunchPath(exe, writableRoot) {
  const pinned = pinnedChromePack(exe);
  if (!pinned) return exe ? resolve(exe) : exe;
  if (!writableRoot || !isAbsolute(resolve(writableRoot))) {
    throw new Error("pinned chrome requires an absolute writable execution root");
  }
  const root = resolve(writableRoot);
  if (contained(pinned.tree, root) || contained(pinned.pack, root)) {
    throw new Error("chrome execution root must not be inside the pinned pack");
  }
  mkdirSync(root, { recursive: true });
  const dest = join(root, "chrome-win");
  const stampPath = join(root, "source-stamp.txt");
  const stamp = stampOf(pinned.tree);
  const current = existsSync(stampPath) ? readFileSync(stampPath, "utf8").trim() : "";
  if (current !== stamp || !existsSync(join(dest, "chrome.exe"))) {
    if (existsSync(dest)) {
      const destReal = realpathSync(dest);
      if (!contained(realpathSync(root), destReal)) {
        throw new Error("refusing to replace a chrome execution tree outside the writable root");
      }
      rmSync(destReal, { recursive: true, force: true });
    }
    copyTree(pinned.tree, dest);
    writeFileSync(stampPath, `${stamp}\n`, { flag: "w" });
  }
  const launch = join(dest, "chrome.exe");
  if (!existsSync(launch)) throw new Error("pinned chrome execution tree has no chrome.exe");
  return launch;
}
