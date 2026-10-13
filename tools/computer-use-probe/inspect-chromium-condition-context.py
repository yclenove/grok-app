"""Analyze four recorded dependency conditions over verified source defaults.

Boolean analysis is local and bounded. No upstream condition evaluator, code,
GN, hooks, browser or runtime executes. Recorded context is not a measurement
of the builder's .DEPS.git, effective graph or binary exclusions.
"""
import argparse
import ast
import hashlib
import json
from pathlib import Path
import re
import runpy


HERE = Path(__file__).parent.absolute()
ASSEMBLY = runpy.run_path(str(HERE / "assemble-chromium-notice-candidate.py"))
DAWN, INPUTS = ASSEMBLY["DAWN"], ASSEMBLY["INPUTS"]
INTEGRATION = DAWN["INTEGRATION"]
GCLIENT = INTEGRATION["GCLIENT"]
DEPS = DAWN["RECURSIVE"]["DEPS"]
ASSEMBLY_SHA = "b55239a307fcc32c5e6d1e5e3ad801fffc5cfea18b56932633be67aa86c5a2cd"
RECIPE_SHA = "1c52c04129bc3411152932526873b94ee8b642e837728bbbc37aa61bc572cd58"
FLAGS = {"build_with_chromium": True, "checkout_fuchsia_no_hooks": False}
CUSTOM_VARS = {"download_remoteexec_cfg": "True",
    "rbe_instance": "projects/rbe-chromium-trusted/instances/default_instance"}
CONDITIONS = {
    "third_party/angle/third_party/OpenCL-ICD-Loader": "checkout_angle_cl_deps and not build_with_chromium",
    "third_party/angle/third_party/astc-encoder": "not build_with_chromium",
    "third_party/angle/third_party/cpu_features": "checkout_android and not build_with_chromium",
    "third_party/fuchsia-sdk": "checkout_fuchsia_no_hooks",
}
SYNC = "sync --verbose --reset --force --upstream --nohooks --noprehooks --delete_unversioned_trees --revision src@unmanaged"
MAX_EXPRESSION = 512


def analyze_boolean(expression, context):
    if (not isinstance(expression, str) or not expression or len(expression) > MAX_EXPRESSION
            or not isinstance(context, dict) or len(context) > 32
            or any(not isinstance(key, str) or not re.fullmatch(r"[a-z][a-z0-9_]*", key)
                or (type(value) is not bool and value is not None) for key, value in context.items())):
        raise ValueError("bounded explicit boolean context required")
    try:
        tree = ast.parse(expression, mode="eval")
    except SyntaxError as error:
        raise ValueError("boolean expression syntax required") from error
    nodes = list(ast.walk(tree))
    allowed = (ast.Expression, ast.BoolOp, ast.And, ast.Or, ast.UnaryOp, ast.Not, ast.Name, ast.Load, ast.Constant)
    if len(nodes) > 64 or any(not isinstance(row, allowed) for row in nodes):
        raise ValueError("only names, boolean literals, and/or/not permitted")
    for row in nodes:
        if isinstance(row, ast.Constant) and type(row.value) is not bool:
            raise ValueError("boolean constants only; no truthiness conversion")
        if isinstance(row, ast.Name) and not re.fullmatch(r"[a-z][a-z0-9_]*", row.id):
            raise ValueError("canonical boolean variable required")

    def reduce(node):
        if isinstance(node, ast.Name):
            return context.get(node.id)
        if isinstance(node, ast.Constant):
            return node.value
        if isinstance(node, ast.UnaryOp):
            value = reduce(node.operand)
            return None if value is None else not value
        values = [reduce(child) for child in node.values]
        if isinstance(node.op, ast.And):
            return False if False in values else (None if None in values else True)
        return True if True in values else (None if None in values else False)

    return {"expression": expression, "knownContext": dict(context), "localBooleanResult": reduce(tree.body),
        "unknownVariables": sorted({row.id for row in nodes if isinstance(row, ast.Name) and context.get(row.id) is None}),
        "upstreamConditionExecuted": False, "actualConditionMeasured": False, "binaryExclusionProven": False}


def boolean_defaults(contents):
    variables = DEPS["dictionary"](DEPS["assignments"](contents.decode())["vars"])
    result = {}
    for name, expected in FLAGS.items():
        node = variables.get(name)
        if not isinstance(node, ast.Constant) or type(node.value) is not bool or node.value is not expected:
            raise ValueError("exact pinned literal Chromium flag default required")
        result[name] = {"value": node.value, "sourcePath": "DEPS", "line": node.lineno,
            "sourceSha256": hashlib.sha256(contents).hexdigest()}
    return result


