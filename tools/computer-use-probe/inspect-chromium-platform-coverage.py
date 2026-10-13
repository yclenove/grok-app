"""Join sealed source/policy inputs with four actual checkout manifests.

This is provenance triage, not a resolved checkout, NOTICE or binary SBOM.
Keep every original raw/policy error. Manifest absence never means not shipped.
Only trusted local inspection modules run; collected upstream code stays data.
"""
import argparse
from collections import Counter
import json
from pathlib import Path
import re
import runpy


HERE = Path(__file__).parent.absolute()
RECURSIVE = runpy.run_path(str(HERE / "inspect-chromium-checked-recursive-licenses.py"))
CHECKOUTS = runpy.run_path(str(HERE / "inspect-chromium-archive-checkouts.py"))
CLASSIFICATION = runpy.run_path(str(HERE / "inspect-chromium-license-gap-classification.py"))
INPUTS, POLICY = RECURSIVE["INPUTS"], RECURSIVE["POLICY"]
EXTERNAL = INPUTS["EXTERNAL"]
canonical_path, inside = CLASSIFICATION["canonical_path"], CLASSIFICATION["inside"]
# This report reproduces the original five-group, 57-source snapshot. Later
# notice supplements have independent receipts and must not change its scope.
ORIGINAL_GROUPS = ("core", "media", "runtime", "support", "browser")


def counts(rows, key):
    return dict(sorted(Counter(row[key] for row in rows).items()))


def collected_sources(inputs, recursive):
    pins = {key: pin for name in ORIGINAL_GROUPS for key, pin in EXTERNAL["GROUPS"][name].items()}
    if len(pins) != 57 or len(inputs["components"]) != len(pins) or {row["component"] for row in inputs["components"]} != set(pins):
        raise ValueError("all 57 original independently verified source receipts required")
    sources = [{"component": "chromium", "checkoutPath": "src",
                "repository": "https://chromium.googlesource.com/chromium/src.git",
                "revision": INPUTS["TREE"]["COMMIT"], "receiptSha256": inputs["mainSha256"]}]
    for row in inputs["components"]:
        path, repository, revision = pins[row["component"]]
        if row["revision"] != revision:
            raise ValueError("collected source revision changed")
        sources.append({**row, "checkoutPath": "src/" + path, "repository": repository})
    for row in recursive:
        sources.append({**row, "checkoutPath": "src/" + row["checkoutPath"]})
    if len(sources) != 64 or len({row["checkoutPath"] for row in sources}) != 64:
        raise ValueError("main plus 63 unique verified selected sources required")
    return sources


def declarations(source):
    contents = INPUTS["read_data"](source / EXTERNAL["CATALOG"], INPUTS["TREE"]["MAX_BLOB"])
    main = EXTERNAL["checked_catalog"](contents)
    catalogs = [("chromium", "", main)]
    for key, (name, sha) in CHECKOUTS["RECURSIVE"].items():
        catalog, _ = INPUTS["receipt"](source / name, sha)
        # Independently recheck the original recursive catalog contract without
        # inferring conditions from a parent's similarly named dependencies.
        CHECKOUTS["recursive_comparison"]({}, catalog)
        catalogs.append((key, catalog["parentCheckoutPath"] + "/", catalog))
    result = []
    for component, prefix, catalog in catalogs:
        for kind, rows in (("git", catalog["gitDependencies"]), ("non_git", catalog["nonGitDependencies"])):
            for row in rows:
                path = canonical_path(prefix + canonical_path(row["path"]))
                result.append({"path": path, "declarationSource": component, "kind": kind,
                               "conditionRecorded": row.get("condition"), "conditionEvaluated": False,
                               "repository": row.get("repository"), "revision": row.get("revision"),
                               "nonGitKind": row.get("kind") if kind == "non_git" else None})
    if len(result) > 10000 or len({row["path"] for row in result}) != len(result):
        raise ValueError("bounded unique declared source paths required")
    return sorted(result, key=lambda row: (-len(row["path"]), row["path"]))


