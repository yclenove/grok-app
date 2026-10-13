"""Reverify six sealed recursive sources and recompute raw/default-policy gaps.

Do not drop absent dependencies, evaluate conditions, generate final NOTICE or
infer binary shipping. Original 57-source receipts remain unchanged.
"""
import argparse
import hashlib
import json
from pathlib import Path
import re
import runpy


HERE = Path(__file__).parent.absolute()
RECURSIVE = runpy.run_path(str(HERE / "collect-chromium-checked-recursive-sources.py"))
INPUTS = RECURSIVE["INPUTS"]
EXTERNAL = INPUTS["EXTERNAL"]
POLICY = runpy.run_path(str(HERE / "inspect-chromium-license-policy.py"))
ROOT_SHA = "ad3f368823ba0f4432cfe3e0f0311bc47bc876ef92463501e295ba7b3420836a"
SEALS = {
    "angle-glmark2": "ae781bb6b58c5e40b51ecfddd59d4069d69e31e86f470df6be26f48f03983551",
    "angle-rapidjson": "b763fe145b695701ae9f90e1bf684b8e2d85e974683b7db51c4b8d08525abaef",
    "angle-vk-gl-cts": "f151fcbb72e0991c4741272603be7e5bed5e9e2913232a161c50a9bf211adc07",
    "openscreen-buildtools": "d576aa12bfb10daef2d4515c383c30e7e7d2cb12d814a474fbefb73fdbd149dc",
    "openscreen-tinycbor": "ae38aadce6393e66a7f1247e6453340282914d62f48e52f4573da972942628a4",
    "openscreen-simple-dns": "53963757240a46618cf19dcd8231e321a71488a5dadd6ac2b4f99965528415fd",
}


def checked_root(report):
    if (report.get("sourceCommit") != INPUTS["TREE"]["COMMIT"]
            or report.get("checkoutReportSha256") != RECURSIVE["CHECKOUT_REPORT_SHA"]
            or report.get("requestedComponents") != list(RECURSIVE["PINS"])
            or report.get("collectionComplete") is not True
            or any(report.get(key) is not False for key in (
                "sourceFunctionsExecuted", "conditionsEvaluated", "shippingDecisionsMade", "generatorExecuted", "promoted"))
            or not isinstance(report.get("components"), list)
            or [row.get("component") for row in report["components"]] != list(SEALS)
            or any(row.get("receiptSha256") != SEALS[row["component"]] for row in report["components"])):
        raise ValueError("complete original six-source root receipt and independent seals required")


def append_sources(source, inputs):
    root = source / "checked-recursive-license-01"
    report, root_sha = INPUTS["receipt"](root / "checked-recursive-source-inventory.json", ROOT_SHA)
    checked_root(report)
    components = []
    for key, (prefix, repository, revision) in RECURSIVE["PINS"].items():
        directory = root / key
        component, sha = INPUTS["receipt"](directory / "license-source-inventory.json", SEALS[key])
        if ((component.get("checkoutPath"), component.get("repository"), component.get("revision")) != (
                prefix, repository, revision) or component.get("collectionComplete") is not True
                or component.get("generatorExecuted") is not False or component.get("promoted") is not False):
            raise ValueError("exact completed recursive source identity required")
        contents = INPUTS["read_data"](directory / "source-tree.nul", INPUTS["TREE"]["MAX_TREE"])
        if (len(contents), hashlib.sha256(contents).hexdigest()) != (component["treeBytes"], component["treeSha256"]):
            raise ValueError("original recursive source tree changed")
        rows = INPUTS["TREE"]["tree_records"](contents)
        if len(rows) != component["treeEntries"]:
            raise ValueError("original recursive source tree count changed")
        INPUTS["merge_tree"](inputs["index"], rows, prefix)
        INPUTS["merge_files"](inputs["index"], component["selectedFiles"], directory, prefix,
                              inputs["files"], inputs["references"])
        components.append({"component": key, "checkoutPath": prefix, "repository": repository,
                           "revision": revision, "receiptSha256": sha, "sourceLevel": "recursive"})
    return components, root_sha


