/** Target identity is explicit. Host platform never selects a cross-build pack. */
const targets = [
  { arch: "x86_64-windows", triple: "x86_64-pc-windows-msvc", lock: "windows-x64.lock.json" },
  { arch: "aarch64-macos", triple: "aarch64-apple-darwin", lock: "macos-arm64.lock.json" },
  { arch: "x86_64-macos", triple: "x86_64-apple-darwin", lock: "macos-x64.lock.json" },
  { arch: "x86_64-linux", triple: "x86_64-unknown-linux-gnu", lock: "linux-x64.lock.json" },
];

export function computerUseTarget(value) {
  const target = targets.find((candidate) => value === candidate.arch || value === candidate.triple);
  if (!target) throw new Error(`unsupported Computer Use runtime target ${String(value || "(missing)")}`);
  return target;
}

export const computerUseTargets = Object.freeze(targets.map((target) => Object.freeze(target)));
