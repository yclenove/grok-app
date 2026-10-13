"""Verify Dawn's Chromium overrides and real license sources, retaining gaps.

Original metadata, standalone pins, absent manifests and directory declarations
remain data. No GN, DEPS, upstream functions or shipping conditions execute.
"""
import argparse
import hashlib
import json
from pathlib import Path
import re
import runpy


HERE = Path(__file__).parent.absolute()
SKIA = runpy.run_path(str(HERE / "inspect-chromium-skia-coverage.py"))
INPUTS, SOURCES, INTEGRATION, SUPPLEMENT, NODE = (SKIA[key] for key in ("INPUTS", "SOURCES", "INTEGRATION", "SUPPLEMENT", "NODE"))
RECURSIVE = runpy.run_path(str(HERE / "inspect-chromium-recursive-deps.py"))
ROOT = "third_party/dawn/"
SKIA_SEAL = "c0d9f760c3f990b7e920e0719a96724dfa90c460f4399c52ef54dae2607deab5"
RECURSIVE_SEAL = "1171021ade48eaed6c7488dc5e3c03c78f9ab7941db6999791433ae4daba5520"
PROFILES = {
    "dawn-gn": ("dawn-gn-git-source-01", "6ca7b20b102486d7c29e4395bfc2d4b857405ca857f4cb1dc3d2be562339a0b8"),
    "dawn-embedding": ("dawn-embedding-source-01", "def386981759e4a4f5bc554677fe7d015b1d80dddfa4f37fe6b8f319b64833fd"),
}
LICENSE_PATHS = {
    "dawn_angle_dir": ("//third_party/angle", "third_party/angle/LICENSE"),
    "dawn_glfw_dir": ("//third_party/dawn/third_party/glfw3/src", ROOT + "third_party/glfw3/LICENSE"),
    "dawn_jinja2_dir": ("//third_party/jinja2", "third_party/jinja2/LICENSE.rst"),
    "dawn_jsoncpp_dir": ("//third_party/jsoncpp", "third_party/jsoncpp/LICENSE"),
    "dawn_perfetto_dir": ("//third_party/perfetto", "third_party/perfetto/LICENSE"),
    "dawn_spirv_tools_dir": ("//third_party/spirv-tools/src", "third_party/spirv-tools/LICENSE"),
    "dawn_swiftshader_dir": ("//third_party/swiftshader", "third_party/swiftshader/LICENSE.txt"),
    "dawn_vulkan_headers_dir": ("//third_party/vulkan-headers/src", "third_party/vulkan-headers/LICENSE.txt"),
    "dawn_vulkan_loader_dir": ("//third_party/vulkan-loader/src", "third_party/vulkan-loader/src/LICENSE.txt"),
    "dawn_vulkan_utility_libraries_dir": ("//third_party/vulkan-utility-libraries/src", "third_party/vulkan-utility-libraries/src/LICENSE.md"),
    "dawn_vulkan_validation_layers_dir": ("//third_party/vulkan-validation-layers/src", "third_party/vulkan-validation-layers/src/LICENSE.txt"),
    "dawn_dxc_dir": ("//third_party/dawn/third_party/directx-shader-compiler/src", ROOT + "third_party/directx-shader-compiler/LICENSE"),
    "dawn_dxc_gn_dir": ("//third_party/dawn/third_party/directx-shader-compiler", ROOT + "third_party/directx-shader-compiler/LICENSE"),
    "dawn_dxheaders_dir": ("//third_party/dawn/third_party/directx-headers/src", ROOT + "third_party/directx-headers/LICENSE"),
    "dawn_agility_sdk_dir": ("//third_party/dawn/third_party/agility-sdk", ROOT + "third_party/agility-sdk/LICENSE"),
}
UNBOUND_PATHS = {
    "dawn_googletest_dir": "//third_party/googletest/src",
    "dawn_partition_alloc_dir": "//base/allocator/partition_allocator",
}
MAX_FRAGMENT = 256 * 1024


def overrides(blobs):
    source = "build_overrides/dawn.gni"
    text = blobs[source].decode()
    values = re.findall(r'(?m)^(dawn_[a-z0-9_]+_dir) = "(//[^"\n]+)"$', text)
    expected = {**{key: value[0] for key, value in LICENSE_PATHS.items()}, **UNBOUND_PATHS}
    if len(values) != len(expected) or len(dict(values)) != len(values) or dict(values) != expected:
        raise ValueError("all exact original Chromium Dawn directory overrides required")
    helper = ROOT + "scripts/dawn_overrides_with_defaults.gni"
    rules = [INTEGRATION["CRASHPAD"]["excerpt"](blobs, helper, pattern) for pattern in (
        r'^import\("//build_overrides/dawn.gni"\)$',
        r'^if \(!defined\(dawn_standalone\)\) \{\n  dawn_standalone = false\n\}$',
    )]
    return {"declaredDirectories": dict(values), "overrideSource": source,
        "sourceRules": rules, "sourceFunctionsExecuted": False, "effectiveGnMeasured": False}