def summarize(inputs):
    references = [{**row, "resolution": INPUTS["classify"](row, inputs["index"], inputs["files"])}
                  for row in inputs["references"]]
    missing = [row for row in references if row["resolution"] != "resolved_to_verified_source_bytes"]
    payloads = {path: row["payload"] for path, row in inputs["files"].items()}
    policy = POLICY["parse_policy"](payloads["tools/licenses/licenses.py"])
    scan = POLICY["scan"](POLICY["SourceView"](inputs["index"], payloads, policy["VCS_METADATA_DIRS"]), policy)
    shipping_errors = [row for row in scan["entries"] if row["shippedMetadata"] == policy["YES"] and row["errors"]]
    unknown = [row for row in scan["entries"] if row["shippedMetadata"] not in (policy["YES"], policy["NO"])]
    return {"verifiedSelectedFiles": len(inputs["files"]),
            "verifiedSourceBytesByPath": sum(row["bytes"] for row in inputs["files"].values()),
            "rawLicenseReferences": len(references), "unresolvedRawReferences": missing,
            "resolvedRawReferences": len(references) - len(missing),
            "candidateDirectories": len(scan["candidateDirectories"]), "metadataEntries": len(scan["entries"]),
            "shippingMetadataEntriesWithErrors": shipping_errors, "nonstandardShippingValues": unknown,
            "directoryErrors": scan["directoryErrors"]}


def inspect(source, output):
    if output.parent != source or not re.fullmatch(r"checked-recursive-license-audit-[0-9]{2}\.json", output.name):
        raise ValueError("safe fresh checked-recursive audit path required")
    if output.exists() or output.is_symlink():
        raise FileExistsError("never overwrite checked-recursive license audit")
    inputs = INPUTS["load_inputs"](source, True, True, True, True)
    before = summarize(inputs)
    components, root_sha = append_sources(source, inputs)
    after = summarize(inputs)
    closed = [row for row in before["unresolvedRawReferences"]
              if INPUTS["classify"](row, inputs["index"], inputs["files"]) == "resolved_to_verified_source_bytes"]
    report = {"scope": "63 selected fixed sources and raw/default-policy gaps only, not shipping count/NOTICE approval",
              "sourceCommit": INPUTS["TREE"]["COMMIT"], "recursiveReceiptSha256": root_sha,
              "mainReceiptSha256": inputs["mainSha256"], "parentComponents": inputs["components"],
              "recursiveComponents": components, "parentSourceCount": len(inputs["components"]),
              "recursiveSourceCount": len(components), "before": before, "after": after,
              "closedOriginalRawReferences": closed, "originalAbsentSourcesExcluded": False,
              "sourceFunctionsExecuted": False, "conditionsEvaluated": False, "shippingDecisionsMade": False,
              "generatorExecuted": False, "allRecursiveSourcesCollected": False,
              "binaryComponentCoverage": "not_proven", "licenseCoverage": "not_proven", "promoted": False}
    EXTERNAL["write_report"](output, report)
    return report


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--output-name", required=True)
    args = parser.parse_args()
    source = args.source.absolute()
    result = inspect(source, source / args.output_name)
    print(json.dumps({"sources": result["parentSourceCount"] + result["recursiveSourceCount"],
                      "paths": result["after"]["verifiedSelectedFiles"],
                      "rawGapsBefore": len(result["before"]["unresolvedRawReferences"]),
                      "rawGapsAfter": len(result["after"]["unresolvedRawReferences"]),
                      "closedOriginalRawReferences": len(result["closedOriginalRawReferences"]),
                      "shippingMetadataErrors": len(result["after"]["shippingMetadataEntriesWithErrors"]),
                      "directoryErrors": len(result["after"]["directoryErrors"]),
                      "nonstandardValues": len(result["after"]["nonstandardShippingValues"]), "promoted": False}))
