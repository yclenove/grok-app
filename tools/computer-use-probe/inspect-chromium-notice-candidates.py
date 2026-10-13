"""Reproduce reviewed generator selection over verified sources, retaining gaps.

Emit a clearly partial notice preview, never a shipping NOTICE or SBOM. Keep
raw metadata values: upstream shipped-only filtering excludes exact 'no', not
every value other than 'yes'. No upstream functions, hooks or GN are executed.
"""
import argparse
import ast
import hashlib
import json
from pathlib import Path
import re
import runpy


HERE = Path(__file__).parent.absolute()
CONSUMER = runpy.run_path(str(HERE / "inspect-chromium-consumer-coverage.py"))
INPUTS, NODE, INTEGRATION = (CONSUMER[key] for key in ("INPUTS", "NODE", "INTEGRATION"))
POLICY = NODE["POLICY"]
CONSUMER_SHA = "1c4a82836579fde17a64a00a3d7ae62eca9054b56b4fa047f721522a085fed8d"
MAX_NOTICE = 16 * 1024**2


def selection_rule(contents):
    if hashlib.sha256(contents).hexdigest() != POLICY["POLICY_SHA"]:
        raise ValueError("exact reviewed Chromium license policy required")
    module = ast.parse(contents.decode())
    functions = {node.name: node for node in module.body if isinstance(node, ast.FunctionDef)}
    discover = functions.get("_DiscoverMetadatas")
    parse = functions.get("ParseDir")
    if discover is None or parse is None:
        raise ValueError("original metadata discovery and parser required")
    wanted = ast.parse("if args.shipped_only and m['Shipped'] == NO:\n    continue").body[0]
    matched = [node for node in ast.walk(discover) if ast.dump(node) == ast.dump(wanted)]
    invalid = ast.parse("if meta_errors:\n    errors.append('placeholder')\n    continue").body[0]
    error_branches = [node for node in ast.walk(parse) if isinstance(node, ast.If)
                      and ast.dump(node.test) == ast.dump(invalid.test)
                      and len(node.body) == 2 and isinstance(node.body[-1], ast.Continue)]
    special_return = ast.parse("return [directory_metadata], errors").body[0]
    special = [node for node in ast.walk(parse) if ast.dump(node) == ast.dump(special_return)]
    if len(matched) != 1 or len(error_branches) != 1 or len(special) != 1:
        raise ValueError("one exact source rule for shipped filtering and invalid-record omission required")
    return {"path": "tools/licenses/licenses.py", "sourceSha256": POLICY["POLICY_SHA"],
            "shippedOnlyExclusion": "Shipped == NO", "comparisonIsCaseSensitive": True,
            "filterLine": matched[0].lineno, "invalidMetadataOmitted": True,
            "invalidMetadataLine": error_branches[0].lineno, "specialCaseErrorsDoNotOmitRecord": True,
            "specialCaseReturnLine": special[0].lineno, "sourceFunctionsExecuted": False}


def select_candidates(scan, policy):
    valid, excluded, invalid, unknown = [], [], [], []
    for row in scan["entries"]:
        if row["shippedMetadata"] not in (policy["YES"], policy["NO"]):
            unknown.append(row)
        if row["errors"] and row["metadata"] != "licenses.py SPECIAL_CASES":
            invalid.append(row)
        elif row["shippedMetadata"] == policy["NO"]:
            excluded.append(row)
        else:
            valid.append(row)
    return {"generatorCandidates": valid, "exactNoExcluded": excluded,
            "invalidReadmeRecordsOmitted": invalid, "nonstandardShippingValues": unknown,
            "directoryErrorsRetained": scan["directoryErrors"],
            "candidateRecordsWithoutFullText": [row for row in valid if not row["availableLicensePaths"]],
            "specialCaseRecordsWithErrorsRetained": [row for row in valid if row["errors"]]}


def render_preview(candidates, inputs):
    root = inputs["files"].get("LICENSE")
    if root is None or inputs["index"].get("LICENSE", {}).get("mode") not in ("100644", "100755"):
        raise ValueError("verified Chromium root license required")
    data = root["payload"]
    if not data or (len(data), hashlib.sha256(data).hexdigest()) != (root["bytes"], root["sha256"]):
        raise ValueError("unchanged nonempty Chromium root license required")
    parts = [b"PARTIAL SOURCE NOTICE PREVIEW - NOT FOR DISTRIBUTION\n"
             b"This preview omits unresolved metadata/source obligations; it is not a complete Chromium NOTICE or SBOM.\n\n",
             b"Chromium\n" + data]
    bindings = [{"component": "Chromium", "sourcePath": "LICENSE", "sha256": root["sha256"], "bytes": len(data)}]
    rows = sorted(candidates["generatorCandidates"], key=lambda row: (row["name"].lower(), row["directory"]))
    total = sum(map(len, parts))
    for row in rows:
        prefix = ("\nComponent: " + row["name"] + "\nSource directory: " + row["directory"]
                  + "\nDeclared license: " + row["license"] + "\nRaw Shipped: " + row["shippedMetadata"] + "\n").encode()
        total += len(prefix)
        parts.append(prefix)
        for path in row["availableLicensePaths"]:
            source = inputs["files"].get(path)
            entry = inputs["index"].get(path)
            if source is None or entry is None or entry["mode"] not in ("100644", "100755", "archive_regular_file"):
                raise ValueError("every notice license needs its real verified regular source path")
            contents = source["payload"]
            if not contents or (len(contents), hashlib.sha256(contents).hexdigest()) != (source["bytes"], source["sha256"]):
                raise ValueError("every notice text must match its original size and digest")
            header = ("License source: " + path + "\n").encode()
            total += len(header) + len(contents)
            if total > MAX_NOTICE:
                raise ValueError("bounded partial notice preview required")
            parts.extend((header, contents))
            bindings.append({"component": row["name"], "directory": row["directory"], "sourcePath": path,
                             "sha256": source["sha256"], "bytes": len(contents)})
        if total > MAX_NOTICE:
            raise ValueError("bounded partial notice preview required")
    # Separator overhead must also fit, not only the text payloads.
    preview = b"\n".join(parts)
    if len(preview) > MAX_NOTICE:
        raise ValueError("bounded partial notice preview required")
    return preview, bindings


