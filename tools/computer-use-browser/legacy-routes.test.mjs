import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

const ROOT = dirname(fileURLToPath(import.meta.url));
const NEEDLES = [
  'pathname === "/marker"',
  'pathname === "/state"',
  'pathname === "/crash"',
  'pathname === "/popup"',
  'path == "/popup"',
  "slow_click",
  "managed_action_id",
  '|| "#pop"',
  "a#dl",
  "arrayBuffer()",
];

test("production worker and Host client have no test or selector backdoors", () => {
  const files = {
    server: readFileSync(join(ROOT, "server.mjs"), "utf8"),
    typed: readFileSync(join(ROOT, "typed-act.mjs"), "utf8"),
    worker: readFileSync(
      join(ROOT, "../../src-tauri/src/computer_use/playwright_worker.rs"),
      "utf8",
    ),
    host: readFileSync(join(ROOT, "../../src-tauri/computer-use-core/src/browser.rs"), "utf8"),
  };
  for (const [name, src] of Object.entries(files)) {
    for (const needle of NEEDLES) {
      assert.equal(src.includes(needle), false, `${name} ${needle}`);
    }
  }
  assert.equal(files.server.includes('pathname === "/pids"'), true);
  assert.equal(files.server.includes("x-grok-cu-host"), true);
});
