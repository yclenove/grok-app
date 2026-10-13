"""Read-only inventory of explicit Playwright 1.64 CfT source archives.

Does not extract/run software, change product locks, or certify publishers.
Fingerprints inform the subsequent production materializer/repair migration.
"""
import collections
import hashlib
import json
import plistlib
import runpy
import stat
import sys
import zipfile
from pathlib import Path, PurePosixPath


SOURCES = {
    "chrome-win64": "chrome.exe",
    "chrome-linux64": "chrome",
    "chrome-linux-arm64": "chrome",
    "chrome-mac-arm64": "Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing",
    "chrome-mac-x64": "Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing",
}


def digest_stream(stream):
    digest = hashlib.sha256()
    size = 0
    for chunk in iter(lambda: stream.read(1024 * 1024), b""):
        digest.update(chunk)
        size += len(chunk)
    return digest.hexdigest(), size


def inspect_tree(path, root, executable):
    if path.is_symlink() or not path.is_file():
        raise ValueError("regular source archive required")
    with path.open("rb") as stream:
        archive_digest, archive_size = digest_stream(stream)
    rows, links, names, kinds = {}, {}, set(), collections.Counter()
    longest = largest = 0
    macos_headers = None
    with zipfile.ZipFile(path) as archive:
        if sum(item.file_size for item in archive.infolist()) > 2 * 1024**3:
            raise ValueError("source inspection budget exceeded")
        for item in archive.infolist():
            name = item.filename.rstrip("/")
            parts = name.split("/")
            if "\\" in name or any(p in ("", ".", "..") for p in parts):
                raise ValueError(f"ambiguous source path: {name}")
            if parts[0] != root or name.lower() in names:
                raise ValueError(f"duplicate or unexpected archive root: {name}")
            if item.flag_bits & 1 or item.compress_type not in (0, 8):
                raise ValueError("encrypted/unsupported source ZIP entry")
            names.add(name.lower())
            longest = max(longest, len(name))
            largest = max(largest, item.file_size)
            kind = stat.S_IFMT(item.external_attr >> 16)
            if item.is_dir():
                if kind not in (0, stat.S_IFDIR) or item.file_size != 0:
                    raise ValueError(f"directory mode/payload mismatch: {name}")
                kinds["directory"] += 1
                continue
            relative = str(PurePosixPath(name).relative_to(root))
            if kind not in (0, stat.S_IFREG, stat.S_IFLNK):
                raise ValueError(f"special source entry: {name}")
            if kind == stat.S_IFLNK:
                kinds["symlink"] += 1
                if item.file_size > 4096:
                    raise ValueError("oversized source link")
                target = archive.read(item).decode("utf-8")
                if PurePosixPath(target).is_absolute() or "\x00" in target or "\\" in target:
                    raise ValueError(f"invalid source link: {name}")
                links[relative] = target
                digest, size = hashlib.sha256(target.encode()).hexdigest(), len(target.encode())
            else:
                kinds["regular"] += 1
                with archive.open(item) as stream:
                    digest, size = digest_stream(stream)
            if size != item.file_size:
                raise ValueError(f"source size mismatch: {name}")
            rows[relative] = (size, digest)
        native_header = runpy.run_path(str(Path(__file__).with_name(
            "inspect-maintained-node-sources.py")))["native_header"]
        headers = {}
        if executable in links or executable not in rows:
            raise ValueError("browser executable must be a regular source file")
        if root.startswith("chrome-mac-"):
            base = f"{root}/Google Chrome for Testing.app/Contents/"
            info = plistlib.loads(archive.read(base + "Info.plist"))
            paths = [executable,
                     "Google Chrome for Testing.app/Contents/Frameworks/"
                     "Google Chrome for Testing Framework.framework/Versions/156.0.8078.4/"
                     "Google Chrome for Testing Framework"]
            expected_arch = "macho-arm64" if root.endswith("-arm64") else "macho-x86_64"
            for binary_path in paths:
                with archive.open(f"{root}/{binary_path}") as stream:
                    headers[binary_path] = native_header(stream.read(65536))
                if headers[binary_path]["formatArchitecture"] != expected_arch:
                    raise ValueError("browser native architecture does not match source target")
            macos_headers = {"plistMinimumSystemVersion": info.get("LSMinimumSystemVersion"),
                             "nativeHeaderRequirements": headers,
                             "scope": "binary metadata only, not OS runtime acceptance"}
        elif root in SOURCES:
            expected_arch = {"chrome-win64": "pe-x86_64", "chrome-linux64": "elf-x86_64",
                             "chrome-linux-arm64": "elf-arm64"}[root]
            with archive.open(f"{root}/{executable}") as stream:
                headers[executable] = native_header(stream.read(65536))
            if headers[executable]["formatArchitecture"] != expected_arch:
                raise ValueError("browser native architecture does not match source target")
    listing = "".join(f"{name}\t{size}\t{digest}\n" for name, (size, digest) in sorted(rows.items()))
    tree_size = sum(row[0] for row in rows.values())
    return {
        "archive": path.name, "archiveRoot": root,
        "archiveSha256": archive_digest, "archiveBytes": archive_size,
        "treeSha256": hashlib.sha256(listing.encode()).hexdigest(),
        "treeFiles": len(rows), "treeBytes": tree_size,
        "archiveEntries": len(names), "entryTypes": dict(kinds),
        "maxPathChars": longest, "largestEntryBytes": largest,
        "compressionRatio": round(tree_size / max(1, archive_size), 6),
        "macosMinimumSystemMetadata": macos_headers,
        "nativeHeaderRequirements": headers,
        "withinCurrent768MiBQuota": tree_size <= 768 * 1024**2,
        "executableRelpath": executable,
        "executableSha256": rows[executable][1], "executableBytes": rows[executable][0],
        "sourceLinksNotYetApprovedForProduct": links,
        "licenseCandidates": [name for name in sorted(rows) if any(
            marker in name.lower() for marker in ("credits.html", "license", "copying", "notice"))],
    }


def inspect(path, root, executable):
    if SOURCES.get(root) != executable:
        raise ValueError("browser source root/executable does not match a fixed target")
    return inspect_tree(path, root, executable)


if __name__ == "__main__":
    directory = Path(sys.argv[1]).resolve(strict=True)
    print(json.dumps({
        "scope": "read-only source inventory, not product prepare/runtime/install acceptance",
        "playwrightSource": "playwright-core@1.64.0 browsers.json and coreBundle.js registry",
        "browserVersion": "156.0.8078.4",
        "archives": [inspect(directory / f"{root}.zip", root, executable)
                     for root, executable in SOURCES.items()],
    }, indent=2))