def load_notice(source):
    INPUTS["checked_source"](source)
    inputs, baseline = CONSUMER["load_consumer"](source)
    original, original_sha = INPUTS["receipt"](source / "consumer-coverage-01.json", CONSUMER_SHA)
    if baseline != original:
        raise ValueError("entire original consumer audit must reproduce, not only its counts")
    webgl_sha = INPUTS["append_external"](source, "external-license-07", INPUTS["NOTICE_TEST_SHA"],
        INPUTS["EXTERNAL"]["NOTICE_TEST_PINS"], INPUTS["NOTICE_TEST_SEALS"], inputs["index"],
        inputs["files"], inputs["references"], inputs["components"])
    checkout = INTEGRATION["SOURCES"]["CHECKOUTS"]["inspect"](source)
    prefix, repository, revision = INPUTS["EXTERNAL"]["NOTICE_TEST_PINS"]["webgl"]
    correspondence = []
    for target in checkout["targets"]:
        compared = INTEGRATION["SOURCES"]["CHECKOUTS"]["compare_pin"]("src/" + prefix, repository, revision, target["manifest"])
        if compared["status"] != "exact_repository_and_revision_match":
            raise ValueError("actual WebGL source must match each original checkout manifest")
        correspondence.append({"target": target["target"], **compared})
    contents = inputs["files"]["tools/licenses/licenses.py"]["payload"]
    rule = selection_rule(contents)
    policy = POLICY["parse_policy"](contents)
    view = NODE["GeneratedSourceView"](inputs["index"], {path: row["payload"] for path, row in inputs["files"].items()}, policy["VCS_METADATA_DIRS"])
    scan = POLICY["scan"](view, policy)
    candidates = select_candidates(scan, policy)
    preview, bindings = render_preview(candidates, inputs)
    report = {"scope": "partial verified-source notice preview using reviewed source filtering, not binary or full NOTICE approval",
        "sourceCommit": INPUTS["TREE"]["COMMIT"], "originalConsumerReceiptSha256": original_sha,
        "generatorSourceRule": rule, "candidateSelection": candidates, "licenseBindings": bindings,
        "webglSourceReceiptSha256": webgl_sha, "webglCheckoutCorrespondence": correspondence,
        "afterWebglSources": NODE["summarize"](inputs),
        "previewFile": "partial-source-notice.txt", "previewBytes": len(preview), "previewSha256": hashlib.sha256(preview).hexdigest(),
        "originalConsumerErrorsRetained": baseline["after"], "sourceFunctionsExecuted": False,
        "generatorExecuted": False, "metadataValuesNormalized": False, "metadataPathsAliased": False,
        "shippingDecisionsMade": False, "binaryComponentCoverage": "not_proven", "completeChromiumNotice": False, "promoted": False}
    return inputs, report, preview


def inspect(source, output):
    INPUTS["checked_source"](source)
    if output.parent != source or not re.fullmatch(r"notice-candidates-[0-9]{2}", output.name):
        raise ValueError("safe fresh task-owned notice candidate output required")
    if output.exists() or output.is_symlink():
        raise FileExistsError("never overwrite notice candidate evidence")
    _, report, preview = load_notice(source)
    output.mkdir()
    with (output / report["previewFile"]).open("xb") as stream:
        stream.write(preview)
    INPUTS["EXTERNAL"]["write_report"](output / "notice-candidate-inventory.json", report)
    return report


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--output-name", required=True)
    args = parser.parse_args()
    source = args.source.absolute()
    report = inspect(source, source / args.output_name)
    selected = report["candidateSelection"]
    print(json.dumps({"candidates": len(selected["generatorCandidates"]), "invalidOmitted": len(selected["invalidReadmeRecordsOmitted"]),
        "unknownValues": len(selected["nonstandardShippingValues"]), "candidateTextGaps": len(selected["candidateRecordsWithoutFullText"]),
        "previewBytes": report["previewBytes"], "previewSha256": report["previewSha256"], "completeChromiumNotice": False, "promoted": False}))
