#!/usr/bin/env python3
"""Stage an audited private SDK for Tauri; never replace an existing resource tree."""
import argparse
import importlib.util
import json
from pathlib import Path
import shutil
import stat
import subprocess
import sys

HERE = Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location("native_sdk_audit", HERE / "audit-computer-use-native-sdk.py")
audit = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(audit)


def manifest(report):
    # Do not ship build-machine paths or claim installed/capture acceptance.
    return {"schemaVersion": 1, "target": "x86_64-unknown-linux-gnu",
            "pipewireVersion": "1.6.9", "glibcBaseline": "2.35",
            "files": [{key: item[key] for key in ("path", "bytes", "sha256")}
                      for item in report["files"]]}


def resource_modes(destination: Path, normalize=False):
    # Packages may be built by root but must remain readable by ordinary users.
    # Do not change the source SDK or repair an existing different resource tree.
    if sys.platform != "linux":
        return
    for path in (destination, *destination.rglob("*")):
        mode = path.lstat().st_mode
        if not (stat.S_ISDIR(mode) or stat.S_ISREG(mode)):
            raise audit.AuditError("Native resource contains a link or special file")
        expected = 0o755 if stat.S_ISDIR(mode) else 0o644
        if normalize:
            path.chmod(expected)
        elif stat.S_IMODE(mode) != expected:
            raise audit.AuditError("Existing native resource permissions differ; refusing repair")


def stage(runtime: Path, destination: Path):
    if not destination.is_absolute():
        raise audit.AuditError("Absolute native resource destination required")
    report = audit.audit(runtime, "2.35")
    expected = manifest(report)
    if destination.exists() or destination.is_symlink():
        audit.canonical_directory(destination)
        if {path.name for path in destination.iterdir()} != {"runtime", "manifest.json"}:
            raise audit.AuditError("Existing native resource contains unexpected entries")
        index = destination / "manifest.json"
        if index.is_symlink() or not index.is_file():
            raise audit.AuditError("Native manifest must be a regular file")
        actual = manifest(audit.audit(destination / "runtime", "2.35"))
        if actual != expected or json.loads(index.read_text(encoding="utf-8")) != expected:
            raise audit.AuditError("Existing native resource differs; refusing overwrite")
        resource_modes(destination)
        return "reused identical audited resources"
    audit.canonical_directory(destination.parent)
    destination.mkdir(mode=0o700)
    # Preserve links during copy so a concurrent source replacement cannot be
    # silently dereferenced into apparently approved regular files.
    shutil.copytree(runtime, destination / "runtime", symlinks=True)
    # Re-audit copies before publishing their compile-time manifest.
    if manifest(audit.audit(destination / "runtime", "2.35")) != expected:
        raise audit.AuditError("Native resource changed during copy")
    with (destination / "manifest.json").open("x", encoding="utf-8") as stream:
        json.dump(expected, stream, indent=2)
        stream.write("\n")
    resource_modes(destination, normalize=True)
    resource_modes(destination)
    return "staged audited private resources"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("runtime", type=Path)
    parser.add_argument("destination", type=Path)
    args = parser.parse_args()
    try:
        print(stage(args.runtime, args.destination))
        return 0
    except (audit.AuditError, OSError, ValueError, subprocess.SubprocessError) as error:
        print(f"Native SDK staging failed: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
