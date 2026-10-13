"""Reverify integration sources and actual dependency license paths without aliases.

Keep all standalone/raw metadata failures. Fixed DEPS/export/helper rules and
original GN/custom vars are source-level configuration evidence; no generated
remote GN file, effective graph, binary components or full NOTICE are claimed.
"""
import argparse
import ast
import hashlib
import json
from pathlib import Path
import re
import runpy


HERE = Path(__file__).parent.absolute()
SOURCES = runpy.run_path(str(HERE / "collect-chromium-integration-sources.py"))
PERFETTO = runpy.run_path(str(HERE / "collect-chromium-perfetto-licenses.py"))
CRASHPAD = runpy.run_path(str(HERE / "inspect-chromium-crashpad-coverage.py"))
GCLIENT = runpy.run_path(str(HERE / "inspect-chromium-gclient-inheritance.py"))
INPUTS, NODE, TREE = CRASHPAD["INPUTS"], CRASHPAD["NODE"], CRASHPAD["TREE"]
PROFILE_SEALS = {
    "integration": ("integration-source-01", "e13f661a278e0517b82eca45c0008134f5e56a62e3614981c0d1e608f2065b23"),
    "embedding": ("embedding-source-01", "a17967d2acf493b62e2a4308d338eb75f64e828cb8f16b0f9302dedb7c4b0e82"),
    "targets": ("integration-target-source-01", "507cbcb9dabc519613cb065b4b021f8c2cd92f72f015f22dda9612aa0c00406d"),
}
PERFETTO_ROOT_SHA = "fad740c55664cd5ce138c011985df120f65db8a9fe253b57f4579484093e47f6"
PERFETTO_SHA = "dfa59c35a0b46604d500e34903f1a799db69aa8398d17fc589fefe6d48e65267"
ANGLE = "third_party/angle/"
SCREEN = "third_party/openscreen/src/"


def checked_profile(source, profile, inputs, checkout_sha, correspondence, profiles=PROFILE_SEALS, git_objects=False):
    if type(git_objects) is not bool or (git_objects and profile not in SOURCES["GIT_PROFILES"]):
        raise ValueError("explicit Git object verification allowed only for complete compiled source inventories")
    name, seal = profiles[profile]
    root = source / name
    report, sha = INPUTS["receipt"](root / "integration-source-inventory.json", seal)
    requests = SOURCES["requests"](profile)
    if (report.get("gitObjectTransport", False) is not git_objects
            or report.get("collectionComplete") is not True or report.get("sourceCommit") != TREE["COMMIT"]
            or report.get("mainReceiptSha256") != inputs["mainSha256"] or report.get("checkoutReceiptSha256") != checkout_sha
            or report.get("sourceRoots") != {key: list(value) for key, value in SOURCES["source_roots"](profile).items()}
            or report.get("checkoutCorrespondence") != correspondence or report.get("requests") != requests
            or len(report.get("records", [])) != len(requests)
            or any(report.get(key) is not False for key in ("sourceFunctionsExecuted", "conditionsEvaluated", "generatorExecuted",
                "effectiveGnMeasured", "shippingDecisionsMade", "metadataPathsAliased", "promoted"))):
        raise ValueError("complete original fixed integration-profile receipt required")
    blobs = {}
    for request, row in zip(requests, report["records"]):
        oid = request["gitObjectSha1"]
        entry = inputs["index"].get(request["sourcePath"])
        if (entry is None or entry["mode"] not in ("100644", "100755") or entry["gitObjectSha1"] != oid
                or any(row.get(key) != value for key, value in request.items()) or row.get("verified") is not True
                or row.get("rawFile") != oid + (".git-blob-data" if git_objects else ".gitiles-data") or row.get("sourceFile") != oid + ".source-data"):
            raise ValueError("exact main/external source tree and inert record identities required")
        raw = INPUTS["read_data"](root / row["rawFile"], SOURCES["MAX_SOURCE"] if git_objects else SOURCES["MAX_RAW"])
        if (len(raw), hashlib.sha256(raw).hexdigest()) != (row.get("rawBytes"), row.get("rawSha256")):
            raise ValueError("original integration raw source bytes changed")
        data, identity = SOURCES["decode_blob"](raw, oid) if git_objects else SOURCES["decode"](raw, oid)
        if (INPUTS["read_data"](root / row["sourceFile"], SOURCES["MAX_SOURCE"]) != data
                or identity != {key: row.get(key) for key in ("bytes", "sha256")}):
            raise ValueError("original integration source bytes changed")
        blobs[request["sourcePath"]] = data
    if sum(map(len, blobs.values())) != report.get("selectedSourceBytes") or sum(map(len, blobs.values())) > SOURCES["MAX_TOTAL"]:
        raise ValueError("exact bounded integration source total required")
    if git_objects:
        SOURCES["checked_git_batches"](root, report, requests, blobs)
    return blobs, sha


