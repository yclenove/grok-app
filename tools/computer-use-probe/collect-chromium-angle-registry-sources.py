"""Collect fixed Khronos registry XML/license inputs as inert source data.

These are declared generator sources. Original Chromium manifests do not
contain these checkouts; absence is recorded, never called binary exclusion.
No checkout, XML parser, generator, upstream code or product runtime executes.
"""
import argparse
import json
from pathlib import Path
import re
import runpy


HERE = Path(__file__).parent.absolute()
GENERATION = runpy.run_path(str(HERE / "inspect-chromium-angle-generation.py"))
SOURCES, INPUTS = GENERATION["SOURCES"], GENERATION["INPUTS"]
EXTERNAL = SOURCES["EXTERNAL"]
GENERATION_SHA = "58001809c56623a479952230a705cdec2dc0377364f74830e3341f62804b4155"
NAMES = {"egl-registry": "EGL-Registry", "opencl-docs": "OpenCL-Docs", "opengl-registry": "OpenGL-Registry"}
PINS = {key: (GENERATION["ANGLE"] + "third_party/" + name + "/src",
    "https://chromium.googlesource.com/external/github.com/KhronosGroup/" + name, GENERATION["REGISTRIES"][name][0])
    for key, name in NAMES.items()}
EXTRA_PATHS = {key: GENERATION["REGISTRIES"][name][1] for key, name in NAMES.items()}


def checked_inputs(source):
    INPUTS["checked_source"](source)
    inputs, checkout_sha, correspondence = SOURCES["checked_inputs"](source, "angle-registry")
    generation, _ = INPUTS["receipt"](source / "angle-generation-coverage-01.json", GENERATION_SHA)
    context, _ = INPUTS["receipt"](source / "condition-context-01.json", GENERATION["CONTEXT_SHA"])
    catalog, _ = INPUTS["receipt"](source / "recursive-deps-angle-02.json", GENERATION["CATALOG_SHA"])
    checkout = SOURCES["CHECKOUTS"]["inspect"](source)
    declarations = GENERATION["registry_declarations"](catalog, context["targetContexts"], inputs["index"], inputs["files"], checkout["targets"])
    if (generation["sourceCommit"] != INPUTS["TREE"]["COMMIT"] or generation["registryDeclarations"] != declarations
            or generation["checkoutReceiptSha256"] != checkout_sha or generation["fourManifestCorrespondence"] != correspondence
            or context["rootBooleanDefaults"] != GENERATION["CONTEXT"]["boolean_defaults"](inputs["files"]["DEPS"]["payload"])
            or generation["registryProvenanceComplete"] is not False or generation["releaseReady"] is not False):
        raise ValueError("original exact declarations, source defaults and four-manifest correspondence required")
    return inputs, checkout_sha, correspondence, declarations


def component_summary(key, report, receipt_sha):
    prefix, repository, revision = PINS[key]
    paths = {row["path"] for row in report["selectedFiles"]}
    if (report["collectionComplete"] is not True or (report["checkoutPath"], report["repository"], report["revision"]) != PINS[key]
            or not set(EXTRA_PATHS[key]).issubset(paths) or report["missingParentOrExtraPaths"]
            or report["reviewedExtraPaths"] != sorted(EXTRA_PATHS[key])
            or report["generatorExecuted"] is not False or report["promoted"] is not False):
        raise ValueError("all exact original XML inputs and completed inert component required")
    return {"component": key, "checkoutPath": prefix, "repository": repository, "revision": revision,
        "receiptSha256": receipt_sha, "treeEntries": report["treeEntries"], "selectedFiles": len(report["selectedFiles"]),
        "sourceBytes": report["sourceBytes"], "collectionComplete": True, "declaredCondition": "not build_with_chromium",
        "binaryExclusionProven": False}


def collect(source, output):
    if output.parent != source or not re.fullmatch(r"angle-registry-license-source-[0-9]{2}", output.name):
        raise ValueError("safe fresh task-owned registry output required")
    if output.exists() or output.is_symlink():
        raise FileExistsError("never overwrite original registry evidence")
    inputs, checkout_sha, correspondence, declarations = checked_inputs(source)
    output.mkdir()
    report = {"scope": "fixed declared registry XML/license sources only, not actual checkout or binary provenance approval",
        "sourceCommit": INPUTS["TREE"]["COMMIT"], "mainReceiptSha256": inputs["mainSha256"],
        "angleGenerationReceiptSha256": GENERATION_SHA, "recursiveDepsReceiptSha256": GENERATION["CATALOG_SHA"],
        "checkoutReceiptSha256": checkout_sha, "angleFourManifestCorrespondence": correspondence,
        "originalRegistryDeclarations": declarations, "components": [], "collectionComplete": False,
        "sourceFunctionsExecuted": False, "upstreamConditionsExecuted": False, "generatorExecuted": False,
        "metadataPathsAliased": False, "shippingDecisionsMade": False, "completeChromiumNotice": False,
        "binarySbom": False, "releaseReady": False, "promoted": False}
    try:
        for key in PINS:
            component = EXTERNAL["collect_component"](key, output, inputs["references"], pins=PINS, extra_paths=EXTRA_PATHS[key])
            _, sha = INPUTS["receipt"](output / key / "license-source-inventory.json")
            summary = component_summary(key, component, sha)
            report["components"].append(summary)
            print(json.dumps(summary), flush=True)
        report["collectionComplete"] = True
    except Exception as error:
        report["failureType"] = type(error).__name__
        raise
    finally:
        EXTERNAL["write_report"](output / "angle-registry-source-inventory.json", report)
    return report


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--output-name", required=True)
    args = parser.parse_args()
    source = args.source.absolute()
    report = collect(source, source / args.output_name)
    print(json.dumps({"collectionComplete": report["collectionComplete"], "components": len(report["components"]), "promoted": False}))
