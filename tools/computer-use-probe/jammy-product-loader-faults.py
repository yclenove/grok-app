#!/usr/bin/env python3
"""Run the real product Rust loader at installed/relocated paths, without a UI."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def run(binary, resources, output, fallback):
    if not output.is_absolute() or output.exists() or output.is_symlink():
        raise RuntimeError("New absolute probe directory required")
    if output.parent.resolve(strict=True) != output.parent:
        raise RuntimeError("Canonical probe parent required")
    if not fallback.is_absolute() or not fallback.is_file():
        raise RuntimeError("Explicit older fallback library required")
    dynamic = subprocess.run(["readelf", "--dynamic", str(binary)], check=True, capture_output=True, text=True).stdout
    expected_rpath = "$ORIGIN/../lib/grok-app/resources/computer-use/native-pipewire/runtime/lib:$ORIGIN/resources/computer-use/native-pipewire/runtime/lib"
    if "(RPATH)" not in dynamic or "(RUNPATH)" in dynamic or f"[{expected_rpath}]" not in dynamic:
        raise RuntimeError("Product DT_RPATH differs from packaged layout")
    output.mkdir(mode=0o700)
    results = []
    cases = (
        ("installed", 0), ("cargo-layout", 0), ("relocated space 中文", 0), ("nonutf8-\udcff", 0),
        ("host-sdk-prepended", 0), ("ambient-overrides", 0),
        ("missing-dso", 2), ("wrong-dso", 2), ("missing-module", 2),
        ("missing-video-adapter", 2), ("corrupt-video-adapter", 2),
        ("changed-component", 2), ("wrong-manifest", 2),
        ("extra-config", 2), ("extra-resource", 2),
        ("linked-manifest", 2), ("fifo-manifest", 2), ("fifo-component", 2),
        ("linked-resource-parent", 2), ("missing-resource", 2),
    )
    for index, (name, expected) in enumerate(cases):
        package = output / f"case-{index:02d}" / name
        exe = package / "usr/bin/grok-app"
        native = (exe.parent if name == "cargo-layout" else package / "usr/lib/grok-app") / "resources/computer-use/native-pipewire"
        exe.parent.mkdir(parents=True)
        native.parent.mkdir(parents=True)
        shutil.copy2(binary, exe)
        shutil.copytree(resources, native)
        env = dict(os.environ, LD_LIBRARY_PATH=str(fallback.parent),
                   GROK_CU_LOADER_EXPECTED_RESOURCE=str(native))
        library = native / "runtime/lib/libpipewire-0.3.so.0"
        if name == "ambient-overrides":
            env.update(APPDIR="/not-the-installation", GROK_CU_PIPEWIRE_RUNTIME="/not-the-sdk",
                       SPA_PLUGIN_DIR="/wrong-spa", PIPEWIRE_MODULE_DIR="/wrong-modules", PIPEWIRE_CONFIG_DIR="/wrong-config",
                       PIPEWIRE_CONFIG_NAME="null", PIPEWIRE_CONFIG_PREFIX="/wrong-prefix")
        elif name == "missing-dso":
            library.unlink()
        elif name == "wrong-dso":
            shutil.copyfile(fallback, library)
        elif name == "missing-module":
            (native / "runtime/lib/pipewire-0.3/libpipewire-module-client-node.so").unlink()
        elif name == "missing-video-adapter":
            (native / "runtime/lib/spa-0.2/videoconvert/libspa-videoconvert.so").unlink()
        elif name == "corrupt-video-adapter":
            (native / "runtime/lib/spa-0.2/videoconvert/libspa-videoconvert.so").write_bytes(b"invalid video adapter ELF")
        elif name == "changed-component":
            with (native / "runtime/share/pipewire/client.conf").open("ab") as stream:
                stream.write(b"\nchanged")
        elif name == "wrong-manifest":
            (native / "manifest.json").write_text("{}")
        elif name == "extra-config":
            extra = native / "runtime/share/pipewire/client.conf.d/unapproved.conf"
            extra.parent.mkdir()
            extra.write_text("unapproved")
        elif name == "extra-resource":
            (native / "unapproved").write_text("unexpected")
        elif name in ("linked-manifest", "fifo-manifest", "fifo-component"):
            target = native / ("runtime/share/pipewire/client.conf" if name == "fifo-component" else "manifest.json")
            target.unlink()
            if name == "linked-manifest":
                target.symlink_to(resources / "manifest.json")
            else:
                os.mkfifo(target)
        elif name == "linked-resource-parent":
            original = package / "usr/lib/grok-app"
            moved = package / "resource-moved"
            original.rename(moved)
            original.symlink_to(moved, target_is_directory=True)
        elif name == "missing-resource":
            native.rename(package / "unavailable-resources")
        completed = subprocess.run([str(exe)], capture_output=True, text=True, env=env, timeout=15)
        (output / f"case-{index:02d}.stdout.txt").write_text(completed.stdout)
        (output / f"case-{index:02d}.stderr.txt").write_text(completed.stderr)
        item = {"case": name, "expectedExit": expected, "actualExit": completed.returncode,
                "passed": completed.returncode == expected, "binarySha256": digest(exe),
                "stdout": completed.stdout, "stderr": completed.stderr}
        results.append(item)
        print(json.dumps(item, ensure_ascii=True))
    manifest = json.loads((resources / "manifest.json").read_text())
    report = {"schemaVersion": 1, "scope": "actual-product-loader-module-on-linux",
              "target": manifest["target"], "glibc": manifest["glibcBaseline"],
              "appAcceptance": False, "portalAcceptance": False,
              "desktopAcceptance": False, "binarySha256": digest(binary), "cases": results,
              "passed": all(item["passed"] for item in results)}
    with (output / "results.json").open("x") as stream:
        json.dump(report, stream, indent=2)
        stream.write("\n")
    if not report["passed"]:
        raise RuntimeError("Product loader case failed; original evidence retained")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("binary", "resources", "output"):
        parser.add_argument(name, type=Path)
    parser.add_argument("--fallback-library", type=Path, required=True)
    args = parser.parse_args()
    run(args.binary, args.resources, args.output, args.fallback_library)
