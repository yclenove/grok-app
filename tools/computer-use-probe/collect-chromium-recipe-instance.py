"""Read-only CIPD identity/tags for the recipe instance in four sealed builds.

Never resolve a mutable branch/tag, download or execute a package, register
instances, follow redirects, use credentials or infer binary license approval.
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
CHECKOUTS = runpy.run_path(str(HERE / "inspect-chromium-archive-checkouts.py"))
INPUTS = CHECKOUTS["INPUTS"]
EXTERNAL = INPUTS["EXTERNAL"]
CHECKOUT_SHA = "e1349b8e7d96d23ac15c0d3d64085dabb7dc801a83bf36d27f242454b460b5ae"
SERVER = "https://chrome-infra-packages.appspot.com"
PACKAGE = "infra/recipe_bundles/chromium.googlesource.com/chromium/tools/build"
VERSION = "2QjLPDj2oNMKlnOBPW4LhuQHkh6Mc52_OS_yRNlo_RwC"
MAX_RESPONSE = 256 * 1024


def object_ref():
    raw = base64.urlsafe_b64decode(VERSION + "=" * (-len(VERSION) % 4))
    if len(raw) != 33 or raw[-1] != 2:
        raise ValueError("compiled SHA256 CIPD instance ID required")
    return {"hashAlgo": "SHA256", "hexDigest": raw[:-1].hex()}


def requests():
    prefix = SERVER + "/prpc/cipd.Repository/"
    return (
        {"key": "resolve", "url": prefix + "ResolveVersion",
         "body": {"package": PACKAGE, "version": VERSION}},
        {"key": "describe", "url": prefix + "DescribeInstance",
         "body": {"package": PACKAGE, "instance": object_ref(), "describeTags": True,
                  "describeMetadata": True}},
    )


def checked_instance(instance):
    if (not isinstance(instance, dict) or instance.get("package") != PACKAGE
            or instance.get("instance") != object_ref()):
        raise ValueError("returned package/digest differs from original immutable recipe instance")


def parse_response(key, contents):
    if not 0 < len(contents) <= MAX_RESPONSE:
        raise ValueError("bounded nonempty public CIPD JSON required")
    result = CHECKOUTS["LOGS"]["decoded_record"](contents)
    if key == "resolve":
        checked_instance(result)
    elif key == "describe":
        if not isinstance(result, dict):
            raise ValueError("public CIPD description must be an object")
        checked_instance(result.get("instance"))
        for field in ("tags", "metadata"):
            rows = result.get(field, [])
            if not isinstance(rows, list) or len(rows) > 1024 or any(not isinstance(row, dict) for row in rows):
                raise ValueError("bounded public CIPD tags/metadata required")
    else:
        raise ValueError("only compiled recipe RPC keys allowed")
    return result


def checked_inputs(source):
    INPUTS["checked_source"](source)
    report, sha = INPUTS["receipt"](source / "archive-checkouts-01.json", CHECKOUT_SHA)
    if CHECKOUTS["inspect"](source) != report:
        raise ValueError("original four sealed archive/bootstrap inputs must reproduce")
    expected_targets = list(CHECKOUTS["BUILDS"]["ARCHIVES"])
    if [row.get("target") for row in report.get("targets", [])] != expected_targets:
        raise ValueError("all four exact original targets required")
    for row in report["targets"]:
        bootstrap = row["bootstrap"]
        cipd = bootstrap["recipeExe"]["cipd"]
        if (bootstrap["recipe"] != "chromium" or bootstrap["recipeExe"].get("cmd") != ["luciexe"]
                or (cipd.get("server"), cipd.get("package"), cipd.get("actual_version")) != (
                    SERVER, PACKAGE, VERSION)):
            raise ValueError("original exact resolved recipe identity required in all four builds")
    return sha


def fetch(request):
    if request not in requests():
        raise ValueError("only two compiled read-only recipe-instance RPC requests allowed")
    http = urllib.request.Request(request["url"], method="POST",
                                  data=json.dumps(request["body"], separators=(",", ":")).encode(),
                                  headers={"Accept": "application/json", "Content-Type": "application/json"})
    opener = urllib.request.build_opener(CHECKOUTS["LOGS"]["BUILDS"]["SEARCH"]["NoRedirect"]())
    with opener.open(http, timeout=20) as response:
        if response.geturl() != request["url"]:
            raise ValueError("recipe RPC redirect refused")
        content_type = response.headers.get("Content-Type", "")
        contents = response.read(MAX_RESPONSE + 1)
    if content_type.split(";", 1)[0].strip().lower() != "application/json":
        raise ValueError("public CIPD JSON content type required")
    return contents


def collect(source, output):
    if output.parent != source or not re.fullmatch(r"recipe-instance-[0-9]{2}", output.name):
        raise ValueError("safe fresh task-owned recipe evidence path required")
    if output.exists() or output.is_symlink():
        raise FileExistsError("never overwrite recipe evidence")
    sha = checked_inputs(source)
    output.mkdir()
    report = {"scope": "original immutable recipe CIPD identity/tags only, not executed helper/source coverage",
              "checkoutReceiptSha256": sha, "package": PACKAGE, "instanceVersion": VERSION,
              "instanceObject": object_ref(), "requests": list(requests()), "responses": [],
              "collectionComplete": False, "packageDownloaded": False, "sourceFunctionsExecuted": False,
              "actualGclientHelperRevisionVerified": False, "licenseCoverage": "not_proven", "promoted": False}
    try:
        for request in requests():
            contents = fetch(request)
            name = request["key"] + ".public-data"
            with (output / name).open("xb") as stream:
                stream.write(contents)
            row = {"key": request["key"], "evidenceFile": name, "bytes": len(contents),
                   "sha256": hashlib.sha256(contents).hexdigest(), "verified": False}
            report["responses"].append(row)
            result = parse_response(request["key"], contents)
            row["verified"] = True
            if request["key"] == "describe":
                report["tags"] = result.get("tags", [])
                report["metadata"] = result.get("metadata", [])
            print(json.dumps({"key": request["key"], "bytes": len(contents), "verified": True}), flush=True)
        report["collectionComplete"] = True
    except Exception as error:
        report["failureType"] = type(error).__name__
        raise
    finally:
        EXTERNAL["write_report"](output / "recipe-instance-inventory.json", report)
    return report


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--output-name", required=True)
    args = parser.parse_args()
    source = args.source.absolute()
    result = collect(source, source / args.output_name)
    print(json.dumps({"collectionComplete": result["collectionComplete"],
                      "tags": result.get("tags", []), "promoted": False}))