class PlatformView:
    def __init__(self, sources, manifest, declared, index, files):
        paths = [canonical_path(row["checkoutPath"]) for row in sources]
        if len(paths) != len(set(paths)) or "src" not in paths:
            raise ValueError("unique collected source roots including Chromium required")
        # Reuse original path/repository validation even for locally built views.
        CHECKOUTS["manifest_data"](json.dumps({"did_run": True, "root": "src",
            "fixed_revisions": {"src": INPUTS["TREE"]["COMMIT"]}, "manifest": manifest}).encode())
        self.sources = {row["checkoutPath"]: row for row in sources}
        self.source_roots = sorted(paths, key=lambda path: (-len(path), path))
        self.manifest = manifest
        self.manifest_roots = sorted(manifest, key=lambda path: (-len(path), path))
        self.declared = sorted(declared, key=lambda row: (-len(row["path"]), row["path"]))
        for row in self.declared:
            canonical_path(row["path"])
        self.index, self.files = index, files
        self.gitlinks = sorted((path for path, row in index.items() if row["mode"] == "160000"),
                               key=lambda path: (-len(path), path))

    def compare(self, source):
        return CHECKOUTS["compare_pin"](source["checkoutPath"], source["repository"], source["revision"], self.manifest)

    def describe(self, path):
        if path is None:
            return {"path": None, "pathStatus": "no_local_source_path", "shippingDecisionMade": False}
        canonical_path(path)
        full = "src/" + path
        selected_root = next(root for root in self.source_roots if inside(full, root))
        manifest_root = next(root for root in self.manifest_roots if inside(full, root))
        selected = self.sources[selected_root]
        actual_selected = self.sources.get(manifest_root)
        declared = next((row for row in self.declared if inside(path, row["path"])), None)
        gitlink = next((root for root in self.gitlinks if inside(path, root)), None)
        entry = self.index.get(path)
        return {"path": path, "pathStatus": INPUTS["classify"]({"sourcePath": path}, self.index, self.files),
                "indexedMode": entry["mode"] if entry else None,
                "collectedTreeRoot": selected_root, "collectedComponent": selected["component"],
                "collectedTreeManifestStatus": self.compare(selected)["status"],
                "nearestActualManifestRoot": manifest_root, "actualManifestIdentity": self.manifest[manifest_root],
                "actualRootSourceStatus": (self.compare(actual_selected)["status"] if actual_selected
                                           else "actual_manifest_source_not_collected"),
                "nearestDeclaredSource": declared,
                "nearestDeclaredSourceManifestStatus": (CHECKOUTS["compare_pin"](
                    "src/" + declared["path"], declared["repository"], declared["revision"], self.manifest)["status"]
                    if declared and declared["kind"] == "git" else None),
                "nearestIndexedGitlink": ({"path": gitlink, "revision": self.index[gitlink]["gitObjectSha1"],
                                           "selectedSourceCollected": "src/" + gitlink in self.sources} if gitlink else None),
                "effectiveSourceMappingProven": False, "shippingDecisionMade": False}

    def manifest_sources(self):
        result = []
        for path, identity in sorted(self.manifest.items()):
            selected = self.sources.get(path)
            result.append({"checkoutPath": path, **identity,
                           "selectedSourceStatus": (self.compare(selected)["status"] if selected
                                                    else "actual_manifest_source_not_collected"),
                           "selectedComponent": selected["component"] if selected else None,
                           "shippingDecisionMade": False})
        return result


def gap_category(reference, provenance):
    if provenance["path"] is None:
        return "nonlocal_or_unsupported_reference"
    status = provenance["pathStatus"]
    if status != "path_not_in_collected_checkout_trees":
        return status
    if reference["value"] == "Not Included":
        return "nonstandard_not_included_not_an_exemption"
    declared = provenance["nearestDeclaredSource"]
    if provenance["actualRootSourceStatus"] == "actual_manifest_source_not_collected":
        return "actual_manifest_git_source_not_collected"
    if declared and declared["kind"] == "non_git":
        return "declared_non_git_input_not_collected"
    if declared and provenance["nearestDeclaredSourceManifestStatus"] == "exact_manifest_entry_absent":
        return "declared_git_root_absent_from_actual_manifest"
    if provenance["collectedTreeManifestStatus"] == "exact_manifest_entry_absent":
        return "collected_tree_root_absent_from_actual_manifest"
    return "path_absent_in_collected_indexes_mapping_unproven"


