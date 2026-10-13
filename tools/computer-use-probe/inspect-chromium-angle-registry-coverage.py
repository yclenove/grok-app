"""Bind fixed registry XML and copied-header matches to their original notices.

Every selected original blob is reverified. The XML is inspected as text, not
parsed or executed. Generation hashes and identical header blobs prove only
their stated source relationships, never binary inclusion or full provenance.
"""
import argparse
from collections import Counter
import hashlib
import json
from pathlib import Path
import re
import runpy


HERE = Path(__file__).parent.absolute()
COLLECTOR = runpy.run_path(str(HERE / "collect-chromium-angle-registry-sources.py"))
SUPPLEMENT = runpy.run_path(str(HERE / "inspect-chromium-notice-supplements.py"))
INPUTS, EXTERNAL, GENERATION = (COLLECTOR[key] for key in ("INPUTS", "EXTERNAL", "GENERATION"))
ROOT = "angle-registry-license-source-01"
ROOT_SHA = "eba7ef976e55ff25faa19e415a00c55889925ab19cc56468dd2bcbc3a5a9bacd"
COMPONENT_SEALS = {
    "egl-registry": "26f89dda567b019766a8566cd304df7c74e77b20c4830aac2f20395ffd477ff4",
    "opencl-docs": "1917741e230fe57408ddd84e450f451a1d11ecc942231c8776bab5f6fc8052fe",
    "opengl-registry": "41e0d71bbf1a9041c3a81970af0e5522e5af2cec18900c36601c8449d5825681",
}
MAX_FRAGMENT = 64 * 1024


def checked_component(source, key):
    root = source / ROOT / key
    report, sha = INPUTS["receipt"](root / "license-source-inventory.json", COMPONENT_SEALS[key])
    summary = COLLECTOR["component_summary"](key, report, sha)
    tree_data = INPUTS["read_data"](root / "source-tree.nul", INPUTS["TREE"]["MAX_TREE"])
    rows = INPUTS["TREE"]["tree_records"](tree_data)
    if (len(tree_data), hashlib.sha256(tree_data).hexdigest(), len(rows)) != (report["treeBytes"], report["treeSha256"], report["treeEntries"]):
        raise ValueError("complete unchanged fixed registry source tree required")
    if report["recursiveDependenciesCollected"] is not False or report["licenseCoverage"] != "not_proven":
        raise ValueError("original bounded source-only registry scope required")
    selected, refs, extra, missing = EXTERNAL["selected_inputs"](key, rows, [], COLLECTOR["PINS"], COLLECTOR["EXTRA_PATHS"][key])
    if (report["parentLicenseReferences"] != refs or report["reviewedExtraPaths"] != extra
            or report["missingParentOrExtraPaths"] != missing or len(selected) != len(report["selectedFiles"])):
        raise ValueError("entire original selected registry source inventory required")
    blobs, unique = {}, {}
    prefix = COLLECTOR["PINS"][key][0]
    for row, recorded in zip(selected, report["selectedFiles"]):
        if any(recorded.get(field) != value for field, value in row.items()) or recorded["checkoutPath"] != prefix + "/" + row["path"]:
            raise ValueError("each original registry source path and Git object required")
        oid = row["gitObjectSha1"]
        data = INPUTS["read_data"](root / "blobs" / (oid + ".source-data"), INPUTS["TREE"]["MAX_BLOB"])
        identity = INPUTS["TREE"]["check_blob"](oid, data)
        if identity != {field: recorded[field] for field in ("bytes", "sha256")}:
            raise ValueError("original full registry source bytes required")
        blobs[prefix + "/" + row["path"]] = data
        unique[oid] = len(data)
    if sum(unique.values()) != report["sourceBytes"] or len(unique) != report["uniqueBlobs"]:
        raise ValueError("exact unique registry source-byte totals required")
    return summary, rows, blobs


