#!/usr/bin/env bash
# Package on Windows after Tauri's release build. Never runs the app/installer.
# Local: bash scripts/package-windows-portable.sh v0.2.33
# CI publishing is explicit: add --upload (never inferred from a token).
# GROK_PORTABLE_EXE / GROK_PORTABLE_OUTPUT are explicit build/probe inputs only.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

UPLOAD=0
TAG="${TAG:-}"
TAG_ARG=0
for arg in "$@"; do
  case "$arg" in
    --upload) [[ "$UPLOAD" == 0 ]] || { echo 'duplicate --upload' >&2; exit 2; }; UPLOAD=1 ;;
    --*) echo "unknown option: $arg" >&2; exit 2 ;;
    *) [[ "$TAG_ARG" == 0 ]] || { echo 'only one version is allowed' >&2; exit 2; }; TAG="$arg"; TAG_ARG=1 ;;
  esac
done
if [[ -z "$TAG" ]]; then
  TAG="v$(node -p 'JSON.parse(require("node:fs").readFileSync("package.json","utf8")).version')"
fi
VER="${TAG#v}"
node --input-type=module - "$VER" <<'JS'
if(process.platform!=='win32')throw new Error('portable packaging requires the Windows build host');
if(!/^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$/.test(process.argv[2]))throw new Error('invalid portable version');
JS
if [[ "$UPLOAD" == 1 ]]; then
  command -v gh >/dev/null || { echo 'gh is required for explicit upload' >&2; exit 1; }
  [[ -n "${GH_TOKEN:-${GITHUB_TOKEN:-}}" ]] || { echo 'upload token is required' >&2; exit 1; }
  [[ -z "${GROK_PORTABLE_EXE:-}" && -z "${GROK_PORTABLE_OUTPUT:-}" ]] || { echo 'custom build/probe paths cannot be published' >&2; exit 1; }
fi
EXE="${GROK_PORTABLE_EXE:-src-tauri/target/release/grok-app.exe}"
OUTPUT="${GROK_PORTABLE_OUTPUT:-.}"
STAGE="$OUTPUT/dist-portable/Grok_${VER}_x64-portable"
OUT="$OUTPUT/Grok_${VER}_x64-portable.zip"
[[ -f "$EXE" ]] || { echo 'explicit Windows product EXE not found; build it first' >&2; exit 1; }
[[ ! -e "$STAGE" && ! -L "$STAGE" && ! -e "$OUT" && ! -L "$OUT" ]] || { echo 'portable destination already exists; choose a new owned output directory' >&2; exit 1; }

# Use native paths at process boundaries; Git Bash must not rewrite native argv.
native_path() { node -e 'console.log(require("node:path").resolve(process.argv[1]))' "$1"; }
RESOURCES="$(native_path src-tauri/resources)"
SEED="$(native_path src-tauri/resources/computer-use/seed)"
STAGE_NATIVE="$(native_path "$STAGE")"
OUT_NATIVE="$(native_path "$OUT")"
VALIDATION_NATIVE="$(native_path "$OUTPUT/.portable-validation-$(node -p 'require("node:crypto").randomUUID()')")"
OWNED_VALIDATION=0
cleanup_validation() {
  if [[ "$OWNED_VALIDATION" == 1 ]]; then
    node -e 'require("node:fs").rmSync(process.argv[1],{recursive:true,force:true})' "$VALIDATION_NATIVE"
  fi
}
trap cleanup_validation EXIT
GROK_CU_SEED="$SEED" node scripts/prepare-computer-use-runtime.mjs --check --target x86_64-pc-windows-msvc
# The Rust verifier correctly persists a lock/owner sidecar beside its seed.
# Verify in our private build image, then copy only the declared resources into
# the deliverable. Never delete lock files from a shared seed or ship owner data.
node scripts/stage-windows-portable.mjs --exe "$EXE" --resources "$RESOURCES" \
  --config src-tauri/tauri.windows.conf.json --stage "$VALIDATION_NATIVE" --version "$VER"
OWNED_VALIDATION=1
GROK_CU_SEED="$VALIDATION_NATIVE/resources/computer-use/seed" node scripts/prepare-computer-use-runtime.mjs --check --target x86_64-pc-windows-msvc
node scripts/stage-windows-portable.mjs --exe "$VALIDATION_NATIVE/Grok.exe" --resources "$VALIDATION_NATIVE/resources" \
  --config src-tauri/tauri.windows.conf.json --stage "$STAGE_NATIVE" --version "$VER"
node scripts/audit-computer-use-bundle.mjs "$STAGE_NATIVE" --target x86_64-pc-windows-msvc
MSYS2_ARG_CONV_EXCL='*' powershell.exe -NoProfile -NonInteractive -File "$(native_path scripts/package-windows-portable.ps1)" -Stage "$STAGE_NATIVE" -Archive "$OUT_NATIVE"
echo "Portable archive verified: $OUT_NATIVE"
if [[ "$UPLOAD" == 1 ]]; then
  gh release upload "$TAG" "$OUT" --clobber
  echo "uploaded $OUT to $TAG"
else
  echo 'Local artifact only; nothing uploaded.'
fi
