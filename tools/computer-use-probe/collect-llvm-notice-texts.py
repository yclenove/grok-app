"""Collect fixed SPDX NCSA and LLVM exception text for copied SwiftShader code.

These normative texts are not browser components or a complete NOTICE. The
original v3.28.0 tag/commit evidence and every exact raw response are retained.
"""
import argparse
import hashlib
import json
from pathlib import Path
import re
import runpy
import urllib.request


HERE = Path(__file__).parent.absolute()
SPDX = runpy.run_path(str(HERE / "collect-khronos-notice-texts.py"))
INPUTS = SPDX["INPUTS"]
LICENSES = {
    "NCSA": {"oid": "cf5413effa25a89cae2ef0de21ade82e3672cb3b", "bytes": 1700,
             "sha256": "b31bbfd06e221b39e1a877fb3f91b32fe9e88c87c1b46d11cd2bd5c822ec71e3"},
    "LLVM-exception": {"oid": "fa4b725a0ee50ef723ce5292865e54de7b0bfca4", "bytes": 919,
                       "sha256": "e34c58338bd89d43e709e226610d8f32b3e3c47f4ad9a99a8dc1d4ac7842488e"},
}


def requests():
    return [{"key": key, "path": "text/" + key + ".txt", "gitObjectSha1": pin["oid"],
             "url": SPDX["API"] + "/contents/text/" + key + ".txt?ref=" + SPDX["REVISION"]}
            for key, pin in LICENSES.items()]


def fetch(request):
    if request not in requests():
        raise ValueError("only compiled immutable LLVM/NCSA standard-text requests allowed")
    http = urllib.request.Request(request["url"], method="GET", headers={"Accept": "application/vnd.github+json", "Accept-Encoding": "identity"})
    no_redirect = SPDX["SOURCES"]["BASE"]["GENERATED"]["CONFIG"]["CHECKOUTS"]["LOGS"]["BUILDS"]["SEARCH"]["NoRedirect"]()
    with urllib.request.build_opener(no_redirect).open(http, timeout=20) as response:
        if (response.geturl() != request["url"] or response.headers.get("Content-Type", "").split(";", 1)[0] != "application/json"
                or response.headers.get("Content-Encoding") not in (None, "identity")):
            raise ValueError("exact undecoded public LLVM/NCSA standard-text JSON endpoint required")
        raw = response.read(SPDX["MAX_RAW"] + 1)
    if not 0 < len(raw) <= SPDX["MAX_RAW"]:
        raise ValueError("bounded nonempty original SPDX response required")
    return raw


def collect(source, output):
    INPUTS["checked_source"](source)
    if output.parent != source or not re.fullmatch(r"llvm-license-source-[0-9]{2}", output.name):
        raise ValueError("safe fresh task-owned LLVM standard-text output required")
    if output.exists() or output.is_symlink():
        raise FileExistsError("never overwrite LLVM standard-text source evidence")
    parent = SPDX["verify"](source, "khronos-license-source-01")
    output.mkdir()
    report = {"scope": "immutable normative NCSA/LLVM exception texts, not browser component or complete NOTICE approval",
        "browserSourceCommit": INPUTS["TREE"]["COMMIT"], "spdxRepository": SPDX["REPOSITORY"], "spdxRevision": SPDX["REVISION"],
        "releaseTag": SPDX["TAG"], "tagObjectSha1": SPDX["TAG_OBJECT"], "spdxTreeSha1": SPDX["TREE_OBJECT"],
        "originalSpdxIdentityEvidence": parent, "requests": requests(), "records": [], "selectedLicenseBytes": 0,
        "collectionComplete": False, "sourceFunctionsExecuted": False, "spdxDatasetIsBrowserComponent": False,
        "completeChromiumNotice": False, "promoted": False}
    try:
        for request in requests():
            raw = fetch(request)
            raw_name = request["key"] + ".json-data"
            with (output / raw_name).open("xb") as stream:
                stream.write(raw)
            row = {**request, "rawFile": raw_name, "rawBytes": len(raw), "rawSha256": hashlib.sha256(raw).hexdigest(), "verified": False}
            report["records"].append(row)
            data, identity = SPDX["decoded_license"](request["key"], raw, LICENSES)
            name = request["key"] + ".license-data"
            with (output / name).open("xb") as stream:
                stream.write(data)
            row.update(sourceFile=name, verified=True, **identity)
            report["selectedLicenseBytes"] += len(data)
        report["collectionComplete"] = True
    except Exception as error:
        report["failureType"] = type(error).__name__
        raise
    finally:
        INPUTS["EXTERNAL"]["write_report"](output / "llvm-license-inventory.json", report)
    return report


def verify(source, name, seal):
    INPUTS["checked_source"](source)
    if not re.fullmatch(r"llvm-license-source-[0-9]{2}", name) or not re.fullmatch(r"[0-9a-f]{64}", seal):
        raise ValueError("safe original output and fixed completed source seal required")
    root = source / name
    report, sha = INPUTS["receipt"](root / "llvm-license-inventory.json", seal)
    parent = SPDX["verify"](source, "khronos-license-source-01")
    identities = {"browserSourceCommit": INPUTS["TREE"]["COMMIT"], "spdxRepository": SPDX["REPOSITORY"], "spdxRevision": SPDX["REVISION"],
        "releaseTag": SPDX["TAG"], "tagObjectSha1": SPDX["TAG_OBJECT"], "spdxTreeSha1": SPDX["TREE_OBJECT"], "originalSpdxIdentityEvidence": parent}
    if (any(report.get(key) != value for key, value in identities.items()) or report.get("collectionComplete") is not True
            or report.get("requests") != requests() or len(report.get("records", [])) != len(requests())
            or any(report.get(key) is not False for key in ("sourceFunctionsExecuted", "spdxDatasetIsBrowserComponent", "completeChromiumNotice", "promoted"))):
        raise ValueError("exact completed original non-promoted SPDX exception receipt required")
    texts = {}
    for request, row in zip(requests(), report["records"]):
        if (any(row.get(key) != value for key, value in request.items()) or row.get("verified") is not True
                or row.get("rawFile") != request["key"] + ".json-data" or row.get("sourceFile") != request["key"] + ".license-data"):
            raise ValueError("exact original LLVM/NCSA standard-text records required")
        raw = INPUTS["read_data"](root / row["rawFile"], SPDX["MAX_RAW"])
        if (len(raw), hashlib.sha256(raw).hexdigest()) != (row.get("rawBytes"), row.get("rawSha256")):
            raise ValueError("original LLVM/NCSA standard-text raw bytes changed")
        data, identity = SPDX["decoded_license"](request["key"], raw, LICENSES)
        if (identity != {key: row.get(key) for key in ("bytes", "sha256")}
                or INPUTS["read_data"](root / row["sourceFile"], SPDX["MAX_TEXT"]) != data):
            raise ValueError("original complete LLVM/NCSA standard text changed")
        texts[request["key"]] = data
    if sum(map(len, texts.values())) != report.get("selectedLicenseBytes"):
        raise ValueError("exact complete standard-text byte total required")
    return texts, {"sourceReceiptSha256": sha, "standardTextBytes": sum(map(len, texts.values())), "spdxRevision": SPDX["REVISION"]}


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--output-name", required=True)
    args = parser.parse_args()
    source = args.source.absolute()
    report = collect(source, source / args.output_name)
    print(json.dumps({key: report[key] for key in ("selectedLicenseBytes", "collectionComplete", "completeChromiumNotice", "promoted")}))
