/** Explicit modes only: a source audit must never become seed preparation. */
import { computerUseTarget } from "./computer-use-targets.mjs";

export const runtimeUsage = "usage: prepare-computer-use-runtime.mjs --prepare|--check|--verify-sources --target <target>";
const modes = new Set(["--prepare", "--check", "--verify-sources"]);

export function runtimeRequest(args, environment = {}) {
  if (args.length === 1 && args[0] === "--help") return { help: true };
  let mode;
  let target;
  for (let index = 0; index < args.length; index += 1) {
    const arg = args[index];
    if (modes.has(arg)) {
      if (mode) throw new Error("exactly one runtime mode is required");
      mode = arg;
    } else if (arg === "--target") {
      if (target !== undefined) throw new Error("--target must be specified only once");
      const value = args[++index];
      if (!value || value.startsWith("--")) throw new Error("--target requires a value");
      target = value;
    } else {
      throw new Error(`unknown runtime argument ${arg}`);
    }
  }
  if (!mode) throw new Error("exactly one runtime mode is required");
  target ??= environment.GROK_CU_TARGET;
  if (!target) throw new Error("--target is required. Do not guess host equals target.");
  const extra = [];
  if (environment.GROK_CU_SEED) extra.push("--seed", environment.GROK_CU_SEED);
  if (environment.GROK_CU_CACHE) extra.push("--cache", environment.GROK_CU_CACHE);
  return { mode, target: computerUseTarget(target).arch, extra };
}

export function runtimeCargoArgs(request, root) {
  if (request.help) throw new Error("help must not launch cargo");
  return [
    "run", "-p", "grok-computer-use-core", "--locked", "--quiet",
    "--bin", "cu-prepare-runtime", "--target-dir", "target-cu-review", "--",
    request.mode, "--target", request.target, "--repo", root, ...request.extra,
  ];
}
