"""Bind three fixed ANGLE library license sources without binary exclusions.

Keep all five collected notice texts, including the CPU library's BSD appendix
and two separately identified test-image notices. Nothing upstream executes.
"""
import argparse
import hashlib
import json
from pathlib import Path
import re
import runpy


HERE = Path(__file__).parent.absolute()
REGISTRY = runpy.run_path(str(HERE / "collect-chromium-angle-registry-sources.py"))
INPUTS, EXTERNAL = REGISTRY["INPUTS"], REGISTRY["EXTERNAL"]
SOURCE = "angle-remaining-notice-source-01"
SOURCE_SHA = "9f04f6f5c6ced5d32bf87c1990bee5c32b30876391d58aa94769e2a440f75bfa"
ASSEMBLY_SHA = "e9038e3ee26ebcecea2098feb6e85360a9012d399ee736a21a66b523358d03b1"
PINS = {
    "opencl-icd-loader": ("third_party/angle/third_party/OpenCL-ICD-Loader/src",
        "https://chromium.googlesource.com/external/github.com/KhronosGroup/OpenCL-ICD-Loader",
        "ddf6c70230a79cdb8fcccfd3c775b09e6820f42e"),
    "astc-encoder": ("third_party/angle/third_party/astc-encoder/src",
        "https://chromium.googlesource.com/external/github.com/ARM-software/astc-encoder",
        "2319d9c4d4af53a7fc7c52985e264ce6e8a02a9b"),
    "cpu-features": ("third_party/angle/third_party/cpu_features/src",
        "https://chromium.googlesource.com/external/github.com/google/cpu_features.git",
        "81d13c49649f0714dd41fb56bb246398b6584085"),
}
SEALS = {"opencl-icd-loader": "a739a508bae765d75cfdedb818917f0711bb4479d5c7047e1e283d4d058599df",
    "astc-encoder": "cef787f847f2280b1b10b548eadacfe1fa9b130122a2c9182316138b764dfd63",
    "cpu-features": "4936a2297a86c5333f7dff94b3d690dcdd7c8b8abb77cfd9a085e1c85831f658"}
LICENSES = {"opencl-icd-loader": "LICENSE", "astc-encoder": "LICENSE.txt", "cpu-features": "LICENSE"}
TEST_IMAGES = {"Test/Images/HDRIHaven/LICENSE.txt", "Test/Images/Khronos/LICENSE.txt"}
MAX_FRAGMENT = 128 * 1024


def checked_metadata(key, declaration, metadata):
    prefix, repository, revision = PINS[key]
    if (declaration["path"] != prefix[len("third_party/angle/"):]
            or (declaration["repository"], declaration["revision"], declaration["gitlinkRevision"]) != (repository, revision, revision)
            or declaration["gitlinkMatched"] is not True
            or metadata["directory"] != prefix[:-4] or metadata["shippedMetadata"] != "yes"
            or metadata["declaredLicensePaths"] != [prefix + "/" + LICENSES[key]] or not metadata["errors"]):
        raise ValueError("exact original library declaration and unresolved metadata required")


