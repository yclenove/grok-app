import test from "node:test";
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import process from "node:process";
import { fileURLToPath, URL } from "node:url";
import { computerUseTargets } from "./computer-use-targets.mjs";
import { runtimeCargoArgs, runtimeRequest } from "./computer-use-runtime-command.mjs";

const modes = ["--prepare", "--check", "--verify-sources"];
const script = fileURLToPath(new URL("./prepare-computer-use-runtime.mjs", import.meta.url));
const root = fileURLToPath(new URL("..", import.meta.url));

test("all modes pass the canonical identity for each explicit arch or triple", () => {
  for (const mode of modes) {
    for (const target of computerUseTargets) {
      for (const value of [target.arch, target.triple]) {
        const request = runtimeRequest([mode, "--target", value]);
        assert.deepEqual(request, { mode, target: target.arch, extra: [] });
        const args = runtimeCargoArgs(request, root);
        const delimiter = args.indexOf("--");
        assert.deepEqual(args.slice(delimiter + 1), [mode, "--target", target.arch, "--repo", root]);
        assert(args.includes("--locked"));
        assert(!args.includes("--help"));
        if (mode === "--verify-sources") assert(!args.includes("--prepare"));
      }
    }
  }
});

test("every repeated or mixed mode fails instead of silently selecting prepare/check", () => {
  for (const first of modes) {
    for (const second of modes) {
      assert.throws(() => runtimeRequest([first, second, "--target", "x86_64-windows"]), /exactly one/);
    }
  }
});

test("missing, repeated, unsupported and valueless targets fail closed", () => {
  for (const args of [
    [], ["--check"], ["--check", "--target"], ["--check", "--target", ""],
    ["--check", "--target", "--prepare"], ["--check", "--target", "x86_64-unknown-linux-musl"],
    ["--check", "--target", "x86_64-windows", "--target", "x86_64-windows"],
  ]) assert.throws(() => runtimeRequest(args));
  assert.throws(() => runtimeRequest(["--check"], { GROK_CU_TARGET: "darwin" }), /unsupported/);
});

test("unknown arguments and help mixed with work modes cannot launch an audit/prepare", () => {
  for (const arg of ["--verfy-sources", "--unknown", "stray", "--help"]) {
    assert.throws(() => runtimeRequest(["--prepare", "--target", "x86_64-windows", arg]), /unknown/);
  }
  assert.deepEqual(runtimeRequest(["--help"]), { help: true });
  assert.throws(() => runtimeCargoArgs({ help: true }, root), /must not launch/);
});

test("explicit target overrides environment without dropping seed/cache boundaries", () => {
  const environment = { GROK_CU_TARGET: "darwin", GROK_CU_SEED: "seed with spaces", GROK_CU_CACHE: "cache with spaces" };
  const request = runtimeRequest(["--verify-sources", "--target", "aarch64-apple-darwin"], environment);
  assert.equal(request.target, "aarch64-macos");
  assert.deepEqual(request.extra, ["--seed", environment.GROK_CU_SEED, "--cache", environment.GROK_CU_CACHE]);
  assert.deepEqual(runtimeCargoArgs(request, root).slice(-4), request.extra);
  assert.equal(runtimeRequest(["--check"], { GROK_CU_TARGET: "x86_64-unknown-linux-gnu" }).target, "x86_64-linux");
});

test("actual Node entry rejects unsafe arguments and serves help without cargo or seed mutation", () => {
  for (const [args, status] of [
    [["--help"], 0], [[], 2], [["--verify-sources", "--prepare", "--target", "x86_64-windows"], 2],
    [["--verify-sources", "--target"], 2], [["--check", "--target", "x86_64-windows", "--unknown"], 2],
  ]) {
    const result = spawnSync(process.execPath, [script, ...args], {
      encoding: "utf8", env: { ...process.env, PATH: "", GROK_CU_TARGET: "", GROK_CU_SEED: "", GROK_CU_CACHE: "" },
    });
    assert.equal(result.status, status, result.stdout + result.stderr);
    assert(!result.error);
    assert(!result.stderr.includes("Unable to launch cargo"));
    assert.match(status ? result.stderr : result.stdout, /usage: prepare-computer-use-runtime/);
    if (!status) assert.match(result.stdout, /does not prepare or publish/);
  }
});
