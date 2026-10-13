"""Reverify fixed Crashpad inputs and record source-level Chromium mappings.

Keep standalone metadata failures visible. Actual written GN args plus exact
source branches are configuration evidence, not measured effective GN, remote
file bytes, compile graph, binary coverage or complete NOTICE approval.
"""
import argparse
import hashlib
import json
from pathlib import Path
import re
import runpy


HERE = Path(__file__).parent.absolute()
SOURCES = runpy.run_path(str(HERE / "collect-chromium-crashpad-lss-sources.py"))
NODE = runpy.run_path(str(HERE / "inspect-chromium-node-generated-coverage.py"))
ARGS = runpy.run_path(str(HERE / "inspect-chromium-archive-inputs.py"))
INPUTS, TREE = NODE["INPUTS"], NODE["INPUTS"]["TREE"]
SOURCE_SHA = "39f335d0b005a88498a6127f4045a58843c2a8de10062a19db638939239fe830"
DEFAULT_SHA = "9f347a29e64ebe208cf6b17c59062df1fdaf33959a18e259de67e8f9ea0e7252"
CRASHPAD = "third_party/crashpad/crashpad/"


def checked_record(root, request, row, group):
    oid = request["gitObjectSha1"]
    if (any(row.get(key) != value for key, value in request.items()) or row.get("sourceGroup") != group
            or row.get("verified") is not True or row.get("rawFile") != oid + ".gitiles-data"
            or row.get("sourceFile") != oid + ".source-data"):
        raise ValueError("exact selected source request, group and inert data filenames required")
    raw = INPUTS["read_data"](root / row["rawFile"], SOURCES["MAX_RAW"])
    if (len(raw), hashlib.sha256(raw).hexdigest()) != (row.get("rawBytes"), row.get("rawSha256")):
        raise ValueError("original Crashpad raw source bytes changed")
    decoded, identity = SOURCES["decoded_blob"](raw, oid)
    data = INPUTS["read_data"](root / row["sourceFile"], SOURCES["MAX_SOURCE"])
    if decoded != data or identity != {key: row.get(key) for key in ("bytes", "sha256")}:
        raise ValueError("original Crashpad source bytes changed")
    return data


def checked_json(root, filename, metadata, checker):
    data = INPUTS["read_data"](root / filename, SOURCES["MAX_RAW"])
    if (len(data), hashlib.sha256(data).hexdigest()) != (metadata.get("bytes"), metadata.get("sha256")):
        raise ValueError("original lss Gitiles JSON bytes changed")
    value = checker(data)
    if value != metadata.get("value"):
        raise ValueError("original lss JSON interpretation changed")
    return value


def checked_sources(source, inputs):
    main_sha, checkout_sha, correspondence = SOURCES["checked_inputs"](source)
    if main_sha != inputs["mainSha256"]:
        raise ValueError("same independently sealed main tree required")
    root = source / "crashpad-lss-source-01"
    report, sha = INPUTS["receipt"](root / "crashpad-lss-inventory.json", SOURCE_SHA)
    if (report.get("sourceCommit") != TREE["COMMIT"] or report.get("mainReceiptSha256") != main_sha
            or report.get("checkoutReceiptSha256") != checkout_sha or report.get("collectionComplete") is not True
            or (report.get("lssRepository"), report.get("lssRevision")) != (SOURCES["REPOSITORY"], SOURCES["REVISION"])
            or report.get("lssCheckoutCorrespondence") != correspondence
            or any(report.get(key) is not False for key in ("conditionsEvaluated", "sourceFunctionsExecuted",
                "generatorExecuted", "shippingDecisionsMade", "effectiveGnMeasured", "promoted"))):
        raise ValueError("complete original non-promoted Crashpad/lss receipt required")
    commit = checked_json(root, "lss-commit.json-data", report["lssCommit"], SOURCES["checked_commit"])
    tree = checked_json(root, "lss-tree.json-data", report["lssRootTree"],
                        lambda data: SOURCES["checked_tree"](data, commit["tree"]))
    directories = [row for row in tree["entries"] if row["type"] == "tree"]
    if report.get("uncollectedRootDirectories") != directories:
        raise ValueError("uncollected root subtrees must remain explicit")
    requests = [(row, "main") for row in SOURCES["main_requests"]()]
    requests += [(SOURCES["lss_request"](row), "lss") for row in tree["entries"] if SOURCES["selected_entry"](row)]
    if not isinstance(report.get("records"), list) or len(report["records"]) != len(requests):
        raise ValueError("complete exact reviewed main/lss selected paths required")
    blobs = {}
    for (request, group), row in zip(requests, report["records"]):
        path = request["path"] if group == "main" else SOURCES["PREFIX"] + "/" + request["path"]
        blobs[path] = checked_record(root, request, row, group)
    if sum(map(len, blobs.values())) != report.get("selectedSourceBytes") or sum(map(len, blobs.values())) > SOURCES["MAX_TOTAL"]:
        raise ValueError("exact bounded selected source byte total required")
    defaults_root = source / "crashpad-default-source-01"
    defaults, defaults_sha = INPUTS["receipt"](defaults_root / "crashpad-default-inventory.json", DEFAULT_SHA)
    if (defaults.get("sourceCommit") != TREE["COMMIT"] or defaults.get("mainReceiptSha256") != main_sha
            or defaults.get("collectionComplete") is not True or len(defaults.get("records", [])) != 1
            or any(defaults.get(key) is not False for key in ("sourceFunctionsExecuted", "effectiveGnMeasured", "generatorExecuted", "promoted"))):
        raise ValueError("complete original non-promoted .gn default receipt required")
    entry = inputs["index"].get(".gn")
    if entry is None or (entry["mode"], entry["gitObjectSha1"]) != ("100644", SOURCES["DEFAULT_PIN"]["gitObjectSha1"]):
        raise ValueError("independent exact root .gn main-tree blob required")
    blobs[".gn"] = checked_record(defaults_root, SOURCES["DEFAULT_PIN"], defaults["records"][0], "main")
    _, generated, generated_sha, _ = NODE["NODE"]["checked_sources"](source)
    for path in list(SOURCES["GENERATED"]["PINS"])[:3]:
        blobs[path] = generated[path]
    return report, {"crashpadSourceReceiptSha256": sha, "rootDefaultReceiptSha256": defaults_sha,
                    "generatedSourceReceiptSha256": generated_sha}, blobs


