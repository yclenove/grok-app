"""Seal four exact public archive GetBuild records, never shipping proof.

Reverify the independently sealed search and every raw page before networking.
Only fixed read-only RPCs; no arbitrary IDs/URLs, logs, credentials or execution.
An upload path/status does not bind a GCS generation or our downloaded bytes.
"""
import argparse
import hashlib
import json
from pathlib import Path
import re
import runpy
import urllib.request
from urllib.parse import urlsplit


HERE = Path(__file__).parent.absolute()
SEARCH = runpy.run_path(str(HERE / "collect-chromium-public-builds.py"))
INPUTS = SEARCH["INPUTS"]
SEARCH_SHA = "a3473758611aa0ec24a75d7d5017421e29f17583ea21d0fb4ec6a1a90d322e9b"
ENDPOINT = "https://cr-buildbucket.appspot.com/prpc/buildbucket.v2.Builds/GetBuild"
FIELDS = "id,builder,status,input,output,steps,tags"
POSITION = 1716663
MAX_RECORD_BYTES = 4 * 1024**2
ARCHIVES = {
    "windows-x64": ("win-archive-rel", "8668153607039536273", "Win_x64", "chrome-win.zip"),
    "linux-x64": ("linux-archive-rel", "8668152676980535313", "Linux_x64", "chrome-linux.zip"),
    "mac-intel": ("mac-archive-rel", "8668153412503290689", "Mac", "chrome-mac.zip"),
    "mac-arm64": ("mac-arm64-archive-rel", "8668153878055557601", "Mac_Arm", "chrome-mac.zip"),
}


def archive(target):
    if not isinstance(target, str) or target not in ARCHIVES:
        raise ValueError("only four compiled archive targets allowed")
    return ARCHIVES[target]


def get_body(target):
    return {"id": archive(target)[1], "fields": FIELDS}


def checked_commit(commit, position=False):
    if (not isinstance(commit, dict) or (commit.get("host"), commit.get("project"), commit.get("id")) != (
            "chromium.googlesource.com", "chromium/src", INPUTS["TREE"]["COMMIT"])):
        raise ValueError("exact public input/output source commit required")
    if position and (type(commit.get("position")) is not int or commit["position"] != POSITION):
        raise ValueError("exact archive output commit position required")


def checked_search(source):
    INPUTS["checked_source"](source)
    root = source / "public-build-search-01"
    report, sha = INPUTS["receipt"](root / "public-build-inventory.json", SEARCH_SHA)
    if (report.get("sourceCommit") != INPUTS["TREE"]["COMMIT"] or report.get("collectionComplete") is not True
            or report.get("readOnlyRpc") != SEARCH["ENDPOINT"] or report.get("query") != SEARCH["search_body"]("")
            or report.get("nextPageToken") != "" or not isinstance(report.get("pages"), list)
            or not 0 < len(report["pages"]) <= SEARCH["MAX_PAGES"]):
        raise ValueError("complete sealed fixed-commit search receipt required")
    builds, ids, tokens, token = [], set(), set(), ""
    for index, page in enumerate(report["pages"]):
        name = "page-" + str(index + 1).zfill(2) + ".public-data"
        if not isinstance(page, dict) or page.get("evidenceFile") != name:
            raise ValueError("exact sequential raw search page names required")
        contents = INPUTS["read_data"](root / name, SEARCH["MAX_PAGE_BYTES"])
        if (len(contents), hashlib.sha256(contents).hexdigest()) != (page.get("bytes"), page.get("sha256")):
            raise ValueError("raw search page identity changed")
        rows, next_token = SEARCH["public_page"](contents)
        if (len(rows) != page.get("builds") or any(row["id"] in ids for row in rows)
                or (next_token and (next_token == token or next_token in tokens))
                or (bool(next_token) != (index < len(report["pages"]) - 1))):
            raise ValueError("complete unique acyclic sealed search pages required")
        ids.update(row["id"] for row in rows)
        tokens.add(next_token)
        token = next_token
        builds.extend(rows)
    if builds != report.get("builds") or len(builds) != 361:
        raise ValueError("raw search and sealed full inventory must agree")
    for target in ARCHIVES:
        builder, identity, _, _ = archive(target)
        rows = [row for row in builds if row["id"] == identity]
        if (len(rows) != 1 or rows[0]["builder"] != {"project": "chromium", "bucket": "ci", "builder": builder}
                or rows[0].get("status") != "SUCCESS"):
            raise ValueError("compiled archive must be present in sealed same-commit search")
        checked_commit(rows[0].get("input", {}).get("gitilesCommit"))
    return sha


