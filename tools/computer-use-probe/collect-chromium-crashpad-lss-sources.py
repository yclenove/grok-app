"""Collect fixed Chromium Crashpad configuration and actual Linux lss source.

Only compiled public Gitiles endpoints; static Git tree/blob verification and
inert data. No git lazy fetch, upstream code/GN execution, source aliases or
shipping exclusions. Other platforms' absent lss entry remains explicit.
"""
import argparse
import base64
import hashlib
import json
from pathlib import Path
import re
import runpy
import urllib.request


HERE = Path(__file__).parent.absolute()
GENERATED = runpy.run_path(str(HERE / "collect-chromium-generated-input-sources.py"))
CHECKOUTS = runpy.run_path(str(HERE / "inspect-chromium-archive-checkouts.py"))
INPUTS, EXTERNAL = GENERATED["INPUTS"], GENERATED["EXTERNAL"]
COMMIT = INPUTS["TREE"]["COMMIT"]
REVISION = "29164a80da4d41134950d76d55199ea33fbb9613"
REPOSITORY = "https://chromium.googlesource.com/linux-syscall-support.git"
GITILES = "https://chromium.googlesource.com/linux-syscall-support"
PREFIX = "third_party/lss"
MAIN_PINS = {
    "build/config/BUILDCONFIG.gn": "a8dcb4d4675505227c2259a8851d9b2ee904faef",
    "build/config/chrome_build.gni": "4163d9806888f00e37201be05bea8a302c8f757f",
    "third_party/crashpad/crashpad/third_party/lss/lss.h": "90cb9bcfe78391d2873f05c28533ee5986cc6651",
    "third_party/crashpad/crashpad/third_party/zlib/zlib_crashpad.h": "c720872210afcd4fe0e3dccbda788dcf25608d27",
}
DEFAULT_PIN = {"path": ".gn", "gitObjectSha1": "84c68fbae243a60a676b0234cf850331adbb87d9",
               "url": "https://chromium.googlesource.com/chromium/src/+/" + COMMIT + "/.gn?format=TEXT"}
MAX_RAW, MAX_SOURCE, MAX_TOTAL = 2 * 1024**2, 1024**2, 8 * 1024**2
COMMIT_URL = GITILES + "/+/" + REVISION + "?format=JSON"
TREE_URL = GITILES + "/+/" + REVISION + "/?format=JSON"


def main_requests():
    return [{"path": path, "gitObjectSha1": oid,
             "url": "https://chromium.googlesource.com/chromium/src/+/" + COMMIT + "/" + path + "?format=TEXT"}
            for path, oid in MAIN_PINS.items()]


def selected_entry(row):
    name = row["name"].upper()
    return row["type"] == "blob" and row["mode"] in (0o100644, 0o100755) and (
        row["name"] in {"linux_syscall_support.h", "README.chromium", "README.md", "README"}
        or name.startswith(("LICENSE", "LICENCE", "COPYING", "COPYRIGHT", "NOTICE")))


def lss_request(row):
    if not selected_entry(row) or "/" in row["name"] or "\\" in row["name"]:
        raise ValueError("only reviewed root lss header/metadata/license blobs allowed")
    return {"path": row["name"], "gitObjectSha1": row["id"],
            "url": GITILES + "/+/" + REVISION + "/" + row["name"] + "?format=TEXT"}


def fetch(url, content_type):
    allowed_main = {row["url"] for row in main_requests()} | {DEFAULT_PIN["url"]}
    lss_prefix = GITILES + "/+/" + REVISION + "/"
    if url not in allowed_main and url not in {COMMIT_URL, TREE_URL}:
        if (not url.startswith(lss_prefix) or not url.endswith("?format=TEXT")
                or not re.fullmatch(r"[A-Za-z0-9_.-]+", url[len(lss_prefix):-len("?format=TEXT")])):
            raise ValueError("fixed same-revision public Gitiles endpoint required")
        name = url[len(lss_prefix):-len("?format=TEXT")]
        if not selected_entry({"name": name, "mode": 0o100644, "type": "blob"}):
            raise ValueError("only fixed lss header/metadata/license filenames allowed")
    request = urllib.request.Request(url, method="GET", headers={"Accept": content_type})
    no_redirect = GENERATED["CONFIG"]["CHECKOUTS"]["LOGS"]["BUILDS"]["SEARCH"]["NoRedirect"]()
    with urllib.request.build_opener(no_redirect).open(request, timeout=20) as response:
        if response.geturl() != url or response.headers.get("Content-Type", "").split(";", 1)[0] != content_type:
            raise ValueError("exact bounded Gitiles content type and URL required")
        data = response.read(MAX_RAW + 1)
    if not 0 < len(data) <= MAX_RAW:
        raise ValueError("bounded nonempty Gitiles response required")
    return data


