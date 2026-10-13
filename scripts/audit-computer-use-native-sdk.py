#!/usr/bin/env python3
"""Audit a relocated private Linux SDK for an explicit target and ABI baseline."""
from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import stat
import subprocess
import sys

MODULES = (
    "rt", "protocol-native", "client-node", "client-device", "adapter",
    "metadata", "session-manager",
)
LIBRARIES = (
    "lib/libpipewire-0.3.so.0",
    *(f"lib/pipewire-0.3/libpipewire-module-{name}.so" for name in MODULES),
    "lib/spa-0.2/support/libspa-support.so",
    "lib/spa-0.2/audioconvert/libspa-audioconvert.so",
)
NOTICE_HASHES = {
    "share/licenses/pipewire/COPYING":
        "8909c319a7e27dbb33a15b9035f89ab3b7b2f6a12f8bcddc755206a8db1ada44",
    "share/licenses/pipewire/LICENSE":
        "be4be5d77424833edf31f53fc1f1cecb6996b9e2d747d9e6fb8f878362ebc92b",
    "share/licenses/pipewire/0001-CVE-2026-14330.patch":
        "93b45411bfdcc2c839c492421875fb17d38fd76b04ad07c28870fa98ea92bc5e",
    "share/licenses/libei/COPYING":
        "99406b2e7cacd825c770d1652bbac228c8ab33ea0ae8e204c803949f11fb31cd",
}
SOURCE_LINES = (
    "PipeWire 1.6.9 + Debian 1.6.9-2 CVE-2026-14330 patch.",
    "Upstream source SHA256: a0012db8aaca4448fd205035e07d264235e4a4fe50713085fc4a6899e738a298",
    "Debian source delta SHA256: 9ee24622dfd907c8f1283328632156d43d23013c7ce62b04ea8ed6991a1ae65c",
)
REQUIRED = frozenset((*LIBRARIES, *NOTICE_HASHES,
                      "share/licenses/pipewire/SOURCE.txt", "share/pipewire/client.conf"))
SYSTEM_LIBRARIES = frozenset(("libc.so.6", "libm.so.6", "libdl.so.2",
                             "libpthread.so.0", "librt.so.1"))
TARGETS = {
    "x86_64-unknown-linux-gnu": {
        "arch": "x64", "machine": "Advanced Micro Devices X86-64",
        "loader": "ld-linux-x86-64.so.2", "glibc": "2.35",
    },
    "aarch64-unknown-linux-gnu": {
        "arch": "arm64", "machine": "AArch64",
        "loader": "ld-linux-aarch64.so.1", "glibc": "2.39",
    },
}
DEFAULT_TARGET = "x86_64-unknown-linux-gnu"


class AuditError(RuntimeError):
    """An incomplete or unsafe SDK must not produce a passing inventory."""


def target_policy(target: str) -> dict:
    if target not in TARGETS:
        raise AuditError(f"Unsupported private Linux SDK target: {target}")
    return TARGETS[target]


def version_tuple(value: str) -> tuple[int, ...]:
    if not re.fullmatch(r"\d+\.\d+(?:\.\d+)?", value):
        raise AuditError(f"Invalid numeric ABI version: {value}")
    parts = tuple(int(part) for part in value.split("."))
    return parts + (0,) * (3 - len(parts))


def canonical_directory(root: Path) -> None:
    if not root.is_absolute() or root.resolve(strict=True) != root:
        raise AuditError("SDK must be an absolute canonical directory without symlink parents")
    if not stat.S_ISDIR(root.lstat().st_mode):
        raise AuditError("SDK is not a directory")


def digest(path: Path) -> str:
    with path.open("rb") as stream:
        return _digest(stream)


def _digest(stream) -> str:
    result = hashlib.sha256()
    for block in iter(lambda: stream.read(1024 * 1024), b""):
        result.update(block)
    return result.hexdigest()


def runtime_files(root: Path) -> dict[str, Path]:
    canonical_directory(root)
    allowed_directories = {parent.as_posix() for relative in REQUIRED
                           for parent in Path(relative).parents if str(parent) != "."}
    files = {}
    for directory, directories, names in os.walk(root, followlinks=False):
        for name in (*directories, *names):
            path = Path(directory) / name
            mode = path.lstat().st_mode
            relative = path.relative_to(root).as_posix()
            if not (stat.S_ISDIR(mode) or stat.S_ISREG(mode)):
                raise AuditError(f"Symlink or special file is forbidden: {relative}")
            if stat.S_ISDIR(mode) and relative not in allowed_directories:
                raise AuditError(f"Unapproved runtime directory: {relative}")
            if stat.S_ISREG(mode):
                if path.stat().st_size == 0:
                    raise AuditError(f"Empty runtime file: {relative}")
                files[relative] = path
    missing, extra = REQUIRED - files.keys(), files.keys() - REQUIRED
    if missing:
        raise AuditError("Missing runtime components/notices: " + ", ".join(sorted(missing)))
    if extra:
        raise AuditError("Unapproved runtime files (including optional/GPL modules): " + ", ".join(sorted(extra)))
    return files