def builtin_names(contents):
    tree = ast.parse(contents.decode())
    function = GCLIENT["function"](tree, "get_builtin_vars", "Dependency")
    returns = [row.value for row in ast.walk(function) if isinstance(row, ast.Return)]
    if len(returns) != 1 or not isinstance(returns[0], ast.Dict):
        raise ValueError("one original builtin-variable dictionary required")
    names = [ast.literal_eval(key) for key in returns[0].keys]
    if not all(isinstance(key, str) for key in names) or len(set(names)) != len(names) or set(FLAGS).intersection(names):
        raise ValueError("no builtin override of either reviewed flag allowed")
    return names


def target_contexts(args, defaults, names):
    expected = set(INTEGRATION["CRASHPAD"]["ARGS"]["PLATFORMS"])
    targets = args["targets"]
    if (len(targets) != 4 or {row["target"] for row in targets} != expected or set(FLAGS).intersection(names)
            or set(defaults) != set(FLAGS)
            or any(type(defaults[key].get("value")) is not bool or defaults[key]["value"] is not value
                for key, value in FLAGS.items())):
        raise ValueError("all four unique verified non-overridden contexts required")
    result = []
    for row in targets:
        config = row["gclientConfigRecorded"]
        if (row.get("actualGclientCustomVarsVerified") is not True
                or config.get("depsFileRecorded") != ".DEPS.git"
                or config.get("customDeps") != {} or config.get("targetOs") != [] or config.get("targetCpu") != []
                or config.get("customVars") != CUSTOM_VARS):
            raise ValueError("original complete config with no flag/dependency override required")
        result.append({"target": row["target"], "buildId": row["buildId"], "configuration": config,
            "effectiveFlagsFromRecordedContextAndPinnedRules": {key: value["value"] for key, value in defaults.items()},
            "builderDepsFileBytesMeasured": False, "actualConditionMeasured": False, "binaryExclusionProven": False})
    return result


def sync_evidence(contents, target):
    if target not in INTEGRATION["CRASHPAD"]["ARGS"]["PLATFORMS"]:
        raise ValueError("one original target required")
    lines = contents.decode().splitlines()
    starts = [(number, line) for number, line in enumerate(lines, 1) if "===Running " in line and "gclient.py sync " in line]
    if len(starts) != 1 or not starts[0][1].endswith("gclient.py " + SYNC + " ==="):
        raise ValueError("one exact original sync argv without condition/OS overrides required")
    finished = [(number, line) for number, line in enumerate(lines, 1)
        if "===Succeeded " in line and "gclient.py sync " in line]
    failed = [line for line in lines if "===Failed " in line and "gclient.py sync " in line]
    if (failed or len(finished) > 1
            or (finished and (finished[0][0] <= starts[0][0]
                or not finished[0][1].endswith("gclient.py " + SYNC + " ===")))
            or (target != "windows-x64" and not finished)):
        raise ValueError("unambiguous ordered original completion required for full stdout")
    return {"target": target, "line": starts[0][0], "text": starts[0][1],
        "sourceSha256": hashlib.sha256(contents).hexdigest(), "recordedArgs": SYNC,
        "syncDisablesHooks": True, "syncCompletionRecorded": bool(finished),
        "fullStdoutAvailable": target != "windows-x64", "postSyncHookExecutionVerified": False}


def dependency_analysis(obligations, contexts):
    if len(obligations) != 4 or {row["originalOmittedMetadata"]["directory"] for row in obligations} != set(CONDITIONS):
        raise ValueError("all four original unresolved metadata records required")
    result = []
    for obligation in obligations:
        directory = obligation["originalOmittedMetadata"]["directory"]
        declarations = obligation["declarations"]
        if (len(declarations) != 1 or declarations[0]["conditionRecorded"] != CONDITIONS[directory]
                or obligation.get("originalErrorRemoved") is not False or obligation.get("binaryExclusionProven") is not False):
            raise ValueError("exact original condition and unchanged metadata scope required")
        comparisons = obligation["recordedManifestEntries"]
        if (len(comparisons) != 4 or {row["target"] for row in comparisons} != {row["target"] for row in contexts}
                or any(row["entries"] != {} for row in comparisons)):
            raise ValueError("all original empty manifests must remain explicit")
        analyzed = [{"target": row["target"], **analyze_boolean(CONDITIONS[directory], row["effectiveFlagsFromRecordedContextAndPinnedRules"])} for row in contexts]
        if any(row["localBooleanResult"] is not False for row in analyzed):
            raise ValueError("condition must be false under every verified recorded context")
        result.append({"originalObligation": obligation, "recordedContextAnalysis": analyzed,
            "collectionAction": "do_not_collect_standalone_or_sdk_source_just_to_clear_raw_count",
            "originalErrorRemoved": False, "actualDependencyConditionMeasured": False,
            "binaryExclusionProven": False, "completeComponentNoticeCovered": False})
    return result


