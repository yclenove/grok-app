"""Collect six fixed public recursive sources present in all archive manifests.

Presence is not a shipping decision. Reverify sealed original manifests and
source references before network; collect inert Git license data only. Never
execute source/DEPS/hooks, change product pins or overwrite prior evidence.
"""
import argparse
import json
from pathlib import Path
import re
import runpy


HERE = Path(__file__).parent.absolute()
CHECKOUTS = runpy.run_path(str(HERE / "inspect-chromium-archive-checkouts.py"))
INPUTS = CHECKOUTS["INPUTS"]
EXTERNAL = INPUTS["EXTERNAL"]
CHECKOUT_REPORT_SHA = "e1349b8e7d96d23ac15c0d3d64085dabb7dc801a83bf36d27f242454b460b5ae"
PINS = {
    "angle-glmark2": ("third_party/angle/third_party/glmark2/src",
                      "https://chromium.googlesource.com/external/github.com/glmark2/glmark2",
                      "6edcf02205fd1e8979dc3f3964257a81959b80c8"),
    "angle-rapidjson": ("third_party/angle/third_party/rapidjson/src",
                       "https://chromium.googlesource.com/external/github.com/Tencent/rapidjson",
                       "24b5e7a8b27f42fa16b96fc70aade9106cf7102f"),
    "angle-vk-gl-cts": ("third_party/angle/third_party/VK-GL-CTS/src",
                       "https://chromium.googlesource.com/external/github.com/KhronosGroup/VK-GL-CTS",
                       "9335a4e4e455ed600cb7fa486999e3ed6c2a0dbf"),
    "openscreen-buildtools": ("third_party/openscreen/src/buildtools",
                              "https://chromium.googlesource.com/chromium/src/buildtools",
                              "4277578aa9c45906e51ad33cac1a5a7ad5288010"),
    "openscreen-tinycbor": ("third_party/openscreen/src/third_party/tinycbor/src",
                            "https://chromium.googlesource.com/external/github.com/intel/tinycbor.git",
                            "213155c8729b07dea836c5ac6e807a089486bf7b"),
    "openscreen-simple-dns": ("third_party/openscreen/src/third_party/simple_dns/src",
                              "https://chromium.googlesource.com/external/github.com/balliegojr/simple-dns.git",
                              "5c7ec98798d7c9f7d76df7aa3bbe754913ff5159"),
}


def reviewed_pins(report):
    targets = report.get("targets")
    if (report.get("sourceCommit") != INPUTS["TREE"]["COMMIT"] or not isinstance(targets, list)
            or [row.get("target") for row in targets] != list(CHECKOUTS["BUILDS"]["ARCHIVES"])
            or report.get("conditionsEvaluated") is not False or report.get("licenseErrorsExcluded") is not False
            or report.get("promoted") is not False):
        raise ValueError("all four original non-promoted checkout identities required")
    expected = {"src/" + prefix: (repository, revision) for prefix, repository, revision in PINS.values()}
    for target in targets:
        groups = target.get("recursiveComparison")
        if not isinstance(groups, dict) or set(groups) != {"angle", "openscreen"}:
            raise ValueError("both original recursive comparisons required")
        matched = {}
        for rows in groups.values():
            if not isinstance(rows, list):
                raise ValueError("original recursive rows required")
            for row in rows:
                if row.get("status") != "exact_repository_and_revision_match":
                    continue
                path = row.get("checkoutPath")
                identity = row.get("expectedRepository"), row.get("expectedRevision")
                if (path not in expected or path in matched or identity != expected[path]
                        or row.get("actual") != {"repository": identity[0], "revision": identity[1]}
                        or row.get("conditionEvaluated") is not False or row.get("shippingDecisionMade") is not False):
                    raise ValueError("exact same-path recursive identity without shipping/condition decisions required")
                matched[path] = identity
        if matched != expected:
            raise ValueError("all six compiled sources must match every archive manifest")
    return PINS


def checked_inputs(source):
    INPUTS["checked_source"](source)
    original, sha = INPUTS["receipt"](source / "archive-checkouts-01.json", CHECKOUT_REPORT_SHA)
    if CHECKOUTS["inspect"](source) != original:
        raise ValueError("sealed comparison must reproduce from original build/log/catalog inputs")
    reviewed_pins(original)
    inputs = INPUTS["load_inputs"](source, True, True, True, True)
    return inputs["references"], sha


def checked_output(source, output):
    if output.parent != source or not re.fullmatch(r"checked-recursive-license-[0-9]{2}", output.name):
        raise ValueError("safe fresh checked-recursive evidence directory required")
    if output.exists() or output.is_symlink():
        raise FileExistsError("never overwrite checked-recursive source evidence")
    INPUTS["checked_source"](source)


def collect(source, output):
    checked_output(source, output)
    references, sha = checked_inputs(source)
    output.mkdir()
    report = {"scope": "six manifest-matched recursive license-source inputs only, not binary coverage/NOTICE",
              "sourceCommit": INPUTS["TREE"]["COMMIT"], "checkoutReportSha256": sha,
              "requestedComponents": list(PINS), "components": [], "collectionComplete": False,
              "sourceFunctionsExecuted": False, "conditionsEvaluated": False, "shippingDecisionsMade": False,
              "generatorExecuted": False, "licenseCoverage": "not_proven", "promoted": False}
    try:
        for key in PINS:
            result = EXTERNAL["collect_component"](key, output, references, pins=PINS)
            receipt_path = output / key / "license-source-inventory.json"
            _, receipt_sha = INPUTS["receipt"](receipt_path)
            component = {"component": key, "checkoutPath": result["checkoutPath"],
                         "repository": result["repository"], "revision": result["revision"],
                         "receiptSha256": receipt_sha, "treeEntries": result["treeEntries"],
                         "selectedFiles": len(result["selectedFiles"]), "sourceBytes": result["sourceBytes"],
                         "collectionComplete": result["collectionComplete"]}
            report["components"].append(component)
            print(json.dumps(component), flush=True)
        report["collectionComplete"] = True
    except Exception as error:
        report["failureType"] = type(error).__name__
        raise
    finally:
        EXTERNAL["write_report"](output / "checked-recursive-source-inventory.json", report)
    return report


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--output-name", required=True)
    args = parser.parse_args()
    source = args.source.absolute()
    result = collect(source, source / args.output_name)
    print(json.dumps({"sources": len(result["components"]), "collectionComplete": result["collectionComplete"],
                      "licenseCoverage": result["licenseCoverage"], "promoted": False}))
