"""Match four already-downloaded ZIPs to pinned public GCS object generations.

Read-only bounded metadata GETs and streaming SHA256/MD5/CRC32C. No ZIP
download/extraction/execution, credentials, product or shared-site changes.
Path/time correspondence is not a cryptographic publisher/build attestation.
"""
import argparse
import base64
from datetime import datetime
import hashlib
from importlib import metadata as package_metadata
import json
import os
from pathlib import Path
import re
import runpy
import sys
import urllib.request
from urllib.parse import quote


HERE = Path(__file__).parent.absolute()
LOGS = runpy.run_path(str(HERE / "collect-chromium-archive-logs.py"))
BUILDS = LOGS["BUILDS"]
INPUTS = LOGS["INPUTS"]
SOURCES = HERE.parent.parent / ".cache/computer-use/platform-source-20261012-chromium-snapshot"
SOURCE_METADATA_SHA = "06e20000ac0d1a3ded11478afeae8a7a69e4e18979018e455c6bc830e8ad18a2"
ZIP_INVENTORY_SHA = "3bafdb84f281b55dfca707500c6bfae285f6ab59fa7a08407619ced55c25e28b"
GENERATIONS = {"windows-x64": "1791806836355024", "linux-x64": "1791802844815218",
               "mac-intel": "1791802460664545", "mac-arm64": "1791801893976688"}
MAX_METADATA = 64 * 1024
MAX_ZIP = 512 * 1024**2
CHUNK = 1024**2


def object_name(target):
    _, _, platform, filename = BUILDS["archive"](target)
    return platform + "/1716663/" + filename


def object_url(target):
    name = object_name(target)
    return ("https://storage.googleapis.com/storage/v1/b/chromium-browser-snapshots/o/"
            + quote(name, safe="") + "?generation=" + GENERATIONS[target])


def b64_digest(value, length):
    if not isinstance(value, str) or len(value) > 128:
        raise ValueError("bounded base64 checksum required")
    result = base64.b64decode(value, validate=True)
    if len(result) != length or base64.b64encode(result).decode() != value:
        raise ValueError("canonical checksum length required")
    return result


def utc_time(value):
    if not isinstance(value, str):
        raise ValueError("original UTC timestamp string required")
    match = re.fullmatch(r"([0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2})(?:\.([0-9]{1,9}))?Z", value)
    if match is None:
        raise ValueError("exact bounded UTC timestamp required")
    # Python 3.8 only accepts 3/6 fractional digits. Retain raw strings in
    # reports; use explicitly microsecond precision for time correspondence.
    fractional = (match[2] or "").ljust(6, "0")[:6]
    return datetime.fromisoformat(match[1] + "." + fractional + "+00:00")


def object_metadata(target, contents):
    if not 0 < len(contents) <= MAX_METADATA:
        raise ValueError("bounded nonempty GCS metadata required")
    value = json.loads(contents)
    if (not isinstance(value, dict) or value.get("kind") != "storage#object"
            or value.get("bucket") != "chromium-browser-snapshots" or value.get("name") != object_name(target)
            or value.get("generation") != GENERATIONS[target] or not isinstance(value.get("size"), str)
            or not re.fullmatch(r"[0-9]{1,12}", value["size"]) or not 0 < int(value["size"]) <= MAX_ZIP):
        raise ValueError("exact bounded same-target GCS object generation required")
    b64_digest(value.get("md5Hash"), 16)
    b64_digest(value.get("crc32c"), 4)
    for key in ("timeCreated", "updated"):
        utc_time(value.get(key))
    return value


def fetch_metadata(target):
    url = object_url(target)
    request = urllib.request.Request(url, headers={"Accept": "application/json"}, method="GET")
    with urllib.request.build_opener(BUILDS["SEARCH"]["NoRedirect"]()).open(request, timeout=20) as response:
        if response.geturl() != url or response.headers.get("Content-Type", "").split(";", 1)[0] != "application/json":
            raise ValueError("exact public JSON metadata endpoint required")
        contents = response.read(MAX_METADATA + 1)
    if not 0 < len(contents) <= MAX_METADATA:
        raise ValueError("bounded nonempty GCS response required")
    return contents