def xml_notice(data, path):
    if not isinstance(data, bytes) or not 0 < len(data) <= INPUTS["TREE"]["MAX_BLOB"]:
        raise ValueError("bounded complete XML source bytes required")
    text = data.decode("utf-8", errors="strict")
    start = '<?xml version="1.0" encoding="UTF-8"?>\n<registry>\n'
    if not text.startswith(start):
        raise ValueError("original XML declaration and registry root required")
    match = re.match(r"\s*(<!--[\s\S]*?-->|<comment>[\s\S]*?</comment>)", text[len(start):len(start) + 16384])
    if match is None:
        raise ValueError("complete original leading XML attribution required")
    block = match.group(1)
    if len(re.findall(r"Copyright [0-9]{4}-[0-9]{4} The Khronos Group Inc\.", block)) != 1:
        raise ValueError("one original Khronos attribution required")
    exception = path.endswith("OpenCL-Docs/src/xml/cl.xml")
    if exception:
        required = ('Licensed under the Apache License, Version 2.0 (the "License")',
            "you may not use this file except in compliance with the License.", "http://www.apache.org/licenses/LICENSE-2.0",
            'distributed under the License is distributed on an "AS IS" BASIS', "WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND",
            "---- Exceptions to the Apache 2.0 License: ----", "without providing attribution as would otherwise",
            "be required by Sections 4(a), 4(b) and 4(d) of the License.", "software that is licensed under the GPLv2 or the LGPL v2.0 or 2.1",
            "patent provision (Section 3), the indemnity provision (Section 9)", "but only in their entirety and only with respect to the Combined Software.")
        if not all(value in block for value in required):
            raise ValueError("complete original Apache declaration, generation exception and waiver required")
        kind = "apache_with_original_generated_code_exception"
    else:
        if block.count("SPDX-License-Identifier: Apache-2.0") != 1:
            raise ValueError("one original Apache SPDX declaration required")
        kind = "apache_spdx"
    return {"sourcePath": path, "sourceBytes": len(data), "sourceSha256": hashlib.sha256(data).hexdigest(),
        "noticeKind": kind, "licenseId": "Apache-2.0", "originalAttributionText": block,
        "originalAttributionSha256": hashlib.sha256(block.encode()).hexdigest(), "originalExceptionsPreserved": exception,
        "exceptionUsedToExcludeAttribution": False}


def xml_generation_match(path, data, recorded):
    relative = path[len(GENERATION["ANGLE"]):]
    if relative not in GENERATION["XML_PATHS"] or relative not in recorded:
        raise ValueError("one of five original declared registry inputs required")
    raw = hashlib.md5(data).hexdigest()
    normalized = hashlib.md5(data.replace(b"\r\n", b"\n")).hexdigest()
    matches = recorded[relative] in (raw, normalized)
    return {"sourcePath": path, "recordedMd5": recorded[relative], "rawMd5": raw, "windowsNormalizedMd5": normalized,
        "matchesRecordedGenerationInput": matches,
        "status": "fixed_source_matches_generation_record" if matches else "fixed_revision_differs_from_generation_record",
        "md5IsSourceIdentity": False, "generatorReproduced": False, "binaryUseProven": False}


def copied_header_matches(headers, trees):
    by_oid, by_name = {}, {}
    for key, rows in trees.items():
        prefix = COLLECTOR["PINS"][key][0]
        for row in rows:
            if row["mode"] not in ("100644", "100755") or row["kind"] != "blob":
                continue
            candidate = {"component": key, "sourcePath": prefix + "/" + row["path"], "gitObjectSha1": row["gitObjectSha1"]}
            by_oid.setdefault(row["gitObjectSha1"], []).append(candidate)
            if row["path"].startswith("api/") and row["path"].endswith(".h"):
                by_name.setdefault(Path(row["path"]).name, []).append(candidate)
    result = []
    for path, data in sorted(headers.items()):
        oid = hashlib.sha1(b"blob " + str(len(data)).encode() + b"\0" + data).hexdigest()
        matches = by_oid.get(oid, [])
        result.append({"copiedSourcePath": path, "gitObjectSha1": oid, "sourceSha256": hashlib.sha256(data).hexdigest(),
            "identicalFixedRegistryTreeBlobs": matches, "sameBasenameApiCandidates": by_name.get(Path(path).name, []),
            "status": "identical_fixed_registry_tree_blob" if matches else "no_identical_blob_in_these_three_fixed_trees",
            "historicalCopyChainProven": False, "completeHeaderProvenance": False, "binaryUseProven": False})
    return result


