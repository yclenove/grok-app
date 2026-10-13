"""Catalog fixed ANGLE/OpenScreen DEPS/gitlinks as data, not a checkout.

Verifies sealed source inputs before parsing relative paths. Parent/default
variable differences are evidence, not evaluated inheritance or binary usage.
Never fetch recursive remotes, run gclient/hooks, or remove license errors.
"""
import argparse
import ast
import hashlib
import json
from pathlib import Path
import re
import runpy


HERE = Path(__file__).parent.absolute()
INPUTS = runpy.run_path(str(HERE / "inspect-chromium-license-input-gaps.py"))
DEPS = runpy.run_path(str(HERE / "inspect-chromium-deps-pins.py"))
EXTERNAL = INPUTS["EXTERNAL"]
ANGLE_DEPS_SHA1 = "58285b6d243a5c15ee0a1a690962c38f43d4b3e8"
COMPONENTS = {
    "angle": ("PINS", ANGLE_DEPS_SHA1, "CORE_SEALS"),
    "openscreen": ("BROWSER_PINS", "22bc9c6438d34f9ea6b97158bca166eb4b4279cf", "BROWSER_SEALS"),
    "dawn": ("GPU_PINS", "da0a3aa20e5c92253b8a4662f56613f1a088cfbf", "GPU_SEALS"),
}


def component_pin(component):
    if component not in COMPONENTS:
        raise ValueError("only compiled recursive source identities allowed")
    group, deps_oid, seals = COMPONENTS[component]
    return EXTERNAL[group][component], deps_oid, INPUTS[seals][component]


def assignment(contents, name):
    matches = [row.value for row in ast.parse(contents.decode("utf-8")).body
               if isinstance(row, ast.Assign) and any(isinstance(target, ast.Name) and target.id == name
                                                      for target in row.targets)]
    if len(matches) != 1:
        raise ValueError("one explicit literal source assignment required: " + name)
    return DEPS["literal"](matches[0], {})


def build_report(root_contents, relative_contents, rows, component="angle"):
    # These payloads are verified before use by inspect; this function is pure
    # and also handles synthetic bytes in tests without imports or execution.
    (prefix, repository, revision), _, _ = component_pin(component)
    root_nodes = DEPS["assignments"](root_contents.decode("utf-8"))
    relative_nodes = DEPS["assignments"](relative_contents.decode("utf-8"))
    root_vars = DEPS["dictionary"](root_nodes["vars"])
    relative_vars = DEPS["dictionary"](relative_nodes["vars"])
    parent_default = DEPS["literal"](root_vars["build_with_chromium"], root_vars)
    standalone_default = DEPS["literal"](relative_vars["build_with_chromium"], relative_vars)
    relative_layout = assignment(relative_contents, "use_relative_paths")
    recursedeps = assignment(root_contents, "recursedeps")
    if (relative_layout is not True or type(parent_default) is not bool or type(standalone_default) is not bool
            or not isinstance(recursedeps, list) or len(recursedeps) > 1000
            or not all(isinstance(path, str) for path in recursedeps)
            or len(set(recursedeps)) != len(recursedeps)):
        raise ValueError("explicit bounded recursion list and boolean defaults/layout required")
    for path in recursedeps:
        if (not path.startswith("src/") or "\\" in path or ":" in path or any(ord(char) < 32 for char in path)
                or any(part in ("", ".", "..") for part in path.split("/"))):
            raise ValueError("canonical parent recursedeps path required")
    catalog = DEPS["inspect"](relative_contents, rows, path_prefix="", source_commit=revision)
    catalog.update(scope="fixed " + component + " standalone DEPS declarations only, not inherited platform checkout or binary SBOM",
                   parentSourceCommit=INPUTS["TREE"]["COMMIT"], parentDepsSha256=hashlib.sha256(root_contents).hexdigest(),
                   dependencyComponent=component, parentCheckoutPath=prefix, repository=repository, relativeLayoutDeclared=True,
                   parentRecursedeps=recursedeps, parentDeclaresComponentRecursion="src/" + prefix in recursedeps,
                   declaredBuildWithChromiumDefaults={"parent": parent_default, component + "Standalone": standalone_default},
                   inheritanceVerified=False, builderCustomVarsKnown=False, sourceFunctionsExecuted=False,
                   recursiveSourcesFetched=False, binaryComponentCoverage="not_proven", shippingDecisionsMade=False)
    if component == "angle":
        catalog["parentDeclaresAngleRecursion"] = catalog["parentDeclaresComponentRecursion"]
    if component == "dawn":
        value = DEPS["literal"](relative_vars["dawn_standalone"], relative_vars)
        if type(value) is not bool:
            raise ValueError("explicit Dawn standalone boolean default required")
        catalog["declaredDawnStandaloneDefault"] = value
    return catalog


def inspect(source, output, component="angle"):
    (prefix, _, _), deps_oid, component_seal = component_pin(component)
    if output.parent != source or not re.fullmatch(r"recursive-deps-" + component + r"-[0-9]{2}\.json", output.name):
        raise ValueError("safe fresh recursive evidence name required")
    if output.exists() or output.is_symlink():
        raise FileExistsError("never overwrite recursive source evidence")
    inputs = INPUTS["load_inputs"](source, include_browser=component == "openscreen")
    gpu_sha = None
    if component == "dawn":
        gpu_sha = INPUTS["append_external"](source, "external-license-08", INPUTS["GPU_SHA"],
            EXTERNAL["GPU_PINS"], INPUTS["GPU_SEALS"], inputs["index"], inputs["files"], inputs["references"], inputs["components"])
    deps_path = prefix + "/DEPS"
    entry = inputs["files"][deps_path]
    if entry["gitObjectSha1"] != deps_oid:
        raise ValueError("exact reviewed recursive DEPS Git object required")
    rows = [{**row, "path": path[len(prefix) + 1:]} for path, row in inputs["index"].items()
            if path.startswith(prefix + "/")]
    report = build_report(inputs["files"]["DEPS"]["payload"], entry["payload"], rows, component)
    report.update(mainReceiptSha256=inputs["mainSha256"], externalReceiptSha256=inputs["externalSha256"],
                  dependencyReceiptSha256=component_seal, dependencyDepsGitObjectSha1=deps_oid)
    if component == "angle":
        report.update(angleReceiptSha256=component_seal, angleDepsGitObjectSha1=deps_oid)
    elif component == "openscreen":
        report["browserReceiptSha256"] = inputs["browserSha256"]
    else:
        report["gpuReceiptSha256"] = gpu_sha
    EXTERNAL["write_report"](output, report)
    return report


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--output-name", required=True)
    parser.add_argument("--component", choices=tuple(COMPONENTS), default="angle")
    args = parser.parse_args()
    source = args.source.absolute()
    report = inspect(source, source / args.output_name, args.component)
    print(json.dumps({key: len(report[key]) for key in ("gitDependencies", "nonGitDependencies", "unresolved",
                      "mismatches", "uncataloguedGitlinks")}, sort_keys=True))
    if report["mismatches"] or report["uncataloguedGitlinks"] or report["unresolved"]:
        raise SystemExit(2)
