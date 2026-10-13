"""Read-only inventory for the fixed Playwright 1.64.0 maintenance candidate.

Checks archive SHA/npm SRI, every bounded entry, metadata and complete notices.
Never extracts/executes packages or changes the product's compiled pins/locks.
"""
import base64
import hashlib
import json
import runpy
import tarfile
from pathlib import Path

VERSION = "1.64.0"
SHA256 = "90c8b6b19da0a1b1e9a004cd6ff3e49e82668a0856e01efcc0e2d9c2b4466bb6"
ARCHIVE_BYTES = 3171930
MAX_FILES, MAX_BYTES, MAX_FILE_BYTES, MAX_PATH = 2000, 32 * 1024**2, 8 * 1024**2, 240
NOTICES = ("LICENSE", "NOTICE", "ThirdPartyNotices.txt")


def inspect(directory):
    regular_source = runpy.run_path(str(Path(__file__).with_name(
        "inspect-maintained-node-sources.py")))["regular_source"]
    path = directory / f"playwright-core-{VERSION}.tgz"
    metadata_path = directory / f"playwright-core-{VERSION}.metadata.json"
    regular_source(path)
    regular_source(metadata_path)
    raw = path.read_bytes()
    if len(raw) != ARCHIVE_BYTES or hashlib.sha256(raw).hexdigest() != SHA256:
        raise ValueError("maintenance Playwright archive hash/size mismatch")
    metadata_raw = metadata_path.read_bytes()
    metadata = json.loads(metadata_raw)
    sri = "sha512-" + base64.b64encode(hashlib.sha512(raw).digest()).decode("ascii")
    if (metadata.get("name") != "playwright-core" or metadata.get("version") != VERSION
            or metadata.get("dist", {}).get("integrity") != sri):
        raise ValueError("npm package identity/SRI mismatch")
    rows, selected, seen, total = {}, {}, set(), 0
    with tarfile.open(path, "r|gz") as archive:
        for entry in archive:
            name = entry.name.rstrip("/")
            parts = name.split("/")
            if ("\\" in name or ":" in name or "\0" in name or len(name) > MAX_PATH
                    or any(part in ("", ".", "..") for part in parts) or parts[0] != "package"):
                raise ValueError("unsafe/unexpected Playwright archive path")
            if name.lower() in seen or len(seen) >= MAX_FILES:
                raise ValueError("duplicate/excessive Playwright archive entries")
            seen.add(name.lower())
            if entry.isdir():
                if entry.size != 0:
                    raise ValueError("directory payload is not allowed")
                continue
            if not entry.isfile() or len(parts) < 2:
                raise ValueError("links/special/root files are not allowed")
            total += entry.size
            if entry.size < 0 or entry.size > MAX_FILE_BYTES or total > MAX_BYTES:
                raise ValueError("Playwright entry/total byte budget exceeded")
            with archive.extractfile(entry) as stream:
                content = stream.read(entry.size + 1)
            if len(content) != entry.size:
                raise ValueError("Playwright entry size mismatch")
            relative = "/".join(parts[1:])
            rows[relative] = (len(content), hashlib.sha256(content).hexdigest())
            if relative in (*NOTICES, "package.json", "browsers.json"):
                selected[relative] = content
    if total / max(1, len(raw)) > 20:
        raise ValueError("Playwright compression ratio exceeds product quota")
    if metadata["dist"]["fileCount"] != len(rows) or metadata["dist"]["unpackedSize"] != total:
        raise ValueError("npm full tree count/size mismatch")
    package = json.loads(selected["package.json"])
    if (package.get("name") != "playwright-core" or package.get("version") != VERSION
            or package.get("engines", {}).get("node") != ">=20"):
        raise ValueError("actual package metadata/engine mismatch")
    if any(not selected.get(name) for name in NOTICES):
        raise ValueError("complete package notices are missing")
    if b"Apache License" not in selected["LICENSE"]:
        raise ValueError("Playwright LICENSE lacks Apache grant")
    listing = "".join(f"{name}\t{size}\t{digest}\n" for name, (size, digest) in sorted(rows.items()))
    return {
        "scope": "source inventory only; not browser/native OS/runtime acceptance",
        "version": VERSION, "archiveSha256": SHA256, "archiveBytes": len(raw),
        "metadataSha256": hashlib.sha256(metadata_raw).hexdigest(), "npmSriMatched": True,
        "treeSha256": hashlib.sha256(listing.encode()).hexdigest(),
        "treeFiles": len(rows), "treeBytes": total, "nodeEngine": package["engines"]["node"],
        "notices": {name: {"bytes": rows[name][0], "sha256": rows[name][1]} for name in NOTICES},
        "browsers": json.loads(selected["browsers.json"])["browsers"],
    }


if __name__ == "__main__":
    import sys
    print(json.dumps(inspect(Path(sys.argv[1]).resolve(strict=True)), indent=2))
