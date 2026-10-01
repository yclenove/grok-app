// Protocol/lifecycle adversary ONLY. Never used by the native acceptance runner
// or counted as Cocoa/input evidence. Spawned by Rust transport contract tests.
import { createInterface } from "node:readline";
const mode = process.env.GROK_CU_PIPE_CONTRACT_FAULT;
if (!mode || process.argv.at(-2) !== "--owned-fixture") process.exit(2);
const nonce = process.argv.at(-1);
const state = { version: 2, nonce, id: 0, pid: process.pid, windowId: 7,
  architecture: process.arch === "arm64" ? "aarch64" : process.arch === "x64" ? "x86_64" : process.arch,
  translated: false,
  title: `GrokCuOwned-${nonce}`, active: true, focused: true, text: "原有😀text",
  selection: [0, 0], keys: [], clicks: 0,
  pointer: { width: 660, height: 180, scrollOffset: 500,
    boxRect: [290,70,80,40], dragging: false, overflow: false, events: [] } };
const reply = () => process.stdout.write(JSON.stringify(state) + "\n");
if (mode === "startup-eof") process.exit(0);
reply();
const lines = createInterface({ input: process.stdin });
lines.on("close", () => process.exit(0));
lines.on("line", line => {
  const req = JSON.parse(line);
  if (req.version !== 2 || req.nonce !== nonce || req.id !== state.id + 1) process.exit(3);
  state.id = req.id;
  if (req.command === "quit") {
    reply(); process.exitCode = mode === "bad-exit" ? 3 : 0; lines.close();
    // close callback above is intentionally overridden for nonzero-exit case.
    return;
  }
  switch (mode) {
    case "bad-json": process.stdout.write("not-json\n"); return;
    case "oversized": process.stdout.write("x".repeat(32769) + "\n"); return;
    case "partial-eof": process.stdout.write('{"version":1}'); process.exit(0); break;
    case "reply-timeout": return;
    case "wrong-nonce": state.nonce = "wrong"; break;
    case "wrong-pid": state.pid++; break;
    case "wrong-window": state.windowId++; break;
    case "stale-id": state.id--; break;
  }
  reply();
});
if (mode === "bad-exit") {
  lines.removeAllListeners("close");
  lines.on("close", () => process.exit(3));
}
