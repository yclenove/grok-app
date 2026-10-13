"""Assemble checked source notices and supplements; incomplete exports exit 2.

This exports a review candidate, never a distributable NOTICE or binary SBOM.
Reverify the current source view and keep every original metadata error. No
upstream code, GN, conditions, browser or product runtime executes or changes.
"""
import argparse
from collections import Counter
import hashlib
import json
from pathlib import Path
import re
import runpy
import sys


HERE = Path(__file__).parent.absolute()
DAWN = runpy.run_path(str(HERE / "inspect-chromium-dawn-coverage.py"))
INPUTS, NODE = DAWN["INPUTS"], DAWN["NODE"]
CANDIDATES = DAWN["SKIA"]["CANDIDATES"]
MAX_NOTICE = CANDIDATES["MAX_NOTICE"]
DAWN_SHA = "710e626fab93c08965efd7f0f9d83309530396a97c34bd534f5e5e9bb0d7c020"
SUPPLEMENTS = {
    "copied-api": ("notice-supplements-02", "notice-supplement-inventory.json",
        "10e3b61045f3aa88a6ebd90a30f2947fb19d872d4ab0250898a72bb6db2b5574", "copied-api-header-notices.txt"),
    "swiftshader": ("swiftshader-notices-01", "swiftshader-notice-inventory.json",
        "2bf2c81272f462813e9740a28f616c6cecd57f939ea1620a45a3eee9cd3634a8", "swiftshader-copied-notices.txt"),
    "swiftshader-inline": ("swiftshader-copied-notices-01", "swiftshader-copied-inventory.json",
        "ed9429e906d5236dd1282bf79886d5835fd78e5ff644cf53bc1d6d41abe61f36", "swiftshader-special-inline-notices.txt"),
    "skia-dawn": ("skia-coverage-01", "skia-coverage-inventory.json",
        DAWN["SKIA_SEAL"], "skia-dawn-notices.txt"),
    "dawn-configured": ("dawn-coverage-01", "dawn-coverage-inventory.json", DAWN_SHA, "dawn-configured-notices.txt"),
}
DAWN_SUPPORT_SUPPLEMENTS = {**SUPPLEMENTS,
    "dawn-support": ("dawn-support-coverage-01", "dawn-support-inventory.json",
        "1ef5ef60896fabc329cc53c85059d97a088e4ff45372ef65ddfde0c93361cfa3", "dawn-support-notices.txt")}
REGISTRY_SUPPLEMENTS = {**DAWN_SUPPORT_SUPPLEMENTS,
    "registry-xml": ("angle-registry-coverage-01", "angle-registry-coverage-inventory.json",
        "ee5b2586a193a8ba794677b3e44b3d896dc30f0e250cf97f1e2d8e9632855207", "angle-registry-xml-notices.txt")}
CURRENT_SUPPLEMENTS = {**REGISTRY_SUPPLEMENTS,
    "angle-libraries": ("angle-library-notices-01", "angle-library-notice-inventory.json",
        "7330869ea14057472603f93f8502d646ebe4186a0c9c07f4eecdb41117155505", "angle-library-notices.txt")}
SOURCE_ONLY_FLAGS = ("sourceFunctionsExecuted", "shippingExclusionsProven", "completeChromiumNotice", "promoted")
DAWN_LOADER = "third_party/dawn/third_party/vulkan-loader"
REGISTRY_XML_PATHS = {"third_party/angle/third_party/" + name + "/src/" + path
    for name, paths in (("EGL-Registry", ("api/egl.xml",)), ("OpenCL-Docs", ("xml/cl.xml",)),
        ("OpenGL-Registry", ("xml/gl.xml", "xml/glx.xml", "xml/wgl.xml"))) for path in paths}
ANGLE_LIBRARY_BINDINGS = {
    "third_party/angle/third_party/OpenCL-ICD-Loader": ("src/LICENSE", "a739a508bae765d75cfdedb818917f0711bb4479d5c7047e1e283d4d058599df"),
    "third_party/angle/third_party/astc-encoder": ("src/LICENSE.txt", "cef787f847f2280b1b10b548eadacfe1fa9b130122a2c9182316138b764dfd63"),
    "third_party/angle/third_party/cpu_features": ("src/LICENSE", "4936a2297a86c5333f7dff94b3d690dcdd7c8b8abb77cfd9a085e1c85831f658"),
}