def inspect(source, output):
    INPUTS["checked_source"](source)
    if output.parent != source or not re.fullmatch(r"condition-context-[0-9]{2}\.json", output.name):
        raise ValueError("safe fresh task-owned condition-context output required")
    if output.exists() or output.is_symlink():
        raise FileExistsError("never overwrite condition analysis evidence")
    inputs, dawn, _ = DAWN["load_dawn"](source)
    assembled, assembled_sha = INPUTS["receipt"](source / "notice-assembly-01/notice-assembly-inventory.json", ASSEMBLY_SHA)
    if assembled["originalRawErrorsRetained"] != dawn["after"] or assembled["releaseReady"] is not False:
        raise ValueError("entire original raw-error state and incomplete assembly required")
    helpers, helper_sha = GCLIENT["load_helpers"](source)
    shapes = GCLIENT["inspect_shapes"](helpers["gclient.py"], helpers["gclient_eval.py"])
    names = builtin_names(helpers["gclient.py"])
    defaults = boolean_defaults(inputs["files"]["DEPS"]["payload"])
    args = INTEGRATION["CRASHPAD"]["ARGS"]["inspect"](source)
    contexts = target_contexts(args, defaults, names)
    recipe, recipe_sha = INPUTS["receipt"](source / "recipe-provenance-01.json", RECIPE_SHA)
    if (recipe["sourceCommit"] != INPUTS["TREE"]["COMMIT"]
            or recipe["configuredDepsFile"] != ".DEPS.git" or recipe["fixedSourceFallbackFile"] != "DEPS"
            or recipe["depsFallbackImplementationVerified"] is not True or recipe["builderFilesystemMeasured"] is not False):
        raise ValueError("original verified fallback mechanism and measurement limitation required")
    DAWN["SOURCES"]["CHECKOUTS"]["inspect"](source)
    sync = [sync_evidence(INPUTS["read_data"](source / "archive-checkout-logs-03" / (row["target"] + "-checkout.log-data"),
        DAWN["SOURCES"]["CHECKOUTS"]["LOGS"]["MAX_LOG_BYTES"]), row["target"]) for row in contexts]
    analyzed = dependency_analysis(assembled["releaseBlockers"]["unresolvedOriginalMetadata"], contexts)
    report = {"scope": "four dependency conditions under source defaults and recorded contexts only, not actual binary exclusion",
        "sourceCommit": INPUTS["TREE"]["COMMIT"], "originalAssemblyReceiptSha256": assembled_sha,
        "helperSourceReceiptSha256": helper_sha, "helperPrecedenceSourceEvidence": shapes,
        "builtinVariableKeys": names, "rootBooleanDefaults": defaults, "targetContexts": contexts,
        "originalRecipeReceiptSha256": recipe_sha, "configuredDepsFallbackEvidence": {
            "configured": ".DEPS.git", "source": "DEPS", "implementationVerified": True, "builderFilesystemMeasured": False},
        "originalSyncCommandEvidence": sync, "dependencyConditions": analyzed,
        "fuchsiaOtherInputsUnproven": ["Post-sync Download Fuchsia SDK hook uses checkout_fuchsia and not checkout_fuchsia_no_hooks",
            "Builtin checkout_fuchsia and post-sync hook/binary behavior are not measured by this local flag analysis"],
        "originalRawErrorsRetained": dawn["after"], "localBooleanAnalysisPerformed": True,
        "sourceFunctionsExecuted": False, "upstreamConditionsExecuted": False, "actualEffectiveConditionsMeasured": False,
        "metadataPathsAliased": False, "originalErrorsRemoved": False, "shippingExclusionsProven": False,
        "completeChromiumNotice": False, "releaseReady": False, "promoted": False}
    INPUTS["EXTERNAL"]["write_report"](output, report)
    return report


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--output-name", required=True)
    args = parser.parse_args()
    source = args.source.absolute()
    result = inspect(source, source / args.output_name)
    print(json.dumps({"dependencies": len(result["dependencyConditions"]), "recordedContexts": len(result["targetContexts"]),
        "falseInRecordedContext": sum(row["localBooleanResult"] is False for dep in result["dependencyConditions"] for row in dep["recordedContextAnalysis"]),
        "actualEffectiveConditionsMeasured": False, "releaseReady": False}))
