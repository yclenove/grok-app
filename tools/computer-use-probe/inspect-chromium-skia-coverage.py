"""Bind Skia's real Chromium consumers and Dawn notices, preserving source gaps.

Verify the complete two GN inventories and original four checkout manifests.
Standalone build glue is not the embedded Chromium graph. No conditions, GN,
upstream functions or shipping exclusions are executed or inferred.
"""
import argparse
import hashlib
import json
from pathlib import Path
import re
import runpy


HERE = Path(__file__).parent.absolute()
SUPPLEMENT = runpy.run_path(str(HERE / "inspect-chromium-notice-supplements.py"))
CANDIDATES, INTEGRATION = SUPPLEMENT["CANDIDATES"], SUPPLEMENT["INTEGRATION"]
INPUTS, NODE, SOURCES = CANDIDATES["INPUTS"], CANDIDATES["NODE"], INTEGRATION["SOURCES"]
ROOT = "third_party/skia/"
PROFILES = {
    "skia-gn": ("skia-gn-git-source-01", "dea4f951fb3a70ead4e8227eb8c503341dc337802f2d16e8ead40445f4aba7d2"),
    "skia-embedding": ("skia-embedding-source-01", "1c370fc040c44588ad8d9a390f56ff499c46d188dedf7e641bf808d5fc7256f6"),
}
# Real Chromium dependency paths, never aliases for standalone externals.
DEPENDENCIES = {
    "dawn": ("//third_party/dawn/", ("third_party/dawn/LICENSE",)),
    "wuffs": ("//third_party/wuffs", ("third_party/wuffs/src/LICENSE",)),
    "expat": ("//third_party/expat", ("third_party/expat/src/expat/COPYING",)),
    "fontconfig": ("//third_party/fontconfig", ("third_party/fontconfig/src/COPYING",)),
    "icu": ("//third_party/icu:", ("third_party/icu/LICENSE",)),
    "harfbuzz": ("//third_party/harfbuzz", ("third_party/harfbuzz/src/COPYING",)),
    "zlib": ("//third_party/zlib", ("third_party/zlib/LICENSE",)),
    "libwebp": ("//third_party/libwebp", ("third_party/libwebp/src/COPYING", "third_party/libwebp/src/PATENTS")),
    "vulkan-headers": ("//third_party/vulkan-headers/src:", ("third_party/vulkan-headers/LICENSE.txt",)),
}
MAX_FRAGMENT = 512 * 1024


def gn_inventory(index, prefix):
    return {path: row["gitObjectSha1"] for path, row in index.items()
            if path.startswith(prefix) and row["mode"] in ("100644", "100755")
            and (path.endswith((".gn", ".gni")) or path == prefix + ".gn")}


def source_references(blobs, needle):
    return [{"sourcePath": path, "line": number, "text": line}
            for path, data in sorted(blobs.items()) for number, line in enumerate(data.decode().splitlines(), 1)
            if needle in line]


def configuration(blobs):
    features = blobs["skia/features.gni"].decode()
    expression = ("  skia_use_dawn =\n"
        "      is_mac || is_win ||\n"
        '      (is_android && target_cpu != "x86" && target_cpu != "x64") ||\n'
        "      (is_ios && use_blink) || (is_linux && !is_castos) || is_desktop_android")
    if features.count(expression) != 1:
        raise ValueError("one exact original Chromium Dawn default expression required")
    dawn = source_references({"skia/BUILD.gn": blobs["skia/BUILD.gn"]}, "//third_party/dawn/")
    if not dawn or not any('"//third_party/dawn/src/dawn:proc"' in row["text"] for row in dawn):
        raise ValueError("real original Chromium Skia Dawn consumer rules required")
    return {"dawnDefault": {"sourcePath": "skia/features.gni", "text": expression,
                "line": features[:features.index(expression)].count("\n") + 1},
            "dawnConsumers": dawn, "conditionsEvaluated": False, "effectiveGnMeasured": False,
            "sourceRulesProveBinaryExclusions": False}


def checkout_matches(source, pins):
    checkout = SOURCES["CHECKOUTS"]["inspect"](source)
    if {row["target"] for row in checkout["targets"]} != {"windows-x64", "linux-x64", "mac-intel", "mac-arm64"} or len(checkout["targets"]) != 4:
        raise ValueError("all four unique original checkout manifests required")
    result = []
    for target in checkout["targets"]:
        for key, (path, repository, revision) in pins.items():
            compared = SOURCES["CHECKOUTS"]["compare_pin"]("src/" + path, repository, revision, target["manifest"])
            if compared["status"] != "exact_repository_and_revision_match":
                raise ValueError("real source identity must match every original checkout manifest")
            result.append({"target": target["target"], "sourceGroup": key, **compared})
    return result, checkout["checkoutReceiptSha256"]


