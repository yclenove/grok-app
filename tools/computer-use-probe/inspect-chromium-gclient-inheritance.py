"""Verify reviewed gclient variable-precedence source shapes, without execution.

The same-parent pinned helper source/test receipt is independently sealed.
AST comparisons and boolean examples are local analysis, not gclient sync,
upstream tests, platform conditions, actual builder settings or shipping proof.
"""
import argparse
import ast
import json
from pathlib import Path
import re
import runpy
import textwrap


HERE = Path(__file__).parent.absolute()
COLLECTOR = runpy.run_path(str(HERE / "collect-chromium-gclient-sources.py"))
INPUTS = COLLECTOR["INPUTS"]
TREE = INPUTS["TREE"]
SOURCE_SHA = "7875895d63dc45e10361ca09506a9db07a9b25896509a3be8791dcdb005a61e3"
GET_VARS = """
result = {}
result.update(self._vars)
if self.parent:
    merge_vars(result, self.parent.get_vars())
result.update(self.get_builtin_vars())
merge_vars(result, self.custom_vars)
return result
"""
MERGE_VARS = """
for k, v in new_vars.items():
    if k in result:
        if isinstance(result[k], gclient_eval.ConstantString):
            if isinstance(v, gclient_eval.ConstantString):
                result[k] = v
            else:
                result[k].value = v
        else:
            result[k] = v
    else:
        result[k] = v
"""
PARENT_OVERRIDE = """
self._vars = local_scope.get("vars", {})
if self.parent:
    for key, value in self.parent.get_vars().items():
        if key in self._vars:
            self._vars[key] = value
self._vars = gclient_utils.freeze(self._vars)
"""
PARSE_VARS = """
vars_dict = {}
if "vars" in statements:
    vars_statement = statements["vars"]
    value = _gclient_eval(vars_statement, filename)
    local_scope.SetNode("vars", value, vars_statement)
    vars_dict.update(value)
if builtin_vars:
    vars_dict.update(builtin_vars)
if vars_override:
    vars_dict.update({k: v for k, v in vars_override.items() if k in vars_dict})
"""


def function(tree, name, class_name=None):
    scope = tree.body
    if class_name is not None:
        classes = [row for row in scope if isinstance(row, ast.ClassDef) and row.name == class_name]
        if len(classes) != 1:
            raise ValueError("one fixed source class required")
        scope = classes[0].body
    functions = [row for row in scope if isinstance(row, ast.FunctionDef) and row.name == name]
    if len(functions) != 1:
        raise ValueError("one fixed source function required")
    return functions[0]


def statements(function_node):
    rows = function_node.body
    if (rows and isinstance(rows[0], ast.Expr) and isinstance(rows[0].value, ast.Constant)
            and isinstance(rows[0].value.value, str)):
        rows = rows[1:]
    return rows


def same_shape(rows, expected):
    return ast.dump(ast.Module(body=rows, type_ignores=[])) == ast.dump(ast.parse(textwrap.dedent(expected)))


def excerpt(contents, rows):
    start, end = rows[0].lineno, rows[-1].end_lineno
    return {"startLine": start, "endLine": end,
            "sourceExcerpt": "\n".join(contents.decode("utf-8").splitlines()[start - 1:end])}


def checked_sequence(contents, node, expected):
    wanted = len(ast.parse(textwrap.dedent(expected)).body)
    rows = statements(node)
    matches = [rows[index:index + wanted] for index in range(len(rows) - wanted + 1)
               if same_shape(rows[index:index + wanted], expected)]
    if len(matches) != 1:
        raise ValueError("one exact reviewed inheritance statement sequence required")
    return excerpt(contents, matches[0])


def inspect_shapes(gclient_contents, evaluator_contents):
    gclient = ast.parse(gclient_contents.decode("utf-8"))
    evaluator = ast.parse(evaluator_contents.decode("utf-8"))
    getter = function(gclient, "get_vars", "Dependency")
    merge = function(gclient, "merge_vars")
    if not same_shape(statements(getter), GET_VARS) or not same_shape(statements(merge), MERGE_VARS):
        raise ValueError("exact reviewed gclient precedence and merge implementation required")
    parser = function(gclient, "ParseDepsFile", "Dependency")
    parse_calls = [row for row in ast.walk(parser) if isinstance(row, ast.Call)
                   and isinstance(row.func, ast.Attribute) and row.func.attr == "Parse"
                   and isinstance(row.func.value, ast.Name) and row.func.value.id == "gclient_eval"]
    expected = ast.parse("gclient_eval.Parse(deps_content, filepath, self.get_vars(), self.get_builtin_vars())").body[0].value
    if len(parse_calls) != 1 or ast.dump(parse_calls[0]) != ast.dump(expected):
        raise ValueError("exact ParseDepsFile variable-override call required")
    return {"getVarsPrecedence": excerpt(gclient_contents, [getter]),
            "mergeVars": excerpt(gclient_contents, [merge]),
            "parentOverridesDeclaredChildVariables": checked_sequence(gclient_contents, parser, PARENT_OVERRIDE),
            "parseOverrideForDeclaredVarsOnly": checked_sequence(evaluator_contents, function(evaluator, "Exec"), PARSE_VARS)}


