"""Read-only inventory of the four official Node maintenance source archives.

Checks the recorded official SHASUMS256, every archive entry, and native headers.
Never extracts or executes software; does not promote product runtime pins.
"""
import argparse
import collections
import hashlib
import json
import stat
import struct
import tarfile
import zipfile
from pathlib import Path


VERSION = "24.21.0"
TARGETS = {
    "win-x64": ("zip", "node.exe", "pe-x86_64"),
    "linux-x64": ("tar.gz", "bin/node", "elf-x86_64"),
    "darwin-arm64": ("tar.gz", "bin/node", "macho-arm64"),
    "darwin-x64": ("tar.gz", "bin/node", "macho-x86_64"),
}
MAX_ENTRIES = 10_000
MAX_BYTES = 512 * 1024**2
MAX_SELECTED_BYTES = 128 * 1024**2


def regular_source(path):
    for component in (path, *path.parents):
        meta = component.lstat()
        if stat.S_ISLNK(meta.st_mode) or getattr(meta, "st_file_attributes", 0) & 0x400:
            raise ValueError("source paths must not contain links/reparse points")
    if not path.is_file():
        raise ValueError("regular source file required")


def digest_stream(stream, prefix_bytes=65536):
    digest, size, prefix = hashlib.sha256(), 0, bytearray()
    for chunk in iter(lambda: stream.read(1024 * 1024), b""):
        digest.update(chunk)
        size += len(chunk)
        if len(prefix) < prefix_bytes:
            prefix.extend(chunk[:prefix_bytes - len(prefix)])
    return digest.hexdigest(), size, bytes(prefix)


def native_header(data):
    if data.startswith(b"MZ") and len(data) >= 64:
        offset = struct.unpack_from("<I", data, 60)[0]
        if offset + 26 > len(data) or data[offset:offset + 4] != b"PE\0\0":
            raise ValueError("invalid PE header")
        machine = struct.unpack_from("<H", data, offset + 4)[0]
        magic = struct.unpack_from("<H", data, offset + 24)[0]
        if machine != 0x8664 or magic != 0x20B:
            raise ValueError("unexpected Node PE architecture")
        return {"formatArchitecture": "pe-x86_64"}
    if data.startswith(b"\x7fELF") and len(data) >= 64:
        if data[4:6] != b"\x02\x01" or struct.unpack_from("<H", data, 18)[0] != 62:
            raise ValueError("unexpected Node ELF architecture")
        return {"formatArchitecture": "elf-x86_64"}
    if data.startswith(b"\xcf\xfa\xed\xfe") and len(data) >= 32:
        cpu, _, _, commands, command_bytes = struct.unpack_from("<IIIII", data, 4)
        architecture = {0x01000007: "x86_64", 0x0100000C: "arm64"}.get(cpu)
        if architecture is None or 32 + command_bytes > len(data):
            raise ValueError("unexpected/truncated Node Mach-O header")
        offset, minimums = 32, []
        for _ in range(commands):
            if offset + 8 > 32 + command_bytes:
                raise ValueError("truncated Mach-O load command")
            command, size = struct.unpack_from("<II", data, offset)
            if size < 8 or offset + size > 32 + command_bytes:
                raise ValueError("invalid Mach-O load command size")
            if command in (0x24, 0x32):  # LC_VERSION_MIN_MACOSX / LC_BUILD_VERSION
                if size < (24 if command == 0x32 else 16):
                    raise ValueError("truncated Mach-O deployment command")
                if command == 0x32 and struct.unpack_from("<I", data, offset + 8)[0] != 1:
                    raise ValueError("Node Mach-O is not a macOS binary")
                value = struct.unpack_from("<I", data, offset + (12 if command == 0x32 else 8))[0]
                minimums.append(f"{value >> 16}.{(value >> 8) & 255}.{value & 255}")
            offset += size
        if offset != 32 + command_bytes or len(minimums) != 1:
            raise ValueError("missing/ambiguous Mach-O deployment version")
        return {"formatArchitecture": f"macho-{architecture}",
                "headerMinimumMacOS": minimums[0]}
    raise ValueError("unrecognized Node native binary")


