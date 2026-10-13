"""Classify verified license gaps by source ownership, not shipping status.

Reads immutable collected bytes and a fixed static DEPS catalog. Does not
trust earlier gap reports, evaluate conditions, follow remote references,
execute upstream code, generate NOTICE or mutate runtime/product state.
"""
import argparse
from collections import Counter
import json
from pathlib import Path
import re
import runpy


HERE = Path(__file__).parent.absolute()
POLICY = runpy.run_path(str(HERE / "inspect-chromium-license-policy.py"))
INPUTS = POLICY["INPUTS"]
EXTERNAL = INPUTS["EXTERNAL"]


def canonical_path(path):
    if (not isinstance(path, str) or not path or "\\" in path or ":" in path
            or any(ord(char) < 32 for char in path)
            or any(part in ("", ".", "..") for part in path.split("/"))):
        raise ValueError("canonical Chromium-relative source path required")
    return path


def inside(path, root):
    return path == root or path.startswith(root + "/")


class SourceOwners:
    def __init__(self, catalog, index, collected):
        self.declarations = []
        seen = set()
        for kind, rows in (("git", catalog["gitDependencies"]), ("non_git", catalog["nonGitDependencies"])):
            for row in rows:
                path = canonical_path(row["path"])
                if path in seen:
                    raise ValueError("unique cross-kind source declaration required")
                seen.add(path)
                self.declarations.append({"path": path, "kind": kind, "condition": row["condition"],
                                          "revision": row.get("revision"), "repository": row.get("repository"),
                                          "nonGitKind": row.get("kind") if kind == "non_git" else None})
        if len(self.declarations) > 10000:
            raise ValueError("bounded static source declarations required")
        self.declarations.sort(key=lambda row: (-len(row["path"]), row["path"]))
        self.collected = {canonical_path(path) for path in collected}
        self.gitlinks = sorted(({"path": path, "revision": row["gitObjectSha1"]}
                                for path, row in index.items() if row["mode"] == "160000"),
                               key=lambda row: (-len(row["path"]), row["path"]))

    def describe(self, path):
        canonical_path(path)
        declaration = next((row for row in self.declarations if inside(path, row["path"])), None)
        pending_link = next((link for link in self.gitlinks
                             if link["path"] not in self.collected and inside(path, link["path"])
                             and (declaration is None or inside(link["path"], declaration["path"]))), None)
        if pending_link and (declaration is None or pending_link["path"] != declaration["path"]):
            kind = "uncollected_recursive_gitlink"
        elif declaration is None:
            kind = "collected_main_tree"
        elif declaration["kind"] == "non_git":
            kind = "declared_non_git_source"
        elif declaration["path"] in self.collected:
            kind = "collected_parent_git_source"
        else:
            kind = "uncollected_parent_git_source"
        return {"originKind": kind, "parentDeclaration": declaration, "pendingGitlink": pending_link,
                "declaredConditionsEvaluated": False, "binaryUsageProven": False}


def classify_reference(reference, index, files, owners):
    path = reference["sourcePath"]
    resolution = INPUTS["classify"](reference, index, files)
    owner = owners.describe(path) if path is not None else None
    if (path is None and reference.get("reason") == "unsupported_or_outside_root"
            and not reference["value"].startswith("https://")):
        resolution = category = "unsupported_or_outside_root_license_reference"
    elif resolution == "path_not_in_collected_checkout_trees" and reference.get("value") == "Not Included":
        # Preserve the original unresolved reference and error. This is not
        # NOT_SHIPPED, an exclusion, or evidence satisfying a license notice.
        category = "nonstandard_not_included_license_value"
    elif resolution == "path_not_in_collected_checkout_trees":
        by_owner = {
            "uncollected_recursive_gitlink": "uncollected_recursive_gitlink",
            "uncollected_parent_git_source": "uncollected_parent_git_source",
            "declared_non_git_source": "declared_non_git_source",
        }
        category = by_owner.get(owner["originKind"], "absent_in_collected_source_trees")
    else:
        category = resolution
    return {**reference, "resolution": resolution, "category": category, "sourceOwner": owner}


def counts(rows, key):
    return dict(sorted(Counter(row[key] for row in rows).items()))