def gitiles_json(contents):
    if not 0 < len(contents) <= MAX_RAW or not contents.startswith(b")]}'\n"):
        raise ValueError("bounded original Gitiles JSON prefix required")
    result = json.loads(contents[5:])
    if not isinstance(result, dict):
        raise ValueError("Gitiles object required")
    return result


def checked_commit(contents):
    row = gitiles_json(contents)
    if row.get("commit") != REVISION or not re.fullmatch(r"[0-9a-f]{40}", row.get("tree", "")):
        raise ValueError("exact original lss commit and root tree required")
    return row


def checked_tree(contents, tree_oid):
    row = gitiles_json(contents)
    entries = row.get("entries")
    if row.get("id") != tree_oid or not isinstance(entries, list) or not 0 < len(entries) <= 1000:
        raise ValueError("complete bounded exact lss root tree required")
    names = set()
    for entry in entries:
        name = entry.get("name")
        if (not isinstance(name, str) or not re.fullmatch(r"[A-Za-z0-9_.-]+", name) or name in (".", "..")
                or name in names or not re.fullmatch(r"[0-9a-f]{40}", entry.get("id", ""))
                or type(entry.get("mode")) is not int
                or (entry["mode"], entry.get("type")) not in ((0o100644, "blob"), (0o100755, "blob"),
                    (0o120000, "blob"), (0o40000, "tree"), (0o160000, "commit"))):
            raise ValueError("unique inert root Git tree entry identities required")
        names.add(name)
    ordered = sorted(entries, key=lambda entry: (entry["name"] + ("/" if entry["type"] == "tree" else "")).encode("utf-8"))
    raw = b"".join(format(entry["mode"], "o").encode() + b" " + entry["name"].encode() + b"\0" + bytes.fromhex(entry["id"])
                   for entry in ordered)
    oid = hashlib.sha1(b"tree " + str(len(raw)).encode() + b"\0" + raw).hexdigest()
    if oid != tree_oid:
        raise ValueError("complete reconstructed lss Git tree hash mismatch")
    if "linux_syscall_support.h" not in names:
        raise ValueError("original lss root header required")
    return row


def decoded_blob(raw, oid):
    if not 0 < len(raw) <= MAX_RAW:
        raise ValueError("bounded original base64 blob required")
    data = base64.b64decode(raw, validate=True)
    if not 0 < len(data) <= MAX_SOURCE:
        raise ValueError("bounded nonempty source bytes required")
    identity = INPUTS["TREE"]["check_blob"](oid, data)
    data.decode("utf-8", errors="strict")
    return data, identity


def checked_inputs(source):
    inputs = INPUTS["load_inputs"](source)
    for path, oid in MAIN_PINS.items():
        row = inputs["index"].get(path)
        if row is None or row["mode"] != "100644" or row["gitObjectSha1"] != oid:
            raise ValueError("four independent main-tree exact source blobs required")
    link = inputs["index"].get(PREFIX)
    if link is None or (link["mode"], link["gitObjectSha1"]) != ("160000", REVISION):
        raise ValueError("original main-tree exact lss Gitlink required")
    raw = INPUTS["read_data"](source / EXTERNAL["CATALOG"], INPUTS["TREE"]["MAX_BLOB"])
    catalog = EXTERNAL["checked_catalog"](raw)
    lss = [row for row in catalog["gitDependencies"] if row["path"] == PREFIX]
    if len(lss) != 1 or lss[0] != {"path": PREFIX, "repository": REPOSITORY, "revision": REVISION,
            "condition": "checkout_android or checkout_linux", "gitlinkRevision": REVISION, "gitlinkMatched": True}:
        raise ValueError("exact reviewed main DEPS lss declaration required")
    checkouts = CHECKOUTS["inspect"](source)
    rows = [{"target": row["target"], **CHECKOUTS["compare_pin"]("src/" + PREFIX, REPOSITORY, REVISION, row["manifest"])}
            for row in checkouts["targets"]]
    expected = {"windows-x64": "exact_manifest_entry_absent", "linux-x64": "exact_repository_and_revision_match",
                "mac-intel": "exact_manifest_entry_absent", "mac-arm64": "exact_manifest_entry_absent"}
    if {row["target"]: row["status"] for row in rows} != expected:
        raise ValueError("exact original four-platform lss checkout correspondence required")
    return inputs["mainSha256"], checkouts["checkoutReceiptSha256"], rows


def collect_blob(output, request, records=None, source_group=None):
    raw = fetch(request["url"], "text/plain")
    oid = request["gitObjectSha1"]
    raw_name, source_name = oid + ".gitiles-data", oid + ".source-data"
    with (output / raw_name).open("xb") as stream:
        stream.write(raw)
    row = {**request, "rawFile": raw_name, "rawBytes": len(raw), "rawSha256": hashlib.sha256(raw).hexdigest(), "verified": False}
    if source_group is not None:
        row["sourceGroup"] = source_group
    if records is not None:
        records.append(row)
    data, identity = decoded_blob(raw, oid)
    with (output / source_name).open("xb") as stream:
        stream.write(data)
    row.update(sourceFile=source_name, verified=True, **identity)
    return row


