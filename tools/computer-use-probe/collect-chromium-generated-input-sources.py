"""Collect eight exact Crashpad/node build-input files as inert source data.

The allowlist and Git blob IDs independently match the sealed fixed main tree.
No hooks, package managers, upstream Python/GN or generated artifacts run.
Source mechanism collection alone does not resolve any original license error.
"""
import argparse
import base64
import hashlib
import json
from pathlib import Path
import re
import runpy
import urllib.request


HERE = Path(__file__).parent.absolute()
CONFIG = runpy.run_path(str(HERE / "collect-chromium-builder-config.py"))
INPUTS, EXTERNAL = CONFIG["INPUTS"], CONFIG["EXTERNAL"]
COMMIT = INPUTS["TREE"]["COMMIT"]
PINS = {
    "third_party/crashpad/crashpad/build/crashpad_buildconfig.gni": "80992a1b992bd3f3049cac2b598c974d468de832",
    "third_party/crashpad/crashpad/third_party/lss/BUILD.gn": "027c3a22f67fe879854a262379a6f34e5f541aae",
    "third_party/crashpad/crashpad/third_party/zlib/BUILD.gn": "a75623d9f8e17fb99baf98f81d4a0989eb8d1149",
    "third_party/crashpad/update.py": "46135a96fd8c931cea75ad2940fb9e1d4ea2ac2a",
    "third_party/node/BUILD.gn": "1741e14401c8c83b95f3c39e401b251a47df35e9",
    "third_party/node/node_modules.py": "435a0c8a1f0474096a8d00c72d6c5ef81de02386",
    "third_party/node/package-lock.json": "34a4702d312e9c2bb8fd674731abc427cca7dcd0",
    "third_party/node/package.json": "f1cd91faf26b2180ea09a30c60b4a28e47f4be42",
}
MAX_RAW, MAX_SOURCE, MAX_TOTAL = 4 * 1024**2, 2 * 1024**2, 8 * 1024**2


def requests():
    return [{"path": path, "gitObjectSha1": oid,
             "url": "https://chromium.googlesource.com/chromium/src/+/" + COMMIT + "/" + path + "?format=TEXT"}
            for path, oid in PINS.items()]


def checked_inputs(source):
    inputs = INPUTS["load_inputs"](source)
    for path, oid in PINS.items():
        row = inputs["index"].get(path)
        if row is None or row["mode"] not in ("100644", "100755") or row["kind"] != "blob" or row["gitObjectSha1"] != oid:
            raise ValueError("independent sealed main tree must match every exact allowlisted source blob")
    return inputs["mainSha256"]


def fetch(request):
    if request not in requests():
        raise ValueError("only eight compiled fixed-commit Gitiles URLs allowed")
    http = urllib.request.Request(request["url"], method="GET", headers={"Accept": "text/plain"})
    no_redirect = CONFIG["CHECKOUTS"]["LOGS"]["BUILDS"]["SEARCH"]["NoRedirect"]()
    with urllib.request.build_opener(no_redirect).open(http, timeout=20) as response:
        if response.geturl() != request["url"]:
            raise ValueError("generated input source redirect refused")
        if response.headers.get("Content-Type", "").split(";", 1)[0].strip().lower() != "text/plain":
            raise ValueError("Gitiles TEXT source response required")
        contents = response.read(MAX_RAW + 1)
    if not 0 < len(contents) <= MAX_RAW:
        raise ValueError("bounded nonempty raw source response required")
    return contents


def decode(contents, oid):
    if not 0 < len(contents) <= MAX_RAW:
        raise ValueError("bounded raw base64 source required")
    decoded = base64.b64decode(contents, validate=True)
    if not 0 < len(decoded) <= MAX_SOURCE:
        raise ValueError("bounded nonempty decoded source required")
    identity = INPUTS["TREE"]["check_blob"](oid, decoded)
    decoded.decode("utf-8", errors="strict")
    return decoded, identity


def collect(source, output):
    if output.parent != source or not re.fullmatch(r"generated-input-source-[0-9]{2}", output.name):
        raise ValueError("safe fresh generated input source directory required")
    if output.exists() or output.is_symlink():
        raise FileExistsError("never overwrite generated input source evidence")
    main_sha = checked_inputs(source)
    output.mkdir()
    report = {"scope": "exact Crashpad/node build-input source only; not generated checkout, NOTICE or shipping decision",
              "sourceCommit": COMMIT, "mainReceiptSha256": main_sha, "requests": requests(), "records": [],
              "collectionComplete": False, "sourceFunctionsExecuted": False, "conditionsEvaluated": False,
              "generatedInputsReproduced": False, "originalLicenseErrorsResolved": False,
              "licenseCoverage": "not_proven", "promoted": False}
    total = 0
    try:
        for request in requests():
            raw = fetch(request)
            name = request["gitObjectSha1"] + ".gitiles-data"
            with (output / name).open("xb") as stream:
                stream.write(raw)
            row = {**request, "rawFile": name, "rawBytes": len(raw),
                   "rawSha256": hashlib.sha256(raw).hexdigest(), "verified": False}
            report["records"].append(row)
            decoded, identity = decode(raw, request["gitObjectSha1"])
            total += len(decoded)
            if total > MAX_TOTAL:
                raise ValueError("total selected source byte budget exceeded")
            name = request["gitObjectSha1"] + ".source-data"
            with (output / name).open("xb") as stream:
                stream.write(decoded)
            row.update(sourceFile=name, verified=True, **identity)
            print(json.dumps({"path": row["path"], "bytes": row["bytes"], "verified": True}), flush=True)
        report["collectionComplete"] = True
    except Exception as error:
        report["failureType"] = type(error).__name__
        raise
    finally:
        EXTERNAL["write_report"](output / "generated-input-inventory.json", report)
    return report


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--output-name", required=True)
    args = parser.parse_args()
    source = args.source.absolute()
    report = collect(source, source / args.output_name)
    print(json.dumps({"collectionComplete": report["collectionComplete"], "sources": len(report["records"]),
                      "sourceBytes": sum(row.get("bytes", 0) for row in report["records"]), "promoted": False}))
