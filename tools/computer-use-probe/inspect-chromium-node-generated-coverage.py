"""Reverify exact generated node source, original build logs and license closure.

Archive input records keep their own kind; never fabricate Git objects/modes.
Original receipts stay immutable. Default policy errors are recomputed, not
excluded. Source/log correspondence is not remote bytes or binary attestation.
"""
import argparse
import hashlib
import json
from pathlib import Path
import re
import runpy


HERE = Path(__file__).parent.absolute()
NODE = runpy.run_path(str(HERE / "collect-chromium-node-generated-licenses.py"))
RECURSIVE = runpy.run_path(str(HERE / "inspect-chromium-checked-recursive-licenses.py"))
CHECKOUTS = runpy.run_path(str(HERE / "inspect-chromium-archive-checkouts.py"))
INPUTS, POLICY = RECURSIVE["INPUTS"], RECURSIVE["POLICY"]
ROOT_SHA = "96783deaa138a896b0d24d0fbc86d67834f6b85f4ff1775543616843d9640576"


class GeneratedSourceView(POLICY["SourceView"]):
    def regular(self, path):
        return (path in self.index and self.index[path]["mode"] == "archive_regular_file") or super().regular(path)


def resolution(reference, inputs):
    path = reference["sourcePath"]
    entry = inputs["index"].get(path)
    if entry and entry["mode"] == "archive_regular_file":
        return "resolved_to_verified_source_bytes" if path in inputs["files"] else "regular_archive_bytes_not_collected"
    return INPUTS["classify"](reference, inputs["index"], inputs["files"])


def summarize(inputs):
    payloads = {path: row["payload"] for path, row in inputs["files"].items()}
    policy = POLICY["parse_policy"](payloads["tools/licenses/licenses.py"])
    scan = POLICY["scan"](GeneratedSourceView(inputs["index"], payloads, policy["VCS_METADATA_DIRS"]), policy)
    refs = [{**row, "resolution": resolution(row, inputs)} for row in inputs["references"]]
    return {"verifiedSelectedFiles": len(inputs["files"]), "rawLicenseReferences": len(refs),
            "unresolvedRawReferences": [row for row in refs if row["resolution"] != "resolved_to_verified_source_bytes"],
            "resolvedRawReferences": sum(row["resolution"] == "resolved_to_verified_source_bytes" for row in refs),
            "candidateDirectories": len(scan["candidateDirectories"]), "metadataEntries": len(scan["entries"]),
            "shippingMetadataEntriesWithErrors": [row for row in scan["entries"] if row["shippedMetadata"] == policy["YES"] and row["errors"]],
            "nonstandardShippingValues": [row for row in scan["entries"] if row["shippedMetadata"] not in (policy["YES"], policy["NO"])],
            "directoryErrors": scan["directoryErrors"]}


def checked_node(source):
    refs, blobs, _, layout = NODE["checked_sources"](source)
    root = source / "node-generated-license-03"
    report, sha = INPUTS["receipt"](root / "node-generated-inventory.json", ROOT_SHA)
    if (report.get("collectionComplete") is not True or report.get("sourceCommit") != INPUTS["TREE"]["COMMIT"]
            or report.get("declaration") != NODE["DECLARATION"] or report.get("layoutEvidence") != layout
            or report.get("generatedSourceReceiptSha256") != NODE["SOURCE_SHA"]
            or report.get("parentCatalogSha256") != INPUTS["EXTERNAL"]["CATALOG_SHA"]
            or any(report.get(key) is not False for key in ("conditionsEvaluated", "sourceFunctionsExecuted",
                "packageManagerExecuted", "actualBuilderInputMeasured", "promoted"))):
        raise ValueError("complete sealed non-promoted original generated-node source required")
    raw, archive, identity = NODE["cached_archive"](source, "node-generated-license-01")
    metadata = report.get("objectMetadata", {})
    if ((metadata.get("bytes"), metadata.get("sha256"), metadata.get("value")) != (
            len(raw), hashlib.sha256(raw).hexdigest(), NODE["metadata"](raw))
            or report.get("archive") != {"file": archive.name, "sourceRelativeFile": archive.relative_to(source).as_posix(), **identity}):
        raise ValueError("original metadata/archive identities changed")
    actual = NODE["inspect_archive"](archive, root, refs, require_existing=True)
    comparison = NODE["package_comparison"](actual, root, blobs)
    if actual != report.get("inspection") or comparison != report.get("packageComparison"):
        raise ValueError("full original archive entries/selected paths/package metadata must reproduce")
    return root, report, sha


def append_node(inputs, root, report):
    inspection = report["inspection"]
    paths = set()
    for row in inspection["entries"]:
        path = "third_party/node/" + row["path"]
        if path in inputs["index"] or path in paths:
            raise ValueError("generated archive cannot overwrite Git source indexes")
        paths.add(path)
        inputs["index"][path] = {"path": path, "mode": "archive_regular_file" if row["kind"] == "file" else "archive_" + row["kind"],
                                 "kind": "generated_archive_input", "archiveSha256": NODE["OBJECT"]["sha256sum"],
                                 "payloadSha256": row.get("sha256"), "bytes": row["bytes"]}
    for row in inspection["selectedFiles"]:
        path = row["sourcePath"]
        entry = inputs["index"].get(path)
        if (entry is None or entry["mode"] != "archive_regular_file" or path in inputs["files"]
                or (entry["payloadSha256"], entry["bytes"]) != (row["sha256"], row["bytes"])):
            raise ValueError("unique selected regular generated source identity required")
        contents = INPUTS["read_data"](root / row["sourceFile"], NODE["MAX_SELECTED_FILE"])
        if (len(contents), hashlib.sha256(contents).hexdigest()) != (row["bytes"], row["sha256"]):
            raise ValueError("original selected generated source bytes changed")
        inputs["files"][path] = {"bytes": len(contents), "sha256": row["sha256"], "payload": contents,
                                "sourceKind": "verified_generated_archive_input"}