def checked_perfetto(source, inputs):
    checked, checkout_sha, correspondence = PERFETTO["checked_inputs"](source)
    if checked["mainSha256"] != inputs["mainSha256"]:
        raise ValueError("same original main source receipt required")
    root = source / "perfetto-license-source-01"
    report, sha = INPUTS["receipt"](root / "perfetto-source-inventory.json", PERFETTO_ROOT_SHA)
    prefix, repository, revision = PERFETTO["PINS"]["perfetto"]
    if (report.get("collectionComplete") is not True or report.get("sourceCommit") != TREE["COMMIT"]
            or report.get("mainReceiptSha256") != inputs["mainSha256"] or report.get("checkoutReceiptSha256") != checkout_sha
            or report.get("parentCatalogSha256") != INPUTS["EXTERNAL"]["CATALOG_SHA"]
            or report.get("checkoutCorrespondence") != correspondence or len(report.get("components", [])) != 1
            or report["components"][0].get("receiptSha256") != PERFETTO_SHA
            or any(report.get(key) is not False for key in ("sourceFunctionsExecuted", "conditionsEvaluated", "generatorExecuted",
                "shippingDecisionsMade", "metadataPathsAliased", "promoted"))):
        raise ValueError("complete original manifest-matched non-promoted Perfetto source receipt required")
    directory = root / "perfetto"
    component, component_sha = INPUTS["receipt"](directory / "license-source-inventory.json", PERFETTO_SHA)
    if ((component.get("checkoutPath"), component.get("repository"), component.get("revision")) != (prefix, repository, revision)
            or component.get("collectionComplete") is not True or component.get("generatorExecuted") is not False
            or component.get("promoted") is not False):
        raise ValueError("exact original completed Perfetto component required")
    data = INPUTS["read_data"](directory / "source-tree.nul", TREE["MAX_TREE"])
    if (len(data), hashlib.sha256(data).hexdigest()) != (component.get("treeBytes"), component.get("treeSha256")):
        raise ValueError("original Perfetto source tree bytes changed")
    rows = TREE["tree_records"](data)
    if len(rows) != component.get("treeEntries"):
        raise ValueError("complete Perfetto source tree entry count required")
    INPUTS["merge_tree"](inputs["index"], rows, prefix)
    INPUTS["merge_files"](inputs["index"], component["selectedFiles"], directory, prefix, inputs["files"], inputs["references"])
    return {"rootReceiptSha256": sha, "componentReceiptSha256": component_sha, "revision": revision,
            "rootTreeSha1": component["rootTreeSha1"], "treeEntries": len(rows), "selectedFiles": len(component["selectedFiles"]),
            "selectedSourceBytes": component["sourceBytes"], "checkoutCorrespondence": correspondence}


def append_blobs(inputs, blobs):
    for path, data in blobs.items():
        row = inputs["index"].get(path)
        if row is None or row["mode"] not in ("100644", "100755"):
            raise ValueError("selected integration source must have its real regular Git path")
        identity = TREE["check_blob"](row["gitObjectSha1"], data)
        original = inputs["files"].get(path)
        if original:
            if original["payload"] != data:
                raise ValueError("never replace a verified source with other bytes")
            continue
        selected = {"path": path, "gitObjectSha1": row["gitObjectSha1"], **identity}
        inputs["files"][path] = {**selected, "payload": data}
        inputs["references"].extend(TREE["references"](selected, data))