def checked_headers(source, inputs, checkout_sha):
    header_inputs, header_checkout_sha, header_correspondence = GENERATION["SOURCES"]["checked_inputs"](source, "angle-headers")
    if header_inputs["mainSha256"] != inputs["mainSha256"] or header_checkout_sha != checkout_sha:
        raise ValueError("same original main source and manifest receipt required for header comparison")
    profiles = {"angle-headers": ("angle-header-git-source-01", SUPPLEMENT["HEADER_SEAL"])}
    headers, header_sha = GENERATION["INTEGRATION"]["checked_profile"](source, "angle-headers", header_inputs,
        header_checkout_sha, header_correspondence, profiles=profiles, git_objects=True)
    if (set(headers) != set(SUPPLEMENT["header_paths"](header_inputs["index"]))
            or SUPPLEMENT["header_paths"](header_inputs["index"]) != SUPPLEMENT["header_paths"](inputs["index"])):
        raise ValueError("entire original copied API header inventory required")
    return headers, header_sha


def render_fragment(notices, apache):
    if (len(notices) != 5 or len({row["sourcePath"] for row in notices}) != 5
            or not isinstance(apache, bytes) or not apache or any(row["licenseId"] != "Apache-2.0" for row in notices)):
        raise ValueError("all five original XML notices and complete Apache text required")
    parts = [b"PARTIAL ANGLE REGISTRY XML SOURCE NOTICE SUPPLEMENT - NOT FOR DISTRIBUTION\n"
        b"Fixed generator input notices only; not full browser NOTICE or binary SBOM.\n"]
    for row in notices:
        parts.extend((("\nOriginal source: " + row["sourcePath"] + "\n").encode(), row["originalAttributionText"].encode()))
    parts.extend((b"\nComplete license: Apache-2.0, immutable SPDX v3.28.0 source\n", apache))
    data = b"\n".join(parts)
    if len(data) > MAX_FRAGMENT:
        raise ValueError("bounded complete registry notice fragment required")
    return data