def log_descriptor(log, identity):
    if not isinstance(log, dict) or not isinstance(log.get("name"), str) or not 0 < len(log["name"]) <= 256:
        raise ValueError("bounded archive log descriptor required")
    url = log.get("viewUrl")
    prefix = "/logs/chromium/buildbucket/cr-buildbucket/" + identity + "/+/u/"
    if not isinstance(url, str) or len(url) > 2048:
        raise ValueError("bounded same-build log view URL required")
    parts = urlsplit(url)
    suffix = parts.path[len(prefix):] if parts.path.startswith(prefix) else ""
    if ((parts.scheme, parts.netloc) != ("https", "logs.chromium.org") or parts.query or parts.fragment
            or not suffix or not re.fullmatch(r"[A-Za-z0-9_.()/-]+", suffix)
            or any(part in ("", ".", "..") for part in suffix.split("/"))):
        raise ValueError("only exact same-build inert log descriptors allowed")
    return {"name": log["name"], "viewUrl": url, "fetched": False}


def inspect_record(target, contents):
    builder, identity, platform, filename = archive(target)
    if not 0 < len(contents) <= MAX_RECORD_BYTES:
        raise ValueError("bounded nonempty archive build record required")
    text = contents.decode("utf-8")
    if text.startswith(")]}'\n"):
        text = text[5:]
    elif text.startswith(")]}'\r\n"):
        text = text[6:]
    build = json.loads(text)
    if (not isinstance(build, dict) or set(build) != set(FIELDS.split(",")) or build["id"] != identity
            or build["builder"] != {"project": "chromium", "bucket": "ci", "builder": builder}
            or build["status"] != "SUCCESS" or not isinstance(build["input"], dict)
            or not isinstance(build["output"], dict)):
        raise ValueError("complete exact successful archive build identity required")
    checked_commit(build["input"].get("gitilesCommit"))
    checked_commit(build["output"].get("gitilesCommit"), position=True)
    properties = build["output"].get("properties")
    if (not isinstance(properties, dict) or properties.get("got_revision") != INPUTS["TREE"]["COMMIT"]
            or properties.get("got_revision_cp") != "refs/heads/main@{#" + str(POSITION) + "}"):
        raise ValueError("actual output revision properties must agree with archive source")
    pins = {}
    for component in ("angle", "swiftshader", "v8", "webrtc"):
        key = "got_" + component + "_revision"
        if properties.get(key) != INPUTS["EXTERNAL"]["PINS"][component][2]:
            raise ValueError("actual output component pin differs: " + component)
        pins[component] = properties[key]
    relative = platform + "/" + str(POSITION) + "/" + filename
    upload_name = "Generic Archiving Steps|gsutil upload " + relative
    expected_url = "gs://chromium-browser-snapshots/" + relative
    urls = properties.get("gsutil_urls")
    if not isinstance(urls, dict) or urls.get(upload_name) != expected_url:
        raise ValueError("exact target archive upload path must be recorded")
    steps, tags = build["steps"], build["tags"]
    if (not isinstance(steps, list) or not 0 < len(steps) <= 2000
            or not isinstance(tags, list) or len(tags) > 256):
        raise ValueError("bounded actual archive steps and tags required")
    names, selected = set(), []
    for step in steps:
        if (not isinstance(step, dict) or not isinstance(step.get("name"), str)
                or not 0 < len(step["name"]) <= 2048 or step["name"] in names):
            raise ValueError("unique bounded archive step names required")
        names.add(step["name"])
        if (step["name"] == upload_name or step["name"] in ("gclient config", "bot_update", "generate_build_files")
                or re.search(r"(^|[| ])gn( |$)|gclient", step["name"], re.IGNORECASE)):
            logs = step.get("logs", [])
            if not isinstance(logs, list) or len(logs) > 64:
                raise ValueError("bounded selected archive log descriptors required")
            selected.append({"name": step["name"], "status": step.get("status"),
                             "logs": [log_descriptor(log, identity) for log in logs]})
    upload = [step for step in selected if step["name"] == upload_name]
    if len(upload) != 1 or upload[0]["status"] != "SUCCESS":
        raise ValueError("exact archive upload step must have succeeded")
    buildset = "commit/gitiles/chromium.googlesource.com/chromium/src/+/" + INPUTS["TREE"]["COMMIT"]
    if sum(tag == {"key": "buildset", "value": buildset} for tag in tags) != 1:
        raise ValueError("exact unique same-commit archive buildset tag required")
    input_properties = build["input"].get("properties", {})
    if not isinstance(input_properties, dict):
        raise ValueError("actual build input properties must be an object")
    return {"target": target, "builder": builder, "buildId": identity, "status": build["status"],
            "sourceCommit": INPUTS["TREE"]["COMMIT"], "position": POSITION, "actualComponentPins": pins,
            "archiveUploadUrl": expected_url, "archiveUploadStep": upload_name, "archiveUploadStatus": "SUCCESS",
            "inputPropertyKeys": sorted(input_properties), "outputPropertyKeys": sorted(properties),
            "selectedSteps": selected, "publisherPathVerified": True,
            "gcsGenerationAssociationProven": False, "downloadedZipBytesAssociationProven": False,
            "actualGnArgsVerified": False, "actualGclientVarsVerified": False, "promoted": False}