def original_sources(source):
    INPUTS["checked_source"](source)
    INPUTS["TREE"]["regular_parents"](SOURCES)
    if not SOURCES.is_dir() or SOURCES.resolve(strict=True) != SOURCES:
        raise ValueError("canonical original four-ZIP source directory required")
    report, sha = INPUTS["receipt"](SOURCES / "source-metadata.json", SOURCE_METADATA_SHA)
    inventory_path = HERE / ".run/runtime-contract-20261013-01/unbranded-source-inventory.json"
    inventory, inventory_sha = INPUTS["receipt"](inventory_path, ZIP_INVENTORY_SHA)
    for value in (report, inventory):
        if value.get("sourceCommit") != INPUTS["TREE"]["COMMIT"] or value.get("sourcePosition") != "1716663":
            raise ValueError("original sealed source commit and position required")
    rows, oracle = report.get("archives"), inventory.get("archives")
    if not isinstance(rows, list) or not isinstance(oracle, list) or len(rows) != 4 or len(oracle) != 4:
        raise ValueError("four complete original archive identities required")
    sources = {}
    for target in GENERATIONS:
        _, _, platform, _ = BUILDS["archive"](target)
        picked = [row for row in rows if row.get("platform") == platform]
        observed = [row for row in oracle if row.get("platform") == platform]
        if len(picked) != 1 or len(observed) != 1:
            raise ValueError("unique original per-target ZIP metadata required")
        metadata, audited = picked[0]["archive"], observed[0]
        if (picked[0].get("sourceCommit") != INPUTS["TREE"]["COMMIT"] or picked[0].get("sourcePosition") != "1716663"
                or metadata.get("name") != object_name(target) or metadata.get("generation") != GENERATIONS[target]
                or audited.get("officialObjectGeneration") != GENERATIONS[target]
                or audited.get("archive") != platform + ".zip"
                or not re.fullmatch(r"[0-9a-f]{64}", audited.get("archiveSha256", ""))
                or int(metadata["size"]) != audited.get("archiveBytes")):
            raise ValueError("original exact ZIP generation/size/SHA receipt required")
        sources[target] = {"path": SOURCES / (platform + ".zip"), "metadata": metadata,
                           "sha256": audited["archiveSha256"]}
    return sources, sha, inventory_sha


def load_crc32c(source):
    # Hash-pinned cp312 wheel was installed only into this fresh task-owned SDK.
    site = source / "crc32c-sdk-01"
    INPUTS["TREE"]["regular_parents"](site)
    if not site.is_dir() or site.resolve(strict=True) != site:
        raise ValueError("original private checksum SDK required")
    sys.path.insert(0, str(site))
    import google_crc32c
    if (package_metadata.version("google-crc32c") != "1.7.1" or google_crc32c.implementation != "c"
            or not Path(google_crc32c.__file__).absolute().is_relative_to(site)):
        raise ValueError("fixed private native checksum SDK required")
    for contents, expected in ((b"", "00000000"), (b"123456789", "e3069283"), (b"abc", "364b3fb7")):
        checksum = google_crc32c.Checksum()
        checksum.update(contents[:1])
        checksum.update(contents[1:])
        if checksum.digest().hex() != expected:
            raise ValueError("native CRC32C known-vector/chunk verification failed")
    return google_crc32c.Checksum


def file_identity(info):
    return info.st_dev, info.st_ino, info.st_size, info.st_mtime_ns, info.st_ctime_ns


def verify_zip(path, original, current, checksum_factory):
    INPUTS["TREE"]["regular_parents"](path)
    if not path.is_file() or not 0 < path.stat().st_size <= MAX_ZIP:
        raise ValueError("bounded original regular ZIP required")
    if (current["generation"], current["size"], current["md5Hash"]) != (
            original["metadata"]["generation"], original["metadata"]["size"], original["metadata"]["md5Hash"]):
        raise ValueError("original and fresh pinned-generation metadata disagree")
    before = file_identity(path.stat())
    sha, md5, crc, size = hashlib.sha256(), hashlib.md5(), checksum_factory(), 0
    with path.open("rb") as stream:
        handle_before = file_identity(os.fstat(stream.fileno()))
        # CPython/Windows path-stat and fstat can expose different ctime
        # semantics. Compare common file identity/content metadata across APIs;
        # retain each API's entire identity (including ctime) within that API.
        if handle_before[:4] != before[:4]:
            raise ValueError("original ZIP handle identity changed")
        while True:
            chunk = stream.read(CHUNK)
            if not chunk:
                break
            size += len(chunk)
            if size > MAX_ZIP or size > int(current["size"]):
                raise ValueError("ZIP grew beyond exact original metadata")
            sha.update(chunk)
            md5.update(chunk)
            crc.update(chunk)
        if file_identity(os.fstat(stream.fileno())) != handle_before:
            raise ValueError("ZIP changed while checksum was read")
    if file_identity(path.stat()) != before:
        raise ValueError("original ZIP path identity changed")
    md5_value, crc_value = base64.b64encode(md5.digest()).decode(), base64.b64encode(crc.digest()).decode()
    if (size != int(current["size"]) or sha.hexdigest() != original["sha256"]
            or md5_value != current["md5Hash"] or crc_value != current["crc32c"]):
        raise ValueError("exact original SHA256 and public MD5/CRC32C must all match")
    return {"path": str(path), "bytes": size, "sha256": sha.hexdigest(), "md5Base64": md5_value,
            "crc32cBase64": crc_value, "originalSha256Matched": True, "objectMd5Matched": True,
            "objectCrc32cMatched": True, "gcsGenerationBytesMatched": True}


