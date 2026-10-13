"""Connect sealed recipe ZIP helpers to source pins and original checkout logs.

Static source/AST and public-log evidence only. No upstream execution, builder
filesystem measurement, condition evaluation, shipping exclusion or promotion.
"""
import argparse
import ast
import hashlib
import io
import json
from pathlib import Path
import re
import runpy
import zipfile


HERE = Path(__file__).parent.absolute()
PACKAGE = runpy.run_path(str(HERE / "collect-chromium-recipe-package.py"))
RECIPE = PACKAGE["RECIPE"]
INPUTS = PACKAGE["INPUTS"]
EXTERNAL = PACKAGE["EXTERNAL"]
INHERITANCE = runpy.run_path(str(HERE / "inspect-chromium-gclient-inheritance.py"))
ARCHIVE_INPUTS = runpy.run_path(str(HERE / "inspect-chromium-archive-inputs.py"))
ARCHIVE_INPUTS_SHA = "37917da5449b8db8430c031aa098c488b149c9f32452efea8e85120d16627589"
PACKAGE_SHA = "8c0bf4a8cb14e5bd49a24745f88e101cd9b1c4360d586321648db950338ecf52"
DEPOT_REVISION = "21ae2d24f8ee0ca3459bda416129d7791c4313f1"
FALLBACK = '''deps_files = [self.deps_file]
if "DEPS" not in deps_files:
    deps_files.append("DEPS")
for deps_file in deps_files:
    filepath = os.path.join(self.root.root_dir, self.name, deps_file)
    if os.path.isfile(filepath):
        logging.info("ParseDepsFile(%s): %s file found at %s", self.name, deps_file, filepath)
        break
    logging.info("ParseDepsFile(%s): No %s file found at %s", self.name, deps_file, filepath)
'''


def source_shape(contents, expected):
    nodes = ast.parse(contents.decode("utf-8"))
    shape = [ast.dump(node, include_attributes=False) for node in ast.parse(expected).body]
    bodies = [node.body for node in ast.walk(nodes) if isinstance(getattr(node, "body", None), list)]
    matches = []
    for body in bodies:
        rows = [ast.dump(node, include_attributes=False) for node in body]
        for offset in range(len(rows) - len(shape) + 1):
            if rows[offset:offset + len(shape)] == shape:
                selected = body[offset:offset + len(shape)]
                matches.append({"startLine": selected[0].lineno, "endLine": selected[-1].end_lineno})
    if len(matches) != 1:
        raise ValueError("one exact reviewed source AST shape required")
    return matches[0]


def package_payloads(source):
    PACKAGE["checked_inputs"](source)
    root = source / "recipe-package-01"
    report, sha = INPUTS["receipt"](root / "recipe-package-inventory.json", PACKAGE_SHA)
    if (report.get("instanceReceiptSha256") != PACKAGE["INSTANCE_SHA"]
            or report.get("package") != RECIPE["PACKAGE"] or report.get("instanceVersion") != RECIPE["VERSION"]
            or report.get("request") != {"url": PACKAGE["ENDPOINT"], "body": PACKAGE["url_request"]()}
            or report.get("collectionComplete") is not True
            or any(report.get(key) is not False for key in (
                "sourceFunctionsExecuted", "packageInstalled", "archiveExtracted", "promoted"))):
        raise ValueError("complete original inert recipe ZIP receipt required")
    raw_url = INPUTS["read_data"](root / "url.public-data", PACKAGE["MAX_RPC"])
    if {"bytes": len(raw_url), "sha256": hashlib.sha256(raw_url).hexdigest()} != report.get("urlResponse"):
        raise ValueError("original package URL raw record changed")
    PACKAGE["parsed_url"](raw_url)
    raw = INPUTS["read_data"](root / "recipe-package.zip-data", PACKAGE["MAX_ZIP"])
    digest = hashlib.sha256(raw).hexdigest()
    if report.get("archive") != {"evidenceFile": "recipe-package.zip-data", "bytes": len(raw), "sha256": digest} \
            or digest != RECIPE["object_ref"]()["hexDigest"]:
        raise ValueError("original recipe ZIP no longer matches immutable CIPD digest")
    inspection = report["inspection"]
    payloads, picked = {}, []
    with zipfile.ZipFile(io.BytesIO(raw)) as archive:
        rows, total = PACKAGE["zip_records"](archive)
        if rows != inspection.get("entries") or total != inspection.get("unpackedBytes"):
            raise ValueError("original recipe ZIP tree changed")
        for row in rows:
            if not row["selected"]:
                continue
            contents = archive.read(row["path"])
            sha256 = hashlib.sha256(contents).hexdigest()
            name = "blobs/" + sha256 + ".source-data"
            if INPUTS["read_data"](root / name, PACKAGE["MAX_FILE"]) != contents:
                raise ValueError("selected recipe source blob differs from immutable ZIP")
            picked.append({**row, "sha256": sha256, "evidenceFile": name})
            payloads[row["path"]] = contents
    if picked != inspection.get("selectedFiles") or sum(map(len, payloads.values())) != inspection.get("selectedBytes"):
        raise ValueError("original selected recipe source inventory differs")
    return payloads, sha