def deps_export(contents):
    names = {"vars", "gclient_gn_args_file", "gclient_gn_args"}
    assignments = {}
    for row in ast.parse(contents.decode()).body:
        if isinstance(row, ast.Assign) and len(row.targets) == 1 and isinstance(row.targets[0], ast.Name) and row.targets[0].id in names:
            if row.targets[0].id in assignments:
                raise ValueError("unique original DEPS export assignments required")
            assignments[row.targets[0].id] = row
    if set(assignments) != names or not isinstance(assignments["vars"].value, ast.Dict):
        raise ValueError("literal original DEPS variable/export declarations required")
    filename = ast.literal_eval(assignments["gclient_gn_args_file"].value)
    exports = ast.literal_eval(assignments["gclient_gn_args"].value)
    flags = [value for key, value in zip(assignments["vars"].value.keys, assignments["vars"].value.values)
             if isinstance(key, ast.Constant) and key.value == "build_with_chromium"]
    if (filename != "src/build/config/gclient_args.gni" or not isinstance(exports, list)
            or not all(isinstance(name, str) for name in exports) or exports.count("build_with_chromium") != 1
            or len(flags) != 1 or not isinstance(flags[0], ast.Constant) or flags[0].value is not True):
        raise ValueError("exact boolean Chromium flag and generated GN export required")
    return {"path": "DEPS", "sourceSha256": hashlib.sha256(contents).hexdigest(), "variableLine": flags[0].lineno,
            "declaredBuildWithChromium": True, "exportedGnFile": filename, "exportDeclarationLine": assignments["gclient_gn_args"].lineno,
            "variableExported": True, "sourceExecuted": False, "remoteGeneratedFileMeasured": False}


def helper_export(source):
    blobs, sha = GCLIENT["load_helpers"](source)
    shapes = GCLIENT["inspect_shapes"](blobs["gclient.py"], blobs["gclient_eval.py"])
    tree = ast.parse(blobs["gclient.py"].decode())
    writer = GCLIENT["function"](tree, "WriteGNArgsFile", "Dependency")
    builtin = GCLIENT["function"](tree, "get_builtin_vars", "Dependency")
    returns = [row.value for row in ast.walk(builtin) if isinstance(row, ast.Return)]
    if len(returns) != 1 or not isinstance(returns[0], ast.Dict):
        raise ValueError("original literal builtin variable-name dictionary required")
    keys = [ast.literal_eval(row) for row in returns[0].keys]
    if "build_with_chromium" in keys:
        raise ValueError("builtin embedding override requires separate review")
    expected = ["variables = self.get_vars()", "value = variables[arg]", 'lines.append(f"{arg} = {ToGNString(value)}")']
    for statement in expected:
        wanted = ast.parse(statement).body[0]
        if sum(ast.dump(row) == ast.dump(wanted) for row in ast.walk(writer)) != 1:
            raise ValueError("exact original GN export writer source rule required")
    return {"helperReceiptSha256": sha, "helperSha256": hashlib.sha256(blobs["gclient.py"]).hexdigest(),
            "variablePrecedenceRules": shapes, "writer": GCLIENT["excerpt"](blobs["gclient.py"], [writer]),
            "builtinVariableKeys": keys, "embeddingBuiltinOverrideDeclared": False,
            "helperExecuted": False, "remoteGeneratedFileMeasured": False}