def configured_bindings(inputs, blobs, configuration):
    rows = []
    for variable, (directory, path) in LICENSE_PATHS.items():
        if configuration["declaredDirectories"].get(variable) != directory:
            raise ValueError("original real consumer directory required")
        rows.append({"variable": variable, "declaredDirectory": directory,
            "license": SUPPLEMENT["source_license"](inputs, path),
            "consumerReferences": SKIA["source_references"](blobs, "${" + variable + "}"),
            "completeComponentNoticeCovered": False, "binaryUseProven": False})
    return rows


def recursive_correspondence(source, inputs):
    catalog, sha = INPUTS["receipt"](source / "recursive-deps-dawn-01.json", RECURSIVE_SEAL)
    rows = [{**row, "path": path[len(ROOT):]} for path, row in inputs["index"].items() if path.startswith(ROOT)]
    actual = RECURSIVE["build_report"](inputs["files"]["DEPS"]["payload"], inputs["files"][ROOT + "DEPS"]["payload"], rows, "dawn")
    actual.update(mainReceiptSha256=inputs["mainSha256"], externalReceiptSha256=inputs["externalSha256"],
        dependencyReceiptSha256=INPUTS["GPU_SEALS"]["dawn"], dependencyDepsGitObjectSha1="da0a3aa20e5c92253b8a4662f56613f1a088cfbf",
        gpuReceiptSha256=INPUTS["GPU_SHA"])
    if actual != catalog or any(catalog[key] for key in ("unresolved", "mismatches", "uncataloguedGitlinks")):
        raise ValueError("entire exact original Dawn DEPS catalog must reproduce")
    checkout = SOURCES["CHECKOUTS"]["inspect"](source)
    comparisons = []
    for target in checkout["targets"]:
        for row in catalog["gitDependencies"]:
            comparisons.append({"target": target["target"], "condition": row["condition"],
                **SOURCES["CHECKOUTS"]["compare_pin"]("src/" + ROOT + row["path"], row["repository"], row["revision"], target["manifest"])})
    return {"receiptSha256": sha, "gitDeclarations": len(catalog["gitDependencies"]),
        "nonGitDeclarations": len(catalog["nonGitDependencies"]), "catalog": catalog,
        "actualManifestComparisons": comparisons, "statusCounts": SOURCES["CHECKOUTS"]["status_counts"](comparisons),
        "absenceMeansNotShipped": False, "inheritedConditionsEvaluated": False}


def render_fragment(bindings, root_license, inputs, apache):
    rows = {row["license"]["sourcePath"]: row["license"] for row in bindings}
    rows[root_license["sourcePath"]] = root_license
    parts = [b"PARTIAL DAWN CONFIGURED SOURCE NOTICE SUPPLEMENT - NOT FOR DISTRIBUTION\n"
        b"Original source notices and full standard text; not full browser NOTICE or binary SBOM.\n"]
    for path, row in sorted(rows.items()):
        if SUPPLEMENT["source_license"](inputs, path) != row:
            raise ValueError("unchanged complete original source notice required")
        parts.extend((("\nOriginal license source: " + path + "\n").encode(), inputs["files"][path]["payload"]))
    spdx = SUPPLEMENT["SPDX"]
    if INPUTS["TREE"]["check_blob"](spdx["LICENSES"]["Apache-2.0"]["oid"], apache) != {
            key: spdx["LICENSES"]["Apache-2.0"][key] for key in ("bytes", "sha256")}:
        raise ValueError("unchanged full pinned Apache license text required")
    parts.extend((b"\nFull Apache-2.0 text for Utility Libraries original SPDX notice:\n", apache))
    fragment = b"\n".join(parts)
    if len(fragment) > MAX_FRAGMENT:
        raise ValueError("bounded configured source notice fragment required")
    return fragment


