"""Read four exact bootstrap config files at the recorded immutable commit.

Bounded Gitiles GET only; independently match each decoded Git blob to the
sealed Chromium tree. No git lazy fetch, source execution or product changes.
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
RECIPE = runpy.run_path(str(HERE / "collect-chromium-recipe-instance.py"))
CHECKOUTS = RECIPE["CHECKOUTS"]
INPUTS = RECIPE["INPUTS"]
EXTERNAL = RECIPE["EXTERNAL"]
REVISION = "82064764761481b897d400961ee71601b5d796e0"
MAX_RAW = 256 * 1024
MAX_SOURCE = 128 * 1024


def requests():
    rows = []
    for target, (builder, _, _, _) in CHECKOUTS["BUILDS"]["ARCHIVES"].items():
        path = "infra/config/generated/builders/ci/" + builder + "/properties.json"
        rows.append({"target": target, "path": path,
                     "url": "https://chromium.googlesource.com/chromium/src/+/" + REVISION + "/" + path + "?format=TEXT"})
    return rows


def checked_inputs(source):
    RECIPE["checked_inputs"](source)
    report, _ = INPUTS["receipt"](source / "archive-checkouts-01.json", RECIPE["CHECKOUT_SHA"])
    inputs = INPUTS["load_inputs"](source)
    expected, bootstraps = {}, {}
    for row, request in zip(report["targets"], requests()):
        config = row["bootstrap"]["configSource"]
        if config.get("path") != request["path"] or config.get("last_changed_commit", {}).get("id") != REVISION:
            raise ValueError("original exact target/config path/last-changed commit required")
        entry = inputs["index"].get(request["path"])
        if entry is None or entry.get("mode") != "100644" or entry.get("kind") != "blob":
            raise ValueError("original main tree must contain exact regular config blob")
        expected[request["target"]] = entry["gitObjectSha1"]
        data = INPUTS["read_data"](source / "archive-checkout-logs-03" / (request["target"] + "-bootstrap.log-data"),
                                   CHECKOUTS["LOGS"]["MAX_LOG_BYTES"])
        bootstraps[request["target"]] = json.loads(data)
    return expected, bootstraps


def fetch(request):
    if request not in requests():
        raise ValueError("only four compiled config Gitiles URLs allowed")
    http = urllib.request.Request(request["url"], method="GET", headers={"Accept": "text/plain"})
    with urllib.request.build_opener(CHECKOUTS["LOGS"]["BUILDS"]["SEARCH"]["NoRedirect"]()).open(http, timeout=20) as response:
        if response.geturl() != request["url"]:
            raise ValueError("builder config redirect refused")
        if response.headers.get("Content-Type", "").split(";", 1)[0].strip().lower() != "text/plain":
            raise ValueError("Gitiles TEXT response required")
        contents = response.read(MAX_RAW + 1)
    if not 0 < len(contents) <= MAX_RAW:
        raise ValueError("bounded nonempty raw config response required")
    return contents


def decoded_config(contents, oid):
    if not 0 < len(contents) <= MAX_RAW:
        raise ValueError("bounded raw base64 config required")
    decoded = base64.b64decode(contents, validate=True)
    if not 0 < len(decoded) <= MAX_SOURCE:
        raise ValueError("bounded decoded config required")
    identity = INPUTS["TREE"]["check_blob"](oid, decoded)
    result = json.loads(decoded)
    if not isinstance(result, dict) or not result or len(result) > 256:
        raise ValueError("nonempty bounded original config object required")
    return decoded, result, identity


def compare_bootstrap(config, bootstrap):
    def equal(left, right):
        return json.dumps(left, sort_keys=True, separators=(",", ":")) == json.dumps(right, sort_keys=True, separators=(",", ":"))
    return {"exactValueMatches": [key for key, value in config.items() if key in bootstrap and equal(bootstrap[key], value)],
            "keysAbsentFromBootstrap": [key for key in config if key not in bootstrap],
            "valueDifferences": [key for key, value in config.items() if key in bootstrap and not equal(bootstrap[key], value)],
            "configExecuted": False, "overrideSemanticsEvaluated": False}


def collect(source, output):
    if output.parent != source or not re.fullmatch(r"builder-config-source-[0-9]{2}", output.name):
        raise ValueError("safe fresh task-owned builder config source path required")
    if output.exists() or output.is_symlink():
        raise FileExistsError("never overwrite builder config evidence")
    expected, bootstraps = checked_inputs(source)
    output.mkdir()
    report = {"scope": "exact original bootstrap config source and final-value comparison, not full recipe/GN execution",
              "configCommit": REVISION, "mainTreeCommit": INPUTS["TREE"]["COMMIT"],
              "checkoutReceiptSha256": RECIPE["CHECKOUT_SHA"], "requests": requests(), "records": [],
              "collectionComplete": False, "sourceFunctionsExecuted": False,
              "conditionsEvaluated": False, "licenseCoverage": "not_proven", "promoted": False}
    try:
        for request in requests():
            contents = fetch(request)
            name = request["target"] + ".gitiles-data"
            with (output / name).open("xb") as stream:
                stream.write(contents)
            row = {**request, "rawFile": name, "rawBytes": len(contents),
                   "rawSha256": hashlib.sha256(contents).hexdigest(), "verified": False}
            report["records"].append(row)
            decoded, config, identity = decoded_config(contents, expected[request["target"]])
            name = request["target"] + ".source-data"
            with (output / name).open("xb") as stream:
                stream.write(decoded)
            row.update({"sourceFile": name, "gitObjectSha1": expected[request["target"]], **identity,
                        "bootstrapComparison": compare_bootstrap(config, bootstraps[request["target"]]), "verified": True})
            print(json.dumps({"target": request["target"], "bytes": len(decoded),
                              "bootstrapComparison": row["bootstrapComparison"]}), flush=True)
        report["collectionComplete"] = True
    except Exception as error:
        report["failureType"] = type(error).__name__
        raise
    finally:
        EXTERNAL["write_report"](output / "builder-config-inventory.json", report)
    return report


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--output-name", required=True)
    args = parser.parse_args()
    source = args.source.absolute()
    result = collect(source, source / args.output_name)
    print(json.dumps({"collectionComplete": result["collectionComplete"], "configs": len(result["records"]), "promoted": False}))
