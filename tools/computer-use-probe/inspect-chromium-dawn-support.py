"""Bind Dawn's two remaining configured libraries to exact original source notices.

Keep recorded metadata revisions, shipped flags and all original errors. This
supplement is source evidence, not a complete component/browser notice or SBOM.
"""
import argparse
import hashlib
import json
from pathlib import Path
import re
import runpy


HERE = Path(__file__).parent.absolute()
DAWN = runpy.run_path(str(HERE / "inspect-chromium-dawn-coverage.py"))
INPUTS, SOURCES, INTEGRATION, SUPPLEMENT = (DAWN[key] for key in ("INPUTS", "SOURCES", "INTEGRATION", "SUPPLEMENT"))
TEST_SHA = "e871791d9374c2654cf4701cdd8be154a995686b3adb3d8c867d4d525c57a29d"
TEST_COMPONENT_SHA = "2b03eae193ce26efbc95f16b179b1831abcf58fda149e41f90730d2383dcde02"
SUPPORT_SHA = "b06da9050e2dbade6428813e51bf56f86947062b919c6833a7bc207f0cca7960"
MAX_FRAGMENT = 16 * 1024
PA = "base/allocator/partition_allocator/"
PA_SOURCES = (PA + "BUILD.gn", PA + "src/partition_alloc/pointers/raw_ptr.h")
GT_LICENSE = "third_party/googletest/src/LICENSE"
GT_README = "third_party/googletest/README.chromium"


def checked_googletest(source):
    external = INPUTS["EXTERNAL"]
    root = source / "external-license-10"
    collected, _ = INPUTS["receipt"](root / "external-source-inventory.json", TEST_SHA)
    if (collected["reviewedGroup"] != "dawn-tests" or collected["requestedComponents"] != ["googletest"]
            or collected["sourceCommit"] != INPUTS["TREE"]["COMMIT"] or collected["collectionComplete"] is not True
            or len(collected["components"]) != 1
            or any(collected[key] is not False for key in ("conditionsEvaluated", "hooksExecuted", "generatorExecuted", "promoted"))):
        raise ValueError("one original complete source-only Googletest collection required")
    catalog = INPUTS["read_data"](source / external["CATALOG"], INPUTS["TREE"]["MAX_BLOB"])
    external["checked_catalog"](catalog, external["DAWN_TEST_PINS"])
    component, _ = INPUTS["receipt"](root / "googletest/license-source-inventory.json", TEST_COMPONENT_SHA)
    pin = external["DAWN_TEST_PINS"]["googletest"]
    if ((component["checkoutPath"], component["repository"], component["revision"]) != pin
            or component["collectionComplete"] is not True or len(component["selectedFiles"]) != 1
            or component["sourceBytes"] != 1475 or component["declaredCheckoutCondition"] is not None
            or any(component[key] is not False for key in ("conditionsEvaluated", "generatorExecuted", "recursiveDependenciesCollected", "promoted"))):
        raise ValueError("exact actual pin and unmeasured original collection scope required")
    tree = INPUTS["read_data"](root / "googletest/source-tree.nul", INPUTS["TREE"]["MAX_TREE"])
    rows = INPUTS["TREE"]["tree_records"](tree)
    if (len(tree), hashlib.sha256(tree).hexdigest(), len(rows)) != (component["treeBytes"], component["treeSha256"], component["treeEntries"]):
        raise ValueError("entire original Googletest tree identity required")
    selected = component["selectedFiles"][0]
    row = next((row for row in rows if row["path"] == "LICENSE"), None)
    if (selected["path"] != "LICENSE" or selected["checkoutPath"] != GT_LICENSE or row is None
            or any(row[key] != selected[key] for key in ("mode", "kind", "gitObjectSha1")) or row["mode"] != "100644"):
        raise ValueError("original regular license must match complete source tree")
    data = INPUTS["read_data"](root / "googletest/blobs" / (row["gitObjectSha1"] + ".source-data"), INPUTS["TREE"]["MAX_BLOB"])
    identity = INPUTS["TREE"]["check_blob"](row["gitObjectSha1"], data)
    if identity != {key: selected[key] for key in ("bytes", "sha256")}:
        raise ValueError("original full Googletest notice bytes required")
    correspondence, checkout_sha = DAWN["SKIA"]["checkout_matches"](source, external["DAWN_TEST_PINS"])
    return {"sourcePath": GT_LICENSE, "gitObjectSha1": row["gitObjectSha1"], **identity}, data, {
        "collectionReceiptSha256": TEST_SHA, "componentReceiptSha256": TEST_COMPONENT_SHA,
        "actualPin": list(pin), "checkoutReceiptSha256": checkout_sha, "fourManifestMatches": correspondence,
        "treeEntries": len(rows), "treeSha256": component["treeSha256"]}


def metadata_evidence(data, pin):
    lines = data.decode().splitlines()
    fields = {}
    for key in ("Revision", "License", "License File", "Shipped"):
        rows = [(number, line) for number, line in enumerate(lines, 1) if line.startswith(key + ": ")]
        if len(rows) != 1:
            raise ValueError("one original metadata field required")
        fields[key] = {"line": rows[0][0], "text": rows[0][1], "value": rows[0][1][len(key) + 2:]}
    if (fields["Revision"]["value"] != "b2b9072ecbe874f5937054653ef8f2731eb0f010"
            or fields["License"]["value"] != "BSD-3-Clause" or fields["License File"]["value"] != "src/LICENSE"
            or fields["Shipped"]["value"] != "no" or pin != "4fe3307fb2d9f86d19777c7eb0e4809e9694dde7"):
        raise ValueError("exact unchanged metadata and actual distinct DEPS revision required")
    return {"sourcePath": GT_README, "originalFields": fields, "actualDepsRevision": pin,
        "metadataRevisionDiffersFromActualDeps": True, "metadataRewritten": False, "shippingExclusionProven": False}


