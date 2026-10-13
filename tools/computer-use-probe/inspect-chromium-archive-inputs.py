"""Reverify sealed original logs and read literal GN/custom-var data only.

Match exact pinned source defaults, but never run GN/gclient/upstream code.
Explicit args plus defaults explain the sample-Credits mechanism; this does
not evaluate the whole build graph, prove checkout closure or bind ZIP bytes.
"""
import argparse
import hashlib
import json
from pathlib import Path
import re
import runpy


HERE = Path(__file__).parent.absolute()
LOGS = runpy.run_path(str(HERE / "collect-chromium-archive-logs.py"))
INPUTS = LOGS["INPUTS"]
TREE = INPUTS["TREE"]
LOG_SHA = "9056f1b81f8822cb6852308ec6b57bf8f5cc2660eac449bfd41b3f8797ec7e17"
ROOT_TREE = "6d3d6ca9ea18a088adc33dfcdddab14fc3ac361d"
DEFAULT_FILES = {
    "build/config/BUILDCONFIG.gn": ("a8dcb4d4675505227c2259a8851d9b2ee904faef",
                                   "1b33f01310e829a6f8b2e675e944fd32b35d14809571df6d1d86d1f6b051e6f7"),
    "components/resources/BUILD.gn": ("fd2fb05219e396333661c94319545aa546191fb6",
                                     "1d06064122da27e753b0da6e3fce990ba6cc13f179fbf19a309420fe657cafb9"),
}
PLATFORMS = {"windows-x64": ("win", "x64"), "linux-x64": ("linux", "x64"),
             "mac-intel": ("mac", "x64"), "mac-arm64": ("mac", "arm64")}


def gn_arguments(contents, target):
    if target not in PLATFORMS:
        raise ValueError("compiled GN platform required")
    text = LOGS["checked_log"](contents, "text/plain").replace("\r\n", "\n")
    matches = list(re.finditer(r'Writing """\\\n(.*?)\n""" to ([^\n]+)\.\n', text, re.DOTALL))
    if (len(matches) != 1 or text[:matches[0].start()].strip()
            or not matches[0].group(2).endswith(("/args.gn", "\\args.gn"))):
        raise ValueError("one complete original GN args writing block required")
    values = {}
    for line in matches[0].group(1).split("\n"):
        match = re.fullmatch(r'([a-z_][a-z0-9_]*) = (true|false|[0-9]+|"[A-Za-z0-9_-]+")', line)
        if match is None or match[1] in values:
            raise ValueError("unique literal GN assignments only; never evaluate expressions")
        raw = match[2]
        values[match[1]] = raw == "true" if raw in ("true", "false") else json.loads(raw)
    if (values.get("target_os"), values.get("target_cpu")) != PLATFORMS[target]:
        raise ValueError("actual GN platform must match exact archive target")
    tail = text[matches[0].end():]
    if not re.search(r"(?m)^Done\. Made [0-9]+ targets from [0-9]+ files in [0-9]+ms$", tail):
        raise ValueError("original successful GN generation completion required")
    return values


def gclient_custom_vars(contents):
    config = json.loads(LOGS["checked_log"](contents, "text/plain"))
    if not isinstance(config, dict) or not isinstance(config.get("solutions"), list) or len(config["solutions"]) != 1:
        raise ValueError("one actual original gclient solution required")
    solution = config["solutions"][0]
    if (not isinstance(solution, dict) or solution.get("name") != "src"
            or solution.get("url") != "https://chromium.googlesource.com/chromium/src.git"
            or not isinstance(solution.get("custom_vars"), dict) or len(solution["custom_vars"]) > 128
            or not isinstance(solution.get("custom_deps"), dict) or len(solution["custom_deps"]) > 128):
        raise ValueError("actual bounded Chromium solution and custom data required")
    return {"depsFileRecorded": solution.get("deps_file"), "customVars": solution["custom_vars"],
            "customDeps": solution["custom_deps"], "targetOs": config.get("target_os"),
            "targetCpu": config.get("target_cpu"), "checkoutManifestVerified": False,
            "effectiveInheritedVarsVerified": False}


