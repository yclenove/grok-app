import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import process from "node:process";
import { fileURLToPath, URL } from "node:url";
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

test("actual Linux before-build refuses a missing private SDK before any preparation", () => {
  const result = spawnSync(process.execPath, [fileURLToPath(new URL("./tauri-before-build.mjs", import.meta.url))], {
    encoding: "utf8", env: { ...process.env, TAURI_ENV_TARGET_TRIPLE: "x86_64-unknown-linux-gnu",
      GROK_CU_BUNDLE_TARGET: "", GROK_CU_SEED: "", GROK_CU_PIPEWIRE_RUNTIME: "" },
  });
  assert.equal(result.status, 1, result.stdout + result.stderr);
  assert.match(result.stderr, /GROK_CU_PIPEWIRE_RUNTIME must point at an audited/);
  assert.equal(result.stdout, "");
});

test("Linux resources include the exact private manifest and runtime tree", () => {
  const config = JSON.parse(readFileSync(new URL("../src-tauri/tauri.linux.conf.json", import.meta.url)));
  assert(config.bundle.resources.includes("resources/computer-use/native-pipewire/manifest.json"));
  assert(config.bundle.resources.includes("resources/computer-use/native-pipewire/runtime/**/*"));
  for (const platform of ["macos", "windows"]) {
    const other = JSON.parse(readFileSync(new URL(`../src-tauri/tauri.${platform}.conf.json`, import.meta.url)));
    assert(!other.bundle.resources.some(path => path.includes("native-pipewire")));
  }
});

test("Linux packages declare managed browser NSS, NSPR and ALSA loader dependencies", () => {
  const config = JSON.parse(readFileSync(new URL("../src-tauri/tauri.conf.json", import.meta.url)));
  const { deb, rpm } = config.bundle.linux;
  for (const dependency of ["libwebkit2gtk-4.1-0", "libgtk-3-0", "libnspr4", "libnss3", "libasound2 | libasound2t64"]) {
    assert(deb.depends.includes(dependency), `missing Debian runtime dependency: ${dependency}`);
  }
  for (const library of ["libnspr4.so", "libnss3.so", "libnssutil3.so", "libsmime3.so", "libasound.so.2"]) {
    assert(rpm.depends.includes(`${library}()(64bit)`), `missing RPM runtime capability: ${library}`);
  }
  assert.equal(rpm.release, "1");
  assert.equal(rpm.requires, undefined, "Tauri uses depends, not an ignored requires field");
  const linux = JSON.parse(readFileSync(new URL("../src-tauri/tauri.linux.conf.json", import.meta.url)));
  assert.equal(linux.bundle.linux, undefined, "platform resource config must not replace package dependencies");
});

test("Jammy production CI uses the real frontend, product config and release gates", () => {
  const workflow = readFileSync(new URL("../.github/workflows/ci.yml", import.meta.url), "utf8");
  const heading = "  linux-production-jammy:";
  const start = workflow.indexOf(heading);
  assert(start >= 0, "full production Jammy job is required");
  const tail = workflow.slice(start + heading.length);
  const nextJob = tail.search(/^ {2}[\w-]+:/m);
  const job = nextJob < 0 ? tail : tail.slice(0, nextJob);
  assert.match(workflow, /uses: actions\/upload-artifact@v4\s+with:\s+name: production-frontend\s+path: dist\/\s+if-no-files-found: error/);
  assert.match(job, /needs: frontend/);
  assert.match(job, /runs-on: ubuntu-22\.04/);
  assert.match(job, /getconf GNU_LIBC_VERSION/);
  assert.match(job, /glibc 2\.35/);
  assert.match(job, /uses: actions\/download-artifact@v4\s+with:\s+name: production-frontend\s+path: dist\//);
  assert.match(job, /test -f dist\/index\.html/);
  assert.match(job, /export TAURI_CONFIG="\$\(<tauri\.linux\.conf\.json\)"/);
  assert.doesNotMatch(job, /TAURI_CONFIG:/, "do not maintain a second hand-written resource map");
  assert.match(job, /cargo build --locked --release --features tauri\/custom-protocol --bin grok-app/);
  assert.match(job, /cargo clippy --locked --release --all-targets --features tauri\/custom-protocol -- -D warnings/);
  assert.match(job, /cargo test --locked --release --features tauri\/custom-protocol/);
  assert.match(job, /computer_use::webview::tests::navigation/);
});
