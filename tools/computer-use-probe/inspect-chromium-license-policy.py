"""Static fixed-policy scan over verified Git indexes, not an upstream run.

No source/DEPS execution, imports, checkout, filesystem materialization or
network. Counts default Credits metadata candidates, not binary components.
The incomplete checkout and original source/notice obligations stay open.
"""
import argparse
import ast
from collections import defaultdict, deque
import hashlib
import json
from pathlib import Path
import posixpath
import re
import runpy


HERE = Path(__file__).parent.absolute()
INPUTS = runpy.run_path(str(HERE / "inspect-chromium-license-input-gaps.py"))
POLICY_SHA = "b312c62523e955b1893923e8b45c13092eb58a6df22504c4e045205bf1904063"
POLICY_FIELDS = {"METADATA_FILE_NAMES", "PRUNE_PATHS", "VCS_METADATA_DIRS", "PRUNE_DIRS",
                 "ADDITIONAL_PATHS_FILENAME", "ADDITIONAL_PATHS", "SPECIAL_CASES", "MANDATORY_FIELDS",
                 "ALIAS_FIELDS", "OPTIONAL_FIELDS", "MULTIVALUE_FIELDS", "PATTERN_DEPENDENCY_DIVIDER",
                 "VALUE_DELIMITER", "NOT_SHIPPED", "YES", "NO"}


def dotted_name(node):
    if isinstance(node, ast.Name):
        return node.id
    if isinstance(node, ast.Attribute):
        parent = dotted_name(node.value)
        return parent + "." + node.attr if parent is not None else None
    return None


def literal(node, values):
    if isinstance(node, ast.Constant) and type(node.value) in (str, int, bool, type(None)):
        return node.value
    if isinstance(node, ast.Name) and node.id in values:
        return values[node.id]
    if isinstance(node, (ast.List, ast.Tuple, ast.Set)):
        items = [literal(item, values) for item in node.elts]
        return {ast.List: list, ast.Tuple: tuple, ast.Set: set}[type(node)](items)
    if isinstance(node, ast.Dict):
        result = {}
        for key, value in zip(node.keys, node.values):
            key = literal(key, values)
            if key in result:
                raise ValueError("duplicate fixed policy key")
            result[key] = literal(value, values)
        return result
    if isinstance(node, ast.BinOp) and isinstance(node.op, ast.Add):
        left, right = literal(node.left, values), literal(node.right, values)
        if type(left) is type(right) and isinstance(left, (tuple, str)):
            return left + right
    if isinstance(node, ast.Call) and not node.keywords:
        args = [literal(arg, values) for arg in node.args]
        name = dotted_name(node.func)
        if name in ("set", "frozenset") and len(args) == 1:
            return set(args[0]) if name == "set" else frozenset(args[0])
        if name == "os.path.join" and args and all(isinstance(arg, str) for arg in args):
            return posixpath.join(*args)
        if name == "re.compile" and len(args) == 1 and isinstance(args[0], str):
            return re.compile(args[0])
    raise ValueError("unsupported fixed policy expression, never executed")


def parse_policy(contents):
    if hashlib.sha256(contents).hexdigest() != POLICY_SHA:
        raise ValueError("reviewed exact Chromium license policy SHA256 required")
    values = {}
    for statement in ast.parse(contents.decode("utf-8")).body:
        if (isinstance(statement, ast.Assign) and len(statement.targets) == 1
                and isinstance(statement.targets[0], ast.Name) and statement.targets[0].id in POLICY_FIELDS):
            name = statement.targets[0].id
            if name in values:
                raise ValueError("ambiguous fixed policy assignment")
            values[name] = literal(statement.value, values)
        elif (isinstance(statement, ast.Expr) and isinstance(statement.value, ast.Call)
              and dotted_name(statement.value.func) == "PRUNE_PATHS.update"):
            call = statement.value
            if len(call.args) != 1 or call.keywords:
                raise ValueError("unsupported fixed prune update")
            values["PRUNE_PATHS"].update(literal(call.args[0], values))
    if set(values) != POLICY_FIELDS:
        raise ValueError("all reviewed policy fields required")
    return values


def safe_join(base, path):
    if not isinstance(path, str) or "\\" in path or ":" in path or any(ord(char) < 32 for char in path):
        raise ValueError("unsafe source-relative policy path")
    joined = posixpath.normpath(posixpath.join("" if path.startswith("/") else base, path.lstrip("/")))
    if joined == ".." or joined.startswith(("../", "/")):
        raise ValueError("policy path escapes Chromium root")
    return "" if joined == "." else joined


