import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { computerUseTarget, computerUseTargets } from "./computer-use-targets.mjs";

test("each supported triple selects its own checked-in source lock", () => {
  for (const target of computerUseTargets) {
    assert.equal(computerUseTarget(target.triple), target);
    assert.equal(computerUseTarget(target.arch), target);
    const lock = JSON.parse(readFileSync(new URL(`../src-tauri/resources/computer-use/${target.lock}`, import.meta.url)));
    assert.equal(lock.target, target.arch);
    for (const component of [lock.jsRuntime, lock.playwright, lock.chromium]) {
      assert.match(component.archiveSha256, /^[a-f0-9]{64}$/);
      assert(component.archiveBytes > 0);
      assert.equal(new URL(component.url).protocol, "https:");
    }
  }
});

test("before-build admits all four explicit targets without a Windows-seed skip", () => {
  for (const target of computerUseTargets) {
    const result = spawnSync(process.execPath, [fileURLToPath(new URL("./tauri-before-build.mjs", import.meta.url)), "--gate-only"], {
      encoding: "utf8", env: { ...process.env, TAURI_ENV_TARGET_TRIPLE: target.triple, GROK_CU_BUNDLE_TARGET: "", GROK_CU_SEED: "" },
    });
    assert.equal(result.status, 0, result.stderr);
    assert(result.stdout.includes(`runtime=${target.arch}`));
    assert(!result.stdout.includes("skip"));
  }
});

test("bundle gate rejects a seed override that differs from packaged resources", () => {
  const script = fileURLToPath(new URL("./tauri-before-build.mjs", import.meta.url));
  const packaged = fileURLToPath(new URL("../src-tauri/resources/computer-use/seed", import.meta.url));
  for (const [seed, status] of [[packaged, 0], [packaged + "-fixture", 1]]) {
    const result = spawnSync(process.execPath, [script, "--gate-only"], {
      encoding: "utf8", env: { ...process.env, TAURI_ENV_TARGET_TRIPLE: "x86_64-pc-windows-msvc", GROK_CU_SEED: seed },
    });
    assert.equal(result.status, status, result.stdout + result.stderr);
    if (status) assert(result.stderr.includes("checked seed must match Tauri resources"));
  }
});

test("missing and unsupported targets fail closed", () => {
  for (const value of ["", "aarch64-pc-windows-msvc", "x86_64-unknown-linux-musl", "riscv64-linux", "darwin"]) {
    assert.throws(() => computerUseTarget(value), /unsupported/);
  }
});

test("platform bundle resources select matching native browser trees", () => {
  for (const [platform, tree] of [["windows", "chrome-win"], ["macos", "chrome-mac"], ["linux", "chrome-linux"]]) {
    const config = JSON.parse(readFileSync(new URL(`../src-tauri/tauri.${platform}.conf.json`, import.meta.url)));
    assert(config.bundle.resources.some((path) => path.endsWith(`chromium/${tree}/**/*`)));
    assert(!config.bundle.resources.some((path) => /chromium.*\.zip/.test(path)));
  }
});