def classify(inputs, catalog):
    files, index = inputs["files"], inputs["index"]
    payloads = {path: row["payload"] for path, row in files.items()}
    policy = POLICY["parse_policy"](payloads["tools/licenses/licenses.py"])
    scan = POLICY["scan"](POLICY["SourceView"](index, payloads, policy["VCS_METADATA_DIRS"]), policy)
    compiled = {key: pin for pins in EXTERNAL["GROUPS"].values() for key, pin in pins.items()}
    collected = {compiled[row["component"]][0] for row in inputs["components"]}
    owners = SourceOwners(catalog, index, collected)
    references = [classify_reference(row, index, files, owners) for row in inputs["references"]]
    missing = [row for row in references if row["resolution"] != "resolved_to_verified_source_bytes"]
    shipping_errors, unknown = [], []
    for row in scan["entries"]:
        if row["shippedMetadata"] == policy["YES"] and row["errors"]:
            gaps = [classify_reference({"sourcePath": ref["path"], "value": ref["value"],
                                       "reason": ref["reason"], "metadata": row["metadata"]}, index, files, owners)
                    for ref in row["unresolvedDeclaredReferences"]]
            shipping_errors.append({**row, "metadataOwner": owners.describe(row["directory"]),
                                    "classifiedDeclaredGaps": gaps})
        elif row["shippedMetadata"] not in (policy["YES"], policy["NO"]):
            unknown.append({**row, "metadataOwner": owners.describe(row["directory"]),
                            "caseNormalized": False, "shippingDecision": "not_made"})
    directory_errors = [{**row, "sourceOwner": owners.describe(row["directory"])}
                        for row in scan["directoryErrors"]]
    return {"rawLicenseReferences": len(references), "resolvedRawReferences": len(references) - len(missing),
            "unresolvedRawReferences": missing, "unresolvedReferenceCategories": counts(missing, "category"),
            "shippingMetadataEntries": sum(row["shippedMetadata"] == policy["YES"] for row in scan["entries"]),
            "shippingMetadataEntriesWithErrors": shipping_errors, "nonstandardShippingValues": unknown,
            "directoryErrors": directory_errors,
            "candidateDirectories": len(scan["candidateDirectories"]), "metadataEntries": len(scan["entries"]),
            "declaredParentGitSources": len(catalog["gitDependencies"]),
            "collectedParentGitSources": len(inputs["components"]),
            "declaredNonGitSources": len(catalog["nonGitDependencies"])}


def inspect(source, output):
    if output.parent != source or not re.fullmatch(r"license-gap-classification-[0-9]{2}\.json", output.name):
        raise ValueError("safe fresh gap classification name required")
    if output.exists() or output.is_symlink():
        raise FileExistsError("never overwrite source classification evidence")
    inputs = INPUTS["load_inputs"](source, include_media=True, include_runtime=True,
                                  include_support=True, include_browser=True)
    catalog_bytes = INPUTS["read_data"](source / EXTERNAL["CATALOG"], INPUTS["TREE"]["MAX_BLOB"])
    catalog = EXTERNAL["checked_catalog"](catalog_bytes)
    EXTERNAL["checked_catalog"](catalog_bytes, EXTERNAL["MEDIA_PINS"])
    EXTERNAL["checked_catalog"](catalog_bytes, EXTERNAL["RUNTIME_PINS"])
    EXTERNAL["checked_catalog"](catalog_bytes, EXTERNAL["SUPPORT_PINS"])
    EXTERNAL["checked_catalog"](catalog_bytes, EXTERNAL["BROWSER_PINS"])
    report = classify(inputs, catalog)
    report.update(scope="source ownership of raw and default-policy metadata gaps only, not binary components",
                  sourceCommit=INPUTS["TREE"]["COMMIT"], policySha256=POLICY["POLICY_SHA"],
                  parentCatalogSha256=EXTERNAL["CATALOG_SHA"], mainReceiptSha256=inputs["mainSha256"],
                  externalReceiptSha256=inputs["externalSha256"], mediaReceiptSha256=inputs["mediaSha256"],
                  runtimeReceiptSha256=inputs["runtimeSha256"], supportReceiptSha256=inputs["supportSha256"],
                  browserReceiptSha256=inputs["browserSha256"],
                  sourceFunctionsExecuted=False,
                  generatorExecuted=False, conditionsEvaluated=False, referencesFollowed=False,
                  checkoutCoverage="partial", binaryComponentCoverage="not_proven", licenseCoverage="not_proven",
                  shippingDecisionsMade=False, promoted=False)
    EXTERNAL["write_report"](output, report)
    return report


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--output-name", required=True)
    args = parser.parse_args()
    source = args.source.absolute()
    report = inspect(source, source / args.output_name)
    print(json.dumps({"unresolvedRawReferences": len(report["unresolvedRawReferences"]),
                      "categories": report["unresolvedReferenceCategories"],
                      "shippingMetadataEntriesWithErrors": len(report["shippingMetadataEntriesWithErrors"]),
                      "directoryErrors": len(report["directoryErrors"]),
                      "nonstandardShippingValues": len(report["nonstandardShippingValues"]),
                      "licenseCoverage": report["licenseCoverage"], "promoted": report["promoted"]}))