def collect(source, output):
    if output.parent != source or not re.fullmatch(r"crashpad-lss-source-[0-9]{2}", output.name):
        raise ValueError("safe fresh crashpad/lss source directory required")
    if output.exists() or output.is_symlink():
        raise FileExistsError("never overwrite crashpad/lss source evidence")
    main_sha, checkout_sha, correspondence = checked_inputs(source)
    output.mkdir()
    report = {"scope": "fixed Crashpad main configuration and actual Linux lss source only, not NOTICE/binary approval",
              "sourceCommit": COMMIT, "mainReceiptSha256": main_sha, "checkoutReceiptSha256": checkout_sha,
              "lssRepository": REPOSITORY, "lssRevision": REVISION, "lssCheckoutCorrespondence": correspondence,
              "records": [], "collectionComplete": False, "conditionsEvaluated": False,
              "sourceFunctionsExecuted": False, "generatorExecuted": False, "shippingDecisionsMade": False,
              "effectiveGnMeasured": False, "licenseCoverage": "not_proven", "promoted": False}
    total = 0
    try:
        for request in main_requests():
            row = collect_blob(output, request, report["records"], "main")
            total += row["bytes"]
            print(json.dumps({"path": row["path"], "bytes": row["bytes"], "verified": True}), flush=True)
        raw = fetch(COMMIT_URL, "application/json")
        with (output / "lss-commit.json-data").open("xb") as stream:
            stream.write(raw)
        report["lssCommit"] = {"bytes": len(raw), "sha256": hashlib.sha256(raw).hexdigest(), "value": checked_commit(raw)}
        tree_oid = report["lssCommit"]["value"]["tree"]
        raw = fetch(TREE_URL, "application/json")
        with (output / "lss-tree.json-data").open("xb") as stream:
            stream.write(raw)
        report["lssRootTree"] = {"bytes": len(raw), "sha256": hashlib.sha256(raw).hexdigest(), "value": checked_tree(raw, tree_oid)}
        for entry in report["lssRootTree"]["value"]["entries"]:
            if selected_entry(entry):
                row = collect_blob(output, lss_request(entry), report["records"], "lss")
                total += row["bytes"]
                if total > MAX_TOTAL:
                    raise ValueError("selected source total byte budget")
                print(json.dumps({"path": "third_party/lss/" + row["path"], "bytes": row["bytes"], "verified": True}), flush=True)
        report["uncollectedRootDirectories"] = [row for row in report["lssRootTree"]["value"]["entries"] if row["type"] == "tree"]
        report["collectionComplete"] = True
    except Exception as error:
        report["failureType"] = type(error).__name__
        raise
    finally:
        report["selectedSourceBytes"] = total
        EXTERNAL["write_report"](output / "crashpad-lss-inventory.json", report)
    return report


def collect_defaults(source, output):
    if output.parent != source or not re.fullmatch(r"crashpad-default-source-[0-9]{2}", output.name):
        raise ValueError("safe fresh Crashpad default source directory required")
    if output.exists() or output.is_symlink():
        raise FileExistsError("never overwrite Crashpad default source evidence")
    inputs = INPUTS["load_inputs"](source)
    entry = inputs["index"].get(DEFAULT_PIN["path"])
    if entry is None or entry["mode"] != "100644" or entry["gitObjectSha1"] != DEFAULT_PIN["gitObjectSha1"]:
        raise ValueError("independent exact root .gn main-tree blob required")
    output.mkdir()
    report = {"scope": "exact Chromium root .gn default source only, not measured effective GN or binary approval",
              "sourceCommit": COMMIT, "mainReceiptSha256": inputs["mainSha256"], "records": [],
              "collectionComplete": False, "sourceFunctionsExecuted": False, "effectiveGnMeasured": False,
              "generatorExecuted": False, "promoted": False}
    try:
        row = collect_blob(output, DEFAULT_PIN, report["records"], "main")
        print(json.dumps({"path": row["path"], "bytes": row["bytes"], "verified": True}), flush=True)
        report["collectionComplete"] = True
    except Exception as error:
        report["failureType"] = type(error).__name__
        raise
    finally:
        EXTERNAL["write_report"](output / "crashpad-default-inventory.json", report)
    return report


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--output-name", required=True)
    parser.add_argument("--profile", choices=("sources", "defaults"), default="sources")
    args = parser.parse_args()
    source = args.source.absolute()
    report = (collect if args.profile == "sources" else collect_defaults)(source, source / args.output_name)
    print(json.dumps({"collectionComplete": report["collectionComplete"], "records": len(report["records"]),
                      "sourceBytes": sum(row.get("bytes", 0) for row in report["records"]), "promoted": False}))
