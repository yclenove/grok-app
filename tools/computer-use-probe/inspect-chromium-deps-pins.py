"""Static, non-executing DEPS/gitlink pin catalog for the fixed Chromium tree.

No eval/exec/gclient/hooks or upstream code imports. Conditions are preserved,
not guessed. This catalog is not a resolved platform checkout or binary SBOM.
"""
import argparse
import ast
import hashlib
import json
from pathlib import Path
import re
import runpy
from urllib.parse import urlsplit


HERE = Path(__file__).parent
TREE = runpy.run_path(str(HERE / "inspect-chromium-license-tree.py"))
COMMIT = TREE["COMMIT"]
DEPS_BLOB = "fb64ccf0c09313a5f3ec03d0fcb1b795f1573e09"


def assignments(contents):
    tree = ast.parse(contents, mode="exec")
    values = {}
    for statement in tree.body:
        if isinstance(statement, ast.Assign):
            for target in statement.targets:
                if isinstance(target, ast.Name) and target.id in ("vars", "deps"):
                    if target.id in values or not isinstance(statement.value, ast.Dict):
                        raise ValueError("one literal dictionary required for vars/deps")
                    values[target.id] = statement.value
    if set(values) != {"vars", "deps"}:
        raise ValueError("literal vars/deps dictionaries required")
    return values


def dictionary(node):
    if not isinstance(node, ast.Dict):
        raise ValueError("literal dictionary required")
    result = {}
    for key, value in zip(node.keys, node.values):
        if not isinstance(key, ast.Constant) or not isinstance(key.value, str) or key.value in result:
            raise ValueError("unique literal dictionary keys required")
        result[key.value] = value
    return result


def literal(node, variables, active=()):
    if isinstance(node, ast.Constant) and type(node.value) in (str, int, bool, type(None)):
        return node.value
    if isinstance(node, ast.List):
        return [literal(value, variables, active) for value in node.elts]
    if isinstance(node, ast.Dict):
        return {key: literal(value, variables, active) for key, value in dictionary(node).items()}
    if isinstance(node, ast.BinOp) and isinstance(node.op, ast.Add):
        left, right = literal(node.left, variables, active), literal(node.right, variables, active)
        if isinstance(left, str) and isinstance(right, str):
            return left + right
    if (isinstance(node, ast.Call) and isinstance(node.func, ast.Name)
            and node.func.id in ("Var", "Str") and len(node.args) == 1 and not node.keywords):
        key = literal(node.args[0], variables, active)
        if not isinstance(key, str):
            raise ValueError("literal Var/Str argument required")
        if node.func.id == "Str":
            return key
        if key not in variables or key in active:
            raise ValueError("unknown/cyclic Var reference")
        return literal(variables[key], variables, (*active, key))
    raise ValueError("unsupported expression preserved as unresolved, never executed")


def git_pin(value):
    if not isinstance(value, str) or "@" not in value:
        raise ValueError("explicit Git repository and revision required")
    url, revision = value.rsplit("@", 1)
    parsed = urlsplit(url)
    if (not re.fullmatch(r"[0-9a-f]{40}", revision) or parsed.scheme != "https"
            or parsed.username or parsed.password or parsed.query or parsed.fragment
            or not parsed.hostname or parsed.path in ("", "/")
            or "\\" in url or any(ord(char) <= 32 for char in url)):
        raise ValueError("credential-free HTTPS Git URL and immutable revision required")
    return url, revision


def expand_url_variables(value, variables):
    if not isinstance(value, str):
        raise ValueError("literal Git URL string required")

    def replace(match):
        key = match.group(1)
        if key not in variables:
            raise ValueError("unknown URL placeholder")
        replacement = literal(variables[key], variables, (key,))
        if not isinstance(replacement, str):
            raise ValueError("URL placeholder must resolve to a literal string")
        return replacement

    result = re.sub(r"\{([A-Za-z_][A-Za-z0-9_]*)\}", replace, value)
    if "{" in result or "}" in result:
        raise ValueError("only simple literal URL placeholders are supported")
    return result