def helper_identity(payloads, helpers):
    cfg = json.loads(payloads["build/infra/config/recipes.cfg"])
    depot = cfg.get("deps", {}).get("depot_tools")
    expected = {"branch": "refs/heads/main", "revision": DEPOT_REVISION,
                "url": "https://chromium.googlesource.com/chromium/tools/depot_tools.git"}
    if cfg.get("repo_name") != "build" or depot != expected:
        raise ValueError("original recipe's immutable depot_tools dependency required")
    result = []
    for name in ("gclient.py", "gclient_eval.py"):
        contents = payloads["depot_tools/" + name]
        if contents != helpers[name]:
            raise ValueError("bundled helper bytes differ from independently sealed Git source")
        result.append({"packagePath": "depot_tools/" + name, "sourcePath": name,
                       "bytes": len(contents), "sha256": hashlib.sha256(contents).hexdigest(),
                       "exactBytesMatch": True})
    return {"repository": expected["url"], "revision": DEPOT_REVISION,
            "branchRequestedOnly": expected["branch"], "helpers": result}


def normalized(path):
    return re.sub(r"/+", "/", path.replace("\\", "/")).rstrip("/")


def log_layout(contents):
    text = contents.decode("utf-8")
    values = {}
    for key in ("DEPOT_TOOLS_DIR", "THIS_DIR"):
        rows = [line[len(key) + 2:] for line in text.splitlines() if line.startswith(key + ": ")]
        if len(rows) != 1:
            raise ValueError("one original bot_update directory literal required")
        value = ast.literal_eval(rows[0])
        if not isinstance(value, str) or not 0 < len(value) <= 2048 or any(ord(char) < 32 for char in value):
            raise ValueError("bounded original public directory literal required")
        values[key] = value
    root = normalized(values["DEPOT_TOOLS_DIR"])
    if (not root.endswith("/packages/cipd/exe/depot_tools")
            or normalized(values["THIS_DIR"]) != root + "/recipes/recipe_modules/bot_update/resources"):
        raise ValueError("original executed bot_update root differs from recipe bundle layout")
    commands = [line for line in text.splitlines() if "===Running " in line
                and root + "/gclient.py sync " in normalized(line)]
    if len(commands) != 1:
        raise ValueError("one original same-root gclient sync command required")
    return {"depotToolsDir": values["DEPOT_TOOLS_DIR"], "botUpdateDir": values["THIS_DIR"],
            "gclientCommand": commands[0], "packageRelativeLayoutMatches": True,
            "logExecutedByInspector": False, "builderFilesystemMeasured": False}


