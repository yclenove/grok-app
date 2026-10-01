import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import test from "node:test";
import { childEnv, keepChildEnvKey } from "./child-env.mjs";

test("child env allowlist drops API keys and proxies", () => {
  assert.equal(keepChildEnvKey("OPENAI_API_KEY"), false);
  assert.equal(keepChildEnvKey("GROK_API_KEY"), false);
  assert.equal(keepChildEnvKey("HTTP_PROXY"), false);
  assert.equal(keepChildEnvKey("NODE_OPTIONS"), false);
  assert.equal(keepChildEnvKey("GROK_CU_BROWSER_TOKEN"), true);
  assert.equal(keepChildEnvKey("SystemRoot") || keepChildEnvKey("HOME"), true);
});

test("spawned child cannot see SHOULD_NOT_REACH_CHILD sentinels", async () => {
  const previousOpen = process.env.OPENAI_API_KEY;
  const previousGrok = process.env.GROK_API_KEY;
  process.env.OPENAI_API_KEY = "SHOULD_NOT_REACH_CHILD";
  process.env.GROK_API_KEY = "SHOULD_NOT_REACH_CHILD";
  try {
    const env = childEnv({
      GROK_CU_BROWSER_TOKEN: "fixture-token",
      OPENAI_API_KEY: "SHOULD_NOT_REACH_CHILD",
      GROK_API_KEY: "SHOULD_NOT_REACH_CHILD",
    });
    assert.equal(env.OPENAI_API_KEY, undefined);
    assert.equal(env.GROK_API_KEY, undefined);
    const child = spawn(
      process.execPath,
      [
        "-e",
        "process.stdout.write(JSON.stringify({o:process.env.OPENAI_API_KEY||null,g:process.env.GROK_API_KEY||null,t:process.env.GROK_CU_BROWSER_TOKEN||null}))",
      ],
      { env, stdio: ["ignore", "pipe", "ignore"] },
    );
    const raw = await new Promise((resolve, reject) => {
      const chunks = [];
      child.stdout.on("data", (chunk) => chunks.push(chunk));
      child.on("error", reject);
      child.on("exit", (code) => {
        if (code !== 0) reject(new Error(`env probe exit ${code}`));
        else resolve(Buffer.concat(chunks).toString("utf8"));
      });
    });
    const seen = JSON.parse(raw);
    assert.equal(seen.o, null);
    assert.equal(seen.g, null);
    assert.equal(seen.t, "fixture-token");
  } finally {
    if (previousOpen === undefined) delete process.env.OPENAI_API_KEY;
    else process.env.OPENAI_API_KEY = previousOpen;
    if (previousGrok === undefined) delete process.env.GROK_API_KEY;
    else process.env.GROK_API_KEY = previousGrok;
  }
});