def checkout_section(contents):
    lines = contents.decode("utf-8").splitlines()
    prefix = "src/" + NODE["PREFIX"] + ":" + NODE["OBJECT"]["object_name"]
    matches = [number for number, line in enumerate(lines) if line.startswith(prefix + " (Elapsed: ")]
    if len(matches) != 1:
        raise ValueError("one exact same-object original checkout header required")
    start = matches[0]
    section = lines[start:start + 6]
    if (len(section) != 6 or section[1] != "-" * 40 or section[4] != "-" * 40
            or not re.fullmatch(r"\[[0-9:]+\] Started\.", section[2])
            or not re.fullmatch(r"\[[0-9:]+\] Finished\.", section[3]) or section[5] != ""):
        raise ValueError("exact completed original same-object checkout section required")
    return {"headerLine": start + 1, "lines": section[:5], "exactObjectCheckoutRecorded": True,
            "remoteArchiveBytesMeasured": False, "binaryCoverageProven": False}


def original_checkout_evidence(source):
    checkout = CHECKOUTS["inspect"](source)
    rows = []
    for target in checkout["targets"]:
        path = source / "archive-checkout-logs-03" / (target["target"] + "-checkout.log-data")
        contents = INPUTS["read_data"](path, CHECKOUTS["LOGS"]["MAX_LOG_BYTES"])
        rows.append({"target": target["target"], "buildId": target["buildId"],
                     "wholeCheckoutStdoutComplete": target["target"] != "windows-x64", **checkout_section(contents)})
    return rows, checkout["checkoutReceiptSha256"]


def inspect(source, output):
    if output.parent != source or not re.fullmatch(r"node-generated-coverage-[0-9]{2}\.json", output.name):
        raise ValueError("safe fresh generated-node coverage receipt required")
    if output.exists() or output.is_symlink():
        raise FileExistsError("never overwrite generated-node coverage evidence")
    inputs = INPUTS["load_inputs"](source, True, True, True, True)
    _, recursive_sha = RECURSIVE["append_sources"](source, inputs)
    before = summarize(inputs)
    root, node, node_sha = checked_node(source)
    checkout, checkout_sha = original_checkout_evidence(source)
    append_node(inputs, root, node)
    after = summarize(inputs)
    closed = [row for row in before["unresolvedRawReferences"] if resolution(row, inputs) == "resolved_to_verified_source_bytes"]
    remaining_errors = {(row["directory"], row["name"]) for row in after["shippingMetadataEntriesWithErrors"]}
    closed_errors = [row for row in before["shippingMetadataEntriesWithErrors"] if (row["directory"], row["name"]) not in remaining_errors]
    report = {"scope": "verified generated source/license closure and exact original checkout step, not full binary NOTICE approval",
              "sourceCommit": INPUTS["TREE"]["COMMIT"], "mainReceiptSha256": inputs["mainSha256"],
              "recursiveReceiptSha256": recursive_sha, "generatedNodeReceiptSha256": node_sha,
              "checkoutReceiptSha256": checkout_sha, "archiveIdentity": node["archive"],
              "layoutEvidence": node["layoutEvidence"], "packageComparison": node["packageComparison"],
              "originalCheckoutEvidence": checkout, "before": before, "after": after,
              "closedOriginalRawReferences": closed, "closedOriginalShippingMetadataErrors": closed_errors,
              "generatedIndexKind": "generated_archive_input; no Git modes or Git objects fabricated",
              "originalErrorsExcluded": False, "conditionsEvaluated": False, "sourceFunctionsExecuted": False,
              "generatorExecuted": False, "shippingDecisionsMade": False, "remoteArchiveBytesMeasured": False,
              "binaryComponentCoverage": "not_proven", "licenseCoverage": "not_proven", "promoted": False}
    EXTERNAL = INPUTS["EXTERNAL"]
    EXTERNAL["write_report"](output, report)
    return report


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--output-name", required=True)
    args = parser.parse_args()
    source = args.source.absolute()
    report = inspect(source, source / args.output_name)
    print(json.dumps({"selectedPaths": report["after"]["verifiedSelectedFiles"],
                      "rawGapsBefore": len(report["before"]["unresolvedRawReferences"]),
                      "rawGapsAfter": len(report["after"]["unresolvedRawReferences"]),
                      "closedRawReferences": len(report["closedOriginalRawReferences"]),
                      "yesErrorsBefore": len(report["before"]["shippingMetadataEntriesWithErrors"]),
                      "yesErrorsAfter": len(report["after"]["shippingMetadataEntriesWithErrors"]),
                      "directoryErrors": len(report["after"]["directoryErrors"]),
                      "nonstandardValues": len(report["after"]["nonstandardShippingValues"]),
                      "actualObjectCheckoutRecords": len(report["originalCheckoutEvidence"]), "promoted": False}))