def partition_attribution(blobs):
    result = []
    for path, prefix in zip(PA_SOURCES, (b"# Copyright 2023", b"// Copyright 2020")):
        data = blobs[path]
        original = b"\n".join(data.splitlines()[:3])
        if (not original.startswith(prefix + b" The Chromium Authors")
                or b"Use of this source code is governed by a BSD-style license" not in original
                or b"found in the LICENSE file." not in original):
            raise ValueError("both exact original BSD attribution headers required")
        result.append({"sourcePath": path, "sourceSha256": hashlib.sha256(data).hexdigest(),
            "originalAttribution": original.decode(), "resolvedLicenseSource": "LICENSE"})
    return result


def render_fragment(googletest, chromium, attributions):
    if not isinstance(googletest, bytes) or not googletest or not isinstance(chromium, bytes) or not chromium or len(attributions) != 2:
        raise ValueError("both original full notices and attribution headers required")
    parts = [b"PARTIAL DAWN SUPPORT SOURCE NOTICE SUPPLEMENT - NOT FOR DISTRIBUTION\n"
        b"Configured source licenses only; not full browser/component NOTICE or binary SBOM.\n",
        ("\nOriginal license source: " + GT_LICENSE + "\n").encode(), googletest,
        b"\nOriginal PartitionAlloc source attribution:\n"]
    for row in attributions:
        parts.extend((("\nOriginal source: " + row["sourcePath"] + "\n").encode(), row["originalAttribution"].encode()))
    parts.extend((b"\nOriginal license source: LICENSE\n", chromium))
    fragment = b"\n".join(parts)
    if len(fragment) > MAX_FRAGMENT:
        raise ValueError("bounded complete notice fragment required")
    return fragment


def inspect(source, output):
    INPUTS["checked_source"](source)
    if output.parent != source or not re.fullmatch(r"dawn-support-coverage-[0-9]{2}", output.name):
        raise ValueError("safe fresh task-owned support output required")
    if output.exists() or output.is_symlink():
        raise FileExistsError("never overwrite support evidence")
    inputs, dawn, _ = DAWN["load_dawn"](source)
    _, checkout_sha, correspondence = SOURCES["checked_inputs"](source, "dawn-support")
    blobs, profile_sha = INTEGRATION["checked_profile"](source, "dawn-support", inputs, checkout_sha, correspondence,
        profiles={"dawn-support": ("dawn-support-source-01", SUPPORT_SHA)})
    attribution = partition_attribution(blobs)
    googletest, gt_text, gt_source = checked_googletest(source)
    original_readme = inputs["files"][GT_README]["payload"]
    metadata = metadata_evidence(original_readme, gt_source["actualPin"][2])
    chromium = SUPPLEMENT["source_license"](inputs, "LICENSE")
    bindings = []
    for variable, directory, license_row in (
            ("dawn_googletest_dir", "//third_party/googletest/src", googletest),
            ("dawn_partition_alloc_dir", "//base/allocator/partition_allocator", chromium)):
        if dawn["configurationEvidence"]["declaredDirectories"].get(variable) != directory:
            raise ValueError("original exact configured directory required")
        bindings.append({"variable": variable, "declaredDirectory": directory, "license": license_row,
            "completeComponentNoticeCovered": False, "binaryUseProven": False})
    fragment = render_fragment(gt_text, inputs["files"]["LICENSE"]["payload"], attribution)
    report = {"scope": "two exact configured source-license bindings only; original metadata and errors retained",
        "sourceCommit": INPUTS["TREE"]["COMMIT"], "originalDawnErrorsRetained": dawn["after"],
        "configurationEvidence": dawn["configurationEvidence"], "newConfiguredSourceLicenseBindings": bindings,
        "originalConfiguredBindingsRetained": dawn["configuredSourceLicenseBindings"],
        "configuredDirectoryBindings": len(dawn["configuredSourceLicenseBindings"]) + len(bindings),
        "googletestSource": gt_source, "googletestOriginalMetadata": metadata,
        "partitionSourceReceiptSha256": profile_sha, "partitionFourManifestCorrespondence": correspondence,
        "partitionOriginalAttribution": attribution, "additionalComponentNoticesUnproven": [
            "PartitionAlloc transitive components are not exhaustively audited by these two attribution samples",
            "Googletest test-only uses are source evidence, not proof of binary exclusion"],
        "fragmentFile": "dawn-support-notices.txt", "fragmentBytes": len(fragment),
        "fragmentSha256": hashlib.sha256(fragment).hexdigest(),
        "sourceFunctionsExecuted": False, "conditionsEvaluated": False, "generatorExecuted": False,
        "metadataPathsAliased": False, "originalErrorsRemoved": False, "shippingExclusionsProven": False,
        "completeChromiumNotice": False, "binarySbom": False, "releaseReady": False, "promoted": False}
    output.mkdir()
    with (output / report["fragmentFile"]).open("xb") as stream:
        stream.write(fragment)
    INPUTS["EXTERNAL"]["write_report"](output / "dawn-support-inventory.json", report)
    return report


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--output-name", required=True)
    args = parser.parse_args()
    source = args.source.absolute()
    result = inspect(source, source / args.output_name)
    print(json.dumps({key: result[key] for key in ("configuredDirectoryBindings", "fragmentBytes", "fragmentSha256", "releaseReady")}))