def checked_component(source, key, summary, references):
    root = source / SOURCE / key
    report, sha = INPUTS["receipt"](root / "license-source-inventory.json", SEALS[key])
    if (summary["receiptSha256"] != sha or report["component"] != key or report["collectionComplete"] is not True
            or (report["checkoutPath"], report["repository"], report["revision"]) != PINS[key]
            or report["recursiveDependenciesCollected"] is not False or report["generatorExecuted"] is not False
            or report["licenseCoverage"] != "not_proven" or report["promoted"] is not False):
        raise ValueError("exact complete unpromoted library source receipt required")
    tree = INPUTS["read_data"](root / "source-tree.nul", INPUTS["TREE"]["MAX_TREE"])
    rows = INPUTS["TREE"]["tree_records"](tree)
    if (len(tree), hashlib.sha256(tree).hexdigest(), len(rows)) != (report["treeBytes"], report["treeSha256"], report["treeEntries"]):
        raise ValueError("complete unchanged library source tree required")
    selected, refs, extra, missing = EXTERNAL["selected_inputs"](key, rows, references, PINS)
    if (report["parentLicenseReferences"] != refs or report["reviewedExtraPaths"] != extra
            or report["missingParentOrExtraPaths"] != missing or missing or len(selected) != len(report["selectedFiles"])):
        raise ValueError("entire original selected library notice inventory required")
    notices, unique = [], {}
    for row, recorded in zip(selected, report["selectedFiles"]):
        if any(recorded.get(field) != value for field, value in row.items()) or recorded["checkoutPath"] != PINS[key][0] + "/" + row["path"]:
            raise ValueError("exact original library path and object required")
        oid = row["gitObjectSha1"]
        data = INPUTS["read_data"](root / "blobs" / (oid + ".source-data"), INPUTS["TREE"]["MAX_BLOB"])
        if INPUTS["TREE"]["check_blob"](oid, data) != {field: recorded[field] for field in ("bytes", "sha256")}:
            raise ValueError("complete original notice bytes and Git/SHA256 identity required")
        data.decode("utf-8", errors="strict")
        notices.append(({"component": key, "sourcePath": recorded["checkoutPath"], "gitObjectSha1": oid,
            "bytes": recorded["bytes"], "sha256": recorded["sha256"],
            "role": "declared_library_license" if row["path"] == LICENSES[key] else "test_image_license",
            "binaryUseProven": False}, data))
        unique[oid] = len(data)
    expected_paths = {LICENSES[key]} | (TEST_IMAGES if key == "astc-encoder" else set())
    if ({row["path"] for row in selected} != expected_paths or len(unique) != report["uniqueBlobs"]
            or sum(unique.values()) != report["sourceBytes"]
            or (summary["treeEntries"], summary["selectedFiles"], summary["sourceBytes"]) != (len(rows), len(selected), sum(unique.values()))):
        raise ValueError("all five original library and test-image notices required")
    license_rows = [row for row in report["selectedFiles"] if row["path"] == LICENSES[key]]
    if summary["license"] != license_rows[0]:
        raise ValueError("exact original missing-license binding required")
    return notices


def render_fragment(notices):
    library_paths = {prefix + "/" + LICENSES[key] for key, (prefix, _, _) in PINS.items()}
    expected = set(library_paths)
    expected.update(PINS["astc-encoder"][0] + "/" + path for path in TEST_IMAGES)
    if len(notices) != 5 or {row["sourcePath"] for row, _ in notices} != expected:
        raise ValueError("all five distinct original notice paths required")
    parts = [b"PARTIAL ANGLE LIBRARY SOURCE NOTICE SUPPLEMENT - NOT FOR DISTRIBUTION\n"
        b"Declared source licenses plus separately labeled test-image notices; binary use is not proven.\n"]
    for row, data in notices:
        role = "declared_library_license" if row["sourcePath"] in library_paths else "test_image_license"
        if (row["role"] != role or not isinstance(data, bytes) or not data
                or (len(data), hashlib.sha256(data).hexdigest()) != (row["bytes"], row["sha256"])):
            raise ValueError("entire original permission text required")
        parts.extend((("\nOriginal source: " + row["sourcePath"] + "\nRole: " + row["role"] + "\n").encode(), data))
    fragment = b"\n".join(parts)
    if len(fragment) > MAX_FRAGMENT:
        raise ValueError("bounded complete library notice supplement required")
    return fragment


