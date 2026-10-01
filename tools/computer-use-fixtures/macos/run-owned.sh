#!/bin/bash
# Opt-in local gate. Never edits TCC, installs a runtime, signs, or changes the user's App.
set -euo pipefail
if [[ "$(uname -s)" != Darwin ]]; then
  echo 'not_run: an interactive macOS host is required' >&2
  exit 2
fi
if [[ $# != 1 || "$1" != --owned-cocoa ]]; then
  echo 'not_run: explicitly pass --owned-cocoa (requires existing Screen Recording and Accessibility grants)' >&2
  exit 2
fi
if [[ "$(sysctl -in sysctl.proc_translated 2>/dev/null || true)" == 1 ]]; then
  echo 'not_run: translated/Rosetta execution is not native architecture evidence' >&2
  exit 2
fi
repo="$(cd "$(dirname "$0")/../../.." && pwd)"
cd "$repo"
root="$(mktemp -d "${TMPDIR:-/tmp}/grok-cu-cocoa.XXXXXXXX")"
echo "Owned Cocoa evidence: $root"
snapshot() {
  {
    find src-tauri/src/computer_use/macos_adapter src-tauri/computer-use-core/src \
      src-tauri/computer-use-core/tests tools/computer-use-fixtures/macos -type f \
      \( -name '*.rs' -o -name '*.swift' -o -name '*.mjs' -o -name '*.sh' \) \
      -exec shasum -a 256 {} \;
    shasum -a 256 src-tauri/src/computer_use/macos_adapter.rs \
      src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/computer-use-core/Cargo.toml
  } | LC_ALL=C sort
}
snapshot > "$root/source-before.sha256"
{ date -u '+%Y-%m-%dT%H:%M:%SZ'; sw_vers; uname -m; \
  sysctl -n hw.machine; sysctl -n hw.optional.arm64 2>/dev/null || true; \
  rustc -vV; xcrun swiftc --version; \
  git rev-parse HEAD; git status --short; } > "$root/environment.txt"
xcrun swiftc -swift-version 5 -warnings-as-errors \
  tools/computer-use-fixtures/macos/InputFixture.swift -o "$root/InputFixture" \
  > "$root/swift-build.log" 2>&1
cargo build --manifest-path src-tauri/Cargo.toml --locked -p grok-computer-use-core \
  --features test-support --bin cu-macos-native --target-dir "$root/target" \
  > "$root/rust-build.log" 2>&1
shasum -a 256 "$root/InputFixture" "$root/target/debug/cu-macos-native" > "$root/binaries.sha256"
set +e
GROK_CU_MACOS_FIXTURE=owned-cocoa "$root/target/debug/cu-macos-native" --owned-cocoa \
  "$root/InputFixture" "$root/evidence" > "$root/native.log" 2>&1
code=$?
set -e
printf '%s\n' "$code" > "$root/native.exit"
snapshot > "$root/source-after.sha256"
if ! cmp -s "$root/source-before.sha256" "$root/source-after.sha256"; then
  echo 'FAIL: source changed during acceptance; do not use the native result as frozen evidence' >&2
  exit 1
fi
cat "$root/native.log"
echo "Artifacts retained at $root (native exit $code). No fixture or user process is restarted."
exit "$code"