class SourceView:
    def __init__(self, index, payloads, vcs_dirs=(".git", ".svn")):
        self.index, self.payloads = index, payloads
        self.children = defaultdict(set)
        self.contains_files = set()
        for path, row in index.items():
            parts = path.split("/")
            directories = parts if row["mode"] == "160000" else parts[:-1]
            for depth, child in enumerate(directories):
                self.children["/".join(directories[:depth])].add(child)
            if row["mode"] != "160000" and not any(part in vcs_dirs for part in parts[:-1]):
                self.contains_files.update("/".join(parts[:depth]) for depth in range(len(parts)))

    def read(self, path):
        if path not in self.payloads:
            raise ValueError("policy input bytes absent from verified source collection: " + path)
        return self.payloads[path]

    def regular(self, path):
        return path in self.index and self.index[path]["mode"] in ("100644", "100755")


def discover(view, policy):
    candidates, json_paths = set(), []

    def add(path):
        if path in policy["PRUNE_PATHS"] or path in candidates or path not in view.contains_files:
            return
        if len(candidates) >= 12000:
            raise ValueError("policy candidate budget")
        candidates.add(path)
        extra = safe_join(path, policy["ADDITIONAL_PATHS_FILENAME"])
        if not view.regular(extra):
            return
        values = json.loads(view.read(extra))
        if not isinstance(values, list) or len(values) > 5000 or not all(isinstance(value, str) for value in values):
            raise ValueError("bounded additional path list required")
        json_paths.append(extra)
        for value in values:
            add(safe_join(path, value))

    queue = deque([""])
    while queue:
        path = queue.popleft()
        if path in policy["PRUNE_PATHS"] or path.startswith("out"):
            continue
        children = set(view.children[path])
        if path in policy["ADDITIONAL_PATHS"]:
            children.clear()
        children.difference_update(policy["PRUNE_DIRS"])
        if posixpath.basename(path) == "third_party":
            for child in sorted(children):
                add(safe_join(path, child))
        parts = path.split("/")
        if len(parts) == 4 and parts[:2] == ["third_party", "rust"] and parts[2] != "chromium_crates_io":
            add(path)
        queue.extend(safe_join(path, child) for child in sorted(children))
    for path in policy["ADDITIONAL_PATHS"]:
        add(path)
    return sorted(candidates), sorted(set(json_paths))


def parse_metadata(contents, policy):
    names = policy["MANDATORY_FIELDS"] | set(policy["ALIAS_FIELDS"]) | set(policy["OPTIONAL_FIELDS"])
    lookup = {name.lower(): name for name in names}
    result, current = [], {}
    for line in contents.decode("utf-8", errors="replace").splitlines():
        line = line.strip()
        if not line:
            continue
        if policy["PATTERN_DEPENDENCY_DIVIDER"].match(line):
            if current:
                result.append(current)
            current = {}
            continue
        parts = line.split(": ", 1)
        field = lookup.get(parts[0].lower()) if len(parts) == 2 else None
        if field is None:
            continue
        if field in current:
            raise ValueError("duplicate metadata field: " + field)
        current[field] = ([value.strip() for value in parts[1].split(policy["VALUE_DELIMITER"])]
                          if field in policy["MULTIVALUE_FIELDS"] else parts[1])
    if current:
        result.append(current)
    return result


def process(metadata, directory, view, policy):
    metadata = dict(metadata)
    for alias, field in policy["ALIAS_FIELDS"].items():
        if alias in metadata:
            metadata[field] = metadata.pop(alias)
    if metadata.get("License File") is None:
        metadata["License File"] = ["LICENSE"]
    if not metadata.get("Shipped"):
        metadata["Shipped"] = (policy["NO"] if policy["NOT_SHIPPED"] in metadata["License File"] else policy["YES"])
    errors = ["missing mandatory field: " + field for field in sorted(policy["MANDATORY_FIELDS"]) if not metadata.get(field)]
    declared, available, missing = [], [], []
    for value in metadata["License File"]:
        if value == policy["NOT_SHIPPED"]:
            continue
        try:
            path = safe_join(directory, value)
        except ValueError:
            missing.append({"value": value, "path": None, "reason": "unsupported_or_outside_root"})
            continue
        declared.append(path)
        if view.regular(path) and path in view.payloads:
            available.append(path)
        else:
            missing.append({"value": value, "path": path, "reason": "license_bytes_not_collected"})
    fallback = None
    if not available:
        path = safe_join(directory, "COPYING")
        if view.regular(path) and path in view.payloads:
            available.append(path)
            fallback = path
    if metadata["Shipped"] == policy["YES"] and not available:
        errors.append("shipping metadata has no collected license text")
    return {"directory": directory, "name": metadata.get("Name"), "url": metadata.get("URL"),
            "license": metadata.get("License"), "shippedMetadata": metadata["Shipped"],
            "declaredLicensePaths": declared, "availableLicensePaths": available,
            "copyingFallback": fallback, "unresolvedDeclaredReferences": missing, "errors": errors}