def configuration(blobs, args):
    rules = [
        ("build_overrides/build.gni", r'^import\("//build/config/gclient_args.gni"\)$'),
        (ANGLE + "build_overrides/build.gni", r'^  build_with_chromium = false$'),
        ("build_overrides/angle.gni", r'^angle_has_build = true$'),
        (ANGLE + "gni/angle.gni", r'^import\("//build_overrides/angle.gni"\)$'),
        (ANGLE + "gni/angle.gni", r'^if \(angle_has_build\) \{\n  import\("//build/config/gclient_args.gni"\)\n\}$'),
        (ANGLE + "gni/angle.gni", r'^  angle_standalone = !build_with_chromium$'),
        (ANGLE + "gni/angle.gni", r'^  angle_has_astc_encoder = angle_has_build && angle_standalone && !is_android$'),
        (ANGLE + "gni/angle.gni", r'^  angle_enable_cl = false\n  if \(!checkout_angle_cl_deps\) \{\n    angle_enable_cl = false'),
        (".gn", r'^  angle_vulkan_loader_dir = "//third_party/vulkan-loader/src"$'),
        (ANGLE + "src/common/vulkan/BUILD.gn", r'^        public_deps = \[ "\$angle_vulkan_loader_dir:libvulkan" \]$'),
        (SCREEN + "BUILD.gn", r'^import\("//build_overrides/build.gni"\)$'),
        (SCREEN + "build_overrides/build.gni", r'^build_with_chromium = false$'),
        (SCREEN + "third_party/googleurl/BUILD.gn", r'^if \(build_with_chromium\) \{\n  source_set\("googleurl"\) \{\n    public_deps = \[ "//url" \]'),
        (SCREEN + "third_party/quiche/BUILD.gn", r'^if \(build_with_chromium\) \{\n  source_set\("quiche"\) \{\n    public_deps = \[ "//net/third_party/quiche" \]'),
        (SCREEN + "third_party/perfetto/BUILD.gn", r'^if \(build_with_chromium\) \{\n  group\("perfetto"\) \{\n    public_deps = \[ "//third_party/perfetto" \]'),
        ("url/gurl.h", r'^// Copyright 2013 The Chromium Authors\n// Use of this source code is governed by a BSD-style license that can be\n// found in the LICENSE file\.$'),
    ]
    evidence = [CRASHPAD["excerpt"](blobs, path, pattern) for path, pattern in rules]
    guarded = {"build_with_chromium", "angle_standalone", "angle_has_build", "angle_has_astc_encoder", "angle_enable_cl", "angle_vulkan_loader_dir"}
    targets = args["targets"]
    if len(targets) != 4 or {row["target"] for row in targets} != set(CRASHPAD["ARGS"]["PLATFORMS"]):
        raise ValueError("all four original integration build targets required")
    for row in targets:
        if guarded.intersection(row["gnArgsWritten"]) or "build_with_chromium" in row["gclientConfigRecorded"]["customVars"]:
            raise ValueError("recorded integration override requires separate source review")
        if row.get("explicitGnArgsVerified") is not True or row.get("actualGclientCustomVarsVerified") is not True:
            raise ValueError("independently verified original GN/custom-var records required")
    return {"rules": evidence, "targets": [{"target": row["target"], "buildId": row["buildId"],
                "gnArgsWritten": row["gnArgsWritten"], "gclientCustomVarsRecorded": row["gclientConfigRecorded"]["customVars"],
                "integrationOverridesRecorded": False, "remoteGeneratedFileMeasured": False, "effectiveGnMeasured": False} for row in targets],
            "defaultGateExpressions": {"ANGLE ASTC encoder": "angle_has_build && angle_standalone && !is_android",
                                       "ANGLE OpenCL": "false; also forced false when !checkout_angle_cl_deps"},
            "sourceLevelMapping": [
                {"component": "ANGLE Vulkan loader", "embeddedMetadataDirectory": ANGLE + "third_party/vulkan-loader",
                 "configuredTarget": "//third_party/vulkan-loader/src:libvulkan", "licensePath": "third_party/vulkan-loader/src/LICENSE.txt"},
                {"component": "OpenScreen URL", "embeddedMetadataDirectory": SCREEN + "third_party/googleurl",
                 "configuredTarget": "//url", "licensePath": "LICENSE", "licenseHeaderSource": "url/gurl.h"},
                {"component": "OpenScreen QUICHE", "embeddedMetadataDirectory": SCREEN + "third_party/quiche",
                 "configuredTarget": "//net/third_party/quiche", "licensePath": "net/third_party/quiche/src/LICENSE"},
                {"component": "OpenScreen Perfetto", "embeddedMetadataDirectory": SCREEN + "third_party/perfetto",
                 "configuredTarget": "//third_party/perfetto", "licensePath": "third_party/perfetto/LICENSE"}],
            "metadataPathsAliased": False, "originalErrorsExcluded": False, "effectiveGnMeasured": False}