def checked_fragment(source, spec):
    directory, name, seal, filename = spec
    report, sha = INPUTS["receipt"](source / directory / name, seal)
    if (report.get("sourceCommit") != INPUTS["TREE"]["COMMIT"]
            or any(report.get(key) is not False for key in SOURCE_ONLY_FLAGS)
            or report.get("fragmentFile") != filename):
        raise ValueError("original unpromoted source-notice receipt required")
    data = INPUTS["read_data"](source / directory / filename, MAX_NOTICE)
    if not data or (len(data), hashlib.sha256(data).hexdigest()) != (report.get("fragmentBytes"), report.get("fragmentSha256")):
        raise ValueError("complete unchanged source-notice fragment required")
    return report, data, {"receiptFile": directory + "/" + name, "receiptSha256": sha,
        "fragmentFile": directory + "/" + filename, "bytes": len(data), "sha256": hashlib.sha256(data).hexdigest()}


def metadata_dispositions(current, copied_api, dawn):
    original = copied_api["omittedMetadataDispositions"]
    by_directory = {row["originalOmittedMetadata"]["directory"]: row for row in original}
    if len(original) != 17 or len(by_directory) != 17 or len(current) != 18 or len({row["directory"] for row in current}) != 18:
        raise ValueError("all eighteen distinct original shipping-metadata errors required")
    if {row["directory"] for row in current} != set(by_directory) | {DAWN_LOADER}:
        raise ValueError("new Dawn metadata and all prior omissions must remain explicit")
    result = []
    for row in current:
        if row["directory"] in by_directory:
            previous = by_directory[row["directory"]]
            if previous["originalOmittedMetadata"] != row:
                raise ValueError("original metadata values and errors may not be rewritten")
            result.append(previous)
        else:
            bindings = [item for item in dawn["configuredSourceLicenseBindings"] if item["variable"] == "dawn_vulkan_loader_dir"]
            if (len(bindings) != 1 or bindings[0]["declaredDirectory"] != "//third_party/vulkan-loader/src"
                    or bindings[0]["license"]["sourcePath"] != "third_party/vulkan-loader/src/LICENSE.txt"
                    or dawn["configurationEvidence"]["declaredDirectories"].get("dawn_vulkan_loader_dir") != "//third_party/vulkan-loader/src"):
                raise ValueError("exact real Chromium Dawn loader binding required")
            result.append({"originalOmittedMetadata": row, "disposition": "configured_source_license_bound",
                "sourceEvidence": bindings[0], "originalErrorRemoved": False, "binaryExclusionProven": False})
    return result


def support_bindings(support, dawn):
    expected = {"dawn_googletest_dir": ("//third_party/googletest/src", "third_party/googletest/src/LICENSE"),
        "dawn_partition_alloc_dir": ("//base/allocator/partition_allocator", "LICENSE")}
    rows = support["newConfiguredSourceLicenseBindings"]
    if (len(rows) != 2 or {row["variable"] for row in rows} != set(expected)
            or support["originalDawnErrorsRetained"] != dawn["after"]
            or support["configurationEvidence"] != dawn["configurationEvidence"]
            or support["originalConfiguredBindingsRetained"] != dawn["configuredSourceLicenseBindings"]
            or support["configuredDirectoryBindings"] != 17):
        raise ValueError("both exact source bindings and all original Dawn evidence required")
    for row in rows:
        directory, path = expected[row["variable"]]
        if (row["declaredDirectory"] != directory or row["license"]["sourcePath"] != path
                or dawn["configurationEvidence"]["declaredDirectories"].get(row["variable"]) != directory
                or row["completeComponentNoticeCovered"] is not False or row["binaryUseProven"] is not False):
            raise ValueError("original configured license location and unmeasured use required")
    return rows


def registry_evidence(registry, copied_api, dawn):
    notices, matches = registry["xmlNotices"], registry["xmlGenerationMatches"]
    headers = registry["copiedHeaderComparisons"]
    original_headers = {row["sourcePath"]: row["sourceSha256"] for row in copied_api["selectedHeaders"]}
    if (registry["originalRawErrorsRetained"] != dawn["after"]
            or registry["copiedHeaderSourceReceiptSha256"] != copied_api["headerSourceReceiptSha256"]
            or len(notices) != 5 or {row["sourcePath"] for row in notices} != REGISTRY_XML_PATHS
            or len(matches) != 5 or {row["sourcePath"] for row in matches} != REGISTRY_XML_PATHS
            or any(row["matchesRecordedGenerationInput"] is not True for row in matches)
            or len(headers) != len(original_headers) or len({row["copiedSourcePath"] for row in headers}) != len(headers)
            or {row["copiedSourcePath"]: row["sourceSha256"] for row in headers} != original_headers
            or any(registry[key] is not False for key in ("completeHeaderProvenance", "releaseReady", "completeChromiumNotice",
                "binarySbom", "promoted", "originalErrorsRemoved", "shippingExclusionsProven"))):
        raise ValueError("all five XML links, original copied headers/errors and incomplete source scope required")
    counts = dict(sorted(Counter(row["status"] for row in headers).items()))
    if counts != registry["copiedHeaderComparisonCounts"]:
        raise ValueError("all original identical and unmatched copied-header comparisons required")
    return {"registryCollectionReceiptSha256": registry["registryCollectionReceiptSha256"],
        "generationReceiptSha256": registry["generationReceiptSha256"], "xmlNotices": notices,
        "xmlGenerationMatches": matches, "copiedHeaderComparisonCounts": counts,
        "completeHeaderProvenance": False, "binaryUseProven": False}


