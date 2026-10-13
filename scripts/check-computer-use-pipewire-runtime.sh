#!/usr/bin/env bash
# Native loader + module proof; no desktop, compositor, portal or remote socket.
set -euo pipefail
if [[ ( $# != 3 && $# != 4 ) || $1 != /* || $2 != /* || $3 != /* || -e $3 || -L $3 ]]; then
  echo 'Usage: check-computer-use-pipewire-runtime.sh /private/PW-SDK /private/EI-SDK /new/output [target]' >&2
  exit 2
fi
sdk=$1
ei_sdk=$2
out=$3
target=${4:-x86_64-unknown-linux-gnu}
case "$target:$(uname -m)" in
  x86_64-unknown-linux-gnu:x86_64|aarch64-unknown-linux-gnu:aarch64) ;;
  *) echo 'Private SDK smoke requires a matching native Linux target' >&2; exit 2 ;;
esac
for directory in "$sdk" "$ei_sdk"; do
  [[ -d $directory && ! -L $directory && $(realpath -e "$directory") == "$directory" ]] || exit 2
done
[[ $(realpath -e "$(dirname -- "$out")") == "$(dirname -- "$out")" ]] || exit 2
for tool in cc pkg-config readelf timeout python3 patchelf; do
  command -v "$tool" >/dev/null || { echo "Required tool missing: $tool" >&2; exit 2; }
done
for required in lib/libpipewire-0.3.so lib/libpipewire-0.3.so.0 lib/pkgconfig/libpipewire-0.3.pc \
  lib/spa-0.2/support/libspa-support.so \
  lib/spa-0.2/videoconvert/libspa-videoconvert.so \
  lib/pipewire-0.3/libpipewire-module-protocol-native.so \
  lib/pipewire-0.3/libpipewire-module-client-node.so share/pipewire/client.conf \
  share/licenses/pipewire/COPYING share/licenses/pipewire/LICENSE \
  share/licenses/pipewire/0001-CVE-2026-14330.patch share/licenses/pipewire/SOURCE.txt; do
  [[ -f $sdk/$required ]] || { echo "Private component missing: $required" >&2; exit 2; }
  [[ $(realpath -e "$sdk/$required") == "$sdk/"* ]] || exit 2
done
for required in lib/libei.a share/licenses/libei/COPYING; do
  [[ -f $ei_sdk/$required && ! -L $ei_sdk/$required ]] || exit 2
  [[ $(realpath -e "$ei_sdk/$required") == "$ei_sdk/"* ]] || exit 2
done
[[ ! -e $sdk/lib/spa-0.2/alsa/libspa-alsa.so && ! -e $sdk/lib/libjackserver.so ]] || exit 2
export PKG_CONFIG_PATH="$sdk/lib/pkgconfig:$ei_sdk/lib/pkgconfig"
[[ $(pkg-config --variable=prefix libpipewire-0.3) == "$sdk" ]] || exit 2
[[ $(pkg-config --variable=prefix libei-1.0) == "$ei_sdk" ]] || exit 2
[[ $(pkg-config --modversion libpipewire-0.3) == 1.6.9 ]] || exit 2
[[ $(pkg-config --modversion libei-1.0) == 1.5.0 ]] || exit 2
version=$(pkg-config --modversion libpipewire-0.3)
repo=$(cd -- "$(dirname -- "$0")/.." && pwd)
mkdir -m 700 "$out"
mkdir -m 700 "$out/empty-config"
# pkg-config escapes spaces; parse into argv, never feed its output to eval.
python3 - "$repo" "$ei_sdk" "$out" <<'PY'
from pathlib import Path
import shlex
import subprocess
import sys

repo, ei, out = map(Path, sys.argv[1:])
def flags(*arguments):
    return shlex.split(subprocess.check_output(["pkg-config", *arguments], text=True))
command = ["cc", "-Wall", "-Wextra", "-Werror",
           str(repo / "tools/computer-use-probe/jammy-sdk-smoke.c"),
           "-o", str(out / "sdk-smoke")]
command += flags("--cflags", "--libs", "libpipewire-0.3")
command += flags("--cflags", "libei-1.0")
command += [str(ei / "lib/libei.a")]
command += [flag for flag in flags("--libs", "--static", "libei-1.0") if flag != "-lei"]
subprocess.run(command + ["-ldl"], check=True, timeout=120)
PY
readelf -d "$out/sdk-smoke" > "$out/dynamic.txt"
if grep -q 'NEEDED.*libei' "$out/dynamic.txt"; then
  echo 'Unexpected dynamic libei dependency' >&2
  exit 2
fi
runtime="$out/runtime"
mkdir -m 700 "$runtime"
mkdir -p "$runtime/lib/pipewire-0.3" "$runtime/lib/spa-0.2/support" \
  "$runtime/lib/spa-0.2/audioconvert" "$runtime/lib/spa-0.2/videoconvert" \
  "$runtime/share/pipewire" "$runtime/share/licenses/libei"
# Dereference the SONAME alias once. No original build symlink enters the runtime.
cp -L -- "$sdk/lib/libpipewire-0.3.so.0" "$runtime/lib/libpipewire-0.3.so.0"
for module in rt protocol-native client-node client-device adapter metadata session-manager; do
  original="$sdk/lib/pipewire-0.3/libpipewire-module-$module.so"
  [[ -f $original && ! -L $original && $(realpath -e "$original") == "$sdk/"* ]] || exit 2
  cp -- "$original" "$runtime/lib/pipewire-0.3/"
done
for plugin in support/libspa-support.so audioconvert/libspa-audioconvert.so \
  videoconvert/libspa-videoconvert.so; do
  original="$sdk/lib/spa-0.2/$plugin"
  [[ -f $original && ! -L $original && $(realpath -e "$original") == "$sdk/"* ]] || exit 2
  cp -- "$original" "$runtime/lib/spa-0.2/$plugin"
done
cp -- "$sdk/share/pipewire/client.conf" "$runtime/share/pipewire/"
cp -a -- "$sdk/share/licenses/pipewire" "$runtime/share/licenses/"
cp -- "$ei_sdk/share/licenses/libei/COPYING" "$runtime/share/licenses/libei/"
# Upstream installed modules retain an absolute build RUNPATH. Strip it only
# on the owned runtime copies; preserve the SDK and record the resulting hash.
for library in "$runtime/lib/libpipewire-0.3.so.0" "$runtime/lib/pipewire-0.3/"*.so \
  "$runtime/lib/spa-0.2/support/libspa-support.so" "$runtime/lib/spa-0.2/audioconvert/libspa-audioconvert.so" \
  "$runtime/lib/spa-0.2/videoconvert/libspa-videoconvert.so"; do
  patchelf --remove-rpath "$library"
done
python3 "$repo/scripts/audit-computer-use-native-sdk.py" "$runtime" --target "$target" \
  --output "$out/runtime-inventory.json"
for prefix in "$sdk" "$runtime"; do
  env -i PATH=/usr/bin:/bin XDG_CONFIG_HOME="$out/empty-config" \
    LD_LIBRARY_PATH="$prefix/lib" SPA_PLUGIN_DIR="$prefix/lib/spa-0.2" \
    PIPEWIRE_MODULE_DIR="$prefix/lib/pipewire-0.3" \
    PIPEWIRE_CONFIG_DIR="$prefix/share/pipewire" PIPEWIRE_CONFIG_PREFIX= \
    timeout 15 "$out/sdk-smoke" "$prefix" "$version" | tee -a "$out/native.log"
done
printf 'Private SDK loader/relocation passed; installed App/portal acceptance=false.\n'
