"""Collect exact public Perfetto source/license inputs from all actual manifests.

The Chromium DEPS/Gitlink and each original checkout must match before fetch.
Reuse the bounded inert Git collector; no checkout/upstream code execution,
aliasing of standalone OpenScreen paths or binary shipping decisions.
"""
import argparse
import json
from pathlib import Path
import re
import runpy


HERE = Path(__file__).parent.absolute()
INTEGRATION = runpy.run_path(str(HERE / "collect-chromium-integration-sources.py"))
INPUTS, EXTERNAL, CHECKOUTS = INTEGRATION["INPUTS"], INTEGRATION["EXTERNAL"], INTEGRATION["CHECKOUTS"]
PINS = {"perfetto": ("third_party/perfetto", "https://chromium.googlesource.com/external/github.com/google/perfetto.git",
                     "59400b15c6ff8186ba0bba69f78c2cd454eea8b4")}


def checked_inputs(source):
    inputs, checkout_sha, _ = INTEGRATION["checked_inputs"](source)
    raw = INPUTS["read_data"](source / EXTERNAL["CATALOG"], INPUTS["TREE"]["MAX_BLOB"])
    catalog = EXTERNAL["checked_catalog"](raw)
    prefix, repository, revision = PINS["perfetto"]
    matching = [row for row in catalog["gitDependencies"] if row["path"] == prefix]
    if matching != [{"path": prefix, "repository": repository, "revision": revision, "condition": None,
                     "gitlinkRevision": revision, "gitlinkMatched": True}]:
        raise ValueError("exact original public Perfetto DEPS declaration required")
    entry = inputs["index"].get(prefix)
    if entry is None or (entry["mode"], entry["gitObjectSha1"]) != ("160000", revision):
        raise ValueError("exact main-tree Perfetto Gitlink required")
    checkout = CHECKOUTS["inspect"](source)
    correspondence = []
    for target in checkout["targets"]:
        row = CHECKOUTS["compare_pin"]("src/" + prefix, repository, revision, target["manifest"])
        if row["status"] != "exact_repository_and_revision_match":
            raise ValueError("Perfetto source must match every original manifest")
        correspondence.append({"target": target["target"], **row})
    return inputs, checkout_sha, correspondence


def collect(source, output):
    if output.parent != source or not re.fullmatch(r"perfetto-license-source-[0-9]{2}", output.name):
        raise ValueError("safe fresh task-owned Perfetto source output required")
    if output.exists() or output.is_symlink():
        raise FileExistsError("never overwrite Perfetto source evidence")
    inputs, checkout_sha, correspondence = checked_inputs(source)
    output.mkdir()
    report = {"scope": "actual manifest-matched Perfetto source/license only, not full NOTICE or binary coverage",
              "sourceCommit": INPUTS["TREE"]["COMMIT"], "mainReceiptSha256": inputs["mainSha256"],
              "parentCatalogSha256": EXTERNAL["CATALOG_SHA"], "checkoutReceiptSha256": checkout_sha,
              "checkoutCorrespondence": correspondence, "components": [], "collectionComplete": False,
              "sourceFunctionsExecuted": False, "conditionsEvaluated": False, "generatorExecuted": False,
              "shippingDecisionsMade": False, "metadataPathsAliased": False, "licenseCoverage": "not_proven", "promoted": False}
    try:
        component = EXTERNAL["collect_component"]("perfetto", output, inputs["references"], pins=PINS)
        _, sha = INPUTS["receipt"](output / "perfetto/license-source-inventory.json")
        report["components"].append({"component": "perfetto", "checkoutPath": component["checkoutPath"],
            "repository": component["repository"], "revision": component["revision"], "receiptSha256": sha,
            "treeEntries": component["treeEntries"], "selectedFiles": len(component["selectedFiles"]),
            "sourceBytes": component["sourceBytes"], "collectionComplete": component["collectionComplete"]})
        report["collectionComplete"] = True
    except Exception as error:
        report["failureType"] = type(error).__name__
        raise
    finally:
        EXTERNAL["write_report"](output / "perfetto-source-inventory.json", report)
    return report


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--output-name", required=True)
    args = parser.parse_args()
    source = args.source.absolute()
    report = collect(source, source / args.output_name)
    print(json.dumps({"collectionComplete": report["collectionComplete"], "components": report["components"], "promoted": False}))