def angle_library_dispositions(original, libraries):
    rows = libraries["sourceLicenseBindings"]
    by_directory = {row["originalOmittedMetadata"]["directory"]: row for row in rows}
    original_by_directory = {row["originalOmittedMetadata"]["directory"]: row for row in original}
    if (libraries["originalMetadataErrorsRetained"] != original
            or libraries["originalAssemblyReceiptSha256"] != "e9038e3ee26ebcecea2098feb6e85360a9012d399ee736a21a66b523358d03b1"
            or len(rows) != 3 or set(by_directory) != set(ANGLE_LIBRARY_BINDINGS)
            or len(original) != 18 or len(original_by_directory) != 18):
        raise ValueError("all three original library bindings and all eighteen raw errors required")
    result = []
    for previous in original:
        directory = previous["originalOmittedMetadata"]["directory"]
        if directory not in by_directory:
            result.append(previous)
            continue
        binding = by_directory[directory]
        relative, receipt_sha = ANGLE_LIBRARY_BINDINGS[directory]
        if (binding["originalOmittedMetadata"] != previous["originalOmittedMetadata"]
                or previous["disposition"] != "unresolved_source_or_shipping_obligation"
                or previous["originalOmittedMetadata"]["declaredLicensePaths"] != [directory + "/" + relative]
                or binding["license"]["sourcePath"] != directory + "/" + relative
                or binding["license"]["role"] != "declared_library_license"
                or binding["sourceReceiptSha256"] != receipt_sha
                or any(binding[key] is not False for key in ("completeComponentNoticeCovered", "binaryUseProven", "binaryExclusionProven"))):
            raise ValueError("exact declared license binding cannot become component or binary approval")
        result.append({**previous, "disposition": "declared_source_license_bound", "previousDisposition": previous["disposition"],
            "sourceEvidence": binding})
    remaining = [row for row in result if row["disposition"] == "unresolved_source_or_shipping_obligation"]
    if libraries["remainingOriginalSourceMetadata"] != remaining or len(remaining) != 1:
        raise ValueError("original remaining SDK/source obligation required")
    return result


def render_candidate(base, supplements, reviewed=SUPPLEMENTS):
    if not any(reviewed is group for group in (SUPPLEMENTS, DAWN_SUPPORT_SUPPLEMENTS, REGISTRY_SUPPLEMENTS, CURRENT_SUPPLEMENTS)):
        raise ValueError("only original or current compiled notice supplements allowed")
    if not base or not supplements or set(supplements) != set(reviewed):
        raise ValueError("base source notices and every reviewed supplement required")
    parts = [b"MAINTAINED BROWSER SOURCE NOTICE CANDIDATE - NOT FOR DISTRIBUTION\n"
        b"Includes default source notices and reviewed supplements. See inventory for all open obligations.\n\n", base]
    total = sum(map(len, parts))
    for key in reviewed:
        data = supplements.get(key)
        if not isinstance(data, bytes) or not data:
            raise ValueError("complete reviewed supplement required")
        header = ("\nSOURCE NOTICE SUPPLEMENT: " + key + "\n").encode()
        total += len(header) + len(data) + 2
        if total > MAX_NOTICE:
            raise ValueError("bounded combined notice candidate required")
        parts.extend((header, data))
    candidate = b"\n".join(parts)
    if len(candidate) > MAX_NOTICE:
        raise ValueError("combined separators must fit notice budget")
    return candidate