def append_sources(inputs, report, blobs):
    for row in report["lssRootTree"]["value"]["entries"]:
        path = SOURCES["PREFIX"] + "/" + row["name"]
        if path in inputs["index"]:
            raise ValueError("lss root must not overwrite another source index")
        inputs["index"][path] = {"path": path, "mode": format(row["mode"], "06o"),
                                 "kind": row["type"], "gitObjectSha1": row["id"]}
    for path, payload in blobs.items():
        entry = inputs["index"].get(path)
        if entry is None or entry["mode"] not in ("100644", "100755"):
            raise ValueError("selected Crashpad/lss source must match a regular pinned tree blob")
        identity = TREE["check_blob"](entry["gitObjectSha1"], payload)
        existing = inputs["files"].get(path)
        if existing:
            if existing["payload"] != payload:
                raise ValueError("existing verified main source must not be replaced")
            continue
        row = {"path": path, "gitObjectSha1": entry["gitObjectSha1"], **identity}
        inputs["files"][path] = {**row, "payload": payload}
        inputs["references"].extend(TREE["references"](row, payload))


def excerpt(blobs, path, pattern):
    text = blobs[path].decode("utf-8")
    matches = list(re.finditer(pattern, text, re.MULTILINE))
    if len(matches) != 1:
        raise ValueError("one exact pinned Crashpad configuration rule required: " + path)
    match = matches[0]
    return {"path": path, "line": text[:match.start()].count("\n") + 1,
            "text": match.group(), "sourceSha256": hashlib.sha256(blobs[path]).hexdigest()}


def configuration_evidence(blobs, targets, correspondence):
    rules = [
        (".gn", r'^buildconfig = "//build/config/BUILDCONFIG.gn"$'),
        (".gn", r'^default_args = \{[\s\S]*?^  crashpad_dependencies = "chromium"$'),
        (CRASHPAD + "build/crashpad_buildconfig.gni", r'^crashpad_is_in_chromium = crashpad_dependencies == "chromium"$'),
        (CRASHPAD + "third_party/lss/BUILD.gn", r'^  if \(crashpad_is_in_chromium\) \{\n    defines = \[ "CRASHPAD_LSS_SOURCE_EXTERNAL" \]'),
        (CRASHPAD + "third_party/lss/lss.h", r'^#if defined\(CRASHPAD_LSS_SOURCE_EXTERNAL\)\n#include "third_party/lss/linux_syscall_support.h"'),
        (CRASHPAD + "third_party/zlib/BUILD.gn", r'^if \(crashpad_is_in_chromium \|\| crashpad_is_in_fuchsia \|\| crashpad_is_in_dart\) \{\n  zlib_source = "external"'),
        (CRASHPAD + "third_party/zlib/BUILD.gn", r'^if \(zlib_source == "external"\) \{\n  group\("zlib"\) \{\n    public_configs = \[ ":zlib_config" \]\n    public_deps = \[ "//third_party/zlib" \]'),
        (CRASHPAD + "third_party/zlib/zlib_crashpad.h", r'^#if defined\(CRASHPAD_ZLIB_SOURCE_SYSTEM\)[\s\S]*?^#include <zlib.h>$'),
    ]
    evidence = [excerpt(blobs, path, pattern) for path, pattern in rules]
    if (len(targets) != 4 or {row["target"] for row in targets} != set(ARGS["PLATFORMS"])
            or len(correspondence) != 4 or {row["target"] for row in correspondence} != set(ARGS["PLATFORMS"])):
        raise ValueError("all four exact original GN targets and checkout correspondences required")
    by_target = {row["target"]: row for row in correspondence}
    rows = []
    for target in targets:
        args = target["gnArgsWritten"]
        if "crashpad_dependencies" in args:
            raise ValueError("recorded Crashpad override requires a separate reviewed mapping")
        if not target.get("explicitGnArgsVerified"):
            raise ValueError("original literal written GN args must be independently verified")
        rows.append({"target": target["target"], "buildId": target["buildId"], "gnArgsWritten": args,
                     "crashpadOverrideRecorded": False, "rootDefault": "chromium",
                     "lssCheckoutStatus": by_target[target["target"]]["status"],
                     "effectiveGnMeasured": False, "binaryCoverageProven": False})
    return {"rules": evidence, "targets": rows, "sourceLevelMapping": [
        {"component": "Crashpad lss", "embeddedMetadataDirectory": CRASHPAD + "third_party/lss",
         "configuredHeaderPath": "third_party/lss/linux_syscall_support.h", "licensePath": "third_party/lss/LICENSE",
         "actualCheckoutMatchTargets": [row["target"] for row in rows if row["lssCheckoutStatus"] == "exact_repository_and_revision_match"]},
        {"component": "Crashpad zlib", "embeddedMetadataDirectory": CRASHPAD + "third_party/zlib",
         "configuredTarget": "//third_party/zlib", "licensePath": "third_party/zlib/LICENSE"}],
        "effectiveGnMeasured": False, "metadataPathsAliased": False, "originalErrorsExcluded": False}


