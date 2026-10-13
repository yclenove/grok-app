"""Read four allowlisted inert logs from each independently sealed archive build.

Never execute commands/configs, follow arbitrary URLs or fetch another build.
Logs are source/GN evidence, not GCS generation or downloaded-binary proof.
"""
import argparse
import hashlib
import json
from pathlib import Path
import re
import runpy
import urllib.request


HERE = Path(__file__).parent.absolute()
BUILDS = runpy.run_path(str(HERE / "collect-chromium-archive-builds.py"))
INPUTS = BUILDS["INPUTS"]
BUILD_SHA = "851226dbba1f79047c0998234bb885feea6d96c65548540e1e15f93bff7476d4"
MAX_LOG_BYTES = 4 * 1024**2
LOGS = (("gclient", "gclient config", "config"),
        ("generate", "generate_build_files", "stdout"),
        ("gn", "read gn_logs.txt", "gn_logs.txt"),
        ("upload", None, "stdout"))
CHECKOUT_LOGS = (("manifest", "bot_update", "json.output"),
                 ("checkout", "bot_update", "stdout"),
                 ("bootstrap", "bootstrapped properties", "properties"))


def profile_logs(profile):
    if profile == "core":
        return LOGS
    if profile == "checkout":
        return CHECKOUT_LOGS
    raise ValueError("only compiled core or checkout log profiles allowed")


def decoded_record(contents):
    text = contents.decode("utf-8")
    if text.startswith(")]}'\n"):
        text = text[5:]
    elif text.startswith(")]}'\r\n"):
        text = text[6:]
    return json.loads(text)


def checked_builds(source):
    INPUTS["checked_source"](source)
    root = source / "archive-build-records-01"
    report, sha = INPUTS["receipt"](root / "archive-build-inventory.json", BUILD_SHA)
    if (report.get("collectionComplete") is not True or report.get("sourceCommit") != INPUTS["TREE"]["COMMIT"]
            or report.get("searchReceiptSha256") != BUILDS["SEARCH_SHA"]
            or report.get("readOnlyRpc") != BUILDS["ENDPOINT"] or report.get("logsFetched") is not False
            or report.get("promoted") is not False or not isinstance(report.get("records"), list)
            or not isinstance(report.get("summaries"), list)
            or len(report["records"]) != 4 or len(report["summaries"]) != 4):
        raise ValueError("complete sealed four-archive inventory required")
    builds = {}
    for target, row, summary in zip(BUILDS["ARCHIVES"], report["records"], report["summaries"]):
        if (row.get("target") != target or row.get("evidenceFile") != target + ".public-data"
                or row.get("request") != BUILDS["get_body"](target) or row.get("verified") is not True):
            raise ValueError("exact compiled archive record identities required")
        contents = INPUTS["read_data"](root / row["evidenceFile"], BUILDS["MAX_RECORD_BYTES"])
        if (len(contents), hashlib.sha256(contents).hexdigest()) != (row.get("bytes"), row.get("sha256")):
            raise ValueError("sealed original archive raw bytes changed")
        if BUILDS["inspect_record"](target, contents) != summary:
            raise ValueError("sealed archive summary differs from original raw record")
        builds[target] = decoded_record(contents)
    return builds, sha


def requests_for(target, build, profile="core"):
    builder, identity, platform, filename = BUILDS["archive"](target)
    if build.get("id") != identity:
        raise ValueError("compiled same-build log identity required")
    requests = []
    for key, step_name, log_name in profile_logs(profile):
        if step_name is None:
            step_name = "Generic Archiving Steps|gsutil upload " + platform + "/1716663/" + filename
        steps = [step for step in build["steps"] if step["name"] == step_name]
        if len(steps) != 1 or steps[0].get("status") != "SUCCESS":
            raise ValueError("exact successful allowlisted log step required: " + key)
        logs = [log for log in steps[0].get("logs", []) if log.get("name") == log_name]
        if len(logs) != 1:
            raise ValueError("exact unique allowlisted log descriptor required: " + key)
        descriptor = BUILDS["log_descriptor"](logs[0], identity)
        request = expected_request(target, key)
        if descriptor["viewUrl"] + "?format=raw" != request["url"]:
            raise ValueError("sealed descriptor differs from compiled allowlisted log path")
        requests.append(request)
    return requests