def assemble(source, output):
    INPUTS["checked_source"](source)
    if output.parent != source or not re.fullmatch(r"notice-assembly-[0-9]{2}", output.name):
        raise ValueError("safe fresh task-owned notice assembly output required")
    if output.exists() or output.is_symlink():
        raise FileExistsError("never overwrite notice assembly evidence")
    inputs, dawn, dawn_text = DAWN["load_dawn"](source)
    reports, fragments, evidence = {}, {}, {}
    for key, spec in CURRENT_SUPPLEMENTS.items():
        reports[key], fragments[key], evidence[key] = checked_fragment(source, spec)
    if reports["dawn-configured"] != dawn or fragments["dawn-configured"] != dawn_text:
        raise ValueError("entire current Dawn evidence must reproduce, not only fragment hashes")
    support = support_bindings(reports["dawn-support"], dawn)
    registry = registry_evidence(reports["registry-xml"], reports["copied-api"], dawn)
    policy = NODE["POLICY"]["parse_policy"](inputs["files"]["tools/licenses/licenses.py"]["payload"])
    view = NODE["GeneratedSourceView"](inputs["index"], {path: row["payload"] for path, row in inputs["files"].items()}, policy["VCS_METADATA_DIRS"])
    selected = CANDIDATES["select_candidates"](NODE["POLICY"]["scan"](view, policy), policy)
    base, bindings = CANDIDATES["render_preview"](selected, inputs)
    original_dispositions = metadata_dispositions(dawn["after"]["shippingMetadataEntriesWithErrors"], reports["copied-api"], dawn)
    dispositions = angle_library_dispositions(original_dispositions, reports["angle-libraries"])
    candidate = render_candidate(base, fragments, CURRENT_SUPPLEMENTS)
    blockers = {
        "unresolvedOriginalMetadata": [row for row in dispositions if row["disposition"] == "unresolved_source_or_shipping_obligation"],
        "copiedRegistryProvenance": [row for row in dispositions if row["disposition"] == "copied_header_notices_bound_registry_checkout_not_proven"],
        "dawnConfiguredLibraries": {},
        "dawnRemainingSourceObligations": reports["dawn-support"]["additionalComponentNoticesUnproven"],
        "registryRemainingSourceObligations": reports["registry-xml"]["remainingSourceObligations"],
        "swiftshaderSourceObligations": reports["swiftshader-inline"]["remainingObligations"],
        "nonstandardMetadataValues": selected["nonstandardShippingValues"],
        "completeCopiedGeneratedSourceCoverage": "not_proven",
        "fourPlatformBinaryCorrespondenceAndSbom": "not_proven",
        "sourceOfferMediaAndSecurityPolicy": "not_complete",
    }
    report = {"scope": "unified reviewed-source notice candidate, not binary SBOM or release approval",
        "sourceCommit": INPUTS["TREE"]["COMMIT"], "mainReceiptSha256": inputs["mainSha256"],
        "originalDawnReceiptSha256": DAWN_SHA, "supplementEvidence": evidence,
        "dawnAdditionalConfiguredSourceLicenseBindings": support,
        "registryXmlSourceEvidence": registry,
        "candidateSelection": selected, "sourceNoticeBindings": bindings,
        "metadataErrorDispositions": dispositions,
        "originalMetadataErrorDispositions": original_dispositions,
        "originalDispositionCounts": dict(sorted(Counter(row["disposition"] for row in original_dispositions).items())),
        "dispositionCounts": dict(sorted(Counter(row["disposition"] for row in dispositions).items())),
        "originalRawErrorsRetained": dawn["after"], "releaseBlockers": blockers,
        "candidateFile": "maintained-browser-notice-candidate.txt", "candidateBytes": len(candidate),
        "candidateSha256": hashlib.sha256(candidate).hexdigest(),
        "basePreviewBytes": len(base), "basePreviewSha256": hashlib.sha256(base).hexdigest(),
        "sourceFunctionsExecuted": False, "generatorExecuted": False, "conditionsEvaluated": False,
        "metadataPathsAliased": False, "originalErrorsRemoved": False, "shippingExclusionsProven": False,
        "completeChromiumNotice": False, "binarySbom": False, "releaseReady": False, "promoted": False}
    output.mkdir()
    with (output / report["candidateFile"]).open("xb") as stream:
        stream.write(candidate)
    INPUTS["EXTERNAL"]["write_report"](output / "notice-assembly-inventory.json", report)
    return report


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--output-name", required=True)
    args = parser.parse_args()
    source = args.source.absolute()
    result = assemble(source, source / args.output_name)
    print(json.dumps({"candidateBytes": result["candidateBytes"], "candidateSha256": result["candidateSha256"],
        "defaultCandidates": len(result["candidateSelection"]["generatorCandidates"]),
        "dispositionCounts": result["dispositionCounts"], "releaseReady": result["releaseReady"]}))
    sys.exit(0 if result["releaseReady"] else 2)