def checked_defaults(source):
    bare = source / "chromium-pinned-tree.git"
    TREE["regular_parents"](bare)
    if (not bare.is_dir() or bare.resolve(strict=True) != bare
            or TREE["git"](bare, ["rev-parse", "--is-bare-repository"], 64) != b"true\n"
            or TREE["git"](bare, ["rev-parse", TREE["COMMIT"] + "^{tree}"], 64).decode().strip() != ROOT_TREE):
        raise ValueError("exact original fixed main bare source tree required")
    result, texts = [], {}
    for path, (oid, sha) in DEFAULT_FILES.items():
        if TREE["git"](bare, ["rev-parse", TREE["COMMIT"] + ":" + path], 64).decode().strip() != oid:
            raise ValueError("compiled default file does not belong to fixed main tree")
        name = path.replace("/", "__") + ".source-data"
        contents = INPUTS["read_data"](source / name, TREE["MAX_BLOB"])
        identity = TREE["check_blob"](oid, contents)
        if identity["sha256"] != sha:
            raise ValueError("original GN default source bytes changed")
        texts[path] = contents.decode("utf-8")
        result.append({"path": path, "gitObjectSha1": oid, **identity, "evidenceFile": name})
    official = re.findall(r"(?m)^  is_official_build = (.*)$", texts["build/config/BUILDCONFIG.gn"])
    credits = re.findall(r"(?m)^  generate_about_credits = (.*)$", texts["components/resources/BUILD.gn"])
    if official != ["false"] or credits != ["is_official_build"]:
        raise ValueError("exact pinned GN default declarations required")
    return result


def inspect(source):
    INPUTS["checked_source"](source)
    builds, build_sha = LOGS["checked_builds"](source)
    defaults = checked_defaults(source)
    root = source / "archive-build-logs-01"
    report, sha = INPUTS["receipt"](root / "archive-log-inventory.json", LOG_SHA)
    requests = [request for target, build in builds.items() for request in LOGS["requests_for"](target, build)]
    if (report.get("collectionComplete") is not True or report.get("sourceCommit") != TREE["COMMIT"]
            or report.get("archiveReceiptSha256") != build_sha or report.get("requests") != requests
            or not isinstance(report.get("logs"), list) or len(report["logs"]) != 16):
        raise ValueError("complete independently sealed same-build log collection required")
    files = {}
    for request, row in zip(requests, report["logs"]):
        if any(row.get(key) != value for key, value in request.items()):
            raise ValueError("sealed logs must match compiled exact same-build requests")
        contents = INPUTS["read_data"](root / request["evidenceFile"], LOGS["MAX_LOG_BYTES"])
        if (len(contents), hashlib.sha256(contents).hexdigest()) != (row.get("bytes"), row.get("sha256")):
            raise ValueError("original log bytes changed")
        LOGS["checked_log"](contents, row.get("contentType"))
        files[request["target"], request["key"]] = contents
    targets = []
    for target in PLATFORMS:
        arguments = gn_arguments(files[target, "generate"], target)
        custom = gclient_custom_vars(files[target, "gclient"])
        targets.append({"target": target, "buildId": builds[target]["id"], "gnArgsWritten": arguments,
                        "gclientConfigRecorded": custom, "explicitGnArgsVerified": True, "actualGclientCustomVarsVerified": True,
                        "creditsDefaultFalseFromRecordedArgsAndPinnedDeclarations":
                            arguments.get("generate_about_credits", arguments.get("is_official_build", False)) is False,
                        "wholeGnGraphEvaluated": False, "binaryCoverageProven": False})
    return {"scope": "original written GN args/custom config plus fixed defaults, not executed build graph or ZIP-byte proof",
            "sourceCommit": TREE["COMMIT"], "buildReceiptSha256": build_sha, "logReceiptSha256": sha,
            "defaultSources": defaults, "targets": targets, "sourceFunctionsExecuted": False,
            "conditionsEvaluated": False, "licenseErrorsExcluded": False, "gcsGenerationAssociationProven": False,
            "downloadedZipBytesAssociationProven": False, "promoted": False}


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--output-name", required=True)
    args = parser.parse_args()
    source = args.source.absolute()
    if not re.fullmatch(r"archive-inputs-[0-9]{2}\.json", args.output_name):
        parser.error("safe fresh fixed archive-input report name required")
    result = inspect(source)
    INPUTS["EXTERNAL"]["write_report"](source / args.output_name, result)
    print(json.dumps({"targets": len(result["targets"]), "explicitGnArgsVerified": True,
                      "creditsDefaultFalse": all(row["creditsDefaultFalseFromRecordedArgsAndPinnedDeclarations"] for row in result["targets"]),
                      "promoted": False}))