def inspect(path, root, executable, architecture, expected):
    regular_source(path)
    with path.open("rb") as stream:
        digest, archive_size, _ = digest_stream(stream)
    if digest != expected:
        raise ValueError(f"official SHASUMS256 mismatch: {path.name}")
    seen, selected, counts, total, longest = set(), {}, collections.Counter(), 0, 0

    def record(name, size, kind, opener):
        nonlocal total, longest
        name = name.rstrip("/")
        parts = name.split("/")
        if (len(name) > 1024 or "\\" in name or ":" in name or "\0" in name
                or any(part in ("", ".", "..") for part in parts) or parts[0] != root):
            raise ValueError(f"unsafe/unexpected Node source path: {name}")
        folded = name.lower()
        if folded in seen or len(seen) >= MAX_ENTRIES:
            raise ValueError("duplicate path or source entry budget exceeded")
        seen.add(folded)
        counts[kind] += 1
        longest = max(longest, len(name))
        total += size
        if total > MAX_BYTES or size < 0:
            raise ValueError("Node source byte budget exceeded")
        if kind == "directory" and size != 0:
            raise ValueError("directory has a payload")
        if name == root and kind != "directory":
            raise ValueError("Node archive root must be a directory")
        relative = "/".join(parts[1:])
        wanted = relative in (executable, "LICENSE")
        if wanted and (kind != "regular" or size == 0 or size > MAX_SELECTED_BYTES):
            raise ValueError("Node executable/license must be bounded nonempty regular files")
        if relative == "LICENSE" and size > 2 * 1024**2:
            raise ValueError("Node source license inspection budget exceeded")
        if kind == "regular":
            with opener() as stream:
                file_digest, actual, prefix = digest_stream(stream, size if relative == "LICENSE" else 65536)
            if actual != size:
                raise ValueError(f"Node source size mismatch: {name}")
            if wanted:
                selected[relative] = {"sha256": file_digest, "bytes": actual}
                if relative == executable:
                    selected[relative].update(native_header(prefix))
                else:
                    if b"Permission is hereby granted, free of charge" not in prefix:
                        raise ValueError("Node LICENSE lacks the expected MIT grant")
                    normalized = prefix.replace(b"\r\n", b"\n")
                    selected[relative].update({
                        "lfComparisonSha256": hashlib.sha256(normalized).hexdigest(),
                        "lfComparisonBytes": len(normalized),
                        "crlfLines": prefix.count(b"\r\n"),
                    })

    if path.name.endswith(".zip"):
        with zipfile.ZipFile(path) as archive:
            for entry in archive.infolist():
                if entry.flag_bits & 1 or entry.compress_type not in (0, 8):
                    raise ValueError("encrypted/unsupported Node ZIP entry")
                mode = stat.S_IFMT(entry.external_attr >> 16)
                if entry.is_dir():
                    kind = "directory"
                    if mode not in (0, stat.S_IFDIR):
                        raise ValueError("Node ZIP directory mode mismatch")
                elif mode in (0, stat.S_IFREG):
                    kind = "regular"
                else:
                    raise ValueError("Node ZIP contains a link/special file")
                record(entry.filename, entry.file_size, kind, lambda e=entry: archive.open(e))
    else:
        with tarfile.open(path, "r|gz") as archive:
            for entry in archive:
                if entry.isdir():
                    kind = "directory"
                elif entry.isfile():
                    kind = "regular"
                elif entry.issym():
                    kind = "unextracted-symlink"
                else:
                    raise ValueError("Node tar contains an unsupported special entry")
                record(entry.name, entry.size, kind, lambda e=entry: archive.extractfile(e))
    if set(selected) != {executable, "LICENSE"}:
        raise ValueError("Node archive must contain its executable and complete LICENSE")
    if selected[executable]["formatArchitecture"] != architecture:
        raise ValueError("Node architecture does not match target")
    return {"archive": path.name, "archiveRoot": root, "archiveSha256": digest,
            "archiveBytes": archive_size, "entries": len(seen), "entryTypes": dict(counts),
            "uncompressedBytes": total, "maxPathChars": longest,
            "compressionRatio": round(total / max(1, archive_size), 6),
            "executableRelpath": executable, "executable": selected[executable],
            "license": selected["LICENSE"]}


def inventory(directory, version=VERSION):
    if version not in ("22.23.3", "24.21.0"):
        raise ValueError("not an audited maintenance source version")
    sums_path = directory / f"node-v{version}-SHASUMS256.txt"
    regular_source(sums_path)
    sums = {}
    for line in sums_path.read_text(encoding="utf-8").splitlines():
        digest, name = line.split(maxsplit=1)
        if name in sums or len(digest) != 64 or any(c not in "0123456789abcdef" for c in digest):
            raise ValueError("invalid/duplicate official SHASUMS256 row")
        sums[name] = digest
    archives = []
    for target, (suffix, executable, architecture) in TARGETS.items():
        root = f"node-v{version}-{target}"
        name = f"{root}.{suffix}"
        archives.append(inspect(directory / name, root, executable, architecture, sums[name]))
    if len({row["license"]["lfComparisonSha256"] for row in archives}) != 1:
        raise ValueError("Node LICENSE content differs across targets beyond line endings")
    return {"scope": "source integrity/layout/header inventory; not runtime/install acceptance",
            "version": version, "officialSumsSha256": hashlib.sha256(sums_path.read_bytes()).hexdigest(),
            "archives": archives}


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", type=Path)
    parser.add_argument("--version", choices=("22.23.3", "24.21.0"), default=VERSION)
    args = parser.parse_args()
    print(json.dumps(inventory(args.directory.absolute(), args.version), indent=2))