def scan(view, policy):
    directories, additional = discover(view, policy)
    entries, errors = [], []
    for directory in directories:
        if directory in policy["SPECIAL_CASES"]:
            row = process(policy["SPECIAL_CASES"][directory], directory, view, policy)
            entries.append({**row, "metadata": "licenses.py SPECIAL_CASES"})
            continue
        found = False
        for name in sorted(policy["METADATA_FILE_NAMES"]):
            path = safe_join(directory, name)
            if not view.regular(path):
                continue
            found = True
            try:
                for metadata in parse_metadata(view.read(path), policy):
                    entries.append({**process(metadata, directory, view, policy), "metadata": path})
            except ValueError as error:
                errors.append({"directory": directory, "metadata": path, "error": str(error)})
        if not found:
            errors.append({"directory": directory, "metadata": None, "error": "missing metadata / special case"})
    return {"candidateDirectories": directories, "additionalPathFiles": additional,
            "entries": entries, "directoryErrors": errors}


def inspect(source, output, include_media=False, include_runtime=False, include_support=False, include_browser=False):
    if output.parent != source or not re.fullmatch(r"license-policy-[0-9]{2}\.json", output.name):
        raise ValueError("safe fresh policy evidence name required")
    if output.exists() or output.is_symlink():
        raise FileExistsError("never overwrite policy evidence")
    inputs = INPUTS["load_inputs"](source, include_media, include_runtime, include_support, include_browser)
    # Use the bytes already checked against the Git object, not a second file read.
    payloads = {path: row["payload"] for path, row in inputs["files"].items()}
    policy = parse_policy(payloads["tools/licenses/licenses.py"])
    result = scan(SourceView(inputs["index"], payloads, policy["VCS_METADATA_DIRS"]), policy)
    shipping = [row for row in result["entries"] if row["shippedMetadata"] == policy["YES"]]
    result.update(scope="static default policy over partially collected sources, not installed/browser component coverage",
                  sourceCommit=INPUTS["TREE"]["COMMIT"], policySha256=POLICY_SHA,
                  mainReceiptSha256=inputs["mainSha256"], externalReceiptSha256=inputs["externalSha256"],
                  shippingMetadataEntries=len(shipping),
                  nonShippingMetadataEntries=sum(row["shippedMetadata"] == policy["NO"] for row in result["entries"]),
                  unknownShippingMetadataEntries=sum(row["shippedMetadata"] not in (policy["YES"], policy["NO"]) for row in result["entries"]),
                  shippingEntriesWithErrors=sum(bool(row["errors"]) for row in shipping),
                  sourceFunctionsExecuted=False, conditionsEvaluated=False, generatorExecuted=False,
                  checkoutCoverage="partial", binaryComponentCoverage="not_proven", licenseCoverage="not_proven", promoted=False)
    if include_media:
        result["mediaReceiptSha256"] = inputs["mediaSha256"]
    if include_runtime:
        result["runtimeReceiptSha256"] = inputs["runtimeSha256"]
    if include_support:
        result["supportReceiptSha256"] = inputs["supportSha256"]
    if include_browser:
        result["browserReceiptSha256"] = inputs["browserSha256"]
    INPUTS["EXTERNAL"]["write_report"](output, result)
    return result


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", required=True, type=Path)
    parser.add_argument("--output-name", required=True)
    parser.add_argument("--include-media", action="store_true")
    parser.add_argument("--include-runtime", action="store_true")
    parser.add_argument("--include-support", action="store_true")
    parser.add_argument("--include-browser", action="store_true")
    args = parser.parse_args()
    source = args.source.absolute()
    result = inspect(source, source / args.output_name, args.include_media, args.include_runtime,
                     args.include_support, args.include_browser)
    print(json.dumps({"candidateDirectories": len(result["candidateDirectories"]),
                      "entries": len(result["entries"]), "directoryErrors": len(result["directoryErrors"]),
                      **{key: result[key] for key in ("shippingMetadataEntries", "nonShippingMetadataEntries",
                          "unknownShippingMetadataEntries", "shippingEntriesWithErrors", "licenseCoverage", "promoted")}}))