def platform_report(target, view, references, scan, policy):
    sources = [{"component": row["component"], **view.compare(row)} for row in view.sources.values()]
    actual = view.manifest_sources()
    gaps = []
    for row in references:
        provenance = view.describe(row["sourcePath"])
        gaps.append({**row, "sourceProvenance": provenance, "category": gap_category(row, provenance),
                     "metadataProvenance": view.describe(row["metadata"])})
    entries = []
    for row in scan["entries"]:
        entries.append({**row, "directoryProvenance": view.describe(row["directory"]),
                        "licenseProvenance": [view.describe(path) for path in row["declaredLicensePaths"]],
                        "availableLicenseProvenance": [view.describe(path) for path in row["availableLicensePaths"]]})
    errors = [{**row, "directoryProvenance": view.describe(row["directory"])} for row in scan["directoryErrors"]]
    shipping_errors = [row for row in entries if row["shippedMetadata"] == policy["YES"] and row["errors"]]
    unknown = [row for row in entries if row["shippedMetadata"] not in (policy["YES"], policy["NO"])]
    return {"target": target["target"], "buildId": target["buildId"], "manifestEntries": target["manifestEntries"],
            "collectedSources": sources, "collectedSourceCounts": counts(sources, "status"),
            "actualManifestSources": actual, "actualManifestSourceCounts": counts(actual, "selectedSourceStatus"),
            "unresolvedRawReferences": gaps, "rawGapCategories": counts(gaps, "category"), "metadataEntries": entries,
            "shippingMetadataEntriesWithErrors": shipping_errors, "nonstandardShippingValues": unknown,
            "directoryErrors": errors, "originalErrorsPreserved": True,
            "sourceCollectionCoverage": "partial", "licenseCoverage": "not_proven", "promoted": False}


def inspect(source, output):
    if output.parent != source or not re.fullmatch(r"platform-source-coverage-[0-9]{2}\.json", output.name):
        raise ValueError("safe fresh platform coverage path required")
    if output.exists() or output.is_symlink():
        raise FileExistsError("never overwrite platform coverage evidence")
    inputs = INPUTS["load_inputs"](source, True, True, True, True)
    recursive, recursive_sha = RECURSIVE["append_sources"](source, inputs)
    sources = collected_sources(inputs, recursive)
    checkout = CHECKOUTS["inspect"](source)
    declared = declarations(source)
    payloads = {path: row["payload"] for path, row in inputs["files"].items()}
    policy = POLICY["parse_policy"](payloads["tools/licenses/licenses.py"])
    scan = POLICY["scan"](POLICY["SourceView"](inputs["index"], payloads, policy["VCS_METADATA_DIRS"]), policy)
    references = [{**row, "resolution": INPUTS["classify"](row, inputs["index"], inputs["files"])}
                  for row in inputs["references"]]
    gaps = [row for row in references if row["resolution"] != "resolved_to_verified_source_bytes"]
    targets = [platform_report(target, PlatformView(sources, target["manifest"], declared, inputs["index"], inputs["files"]),
                               gaps, scan, policy) for target in checkout["targets"]]
    report = {"scope": "per-platform source/policy provenance only; no absence exclusions or binary shipping decisions",
              "sourceCommit": INPUTS["TREE"]["COMMIT"], "mainReceiptSha256": inputs["mainSha256"],
              "recursiveReceiptSha256": recursive_sha, "policySha256": POLICY["POLICY_SHA"],
              "buildReceiptSha256": checkout["buildReceiptSha256"], "checkoutReceiptSha256": checkout["checkoutReceiptSha256"],
              "parentCatalogSha256": EXTERNAL["CATALOG_SHA"], "recursiveCatalogSeals": CHECKOUTS["RECURSIVE"],
              "collectedSourceReceipts": sources, "declaredSources": declared,
              "rawLicenseReferences": len(references), "unresolvedRawReferences": len(gaps),
              "verifiedSelectedFiles": len(inputs["files"]), "candidateDirectories": len(scan["candidateDirectories"]),
              "targets": targets, "originalErrorsExcluded": False, "conditionsEvaluated": False,
              "sourceFunctionsExecuted": False, "generatorExecuted": False, "shippingDecisionsMade": False,
              "binaryComponentCoverage": "not_proven", "licenseCoverage": "not_proven", "promoted": False}
    EXTERNAL["write_report"](output, report)
    return report


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--output-name", required=True)
    args = parser.parse_args()
    source = args.source.absolute()
    report = inspect(source, source / args.output_name)
    print(json.dumps({"sourcesIncludingMain": len(report["collectedSourceReceipts"]),
                      "rawGaps": report["unresolvedRawReferences"],
                      "targets": [{"target": row["target"], "sources": row["collectedSourceCounts"],
                                   "actualSources": row["actualManifestSourceCounts"],
                                   "yesErrors": len(row["shippingMetadataEntriesWithErrors"]),
                                   "directoryErrors": len(row["directoryErrors"]),
                                   "nonstandardValues": len(row["nonstandardShippingValues"])} for row in report["targets"]],
                      "licenseCoverage": report["licenseCoverage"], "promoted": False}))
