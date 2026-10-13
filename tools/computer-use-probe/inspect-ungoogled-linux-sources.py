"""Read a pinned Linux Chromium proposal: all bytes, architecture and Credits.

No extraction/execution, ABI acceptance, model calls or product lock changes.
"""
import argparse
import hashlib
import json
import runpy
import sys
from pathlib import Path

from bounded_tar_xz import iter_tar_xz
from chromium_data_pack import MAX_PACK_BYTES, MAX_RESOURCE_BYTES
from chromium_data_pack import credits_document, read_pack, unpack_resource


HERE = Path(__file__).parent
NATIVE = runpy.run_path(str(HERE / "inspect-maintained-node-sources.py"))
CREDITS = runpy.run_path(str(HERE / "inspect-chromium-credits.py"))
SHA256 = "acc30f037e0080a0bf03456f8cf8cda4aca55a778b17174d0a4aaa8e7749cb79"
ARCHIVE_BYTES = 156893644
ROOT = "ungoogled-chromium-155.0.8059.39-1-x86_64_linux"


def inspect_directory(directory):
    path = (directory / "Linux_x64.tar.xz").absolute()
    NATIVE["regular_source"](path)
    if path.stat().st_size != ARCHIVE_BYTES:
        raise ValueError("fixed Linux proposal size mismatch")
    with path.open("rb") as source:
        digest, _, _ = NATIVE["digest_stream"](source)
    if digest != SHA256:
        raise ValueError("fixed Linux proposal SHA256 mismatch")
    rows, headers, packs, candidates, payloads, seen = {}, {}, [], [], [], set()
    packed_bytes = decoded_bytes = largest = longest = directories = 0
    with path.open("rb") as source:
        for entry, stream in iter_tar_xz(source):
            name = entry.name.rstrip("/")
            parts = name.split("/")
            if (len(name) > 1024 or parts[0] != ROOT or "\\" in name or ":" in name
                    or "\0" in name or any(part in ("", ".", "..") for part in parts)
                    or name.casefold() in seen):
                raise ValueError("ambiguous/duplicate Linux source path")
            seen.add(name.casefold())
            longest, largest = max(longest, len(name)), max(largest, entry.size)
            if entry.isdir():
                directories += 1
                continue
            if len(parts) < 2:
                raise ValueError("Linux archive root must be a directory")
            relative = "/".join(parts[1:])
            is_pack = relative.endswith(".pak")
            if is_pack:
                packed_bytes += entry.size
                if not 0 < entry.size <= MAX_PACK_BYTES or packed_bytes > CREDITS["MAX_PACK_TOTAL_BYTES"]:
                    raise ValueError("Linux resource pack budget exceeded")
            file_digest, prefix, contents, actual = hashlib.sha256(), bytearray(), bytearray(), 0
            while True:
                chunk = stream.read(1024 * 1024)
                if not chunk:
                    break
                actual += len(chunk)
                file_digest.update(chunk)
                prefix.extend(chunk[:max(0, 65536 - len(prefix))])
                if is_pack:
                    contents.extend(chunk)
            if actual != entry.size:
                raise ValueError("Linux source declared/actual size mismatch")
            rows[relative] = (actual, file_digest.hexdigest())
            if prefix.startswith(b"\x7fELF"):
                headers[relative] = NATIVE["native_header"](prefix)
                if headers[relative]["formatArchitecture"] != "elf-x86_64":
                    raise ValueError("Linux source native architecture mismatch")
            if not is_pack:
                continue
            pack = read_pack(contents)
            packs.append({"path": name, "bytes": actual, "sha256": file_digest.hexdigest(),
                          "version": pack.version, "encoding": pack.encoding,
                          "resources": len(pack.resources), "aliases": len(pack.aliases)})
            for resource_id, resource in pack.resources.items():
                remaining = CREDITS["MAX_DECODED_BYTES"] - decoded_bytes
                decoded, compression = unpack_resource(resource, min(MAX_RESOURCE_BYTES, remaining))
                decoded_bytes += len(decoded)
                document = credits_document(decoded, pack.encoding)
                if document is None:
                    continue
                text, kind = document
                candidates.append({"pack": name, "resourceId": resource_id,
                                   "compression": compression, "encoding": pack.encoding,
                                   "packedBytes": len(resource), "decodedBytes": len(decoded),
                                   "sha256": hashlib.sha256(decoded).hexdigest(),
                                   "copyrightMentions": text.casefold().count("copyright"),
                                   "licenseMentions": text.casefold().count("license"),
                                   "documentKind": kind,
                                   "scope": "credits candidate, license coverage not certified"})
                payloads.append(decoded)
    if "chrome" not in headers or not candidates:
        raise ValueError("Linux executable or Credits candidate is missing")
    listing = "".join(f"{name}\t{size}\t{sha}\n" for name, (size, sha) in sorted(rows.items()))
    row = {"platform": "Linux_x64", "archiveSha256": SHA256, "archiveBytes": ARCHIVE_BYTES,
           "treeSha256": hashlib.sha256(listing.encode()).hexdigest(), "treeFiles": len(rows),
           "treeBytes": sum(item[0] for item in rows.values()), "archiveEntries": len(seen),
           "directories": directories, "largestEntryBytes": largest, "maxPathChars": longest,
           "nativeHeaders": headers, "packs": packs, "creditsCandidates": candidates,
           "packBytesInspected": packed_bytes, "decodedResourceBytes": decoded_bytes,
           "licenseCandidates": [name for name in sorted(rows) if any(
               marker in name.lower() for marker in ("license", "notice", "credits", "copying"))]}
    report = {"scope": "pinned Linux source inventory only, not runtime or redistribution approval",
              "publisher": "ungoogled-software/ungoogled-chromium-portablelinux",
              "release": "155.0.8059.39-1", "browserVersion": "155.0.8059.39",
              "sourcePackagingCommit": "025d99db1673a21992534551598b73ca79360762",
              "archives": [row], "nativeAcceptance": "not_run", "glibcAcceptance": "not_run",
              "redistributionGate": "not_proven_exact_source_and_license_review_required",
              "executed": False, "promoted": False}
    report["embeddedFullNoticesGate"] = ("candidate_only_not_certified" if any(
        candidate["documentKind"] == "full_candidate" for candidate in candidates)
                                        else "failed_missing_full_notices")
    return report, [payloads]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", type=Path)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    report, payloads = inspect_directory(args.directory)
    if args.output:
        CREDITS["save_evidence"](args.output, report, payloads)
    for row in report["archives"]:
        row["packsInspected"] = len(row.pop("packs"))
    print(json.dumps(report, indent=2))
    return 0 if report["embeddedFullNoticesGate"] == "candidate_only_not_certified" else 2


if __name__ == "__main__":
    sys.exit(main())