def load_dawn(source):
    """Reverify the same sources for callers that need the checked input view."""
    INPUTS["checked_source"](source)
    inputs, original_notice, _ = SKIA["CANDIDATES"]["load_notice"](source)
    INPUTS["append_external"](source, "external-license-08", INPUTS["GPU_SHA"], INPUTS["EXTERNAL"]["GPU_PINS"],
        INPUTS["GPU_SEALS"], inputs["index"], inputs["files"], inputs["references"], inputs["components"])
    previous, previous_sha = INPUTS["receipt"](source / "skia-coverage-01/skia-coverage-inventory.json", SKIA_SEAL)
    before = NODE["summarize"](inputs)
    if (before != previous["afterDawnSources"] or original_notice["afterWebglSources"] != previous["originalRawErrorsRetained"]):
        raise ValueError("all original source audit and Skia/Dawn errors must reproduce")
    previous_fragment = INPUTS["read_data"](source / "skia-coverage-01/skia-dawn-notices.txt", SKIA["MAX_FRAGMENT"])
    if (len(previous_fragment), hashlib.sha256(previous_fragment).hexdigest()) != (previous["fragmentBytes"], previous["fragmentSha256"]):
        raise ValueError("earlier original notice fragment changed")
    blobs, seals, correspondence = {}, {}, {}
    for profile in PROFILES:
        _, checkout_sha, matched = SOURCES["checked_inputs"](source, profile)
        added, seals[profile] = INTEGRATION["checked_profile"](source, profile, inputs, checkout_sha, matched,
            profiles=PROFILES, git_objects=profile == "dawn-gn")
        blobs.update(added)
        correspondence[profile] = matched
    expected = {row["sourcePath"]: row["gitObjectSha1"] for row in SOURCES["requests"]("dawn-gn")}
    if SKIA["gn_inventory"](inputs["index"], ROOT) != expected or set(expected) != set(blobs) - {"build_overrides/dawn.gni"}:
        raise ValueError("complete independent original Dawn GN inventory required")
    recursive = recursive_correspondence(source, inputs)
    support_sha = INPUTS["append_external"](source, "external-license-09", INPUTS["GPU_SUPPORT_SHA"],
        INPUTS["EXTERNAL"]["GPU_SUPPORT_PINS"], INPUTS["GPU_SUPPORT_SEALS"], inputs["index"], inputs["files"], inputs["references"], inputs["components"])
    support_correspondence, _ = SKIA["checkout_matches"](source, INPUTS["EXTERNAL"]["GPU_SUPPORT_PINS"])
    config = overrides(blobs)
    integration, _ = INPUTS["receipt"](source / "integration-coverage-04.json", SKIA["CANDIDATES"]["CONSUMER"]["BASELINE_SHA"])
    perfetto = integration["perfettoSource"]
    SUPPLEMENT["source_license"](inputs, "third_party/perfetto/LICENSE")
    bindings = configured_bindings(inputs, blobs, config)
    utility = inputs["files"]["third_party/vulkan-utility-libraries/src/LICENSE.md"]["payload"]
    if any(value not in utility for value in (b"SPDX-License-Identifier: Apache-2.0", b"Copyright 2023 The Khronos Group Inc.",
        b"Copyright 2023 Valve Corporation", b"Copyright 2023 LunarG, Inc.")):
        raise ValueError("original complete Utility Libraries attribution and SPDX notice required")
    spdx = SUPPLEMENT["SPDX"]
    apache_source = spdx["verify"](source, "khronos-license-source-01")
    apache = INPUTS["read_data"](source / "khronos-license-source-01/Apache-2.0.license-data", spdx["MAX_TEXT"])
    root_license = SUPPLEMENT["source_license"](inputs, ROOT + "LICENSE")
    fragment = render_fragment(bindings, root_license, inputs, apache)
    after = NODE["summarize"](inputs)
    report = {"scope": "Dawn source overrides, exact recursive declarations and real configured notices only",
        "sourceCommit": INPUTS["TREE"]["COMMIT"], "originalSkiaReceiptSha256": previous_sha,
        "profileReceiptSha256": seals, "checkoutCorrespondence": correspondence,
        "completeDawnGnInventory": True, "gnPaths": len(expected), "gnSourceBytes": sum(map(len, blobs.values())),
        "configurationEvidence": config, "recursiveEvidence": recursive,
        "configuredSourceLicenseBindings": bindings, "unresolvedConfiguredLibraries": UNBOUND_PATHS,
        "gpuSupportReceiptSha256": support_sha, "gpuSupportCheckoutCorrespondence": support_correspondence,
        "perfettoSource": perfetto, "utilitySpdxStandardSource": apache_source,
        "rootDawnLicenseBinding": root_license, "before": before, "after": after,
        "closedOriginalRawReferences": [row for row in before["unresolvedRawReferences"] if NODE["resolution"](row, inputs) == "resolved_to_verified_source_bytes"],
        "originalSourceErrorsRetained": previous["afterDawnSources"],
        "fragmentFile": "dawn-configured-notices.txt", "fragmentBytes": len(fragment), "fragmentSha256": hashlib.sha256(fragment).hexdigest(),
        "sourceFunctionsExecuted": False, "conditionsEvaluated": False, "generatorExecuted": False,
        "metadataPathsAliased": False, "effectiveGnMeasured": False, "shippingExclusionsProven": False,
        "completeChromiumNotice": False, "promoted": False}
    return inputs, report, fragment


def inspect(source, output):
    INPUTS["checked_source"](source)
    if output.parent != source or not re.fullmatch(r"dawn-coverage-[0-9]{2}", output.name):
        raise ValueError("safe fresh task-owned Dawn coverage output required")
    if output.exists() or output.is_symlink():
        raise FileExistsError("never overwrite source coverage evidence")
    _, report, fragment = load_dawn(source)
    output.mkdir()
    with (output / report["fragmentFile"]).open("xb") as stream:
        stream.write(fragment)
    INPUTS["EXTERNAL"]["write_report"](output / "dawn-coverage-inventory.json", report)
    return report


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--output-name", required=True)
    args = parser.parse_args()
    source = args.source.absolute()
    result = inspect(source, source / args.output_name)
    print(json.dumps({key: result[key] for key in ("gnPaths", "gnSourceBytes", "fragmentBytes", "fragmentSha256", "completeChromiumNotice")}))