def inspect(source, output):
    INPUTS["checked_source"](source)
    if output.parent != source or not re.fullmatch(r"angle-library-notices-[0-9]{2}", output.name):
        raise ValueError("safe fresh task-owned output required")
    if output.exists() or output.is_symlink():
        raise FileExistsError("never overwrite source-notice evidence")
    inputs, checkout_sha, _, _ = REGISTRY["checked_inputs"](source)
    collected, collected_sha = INPUTS["receipt"](source / SOURCE / "angle-remaining-source-inventory.json", SOURCE_SHA)
    original, original_sha = INPUTS["receipt"](source / "notice-assembly-05/notice-assembly-inventory.json", ASSEMBLY_SHA)
    catalog, catalog_sha = INPUTS["receipt"](source / "recursive-deps-angle-02.json", REGISTRY["GENERATION"]["CATALOG_SHA"])
    if (collected["sourceCommit"] != INPUTS["TREE"]["COMMIT"] or collected["mainReceiptSha256"] != inputs["mainSha256"]
            or collected["checkoutReceiptSha256"] != checkout_sha or collected["originalAssemblyReceiptSha256"] != original_sha
            or collected["originalCatalogReceiptSha256"] != catalog_sha or collected["collectionComplete"] is not True
            or len(collected["components"]) != 3 or {row["component"] for row in collected["components"]} != set(PINS)):
        raise ValueError("all three exact source declarations and original receipt context required")
    declarations = {row["path"]: row for row in catalog["gitDependencies"]}
    omissions = {row["originalOmittedMetadata"]["directory"]: row["originalOmittedMetadata"]
        for row in original["releaseBlockers"]["unresolvedOriginalMetadata"]}
    notices, bindings = [], []
    for summary in collected["components"]:
        key = summary["component"]
        declaration = declarations[PINS[key][0][len("third_party/angle/"):]]
        metadata = omissions[PINS[key][0][:-4]]
        checked_metadata(key, declaration, metadata)
        if summary["declaration"] != declaration or summary["originalOmittedMetadata"] != metadata:
            raise ValueError("original complete declaration and metadata must remain unchanged")
        selected = checked_component(source, key, summary, inputs["references"])
        notices.extend(selected)
        bindings.append({"originalOmittedMetadata": metadata, "declaration": declaration,
            "sourceReceiptSha256": summary["receiptSha256"],
            "license": next(row for row, _ in selected if row["role"] == "declared_library_license"),
            "completeComponentNoticeCovered": False, "binaryUseProven": False, "binaryExclusionProven": False})
    fragment = render_fragment(notices)
    report = {"scope": "three exact declared library license bindings only, not complete browser notices or binary provenance",
        "sourceCommit": INPUTS["TREE"]["COMMIT"], "sourceCollectionReceiptSha256": collected_sha,
        "originalAssemblyReceiptSha256": original_sha, "originalCatalogReceiptSha256": catalog_sha,
        "sourceLicenseBindings": bindings, "selectedSourceNotices": [row for row, _ in notices],
        "originalMetadataErrorsRetained": original["metadataErrorDispositions"],
        "remainingOriginalSourceMetadata": [row for row in original["releaseBlockers"]["unresolvedOriginalMetadata"]
            if row["originalOmittedMetadata"]["directory"] not in {item["originalOmittedMetadata"]["directory"] for item in bindings}],
        "fragmentFile": "angle-library-notices.txt", "fragmentBytes": len(fragment), "fragmentSha256": hashlib.sha256(fragment).hexdigest(),
        "sourceFunctionsExecuted": False, "conditionsExecuted": False, "generatorExecuted": False,
        "originalErrorsRemoved": False, "metadataPathsAliased": False, "shippingExclusionsProven": False,
        "completeChromiumNotice": False, "binarySbom": False, "releaseReady": False, "promoted": False}
    output.mkdir()
    with (output / report["fragmentFile"]).open("xb") as stream:
        stream.write(fragment)
    EXTERNAL["write_report"](output / "angle-library-notice-inventory.json", report)
    return report


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--output-name", required=True)
    args = parser.parse_args()
    source = args.source.absolute()
    report = inspect(source, source / args.output_name)
    print(json.dumps({"bindings": len(report["sourceLicenseBindings"]), "fragmentBytes": report["fragmentBytes"],
        "fragmentSha256": report["fragmentSha256"], "releaseReady": False}))