def scalar_example(child, parent, builtin=None, custom=None):
    if type(child) is not bool or type(parent) is not bool or any(
            type(value) is not bool and value is not None for value in (builtin, custom)):
        raise ValueError("explicit local boolean example only, not a condition evaluator")
    result = parent
    if builtin is not None:
        result = builtin
    if custom is not None:
        result = custom
    return {"childStandaloneDefault": child, "parentValueAssumed": parent,
            "builtinOverrideAssumed": builtin, "customOverrideAssumed": custom,
            "exampleEffectiveBoolean": result, "hypotheticalOnly": True, "actualBuilderValue": "unknown"}


def load_helpers(source):
    INPUTS["checked_source"](source)
    directory = source / "gclient-source-01"
    report, sha = INPUTS["receipt"](directory / "gclient-source-inventory.json", SOURCE_SHA)
    _, repository, revision = INPUTS["EXTERNAL"]["PINS"]["depot-tools"]
    if ((report["repository"], report["revision"], report["parentSourceCommit"]) != (repository, revision, TREE["COMMIT"])
            or report["depotReceiptSha256"] != INPUTS["CORE_SEALS"]["depot-tools"]
            or report["collectionComplete"] is not True or len(report["selectedFiles"]) != len(COLLECTOR["FILES"])
            or any(report[key] is not False for key in ("sourceFunctionsExecuted", "upstreamTestsExecuted",
                                                       "hooksExecuted", "conditionsEvaluated", "promoted"))):
        raise ValueError("exact complete nonexecuted helper receipt required")
    rows = COLLECTOR["selected"](report["selectedFiles"])
    payloads = {}
    for row in rows:
        contents = INPUTS["read_data"](directory / "blobs" / (row["gitObjectSha1"] + ".source-data"), TREE["MAX_BLOB"])
        identity = TREE["check_blob"](row["gitObjectSha1"], contents)
        if identity["bytes"] != row["bytes"] or identity["sha256"] != row["sha256"]:
            raise ValueError("helper bytes differ from original sealed Git/hash identity")
        payloads[row["path"]] = contents
    if sum(len(contents) for contents in payloads.values()) != report["sourceBytes"]:
        raise ValueError("sealed helper total bytes mismatch")
    return payloads, sha


def inspect(source, output):
    if output.parent != source or not re.fullmatch(r"gclient-inheritance-[0-9]{2}\.json", output.name):
        raise ValueError("safe fresh inheritance evidence name required")
    if output.exists() or output.is_symlink():
        raise FileExistsError("never overwrite inheritance evidence")
    helpers, sha = load_helpers(source)
    excerpts = inspect_shapes(helpers["gclient.py"], helpers["gclient_eval.py"])
    report = {"scope": "verified pinned source mechanism and hypothetical scalar examples, not actual builder inheritance",
              "parentSourceCommit": TREE["COMMIT"], "helperSourceReceiptSha256": sha,
              "precedenceFromLowestToHighest": ["child DEPS vars", "parent effective vars", "builtin vars", "custom vars"],
              "sourceShapesVerified": True, "sourceEvidence": excerpts,
              "booleanExamples": [scalar_example(False, True), scalar_example(False, True, custom=False),
                                  scalar_example(False, True, builtin=False, custom=True)],
              "sourceFunctionsExecuted": False, "upstreamTestsExecuted": False, "conditionsEvaluated": False,
              "actualBuilderInheritanceVerified": False, "builderCustomVarsKnown": False,
              "actualPlatformCheckoutKnown": False, "binaryComponentCoverage": "not_proven",
              "licenseErrorsExcluded": False, "shippingDecisionsMade": False, "promoted": False}
    INPUTS["EXTERNAL"]["write_report"](output, report)
    return report


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--output-name", required=True)
    args = parser.parse_args()
    source = args.source.absolute()
    report = inspect(source, source / args.output_name)
    print(json.dumps({key: report[key] for key in ("sourceShapesVerified", "actualBuilderInheritanceVerified",
                      "builderCustomVarsKnown", "licenseErrorsExcluded", "promoted")}))