def load_integration(source):
    inputs = INPUTS["load_inputs"](source, True, True, True, True)
    _, recursive_sha = NODE["RECURSIVE"]["append_sources"](source, inputs)
    node_root, node, node_sha = NODE["checked_node"](source)
    NODE["append_node"](inputs, node_root, node)
    crashpad, crashpad_seals, blobs = CRASHPAD["checked_sources"](source, inputs)
    CRASHPAD["append_sources"](inputs, crashpad, blobs)
    before = NODE["summarize"](inputs)
    _, checkout_sha, correspondence = SOURCES["checked_inputs"](source)
    seals = {}
    for profile in PROFILE_SEALS:
        added, seals[profile] = checked_profile(source, profile, inputs, checkout_sha, correspondence)
        blobs.update(added)
    original_args = CRASHPAD["ARGS"]["inspect"](source)
    config = configuration(blobs, original_args)
    config["mainDepsExport"] = deps_export(inputs["files"]["DEPS"]["payload"])
    config["gclientExportSource"] = helper_export(source)
    append_blobs(inputs, blobs)
    perfetto = checked_perfetto(source, inputs)
    after = NODE["summarize"](inputs)
    for row in config["sourceLevelMapping"]:
        selected = inputs["files"].get(row["licensePath"])
        if selected is None:
            raise ValueError("configured dependency license must exist at its real source path")
        row.update(licenseGitObjectSha1=selected["gitObjectSha1"], licenseSha256=selected["sha256"], licenseBytes=selected["bytes"])
        row["embeddedMetadataErrorsRetained"] = [item for item in after["shippingMetadataEntriesWithErrors"]
            if item["directory"] == row["embeddedMetadataDirectory"]]
    report = {"scope": "source-level embedding configuration, actual-source licenses and raw/default-policy audit, not complete NOTICE/binary approval",
              "sourceCommit": TREE["COMMIT"], "mainReceiptSha256": inputs["mainSha256"], "recursiveReceiptSha256": recursive_sha,
              "generatedNodeReceiptSha256": node_sha, "crashpadSourceSeals": crashpad_seals, "integrationSourceSeals": seals,
              "checkoutReceiptSha256": checkout_sha, "originalGnLogReceiptSha256": original_args["logReceiptSha256"],
              "configurationEvidence": config, "perfettoSource": perfetto, "before": before, "after": after,
              "closedOriginalRawReferences": [row for row in before["unresolvedRawReferences"] if NODE["resolution"](row, inputs) == "resolved_to_verified_source_bytes"],
              "sourceFunctionsExecuted": False, "conditionsEvaluated": False, "generatorExecuted": False,
              "metadataPathsAliased": False, "originalErrorsExcluded": False, "shippingDecisionsMade": False,
              "effectiveGnMeasured": False, "remoteGeneratedFileMeasured": False, "binaryComponentCoverage": "not_proven",
              "licenseCoverage": "not_proven", "promoted": False}
    return inputs, report


def inspect(source, output):
    if output.parent != source or not re.fullmatch(r"integration-coverage-[0-9]{2}\.json", output.name):
        raise ValueError("safe fresh integration coverage output required")
    if output.exists() or output.is_symlink():
        raise FileExistsError("never overwrite integration coverage evidence")
    _, report = load_integration(source)
    INPUTS["EXTERNAL"]["write_report"](output, report)
    return report


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--output-name", required=True)
    args = parser.parse_args()
    source = args.source.absolute()
    report = inspect(source, source / args.output_name)
    print(json.dumps({"selectedPaths": report["after"]["verifiedSelectedFiles"], "rawGaps": len(report["after"]["unresolvedRawReferences"]),
                      "yesErrors": len(report["after"]["shippingMetadataEntriesWithErrors"]), "directoryErrors": len(report["after"]["directoryErrors"]),
                      "mappings": len(report["configurationEvidence"]["sourceLevelMapping"]), "promoted": False}))