def inspect(source, output):
    if output.parent != source or not re.fullmatch(r"recipe-provenance-[0-9]{2}\.json", output.name):
        raise ValueError("safe fresh recipe provenance report required")
    if output.exists() or output.is_symlink():
        raise FileExistsError("never overwrite recipe provenance evidence")
    payloads, package_sha = package_payloads(source)
    helpers, helper_sha = INHERITANCE["load_helpers"](source)
    identity = helper_identity(payloads, helpers)
    original_args, args_sha = INPUTS["receipt"](source / "archive-inputs-01.json", ARCHIVE_INPUTS_SHA)
    if ARCHIVE_INPUTS["inspect"](source) != original_args:
        raise ValueError("original sealed gclient configs must reproduce")
    configs = {row["target"]: row["gclientConfigRecorded"] for row in original_args["targets"]}
    shapes = {
        "configuredFileThenDEPS": source_shape(payloads["depot_tools/gclient.py"], FALLBACK),
        "botUpdateRelativeRoot": source_shape(payloads["depot_tools/recipes/recipe_modules/bot_update/resources/bot_update.py"],
                                             'DEPOT_TOOLS_DIR = path.abspath(path.join(THIS_DIR, "..", "..", "..", ".."))'),
        "botUpdateGclientPath": source_shape(payloads["depot_tools/recipes/recipe_modules/bot_update/resources/bot_update.py"],
                                             'GCLIENT_PATH = path.join(DEPOT_TOOLS_DIR, "gclient.py")'),
        "botUpdateGclientCall": source_shape(payloads["depot_tools/recipes/recipe_modules/bot_update/resources/bot_update.py"],
                                             'cmd = [sys.executable, "-u", GCLIENT_PATH]\ncmd.extend(args)\nreturn call(*cmd, **kwargs)'),
    }
    inputs = INPUTS["load_inputs"](source)
    deps = {path: inputs["index"].get(path) for path in ("DEPS", ".DEPS.git")}
    if deps["DEPS"] is None or deps[".DEPS.git"] is not None:
        raise ValueError("fixed Chromium Git tree must contain DEPS and no .DEPS.git")
    targets = []
    for target in RECIPE["CHECKOUTS"]["BUILDS"]["ARCHIVES"]:
        if configs[target].get("depsFileRecorded") != ".DEPS.git":
            raise ValueError("original four configs must request .DEPS.git")
        data = INPUTS["read_data"](source / "archive-checkout-logs-03" / (target + "-checkout.log-data"),
                                   RECIPE["CHECKOUTS"]["LOGS"]["MAX_LOG_BYTES"])
        targets.append({"target": target, **log_layout(data),
                        "stdoutComplete": target != "windows-x64", "runtimeFileHashesMeasured": False})
    report = {"scope": "actual immutable recipe package, exact helper source and original log layout; not runtime file attestation/NOTICE",
              "sourceCommit": INPUTS["TREE"]["COMMIT"], "packageReceiptSha256": package_sha,
              "helperSourceReceiptSha256": helper_sha, "archiveInputReceiptSha256": args_sha,
              "recipeInstanceVersion": RECIPE["VERSION"],
              "helperIdentity": identity, "sourceShapes": shapes, "mainDepsTreeEntries": deps, "targets": targets,
              "recipeBundledHelperSourceVerified": True, "depsFallbackImplementationVerified": True,
              "configuredDepsFile": ".DEPS.git", "fixedSourceFallbackFile": "DEPS",
              "builderFilesystemMeasured": False, "actualEffectiveConditionsEvaluated": False,
              "sourceFunctionsExecuted": False, "shippingDecisionsMade": False,
              "licenseErrorsExcluded": False, "licenseCoverage": "not_proven", "promoted": False}
    EXTERNAL["write_report"](output, report)
    return report


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--output-name", required=True)
    args = parser.parse_args()
    source = args.source.absolute()
    result = inspect(source, source / args.output_name)
    print(json.dumps({"recipeBundledHelperSourceVerified": result["recipeBundledHelperSourceVerified"],
                      "depsFallbackImplementationVerified": result["depsFallbackImplementationVerified"],
                      "targets": len(result["targets"]), "builderFilesystemMeasured": False, "promoted": False}))