def fetch_record(target):
    request = urllib.request.Request(ENDPOINT, data=json.dumps(get_body(target), separators=(",", ":")).encode(),
                                     headers={"Content-Type": "application/json", "Accept": "application/json"},
                                     method="POST")
    with urllib.request.build_opener(SEARCH["NoRedirect"]()).open(request, timeout=20) as response:
        if response.geturl() != ENDPOINT:
            raise ValueError("archive RPC endpoint redirect is not accepted")
        contents = response.read(MAX_RECORD_BYTES + 1)
    if not 0 < len(contents) <= MAX_RECORD_BYTES:
        raise ValueError("bounded nonempty archive response required")
    return contents


def collect(source, output):
    if output.parent != source or not re.fullmatch(r"archive-build-records-[0-9]{2}", output.name):
        raise ValueError("safe fresh task-owned archive evidence directory required")
    if output.exists() or output.is_symlink():
        raise FileExistsError("never overwrite archive evidence")
    search_sha = checked_search(source)
    output.mkdir()
    report = {"scope": "four fixed public archive build inputs/upload paths, not binary/license/shipping certification",
              "sourceCommit": INPUTS["TREE"]["COMMIT"], "searchReceiptSha256": search_sha, "readOnlyRpc": ENDPOINT,
              "records": [], "summaries": [], "collectionComplete": False, "logsFetched": False,
              "sourceFunctionsExecuted": False, "conditionsEvaluated": False, "promoted": False}
    try:
        for target in ARCHIVES:
            contents = fetch_record(target)
            name = target + ".public-data"
            with (output / name).open("xb") as stream:
                stream.write(contents)
            row = {"target": target, "request": get_body(target), "evidenceFile": name,
                   "sha256": hashlib.sha256(contents).hexdigest(), "bytes": len(contents), "verified": False}
            report["records"].append(row)
            summary = inspect_record(target, contents)
            report["summaries"].append(summary)
            row["verified"] = True
            print(json.dumps({"target": target, "bytes": len(contents), "publisherPathVerified": True}), flush=True)
        report["collectionComplete"] = True
    except Exception as error:
        report["failureType"] = type(error).__name__
        raise
    finally:
        INPUTS["EXTERNAL"]["write_report"](output / "archive-build-inventory.json", report)
    return report


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--output-name", required=True)
    args = parser.parse_args()
    source = args.source.absolute()
    result = collect(source, source / args.output_name)
    print(json.dumps({"records": len(result["records"]), "collectionComplete": result["collectionComplete"],
                      "gcsGenerationAssociationProven": False, "promoted": False}))
