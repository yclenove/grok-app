#!/usr/bin/env python3
"""Negative loading of exact private SDK copies; no App/desktop/portal action."""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import subprocess


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("smoke_output", type=Path)
    parser.add_argument("new_output", type=Path)
    parser.add_argument("--old-sdk", type=Path)
    args = parser.parse_args()
    original, output = args.smoke_output, args.new_output
    if original.resolve(strict=True) != original or output.parent.resolve(strict=True) != output.parent:
        raise RuntimeError("Canonical absolute directories required")
    if not output.is_absolute() or output.exists() or output.is_symlink():
        raise RuntimeError("New absolute output required")
    source = original / "runtime"
    before = {str(path.relative_to(source)): hashlib.sha256(path.read_bytes()).hexdigest()
              for path in source.rglob("*") if path.is_file()}
    output.mkdir(mode=0o700)
    results = []
    faults = (
        ("missing-library", "lib/libpipewire-0.3.so.0", "missing", [2, 127]),
        ("corrupt-library", "lib/libpipewire-0.3.so.0", "corrupt", [127]),
        ("missing-support", "lib/spa-0.2/support/libspa-support.so", "missing", [3]),
        ("missing-protocol", "lib/pipewire-0.3/libpipewire-module-protocol-native.so", "missing", [4]),
        ("missing-client-node", "lib/pipewire-0.3/libpipewire-module-client-node.so", "missing", [4]),
        ("missing-video-adapter", "lib/spa-0.2/videoconvert/libspa-videoconvert.so", "missing", [6]),
        ("corrupt-video-adapter", "lib/spa-0.2/videoconvert/libspa-videoconvert.so", "corrupt", [6]),
    )
    def invoke(name, runtime, expected_prefix, expected_version, exits):
        env = {"PATH": "/usr/bin:/bin", "XDG_CONFIG_HOME": str(original / "empty-config"),
               "LD_LIBRARY_PATH": str(runtime / "lib"), "SPA_PLUGIN_DIR": str(runtime / "lib/spa-0.2"),
               "PIPEWIRE_MODULE_DIR": str(runtime / "lib/pipewire-0.3"),
               "PIPEWIRE_CONFIG_DIR": str(runtime / "share/pipewire"), "PIPEWIRE_CONFIG_PREFIX": ""}
        result = subprocess.run([str(original / "sdk-smoke"), str(expected_prefix), expected_version],
                                env=env, capture_output=True, text=True, timeout=15)
        results.append({"name": name, "exitCode": result.returncode, "expectedExitCodes": exits,
                        "rejectedAsExpected": result.returncode in exits,
                        "stdout": result.stdout, "stderr": result.stderr})
        (output / "results.json").write_text(json.dumps(results, indent=2) + "\n")
        if result.returncode not in exits:
            raise RuntimeError("Native fault accepted or crashed unexpectedly: " + name)

    for name, relative, fault, exits in faults:
        runtime = output / name
        # Copies belong only to this probe; the positive SDK/runtime is untouched.
        shutil.copytree(source, runtime)
        path = runtime / relative
        if fault == "missing":
            path.unlink()
        else:
            path.write_bytes(b"intentional invalid ELF, native loader must reject")
        invoke(name, runtime, runtime, "1.6.9", exits)
    invoke("wrong-prefix", source, output / "not-the-loaded-prefix", "1.6.9", [2])
    invoke("wrong-version", source, source, "1.0.5", [2])
    if args.old_sdk:
        if args.old_sdk.resolve(strict=True) != args.old_sdk:
            raise RuntimeError("Canonical old SDK required")
        invoke("old-sdk-fallback", args.old_sdk, source, "1.6.9", [2])
    after = {str(path.relative_to(source)): hashlib.sha256(path.read_bytes()).hexdigest()
             for path in source.rglob("*") if path.is_file()}
    if before != after:
        raise RuntimeError("Original passing runtime changed")
    print(f"{len(results)} actual loader faults rejected; original runtime unchanged")


if __name__ == "__main__":
    main()
