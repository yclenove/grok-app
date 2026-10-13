"""Bounded public CI SearchBuilds for one compiled Chromium commit, data only.

The only HTTP POST is the read-only Buildbucket search RPC. No GetBuild logs,
authentication, build scheduling/cancellation, arbitrary endpoints or code
execution. A matching build is not proven to have published our four ZIPs.
"""
import argparse
import hashlib
import json
from pathlib import Path
import re
import runpy
import urllib.request


HERE = Path(__file__).parent.absolute()
INPUTS = runpy.run_path(str(HERE / "inspect-chromium-license-input-gaps.py"))
ENDPOINT = "https://cr-buildbucket.appspot.com/prpc/buildbucket.v2.Builds/SearchBuilds"
MAX_PAGE_BYTES = 4 * 1024**2
MAX_PAGES = 8
PAGE_SIZE = 100


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        raise ValueError("read-only public RPC may not follow any endpoint redirect")


def search_body(token):
    if not isinstance(token, str) or len(token) > 16384 or any(ord(char) < 32 or ord(char) > 126 for char in token):
        raise ValueError("bounded opaque public pagination token required")
    body = {"predicate": {"builder": {"project": "chromium", "bucket": "ci"},
                           "tags": [{"key": "buildset", "value": "commit/gitiles/chromium.googlesource.com/chromium/src/+/"
                                     + INPUTS["TREE"]["COMMIT"]}]}, "pageSize": PAGE_SIZE}
    if token:
        body["pageToken"] = token
    return body


def public_page(contents):
    if not 0 < len(contents) <= MAX_PAGE_BYTES:
        raise ValueError("bounded nonempty public metadata page required")
    text = contents.decode("utf-8")
    if text.startswith(")]}'\n"):
        text = text[5:]
    elif text.startswith(")]}'\r\n"):
        text = text[6:]
    result = json.loads(text)
    if not isinstance(result, dict) or set(result) - {"builds", "nextPageToken"}:
        raise ValueError("only read-only search response fields allowed")
    builds = result.get("builds", [])
    if not isinstance(builds, list) or len(builds) > PAGE_SIZE:
        raise ValueError("bounded public build list required")
    ids = set()
    for build in builds:
        if not isinstance(build, dict) or not isinstance(build.get("id"), str) or not re.fullmatch(r"[0-9]{10,30}", build["id"]):
            raise ValueError("numeric public build identity required")
        builder = build.get("builder", {})
        if (not isinstance(builder, dict) or (builder.get("project"), builder.get("bucket")) != ("chromium", "ci")
                or not isinstance(builder.get("builder"), str) or not 0 < len(builder["builder"]) <= 512
                or build["id"] in ids):
            raise ValueError("unique public Chromium CI build identities required")
        inputs = build.get("input", {})
        if not isinstance(inputs, dict):
            raise ValueError("public build input metadata must be an object")
        commit = inputs.get("gitilesCommit")
        if commit is not None and (not isinstance(commit, dict) or (
                commit.get("host"), commit.get("project"), commit.get("id")) != (
                    "chromium.googlesource.com", "chromium/src", INPUTS["TREE"]["COMMIT"])):
            raise ValueError("available public build input must match the exact queried source commit")
        ids.add(build["id"])
    token = result.get("nextPageToken", "")
    search_body(token)
    return builds, token


def fetch_page(token):
    data = json.dumps(search_body(token), separators=(",", ":")).encode("utf-8")
    request = urllib.request.Request(ENDPOINT, data=data, method="POST",
                                     headers={"Content-Type": "application/json", "Accept": "application/json"})
    with urllib.request.build_opener(NoRedirect()).open(request, timeout=20) as response:
        if response.geturl() != ENDPOINT:
            raise ValueError("public RPC endpoint redirect is not accepted")
        contents = response.read(MAX_PAGE_BYTES + 1)
    public_page(contents)
    return contents


def collect(source, output):
    if output.parent != source or not re.fullmatch(r"public-build-search-[0-9]{2}", output.name):
        raise ValueError("safe fresh public build evidence directory required")
    if output.exists() or output.is_symlink():
        raise FileExistsError("never overwrite public build evidence")
    INPUTS["checked_source"](source)
    output.mkdir()
    report = {"scope": "public CI builds tagged with one fixed commit, not publisher/build-args/binary provenance certification",
              "sourceCommit": INPUTS["TREE"]["COMMIT"], "readOnlyRpc": ENDPOINT,
              "query": search_body(""), "pages": [], "builds": [], "collectionComplete": False,
              "builderInputsCollected": False, "publisherAssociationProven": False,
              "sourceFunctionsExecuted": False, "conditionsEvaluated": False, "promoted": False}
    token, tokens, ids = "", set(), set()
    try:
        for index in range(MAX_PAGES):
            contents = fetch_page(token)
            builds, next_token = public_page(contents)
            if any(build["id"] in ids for build in builds) or (next_token and (next_token == token or next_token in tokens)):
                raise ValueError("duplicate build or cyclic public search pagination")
            ids.update(build["id"] for build in builds)
            sha = hashlib.sha256(contents).hexdigest()
            name = "page-" + str(index + 1).zfill(2) + ".public-data"
            with (output / name).open("xb") as stream:
                stream.write(contents)
            report["pages"].append({"evidenceFile": name, "sha256": sha, "bytes": len(contents), "builds": len(builds)})
            report["builds"].extend(builds)
            print(json.dumps({"page": index + 1, "builds": len(builds), "hasMore": bool(next_token)}), flush=True)
            if not next_token:
                report["collectionComplete"] = True
                break
            tokens.add(next_token)
            token = next_token
        report["nextPageToken"] = "" if report["collectionComplete"] else token
    except Exception as error:
        report["failureType"] = type(error).__name__
        raise
    finally:
        INPUTS["EXTERNAL"]["write_report"](output / "public-build-inventory.json", report)
    return report


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--output-name", required=True)
    args = parser.parse_args()
    source = args.source.absolute()
    report = collect(source, source / args.output_name)
    print(json.dumps({"builds": len(report["builds"]), "collectionComplete": report["collectionComplete"],
                      "publisherAssociationProven": False, "promoted": False}))
    if not report["collectionComplete"]:
        raise SystemExit(2)
