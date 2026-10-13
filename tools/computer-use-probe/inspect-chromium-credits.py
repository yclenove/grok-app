"""Audit embedded credits in four fixed unbranded Chromium source archives.

No browser execution, rendered HTML, product locks or seed publication.
--output optionally preserves decoded candidate bytes in a NEW .run directory.
Candidate detection and upstream BSD permission are not complete license QC.
"""
import argparse
import hashlib
import json
import re
import runpy
import stat
import sys
import zipfile
from pathlib import Path

from chromium_data_pack import MAX_PACK_BYTES, MAX_RESOURCE_BYTES
from chromium_data_pack import credits_document, read_pack, unpack_resource


HERE = Path(__file__).parent
SOURCE = runpy.run_path(str(HERE / "inspect-unbranded-chromium-sources.py"))
REGULAR_SOURCE = SOURCE["NATIVE"]["regular_source"]
SHA256 = {
    "Win_x64": "3bdd57de257651043b160c25ba4aa78a7e0f04738bf0d96a2cd4c2c23c79be2a",
    "Linux_x64": "e3d9fbfb1866494e3515f7885d67ccddebd88d1970f110a6809204fd190609dd",
    "Mac_Arm": "6698935471d74b6a6049d612b0793403b08650614dc1cce667f0cf1c200d2e0c",
    "Mac": "8a38979b82b1df71728deff4d94dcaecd333e036cd33f11ef78c74ae0311ee5a",
}
MAX_ARCHIVE_BYTES = 512 * 1024**2
MAX_PACK_TOTAL_BYTES = 128 * 1024**2
MAX_DECODED_BYTES = 512 * 1024**2
MAX_ARCHIVE_ENTRIES = 1024


def inspect_archive(path, platform):
    return inspect_pinned_archive(path, platform, SOURCE["SNAPSHOTS"][platform][0], SHA256[platform])


def inspect_pinned_archive(path, platform, root, expected_sha256):
    """Inspect an explicitly pinned proposal, never product runtime inputs.

    The snapshot CLI still supplies its original fixed roots and SHA256s.
    An empty root is only for publishers packaging files at the ZIP root.
    No paths from this archive are extracted or executed.
    """
    if not re.fullmatch(r"[a-z][a-z0-9_]*", platform, re.I):
        raise ValueError("safe source platform label required")
    if not re.fullmatch(r"[0-9a-f]{64}", expected_sha256):
        raise ValueError("explicit fixed source SHA256 required")
    if root and (root in (".", "..") or any(value in root for value in ("/", "\\", "\x00", ":"))):
        raise ValueError("source archive root must be one safe segment or empty")
    path = path.absolute()
    REGULAR_SOURCE(path)
    if not 0 < path.stat().st_size <= MAX_ARCHIVE_BYTES:
        raise ValueError("source archive size exceeds inspection budget")
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    if digest.hexdigest() != expected_sha256:
        raise ValueError("fixed source archive SHA256 mismatch")
    packs, candidates, payloads, names = [], [], [], set()
    packed_bytes = decoded_bytes = 0
    with zipfile.ZipFile(path) as archive:
        if len(archive.infolist()) > MAX_ARCHIVE_ENTRIES:
            raise ValueError("source archive entry budget exceeded")
        for item in archive.infolist():
            name = item.filename.rstrip("/")
            parts = name.split("/")
            if (item.orig_filename != item.filename or "\\" in name or "\x00" in name or ":" in name
                    or (root and parts[0] != root) or name.casefold() in names
                    or any(part in ("", ".", "..") for part in parts)):
                raise ValueError("ambiguous/duplicate source path")
            names.add(name.casefold())
            if item.is_dir() or not name.endswith(".pak"):
                continue
            if (stat.S_IFMT(item.external_attr >> 16) not in (0, stat.S_IFREG)
                    or item.flag_bits & 1 or item.compress_type not in (0, 8)):
                raise ValueError("regular, unencrypted resource pack required")
            packed_bytes += item.file_size
            if not 0 < item.file_size <= MAX_PACK_BYTES or packed_bytes > MAX_PACK_TOTAL_BYTES:
                raise ValueError("resource pack inspection budget exceeded")
            with archive.open(item) as stream:
                contents = stream.read(item.file_size + 1)
            if len(contents) != item.file_size:
                raise ValueError("resource pack declared size mismatch")
            pack = read_pack(contents)
            row = {"path": name, "bytes": len(contents), "sha256": hashlib.sha256(contents).hexdigest(),
                   "version": pack.version, "encoding": pack.encoding,
                   "resources": len(pack.resources), "aliases": len(pack.aliases)}
            packs.append(row)
            for resource_id, resource in pack.resources.items():
                remaining = MAX_DECODED_BYTES - decoded_bytes
                decoded, compression = unpack_resource(resource, min(MAX_RESOURCE_BYTES, remaining))
                decoded_bytes += len(decoded)
                document = credits_document(decoded, pack.encoding)
                if document is None:
                    continue
                text, kind = document
                candidate = {"pack": name, "resourceId": resource_id,
                             "aliasIds": sorted(key for key, value in pack.aliases.items()
                                                if value == resource_id),
                             "compression": compression, "encoding": pack.encoding,
                             "packedBytes": len(resource), "decodedBytes": len(decoded),
                             "sha256": hashlib.sha256(decoded).hexdigest(),
                             "copyrightMentions": text.casefold().count("copyright"),
                             "licenseMentions": text.casefold().count("license"),
                             "documentKind": kind,
                             "scope": "credits HTML candidate, license completeness not certified"}
                candidates.append(candidate)
                payloads.append(decoded)
    if not candidates:
        raise ValueError(f"no embedded credits HTML candidate found for {platform}")
    return {"platform": platform, "archiveSha256": digest.hexdigest(),
            "packBytesInspected": packed_bytes, "decodedResourceBytes": decoded_bytes,
            "packs": packs, "creditsCandidates": candidates}, payloads