def inspect(source, output):
    INPUTS["checked_source"](source)
    if output.parent != source or not re.fullmatch(r"angle-registry-coverage-[0-9]{2}", output.name):
        raise ValueError("safe fresh task-owned registry coverage output required")
    if output.exists() or output.is_symlink():
        raise FileExistsError("never overwrite registry coverage evidence")
    inputs, checkout_sha, correspondence, declarations = COLLECTOR["checked_inputs"](source)
    collected, collected_sha = INPUTS["receipt"](source / ROOT / "angle-registry-source-inventory.json", ROOT_SHA)
    if (collected["sourceCommit"] != INPUTS["TREE"]["COMMIT"] or collected["mainReceiptSha256"] != inputs["mainSha256"]
            or collected["angleGenerationReceiptSha256"] != COLLECTOR["GENERATION_SHA"]
            or collected["recursiveDepsReceiptSha256"] != GENERATION["CATALOG_SHA"] or collected["collectionComplete"] is not True
            or collected["checkoutReceiptSha256"] != checkout_sha or collected["angleFourManifestCorrespondence"] != correspondence
            or collected["originalRegistryDeclarations"] != declarations
            or any(collected[key] is not False for key in ("sourceFunctionsExecuted", "upstreamConditionsExecuted", "generatorExecuted",
                "metadataPathsAliased", "shippingDecisionsMade", "completeChromiumNotice", "binarySbom", "releaseReady", "promoted"))):
        raise ValueError("entire original completed source-only registry receipt required")
    summaries, trees, blobs = [], {}, {}
    for key in COLLECTOR["PINS"]:
        summary, tree, data = checked_component(source, key)
        summaries.append(summary)
        trees[key] = tree
        blobs.update(data)
    if summaries != collected["components"]:
        raise ValueError("all three original component summaries required")
    generation, _ = INPUTS["receipt"](source / "angle-generation-coverage-01.json", COLLECTOR["GENERATION_SHA"])
    xml_paths = [GENERATION["ANGLE"] + path for path in GENERATION["XML_PATHS"]]
    notices = [xml_notice(blobs[path], path) for path in xml_paths]
    matches = [xml_generation_match(path, blobs[path], generation["generationRecords"]["registryInputRecordedMd5"]) for path in xml_paths]
    headers, header_sha = checked_headers(source, inputs, checkout_sha)
    header_matches = copied_header_matches(headers, trees)
    spdx = SUPPLEMENT["SPDX"]["verify"](source, "khronos-license-source-01")
    apache = INPUTS["read_data"](source / "khronos-license-source-01/Apache-2.0.license-data", SUPPLEMENT["SPDX"]["MAX_TEXT"])
    fragment = render_fragment(notices, apache)
    report = {"scope": "five fixed XML inputs, complete original XML notices and identical copied-header blob comparisons only",
        "sourceCommit": INPUTS["TREE"]["COMMIT"], "registryCollectionReceiptSha256": collected_sha,
        "registryComponents": summaries, "generationReceiptSha256": COLLECTOR["GENERATION_SHA"],
        "originalRegistryDeclarationsRetained": declarations, "xmlNotices": notices, "xmlGenerationMatches": matches,
        "xmlGenerationMatchCounts": dict(sorted(Counter(row["status"] for row in matches).items())),
        "copiedHeaderSourceReceiptSha256": header_sha, "copiedHeaderComparisons": header_matches,
        "copiedHeaderComparisonCounts": dict(sorted(Counter(row["status"] for row in header_matches).items())),
        "spdxSource": spdx, "originalRawErrorsRetained": generation["originalRawErrorsRetained"],
        "originalGenerationReportRetained": True, "selectedRegistrySourceBytes": sum(row["sourceBytes"] for row in summaries),
        "fragmentFile": "angle-registry-xml-notices.txt", "fragmentBytes": len(fragment), "fragmentSha256": hashlib.sha256(fragment).hexdigest(),
        "remainingSourceObligations": ["Unmatched copied headers need separate source-version evidence; a basename is not provenance",
            "Identical fixed-tree blobs do not prove historical copy operations or binary use",
            "Generation-record differences, if any, require preserving both original fixed revisions and original recorded hashes",
            "Complete browser notification, source-offer, security and final binary correspondence remain open"],
        "sourceFunctionsExecuted": False, "xmlParserExecuted": False, "generatorExecuted": False,
        "metadataPathsAliased": False, "originalErrorsRemoved": False, "shippingExclusionsProven": False,
        "completeHeaderProvenance": False, "completeChromiumNotice": False, "binarySbom": False, "releaseReady": False, "promoted": False}
    output.mkdir()
    with (output / report["fragmentFile"]).open("xb") as stream:
        stream.write(fragment)
    EXTERNAL["write_report"](output / "angle-registry-coverage-inventory.json", report)
    return report


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--output-name", required=True)
    args = parser.parse_args()
    source = args.source.absolute()
    report = inspect(source, source / args.output_name)
    print(json.dumps({key: report[key] for key in ("selectedRegistrySourceBytes", "xmlGenerationMatchCounts",
        "copiedHeaderComparisonCounts", "fragmentBytes", "fragmentSha256", "releaseReady")}))