def dependency_bindings(inputs, embedded):
    result = []
    for component, (needle, licenses) in DEPENDENCIES.items():
        refs = source_references({"skia/BUILD.gn": embedded["skia/BUILD.gn"]}, needle)
        if not refs:
            raise ValueError("real Chromium Skia consumer reference required: " + component)
        result.append({"component": component, "sourceReferences": refs,
            "licenses": [SUPPLEMENT["source_license"](inputs, path) for path in licenses],
            "standaloneMetadataOverridden": False, "completeComponentNoticeCovered": False})
    return result


def wrapper_notices(blobs, skia_license):
    result = []
    for path, data in sorted(blobs.items()):
        if not path.startswith(ROOT + "third_party/"):
            continue
        match = re.match(r"(?:#[^\n]*\n)+", data.decode())
        if match and all(text in match.group() for text in ("Copyright", "BSD-style license", "LICENSE file")):
            block = match.group()
            result.append({"sourcePath": path, "sourceSha256": hashlib.sha256(data).hexdigest(),
                "originalAttributionText": block, "attributionSha256": hashlib.sha256(block.encode()).hexdigest(),
                "license": skia_license, "scope": "own GN build glue only; not the referenced external library",
                "externalLibraryNoticeCovered": False})
    return result


def retained_skia_errors(errors, index, gn, bindings):
    result = []
    for error in errors:
        directory = error["directory"]
        if not directory.startswith(ROOT):
            continue
        descendants = [(path, row) for path, row in sorted(index.items()) if path.startswith(directory + "/")]
        matched = [row for row in bindings if directory == ROOT + "third_party/" + row["component"]]
        result.append({"originalDirectoryError": error, "sourceTreeEntries": len(descendants),
            "regularFiles": sum(row["mode"] in ("100644", "100755") for _, row in descendants),
            "gitlinks": [{"sourcePath": path, **row} for path, row in descendants if row["mode"] == "160000"],
            "standaloneSourceReferences": source_references({path: data for path, data in gn.items()
                if path.startswith(directory + "/")}, "externals/"), "chromiumConsumerBindings": matched,
            "originalErrorRemoved": False, "wholeDirectoryNoticeCovered": False,
            "referenceAbsenceMeansNotShipped": False})
    return result


def render_fragment(bindings, wrappers, inputs):
    parts = [b"PARTIAL SKIA / DAWN SOURCE NOTICE SUPPLEMENT - NOT FOR DISTRIBUTION\n"
        b"Exact source notices and real Chromium consumer paths; not complete Chromium NOTICE or binary SBOM.\n"]
    unique = {row["sourcePath"]: row for group in bindings for row in group["licenses"]}
    for wrapper in wrappers:
        block = wrapper["originalAttributionText"].encode()
        if hashlib.sha256(block).hexdigest() != wrapper["attributionSha256"]:
            raise ValueError("unchanged original build-glue copyright required")
        parts.append(("\nOwn build glue: " + wrapper["sourcePath"] + "\n").encode() + block)
        unique[wrapper["license"]["sourcePath"]] = wrapper["license"]
    for path, row in sorted(unique.items()):
        checked = SUPPLEMENT["source_license"](inputs, path)
        if checked != row:
            raise ValueError("unchanged full original license binding required")
        parts.extend((("\nOriginal license: " + path + "\n").encode(), inputs["files"][path]["payload"]))
    fragment = b"\n".join(parts)
    if len(fragment) > MAX_FRAGMENT:
        raise ValueError("bounded source notice supplement required")
    return fragment