def inspect(contents, rows, *, path_prefix="src/", source_commit=COMMIT):
    if path_prefix not in ("src/", "") or not TREE["SHA1"].fullmatch(source_commit):
        raise ValueError("explicit main or repository-relative DEPS layout and exact commit required")
    nodes = assignments(contents.decode("utf-8"))
    variables, dependencies = dictionary(nodes["vars"]), dictionary(nodes["deps"])
    gitlinks = {row["path"]: row["gitObjectSha1"] for row in rows if row["mode"] == "160000"}
    report = {"scope": "static DEPS/gitlink identity catalog, not checkout conditions, recursive source coverage or binary SBOM",
              "sourceCommit": source_commit, "depsBytes": len(contents), "depsSha256": hashlib.sha256(contents).hexdigest(),
              "gitDependencies": [], "nonGitDependencies": [], "unresolved": [], "mismatches": [],
              "uncataloguedGitlinks": [], "hooksExecuted": False, "conditionsEvaluated": False,
              "dependencySourcesCollected": False, "licenseCoverage": "not_proven", "promoted": False}
    matched = set()
    for source_path, expression in dependencies.items():
        # Non-Git archive/package destinations may explicitly name a directory.
        # Preserve its trailing slash as declaration data; never normalize it
        # into a Git checkout identity or materialize it on the filesystem.
        fields = dictionary(expression) if source_path.endswith("/") and isinstance(expression, ast.Dict) else {}
        directory_input = (source_path.endswith("/") and "dep_type" in fields
                           and literal(fields["dep_type"], variables) in ("cipd", "gcs"))
        path_parts = (source_path[:-1] if directory_input else source_path).split("/")
        if (not source_path.startswith(path_prefix) or "\\" in source_path or ":" in source_path
                or any(ord(char) < 32 for char in source_path)
                or any(part in ("", ".", "..") for part in path_parts)):
            raise ValueError("canonical layout-relative DEPS path required")
        path = source_path[len(path_prefix):]
        if not path:
            raise ValueError("nonempty dependency path required")
        try:
            if isinstance(expression, ast.Dict):
                fields = dictionary(expression)
                kind = literal(fields["dep_type"], variables) if "dep_type" in fields else "git"
                condition = literal(fields["condition"], variables) if "condition" in fields else None
                if kind != "git":
                    item = {"path": path, "kind": kind, "condition": condition,
                            "declaration": literal(expression, variables)}
                    if directory_input:
                        item["directoryPathDeclared"] = True
                    report["nonGitDependencies"].append(item)
                    continue
                value = literal(fields["url"], variables)
            else:
                condition, value = None, literal(expression, variables)
            url, revision = git_pin(expand_url_variables(value, variables))
        except (ValueError, KeyError) as error:
            report["unresolved"].append({"path": path, "reason": str(error), "nodeType": type(expression).__name__})
            continue
        actual = gitlinks.get(path)
        record = {"path": path, "repository": url, "revision": revision,
                  "condition": condition, "gitlinkRevision": actual, "gitlinkMatched": actual == revision}
        report["gitDependencies"].append(record)
        if actual is not None:
            if actual == revision:
                matched.add(path)
            else:
                report["mismatches"].append(record)
    report["uncataloguedGitlinks"] = [{"path": path, "revision": revision}
                                       for path, revision in sorted(gitlinks.items()) if path not in matched]
    return report


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", required=True, type=Path)
    parser.add_argument("--output-name", default="deps-pin-inventory.json")
    parser.add_argument("--tree-name", default="license-tree-02")
    args = parser.parse_args()
    source = args.source.absolute()
    TREE["regular_parents"](source)
    if (source.parent != HERE.absolute() / ".run"
            or not re.fullmatch(r"maintained-notice-source-[0-9]{8}-[0-9]{2}", source.name)):
        raise ValueError("explicit task-owned source snapshot required")
    if not re.fullmatch(r"deps-pin-inventory(?:-[0-9]{2})?\.json", args.output_name):
        raise ValueError("new safe catalog filename required")
    if not re.fullmatch(r"license-tree-[0-9]{2}", args.tree_name):
        raise ValueError("safe fixed tree generation required")
    output = source / args.output_name
    if output.exists() or output.is_symlink():
        raise FileExistsError("catalog evidence must be new")
    contents = (source / "DEPS.source-data").read_bytes()
    TREE["check_blob"](DEPS_BLOB, contents)
    tree_path = source / args.tree_name / "main-tree.nul"
    TREE["regular_parents"](tree_path)
    rows = TREE["tree_records"](tree_path.read_bytes())
    report = inspect(contents, rows)
    with output.open("x", encoding="utf-8") as stream:
        json.dump(report, stream, indent=2)
        stream.write("\n")
    print(json.dumps({key: len(report[key]) for key in ("gitDependencies", "nonGitDependencies", "unresolved",
          "mismatches", "uncataloguedGitlinks")}))
    if report["mismatches"]:
        raise SystemExit(2)