def upload_times(target, build, metadata):
    _, _, platform, filename = BUILDS["archive"](target)
    name = "Generic Archiving Steps|gsutil upload " + platform + "/1716663/" + filename
    steps = [step for step in build["steps"] if step["name"] == name]
    if len(steps) != 1 or steps[0].get("status") != "SUCCESS":
        raise ValueError("exact original successful upload step required")
    step = steps[0]
    times = [step.get("startTime"), step.get("endTime"), metadata["timeCreated"]]
    if not all(isinstance(value, str) and value.endswith("Z") for value in times):
        raise ValueError("exact original upload and object UTC timestamps required")
    start, end, created = [utc_time(value) for value in times]
    if start > end:
        raise ValueError("ordered original upload step times required")
    return {"uploadStartTime": times[0], "uploadEndTime": times[1], "objectTimeCreated": times[2],
            "createdWithinUploadStep": start <= created <= end,
            "createdAfterUploadEndMs": round((created - end).total_seconds() * 1000, 3),
            "timestampComparisonPrecision": "microseconds; original strings retained",
            "timestampDifferenceIgnored": False, "publisherGenerationCryptographicallyBound": False,
            "metadataBuildIdentityRecorded": bool(metadata.get("metadata")), "promoted": False}


def collect(source, output, checksum_factory=None):
    if output.parent != source or not re.fullmatch(r"snapshot-object-records-[0-9]{2}", output.name):
        raise ValueError("safe fresh task-owned snapshot object directory required")
    if output.exists() or output.is_symlink():
        raise FileExistsError("never overwrite snapshot object evidence")
    builds, build_sha = LOGS["checked_builds"](source)
    sources, source_sha, inventory_sha = original_sources(source)
    factory = checksum_factory or load_crc32c(source)
    output.mkdir()
    report = {"scope": "pinned GCS object bytes/checksums and original upload timestamps, not build attestation or license approval",
              "sourceCommit": INPUTS["TREE"]["COMMIT"], "archiveReceiptSha256": build_sha,
              "sourceMetadataSha256": source_sha, "originalZipInventorySha256": inventory_sha,
              "records": [], "collectionComplete": False, "zipExecuted": False, "promoted": False}
    try:
        for target in GENERATIONS:
            contents = fetch_metadata(target)
            name = target + ".gcs-data"
            with (output / name).open("xb") as stream:
                stream.write(contents)
            row = {"target": target, "url": object_url(target), "evidenceFile": name,
                   "bytes": len(contents), "sha256": hashlib.sha256(contents).hexdigest(), "verified": False}
            report["records"].append(row)
            current = object_metadata(target, contents)
            row["object"] = {key: current[key] for key in ("name", "generation", "size", "md5Hash", "crc32c", "timeCreated", "updated")}
            row["zip"] = verify_zip(sources[target]["path"], sources[target], current, factory)
            row["upload"] = upload_times(target, builds[target], current)
            row["verified"] = True
            print(json.dumps({"target": target, "generationBytesMatched": True,
                              "createdWithinUploadStep": row["upload"]["createdWithinUploadStep"]}), flush=True)
        report["collectionComplete"] = True
    except Exception as error:
        report["failureType"] = type(error).__name__
        raise
    finally:
        INPUTS["EXTERNAL"]["write_report"](output / "snapshot-object-inventory.json", report)
    return report


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--output-name", required=True)
    args = parser.parse_args()
    source = args.source.absolute()
    result = collect(source, source / args.output_name)
    print(json.dumps({"objects": len(result["records"]), "collectionComplete": result["collectionComplete"], "promoted": False}))