def inspect(source, output):
    INPUTS["checked_source"](source)
    if output.parent != source or not re.fullmatch(r"skia-coverage-[0-9]{2}", output.name):
        raise ValueError("safe fresh task-owned Skia coverage output required")
    if output.exists() or output.is_symlink():
        raise FileExistsError("never overwrite source coverage evidence")
    inputs, baseline, preview = CANDIDATES["load_notice"](source)
    original, original_sha = INPUTS["receipt"](source / "notice-candidates-02/notice-candidate-inventory.json", SUPPLEMENT["CANDIDATE_SEAL"])
    if original != baseline or INPUTS["read_data"](source / "notice-candidates-02/partial-source-notice.txt", CANDIDATES["MAX_NOTICE"]) != preview:
        raise ValueError("entire original notice audit and preview must reproduce")
    blobs, seals, correspondence = {}, {}, {}
    for profile in PROFILES:
        _, checkout_sha, matched = SOURCES["checked_inputs"](source, profile)
        added, seals[profile] = INTEGRATION["checked_profile"](source, profile, inputs, checkout_sha, matched,
            profiles=PROFILES, git_objects=profile == "skia-gn")
        prefix = ROOT if profile == "skia-gn" else "skia/"
        expected = {row["sourcePath"]: row["gitObjectSha1"] for row in SOURCES["requests"](profile)}
        if gn_inventory(inputs["index"], prefix) != expected or set(added) != set(expected):
            raise ValueError("entire independent original GN tree required")
        blobs.update(added)
        correspondence[profile] = matched
    gn = {path: data for path, data in blobs.items() if path.startswith(ROOT)}
    embedded = {path: data for path, data in blobs.items() if path.startswith("skia/")}
    before = NODE["summarize"](inputs)
    gpu_sha = INPUTS["append_external"](source, "external-license-08", INPUTS["GPU_SHA"],
        INPUTS["EXTERNAL"]["GPU_PINS"], INPUTS["GPU_SEALS"], inputs["index"], inputs["files"], inputs["references"], inputs["components"])
    dawn_correspondence, checkout_sha = checkout_matches(source, INPUTS["EXTERNAL"]["GPU_PINS"])
    after = NODE["summarize"](inputs)
    if any(row not in after["directoryErrors"] for row in before["directoryErrors"]):
        raise ValueError("all original directory errors must remain explicit")
    policy = CANDIDATES["POLICY"]["parse_policy"](inputs["files"]["tools/licenses/licenses.py"]["payload"])
    view = NODE["GeneratedSourceView"](inputs["index"], {path: row["payload"] for path, row in inputs["files"].items()}, policy["VCS_METADATA_DIRS"])
    selection = CANDIDATES["select_candidates"](CANDIDATES["POLICY"]["scan"](view, policy), policy)
    bindings = dependency_bindings(inputs, embedded)
    wrappers = wrapper_notices(gn, SUPPLEMENT["source_license"](inputs, ROOT + "LICENSE"))
    fragment = render_fragment(bindings, wrappers, inputs)
    dawn_readme = inputs["files"]["third_party/dawn/README.chromium"]["payload"]
    declared = re.findall(rb"(?m)^Revision: ([0-9a-f]{40})$", dawn_readme)
    if len(declared) != 1:
        raise ValueError("one original unmodified Dawn README revision field required")
    report = {"scope": "complete Skia GN source inventories and actual Chromium consumer/license bindings only",
        "sourceCommit": INPUTS["TREE"]["COMMIT"], "originalNoticeInventorySha256": original_sha,
        "profileReceiptSha256": seals, "checkoutCorrespondence": correspondence,
        "completeGnInventories": True, "standaloneGnPaths": len(gn), "embeddedGnPaths": len(embedded),
        "gnSourceBytes": sum(map(len, blobs.values())), "configurationEvidence": configuration(embedded),
        "chromiumConsumerBindings": bindings, "ownBuildGlueNotices": wrappers,
        "originalSkiaDirectoryErrors": retained_skia_errors(before["directoryErrors"], inputs["index"], gn, bindings),
        "dawnSourceReceiptSha256": gpu_sha, "dawnCheckoutCorrespondence": dawn_correspondence,
        "checkoutReceiptSha256": checkout_sha, "dawnReadmeRevision": declared[0].decode(),
        "dawnActualCheckoutRevision": INPUTS["EXTERNAL"]["GPU_PINS"]["dawn"][2],
        "dawnReadmeRevisionMatchesCheckout": declared[0].decode() == INPUTS["EXTERNAL"]["GPU_PINS"]["dawn"][2],
        "dawnReadmeBinding": SUPPLEMENT["source_license"](inputs, "third_party/dawn/README.chromium"),
        "beforeDawnSources": before, "afterDawnSources": after, "candidateSelectionAfterDawn": selection,
        "newDawnDirectoryErrors": [row for row in after["directoryErrors"] if row not in before["directoryErrors"]],
        "originalRawErrorsRetained": original["afterWebglSources"],
        "fragmentFile": "skia-dawn-notices.txt", "fragmentBytes": len(fragment), "fragmentSha256": hashlib.sha256(fragment).hexdigest(),
        "sourceFunctionsExecuted": False, "generatorExecuted": False, "conditionsEvaluated": False,
        "effectiveGnMeasured": False, "metadataPathsAliased": False, "metadataValuesNormalized": False,
        "originalErrorsExcluded": False, "shippingExclusionsProven": False, "completeChromiumNotice": False, "promoted": False}
    output.mkdir()
    with (output / report["fragmentFile"]).open("xb") as stream:
        stream.write(fragment)
    INPUTS["EXTERNAL"]["write_report"](output / "skia-coverage-inventory.json", report)
    return report


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--output-name", required=True)
    args = parser.parse_args()
    source = args.source.absolute()
    result = inspect(source, source / args.output_name)
    print(json.dumps({key: result[key] for key in ("standaloneGnPaths", "embeddedGnPaths", "gnSourceBytes",
        "fragmentBytes", "fragmentSha256", "dawnReadmeRevisionMatchesCheckout", "completeChromiumNotice")}))