def inspect(source, output):
    if output.parent != source or not re.fullmatch(r"crashpad-coverage-[0-9]{2}\.json", output.name):
        raise ValueError("safe fresh Crashpad coverage output required")
    if output.exists() or output.is_symlink():
        raise FileExistsError("never overwrite Crashpad coverage evidence")
    inputs = INPUTS["load_inputs"](source, True, True, True, True)
    _, recursive_sha = NODE["RECURSIVE"]["append_sources"](source, inputs)
    node_root, node, node_sha = NODE["checked_node"](source)
    NODE["append_node"](inputs, node_root, node)
    before = NODE["summarize"](inputs)
    report, seals, blobs = checked_sources(source, inputs)
    original_args = ARGS["inspect"](source)
    config = configuration_evidence(blobs, original_args["targets"], report["lssCheckoutCorrespondence"])
    append_sources(inputs, report, blobs)
    after = NODE["summarize"](inputs)
    for mapping in config["sourceLevelMapping"]:
        path = mapping["licensePath"]
        entry, selected = inputs["index"].get(path), inputs["files"].get(path)
        if not entry or not selected or entry["mode"] not in ("100644", "100755"):
            raise ValueError("mapped license bytes must exist at their real pinned source path")
        mapping["licenseGitObjectSha1"] = entry["gitObjectSha1"]
        mapping["licenseSha256"] = selected["sha256"]
        mapping["licenseBytes"] = selected["bytes"]
        mapping["embeddedMetadataErrorsRetained"] = [row for row in after["shippingMetadataEntriesWithErrors"]
            if row["directory"] == mapping["embeddedMetadataDirectory"]]
    closed = [row for row in before["unresolvedRawReferences"] if NODE["resolution"](row, inputs) == "resolved_to_verified_source_bytes"]
    result = {"scope": "fixed source mapping and original GN-args evidence, not effective graph or complete NOTICE approval",
              "sourceCommit": TREE["COMMIT"], "mainReceiptSha256": inputs["mainSha256"],
              "recursiveReceiptSha256": recursive_sha, "generatedNodeReceiptSha256": node_sha, **seals,
              "originalGnLogReceiptSha256": original_args["logReceiptSha256"], "configurationEvidence": config,
              "lssRootTree": report["lssRootTree"]["value"], "uncollectedLssRootDirectories": report["uncollectedRootDirectories"],
              "before": before, "after": after, "closedOriginalRawReferences": closed,
              "sourceFunctionsExecuted": False, "conditionsEvaluated": False, "generatorExecuted": False,
              "shippingDecisionsMade": False, "metadataPathsAliased": False, "originalErrorsExcluded": False,
              "effectiveGnMeasured": False, "remoteArchiveBytesMeasured": False,
              "binaryComponentCoverage": "not_proven", "licenseCoverage": "not_proven", "promoted": False}
    INPUTS["EXTERNAL"]["write_report"](output, result)
    return result


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--output-name", required=True)
    args = parser.parse_args()
    source = args.source.absolute()
    report = inspect(source, source / args.output_name)
    print(json.dumps({"selectedPaths": report["after"]["verifiedSelectedFiles"],
                      "rawGaps": len(report["after"]["unresolvedRawReferences"]),
                      "closedRawReferences": len(report["closedOriginalRawReferences"]),
                      "yesErrors": len(report["after"]["shippingMetadataEntriesWithErrors"]),
                      "directoryErrors": len(report["after"]["directoryErrors"]),
                      "configuredMappings": len(report["configurationEvidence"]["sourceLevelMapping"]), "promoted": False}))