def save_evidence(output, report, payloads):
    # Output is task-local and exclusively new; never overwrite prior evidence.
    output = output.absolute()
    root = (HERE / ".run").absolute()
    try:
        output.relative_to(root)
    except ValueError as error:
        raise ValueError("evidence must be in a new task-owned .run subdirectory") from error
    if output == root:
        raise ValueError("evidence must be in a new task-owned .run subdirectory")
    for component in (output, *output.parents):
        if component.exists() or component.is_symlink():
            metadata = component.lstat()
            if stat.S_ISLNK(metadata.st_mode) or getattr(metadata, "st_file_attributes", 0) & 0x400:
                raise ValueError("evidence path must not contain links/reparse points")
    if output.exists() or any(part in (".", "..") for part in output.parts):
        raise ValueError("evidence directory must be new and unambiguous")
    if len(report["archives"]) != len(payloads):
        raise ValueError("archive evidence cardinality mismatch")
    if any(len(row["creditsCandidates"]) != len(contents)
           for row, contents in zip(report["archives"], payloads)):
        raise ValueError("resource evidence cardinality mismatch")
    output.mkdir()
    for row, contents in zip(report["archives"], payloads):
        for index, (candidate, data) in enumerate(zip(row["creditsCandidates"], contents)):
            name = f"{row['platform']}-resource-{candidate['resourceId']}-{index}.html"
            with (output / name).open("xb") as stream:
                stream.write(data)
            candidate["evidenceFile"] = name
    with (output / "credits-inventory.json").open("x", encoding="utf-8", newline="\n") as stream:
        json.dump(report, stream, indent=2)
        stream.write("\n")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", type=Path)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    results = [inspect_archive(args.directory / (platform + ".zip"), platform)
               for platform in SHA256]
    report = {"scope": "embedded credits source audit, not runtime or redistribution approval",
              "sourceCommit": SOURCE["COMMIT"], "sourcePosition": SOURCE["POSITION"],
              "browserVersion": SOURCE["VERSION"], "executed": False, "promoted": False,
              "archives": [result[0] for result in results]}
    report["embeddedFullNoticesGate"] = ("candidate_only_not_certified" if all(
        any(candidate["documentKind"] == "full_candidate" for candidate in row["creditsCandidates"])
        for row in report["archives"]) else "failed_missing_full_notices")
    if args.output:
        save_evidence(args.output, report, [result[1] for result in results])
    # Locale packs dominate the inventory; keep stdout compact without losing
    # their complete hashed rows in the optional on-disk evidence.
    for row in report["archives"]:
        row["packsInspected"] = len(row.pop("packs"))
    print(json.dumps(report, indent=2))
    return 0 if report["embeddedFullNoticesGate"] == "candidate_only_not_certified" else 2


if __name__ == "__main__":
    sys.exit(main())
