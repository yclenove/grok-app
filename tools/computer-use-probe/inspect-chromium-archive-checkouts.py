"""Match actual sealed bot_update manifests to collected source/recursive pins.

Manifest presence/absence is checkout evidence, never a shipping decision or
license exemption. Preserve all discrepancies and do not evaluate DEPS/code.
"""
import argparse
import hashlib
import json
from pathlib import Path
import re
import runpy
from urllib.parse import urlsplit


HERE = Path(__file__).parent.absolute()
LOGS = runpy.run_path(str(HERE / "collect-chromium-archive-logs.py"))
BUILDS = LOGS["BUILDS"]
INPUTS = LOGS["INPUTS"]
CHECKOUT_SHA = "c3ad23bae80560a7249d0295df6c765bca0d88951b7b52c0b6077fa9145e8048"
RECURSIVE = {"angle": ("recursive-deps-angle-02.json", "eca26cd95153d528f8fd4df2275b208c936a2490a75edb9b09aeb27833c2ec00"),
             "openscreen": ("recursive-deps-openscreen-01.json", "c80c95ff85dbc6e091f806038df1bd2ed91ad67c46e9e1e6c4c3619457050608")}
CONFIG_REVISION = "82064764761481b897d400961ee71601b5d796e0"
RECIPE_INSTANCE = "2QjLPDj2oNMKlnOBPW4LhuQHkh6Mc52_OS_yRNlo_RwC"


def manifest_data(contents):
    report = json.loads(LOGS["checked_log"](contents, "text/plain"))
    if (not isinstance(report, dict) or report.get("did_run") is not True or report.get("root") != "src"
            or report.get("fixed_revisions") != {"src": INPUTS["TREE"]["COMMIT"]}
            or not isinstance(report.get("manifest"), dict) or not 0 < len(report["manifest"]) <= 2000):
        raise ValueError("complete original same-source bot_update manifest required")
    manifest = report["manifest"]
    if manifest.get("src") != {"repository": "https://chromium.googlesource.com/chromium/src.git",
                              "revision": INPUTS["TREE"]["COMMIT"]}:
        raise ValueError("actual manifest root must match exact pinned source")
    for path, row in manifest.items():
        if (not isinstance(path, str) or len(path) > 2048 or not (path == "src" or path.startswith("src/"))
                or any(part in ("", ".", "..") for part in path.split("/"))
                or not isinstance(row, dict) or set(row) != {"repository", "revision"}
                or not isinstance(row["revision"], str) or not re.fullmatch(r"[0-9a-f]{40}", row["revision"])
                or not isinstance(row["repository"], str) or len(row["repository"]) > 2048):
            raise ValueError("bounded actual checkout paths and Git identities required")
        url = urlsplit(row["repository"])
        if url.scheme != "https" or not url.netloc or url.username or url.password or url.query or url.fragment:
            raise ValueError("inert HTTPS Git repository identity required")
    return manifest


def bootstrap_data(target, contents):
    report = json.loads(LOGS["checked_log"](contents, "text/plain"))
    if not isinstance(report, dict) or report.get("recipe") != "chromium":
        raise ValueError("actual Chromium bootstrap recipe data required")
    builder, _, _, _ = BUILDS["archive"](target)
    bootstrap = report.get("$build/chromium_bootstrap")
    if not isinstance(bootstrap, dict) or not isinstance(bootstrap.get("commits"), list) or len(bootstrap["commits"]) != 1:
        raise ValueError("one original bootstrap source commit required")
    BUILDS["checked_commit"](bootstrap["commits"][0])
    config, exe = bootstrap.get("config_source"), bootstrap.get("exe")
    if (not isinstance(config, dict) or config.get("path") != "infra/config/generated/builders/ci/" + builder + "/properties.json"
            or not isinstance(config.get("last_changed_commit"), dict)
            or (config["last_changed_commit"].get("host"), config["last_changed_commit"].get("project"),
                config["last_changed_commit"].get("id")) != ("chromium.googlesource.com", "chromium/src", CONFIG_REVISION)
            or not isinstance(exe, dict) or exe.get("cmd") != ["luciexe"] or not isinstance(exe.get("cipd"), dict)
            or (exe["cipd"].get("server"), exe["cipd"].get("package"), exe["cipd"].get("actual_version")) != (
                "https://chrome-infra-packages.appspot.com",
                "infra/recipe_bundles/chromium.googlesource.com/chromium/tools/build", RECIPE_INSTANCE)):
        raise ValueError("exact original config revision/path and resolved recipe instance required")
    config_container = report.get("$build/chromium_tests_builder_config", {})
    if not isinstance(config_container, dict):
        raise ValueError("original builder config container must be an object")
    config_body = config_container.get("builder_config", {})
    if not isinstance(config_body, dict):
        raise ValueError("original bootstrapped builder config must be an object")
    return {"recipe": report["recipe"], "configSource": config, "recipeExe": exe,
            "builderConfigKeys": sorted(config_body),
            "builderConfigSelected": {key: value for key, value in config_body.items()
                                      if key in {"chromium_config", "chromium_config_kwargs", "gclient_config", "gclient_apply_config"}},
            "resolvedInstanceRecorded": True, "recipeSourceCollected": False, "actualGclientHelperRevisionVerified": False}