def expected_request(target, key):
    _, identity, platform, filename = BUILDS["archive"](target)
    paths = {"gclient": ("gclient config", "config", "gclient_config/config"),
             "generate": ("generate_build_files", "stdout", "generate_build_files/stdout"),
             "gn": ("read gn_logs.txt", "gn_logs.txt", "read_gn_logs.txt/gn_logs.txt"),
             "upload": ("Generic Archiving Steps|gsutil upload " + platform + "/1716663/" + filename,
                        "stdout", "Generic_Archiving_Steps/gsutil_upload_" + platform + "_1716663_" + filename + "/stdout"),
             "manifest": ("bot_update", "json.output", "bot_update/json.output"),
             "checkout": ("bot_update", "stdout", "bot_update/stdout"),
             "bootstrap": ("bootstrapped properties", "properties", "bootstrapped_properties/properties")}
    if not isinstance(key, str) or key not in paths:
        raise ValueError("only compiled allowlisted log keys required")
    step, log, suffix = paths[key]
    return {"target": target, "key": key, "step": step, "log": log,
            "url": "https://logs.chromium.org/logs/chromium/buildbucket/cr-buildbucket/" + identity + "/+/u/"
            + suffix + "?format=raw", "buildId": identity, "evidenceFile": target + "-" + key + ".log-data"}


def checked_log(contents, content_type, allow_empty=False):
    if ((not contents and not allow_empty) or len(contents) > MAX_LOG_BYTES
            or not isinstance(content_type, str) or len(content_type) > 512
            or content_type.split(";", 1)[0].strip().lower() != "text/plain"):
        raise ValueError("bounded nonempty plain-text public log required")
    text = contents.decode("utf-8")
    if "\0" in text or text.lstrip().startswith(("<!DOCTYPE", "<!doctype", "<html", "<HTML")):
        raise ValueError("HTML/login/binary response is not public build log evidence")
    return text


def fetch_log(request):
    target = request.get("target")
    _, identity, _, _ = BUILDS["archive"](target)
    if request != expected_request(target, request.get("key")):
        raise ValueError("only compiled same-build log requests allowed")
    url = request.get("url")
    if not isinstance(url, str) or not url.endswith("?format=raw"):
        raise ValueError("only raw public log query allowed")
    BUILDS["log_descriptor"]({"name": request.get("log"), "viewUrl": url[:-11]}, identity)
    http_request = urllib.request.Request(url, headers={"Accept": "text/plain"}, method="GET")
    with urllib.request.build_opener(BUILDS["SEARCH"]["NoRedirect"]()).open(http_request, timeout=20) as response:
        if response.geturl() != url:
            raise ValueError("public log endpoint redirect is not accepted")
        contents = response.read(MAX_LOG_BYTES + 1)
        content_type = response.headers.get("Content-Type", "")
    if (len(contents) > MAX_LOG_BYTES and request["key"] != "checkout") or (not contents and request["key"] != "checkout"):
        raise ValueError("bounded nonempty public log response required")
    return contents, content_type


def collect(source, output, profile="core"):
    profile_logs(profile)
    prefix = "archive-build-logs" if profile == "core" else "archive-checkout-logs"
    if output.parent != source or not re.fullmatch(prefix + r"-[0-9]{2}", output.name):
        raise ValueError("safe fresh task-owned build-log directory required")
    if output.exists() or output.is_symlink():
        raise FileExistsError("never overwrite build log evidence")
    builds, sha = checked_builds(source)
    requests = [request for target, build in builds.items() for request in requests_for(target, build, profile)]
    output.mkdir()
    report = {"scope": "allowlisted original build logs, not executed commands or binary/license certification",
              "sourceCommit": INPUTS["TREE"]["COMMIT"], "archiveReceiptSha256": sha, "profile": profile,
              "requests": requests, "logs": [], "collectionComplete": False, "sourceFunctionsExecuted": False,
              "fullLogsComplete": True, "conditionsEvaluated": False, "gcsGenerationAssociationProven": False, "promoted": False}
    try:
        for request in requests:
            contents, content_type = fetch_log(request)
            truncated = request["key"] == "checkout" and len(contents) > MAX_LOG_BYTES
            if truncated:
                contents = contents[:MAX_LOG_BYTES]
                report["fullLogsComplete"] = False
            with (output / request["evidenceFile"]).open("xb") as stream:
                stream.write(contents)
            row = {**request, "bytes": len(contents), "sha256": hashlib.sha256(contents).hexdigest(),
                   "contentType": content_type, "verified": False, "truncated": truncated}
            report["logs"].append(row)
            checked_log(contents, content_type, allow_empty=request["key"] == "checkout")
            row["verified"] = True
            print(json.dumps({"target": request["target"], "log": request["key"], "bytes": len(contents),
                              "truncated": truncated}), flush=True)
        report["collectionComplete"] = True
    except Exception as error:
        report["failureType"] = type(error).__name__
        raise
    finally:
        INPUTS["EXTERNAL"]["write_report"](output / "archive-log-inventory.json", report)
    return report


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--output-name", required=True)
    parser.add_argument("--profile", choices=("core", "checkout"), default="core")
    args = parser.parse_args()
    source = args.source.absolute()
    result = collect(source, source / args.output_name, args.profile)
    print(json.dumps({"logs": len(result["logs"]), "collectionComplete": result["collectionComplete"],
                      "fullLogsComplete": result["fullLogsComplete"], "promoted": False}))