def readelf(path: Path, option: str) -> str:
    result = subprocess.run(["readelf", option, str(path)], capture_output=True,
                            text=True, timeout=15,
                            env=dict(os.environ, LC_ALL="C", LANG="C"))
    if result.returncode or result.stderr.strip():
        raise AuditError(f"Malformed ELF {path.name}: {result.stderr.strip()}")
    return result.stdout


def inspect_elf(path: Path, glibc_max: str, target: str = DEFAULT_TARGET) -> dict:
    policy = target_policy(target)
    with path.open("rb") as stream:
        if stream.read(4) != b"\x7fELF":
            raise AuditError(f"Not an ELF library: {path.name}")
    header = readelf(path, "--file-header")
    for field in (r"Class:\s+ELF64\b", r"Data:.*little endian",
                  rf"Machine:\s+{re.escape(policy['machine'])}\b", r"Type:\s+DYN\b"):
        if not re.search(field, header):
            raise AuditError(f"Expected Linux {policy['arch']} shared ELF: {path.name}")
    dynamic = readelf(path, "--dynamic")
    needed = re.findall(r"\(NEEDED\).*\[([^\]]+)\]", dynamic)
    for library in needed:
        if library not in SYSTEM_LIBRARIES | {policy["loader"], "libpipewire-0.3.so.0"}:
            raise AuditError(f"Unapproved dynamic dependency {library} in {path.name}")
    if re.search(r"\((?:RPATH|RUNPATH)\)", dynamic):
        raise AuditError(f"Build/ambient search path retained in {path.name}")
    sonames = re.findall(r"\(SONAME\).*\[([^\]]+)\]", dynamic)
    if path.name == "libpipewire-0.3.so.0" and sonames != [path.name]:
        raise AuditError("Private PipeWire SONAME mismatch")
    versions = readelf(path, "--version-info")
    required = sorted(set(re.findall(r"Name:\s+GLIBC_([^\s]+)", versions)))
    for version in required:
        if version_tuple(version) > version_tuple(glibc_max):
            raise AuditError(f"GLIBC_{version} exceeds baseline {glibc_max}: {path.name}")
    return {"needed": needed, "sonames": sonames,
            "glibcRequirements": required,
            "glibcMaxRequired": max(required, key=version_tuple) if required else None}


def audit(root: Path, glibc_max: str = None, target: str = DEFAULT_TARGET) -> dict:
    policy = target_policy(target)
    glibc_max = policy["glibc"] if glibc_max is None else glibc_max
    version_tuple(glibc_max)
    if version_tuple(glibc_max) > version_tuple(policy["glibc"]):
        raise AuditError(f"Requested ABI baseline exceeds {target} policy {policy['glibc']}")
    files = runtime_files(root)
    hashes = {relative: digest(path) for relative, path in files.items()}
    for relative, expected in NOTICE_HASHES.items():
        if hashes[relative] != expected:
            raise AuditError(f"Pinned license/security patch content changed: {relative}")
    source = files["share/licenses/pipewire/SOURCE.txt"].read_text(encoding="utf-8")
    if source.splitlines() != list(SOURCE_LINES):
        raise AuditError("Source provenance notice does not match pinned release")
    inventory = []
    for relative, path in sorted(files.items()):
        entry = {"path": relative, "bytes": path.stat().st_size, "sha256": hashes[relative]}
        if relative in LIBRARIES:
            entry["elf"] = inspect_elf(path, glibc_max, target)
        inventory.append(entry)
    return {"schemaVersion": 1, "scope": f"private-linux-{policy['arch']}-client-sdk",
            "target": target,
            "pipewireSource": "1.6.9+Debian-1.6.9-2-security-patch", "libeiStaticVersion": "1.5.0",
            "glibcBaseline": glibc_max, "runtimeRoot": str(root), "files": inventory,
            "installedAppAcceptance": False, "portalAcceptance": False,
            "captureInputAcceptance": False}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("runtime", type=Path)
    parser.add_argument("--target", choices=TARGETS, default=DEFAULT_TARGET)
    parser.add_argument("--glibc-max")
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    try:
        canonical_directory(args.output.parent)
        if args.output.exists() or args.output.is_symlink():
            raise AuditError("Inventory output must be new; refusing overwrite")
        report = audit(args.runtime, args.glibc_max, args.target)
        with args.output.open("x", encoding="utf-8") as stream:
            json.dump(report, stream, indent=2)
            stream.write("\n")
        print(f"Private SDK inventory passed: {len(report['files'])} files; App/portal not accepted")
        return 0
    except (AuditError, OSError, ValueError, subprocess.SubprocessError) as error:
        print(f"Private SDK audit failed: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