def compare_pin(path, repository, revision, manifest):
    row = manifest.get(path)
    if row is None:
        status = "exact_manifest_entry_absent"
    elif row != {"repository": repository, "revision": revision}:
        status = "manifest_identity_differs"
    else:
        status = "exact_repository_and_revision_match"
    return {"checkoutPath": path, "expectedRepository": repository, "expectedRevision": revision,
            "actual": row, "status": status, "shippingDecisionMade": False}


def source_comparison(manifest):
    rows = []
    for group in ("PINS", "MEDIA_PINS", "RUNTIME_PINS", "SUPPORT_PINS", "BROWSER_PINS"):
        for key, (path, repository, revision) in INPUTS["EXTERNAL"][group].items():
            rows.append({"sourceComponent": key, "sourceGroup": group,
                         **compare_pin("src/" + path, repository, revision, manifest)})
    return rows


def recursive_comparison(manifest, catalog):
    if (catalog.get("parentSourceCommit") != INPUTS["TREE"]["COMMIT"]
            or catalog.get("dependencyComponent") not in RECURSIVE
            or catalog.get("unresolved") or catalog.get("mismatches") or catalog.get("uncataloguedGitlinks")
            or catalog.get("relativeLayoutDeclared") is not True or not isinstance(catalog.get("gitDependencies"), list)):
        raise ValueError("original verified relative-layout recursive catalog required")
    parent = "src/" + catalog["parentCheckoutPath"]
    rows = []
    for row in catalog["gitDependencies"]:
        if (not isinstance(row.get("path"), str) or not 0 < len(row["path"]) <= 2048
                or any(part in ("", ".", "..") for part in row["path"].split("/"))):
            raise ValueError("bounded relative recursive checkout identity required")
        path = parent + "/" + row["path"]
        rows.append({**compare_pin(path, row["repository"], row["revision"], manifest),
                     "conditionRecorded": row.get("condition"), "conditionEvaluated": False})
    return rows


def status_counts(rows):
    return {key: sum(row["status"] == key for row in rows) for key in (
        "exact_repository_and_revision_match", "exact_manifest_entry_absent", "manifest_identity_differs")}


def inspect(source):
    builds, build_sha = LOGS["checked_builds"](source)
    root = source / "archive-checkout-logs-03"
    report, sha = INPUTS["receipt"](root / "archive-log-inventory.json", CHECKOUT_SHA)
    requests = [request for target, build in builds.items() for request in LOGS["requests_for"](target, build, "checkout")]
    if (report.get("collectionComplete") is not True or report.get("profile") != "checkout"
            or report.get("archiveReceiptSha256") != build_sha or report.get("requests") != requests
            or not isinstance(report.get("logs"), list) or len(report["logs"]) != 12):
        raise ValueError("complete sealed original manifest/bootstrap collection required")
    catalogs = {}
    for component, (name, expected_sha) in RECURSIVE.items():
        catalogs[component], _ = INPUTS["receipt"](source / name, expected_sha)
    files = {}
    for request, row in zip(requests, report["logs"]):
        if (any(row.get(key) != value for key, value in request.items()) or row.get("verified") is not True
                or (row.get("truncated") is not False and request["key"] != "checkout")):
            raise ValueError("original exact verified log identities required; manifest/bootstrap cannot truncate")
        contents = INPUTS["read_data"](root / row["evidenceFile"], LOGS["MAX_LOG_BYTES"])
        if (len(contents), hashlib.sha256(contents).hexdigest()) != (row.get("bytes"), row.get("sha256")):
            raise ValueError("original checkout log bytes changed")
        LOGS["checked_log"](contents, row.get("contentType"), allow_empty=request["key"] == "checkout")
        files[request["target"], request["key"]] = contents
    targets = []
    for target in BUILDS["ARCHIVES"]:
        manifest = manifest_data(files[target, "manifest"])
        bootstrap = bootstrap_data(target, files[target, "bootstrap"])
        collected = source_comparison(manifest)
        recursive = {component: recursive_comparison(manifest, catalog) for component, catalog in catalogs.items()}
        targets.append({"target": target, "buildId": builds[target]["id"], "manifestEntries": len(manifest),
                        "manifest": manifest, "bootstrap": bootstrap, "collectedSourceComparison": collected,
                        "collectedSourceCounts": status_counts(collected), "recursiveComparison": recursive,
                        "recursiveCounts": {key: status_counts(rows) for key, rows in recursive.items()},
                        "actualGclientEffectiveVarsEvaluated": False, "binaryCoverageProven": False})
    return {"scope": "original checkout manifests/config identity; exact absence is not NOT_SHIPPED or a license exemption",
            "sourceCommit": INPUTS["TREE"]["COMMIT"], "buildReceiptSha256": build_sha, "checkoutReceiptSha256": sha,
            "checkoutStdoutComplete": report.get("fullLogsComplete"), "targets": targets,
            "sourceFunctionsExecuted": False, "conditionsEvaluated": False, "licenseErrorsExcluded": False,
            "promoted": False}


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--output-name", required=True)
    args = parser.parse_args()
    source = args.source.absolute()
    if not re.fullmatch(r"archive-checkouts-[0-9]{2}\.json", args.output_name):
        parser.error("safe fresh archive-checkout report name required")
    result = inspect(source)
    INPUTS["EXTERNAL"]["write_report"](source / args.output_name, result)
    print(json.dumps({"targets": [{"target": row["target"], "manifestEntries": row["manifestEntries"],
                                   "sources": row["collectedSourceCounts"], "recursive": row["recursiveCounts"]}
                                  for row in result["targets"]], "licenseErrorsExcluded": False, "promoted": False}))
