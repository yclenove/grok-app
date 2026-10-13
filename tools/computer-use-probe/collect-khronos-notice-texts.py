"""Bind verified ANGLE header declarations to immutable SPDX license texts.

The SPDX dataset is a normative license-text source, not a browser dependency.
Original header attributions, inline permission text and standard placeholders
remain unchanged. Emit only an eight-header notice fragment, never a full
Chromium NOTICE, binary SBOM or shipping decision.
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
CONSUMER = runpy.run_path(str(HERE / "inspect-chromium-consumer-coverage.py"))
INPUTS, SOURCES = CONSUMER["INPUTS"], CONSUMER["SOURCES"]
REPOSITORY = "https://github.com/spdx/license-list-data"
API = "https://api.github.com/repos/spdx/license-list-data"
TAG = "v3.28.0"
TAG_OBJECT = "779ef2e5dff6d4af389c53de5e97116ab0bb52e8"
REVISION = "c4a7237ec8f4654e867546f9f409749300f1bf4c"
TREE_OBJECT = "f7502cc75f02f09e94bd67930022c74d408cfbbe"
CONSUMER_SHA = "1c4a82836579fde17a64a00a3d7ae62eca9054b56b4fa047f721522a085fed8d"
SOURCE_SEAL = "179b480aff39a2f024ec8e0b0fc1b0abefc77d21f59a8ddec29329b51b6db240"
MAX_RAW, MAX_TEXT, MAX_TOTAL = 256 * 1024, 64 * 1024, 64 * 1024
LICENSES = {
    "Apache-2.0": {"oid": "137069b823873b8bcf42979bcf8e9371052d26a2", "bytes": 10280,
                   "sha256": "074e6e32c86a4c0ef8b3ed25b721ca23aca83df277cd88106ef7177c354615ff"},
    "MIT": {"oid": "d817195dad53ec992418c28ffca5fbd1cd86502a", "bytes": 1078,
            "sha256": "b05785f9f18e6716bab63424b11454513b9943a222595b70411009202fc592b5"},
}


def requests():
    rows = [{"key": "tag-ref", "url": API + "/git/ref/tags/" + TAG},
            {"key": "tag-object", "url": API + "/git/tags/" + TAG_OBJECT},
            {"key": "commit", "url": API + "/git/commits/" + REVISION}]
    for license_id, pin in LICENSES.items():
        rows.append({"key": license_id, "path": "text/" + license_id + ".txt", "gitObjectSha1": pin["oid"],
                     "url": API + "/contents/text/" + license_id + ".txt?ref=" + REVISION})
    return rows


def fetch(request):
    if request not in requests():
        raise ValueError("only compiled immutable SPDX requests allowed")
    no_redirect = SOURCES["BASE"]["GENERATED"]["CONFIG"]["CHECKOUTS"]["LOGS"]["BUILDS"]["SEARCH"]["NoRedirect"]()
    http = urllib.request.Request(request["url"], method="GET", headers={"Accept": "application/vnd.github+json", "Accept-Encoding": "identity"})
    with urllib.request.build_opener(no_redirect).open(http, timeout=20) as response:
        if (response.geturl() != request["url"] or response.headers.get("Content-Type", "").split(";", 1)[0] != "application/json"
                or response.headers.get("Content-Encoding") not in (None, "identity")):
            raise ValueError("exact undecoded public SPDX JSON endpoint required")
        raw = response.read(MAX_RAW + 1)
    if not 0 < len(raw) <= MAX_RAW:
        raise ValueError("bounded nonempty SPDX response required")
    return raw


def checked_metadata(key, raw):
    if not 0 < len(raw) <= MAX_RAW:
        raise ValueError("bounded original SPDX JSON required")
    value = json.loads(raw.decode("utf-8", errors="strict"))
    if not isinstance(value, dict):
        raise ValueError("original SPDX JSON object required")
    if key == "tag-ref":
        valid = value.get("ref") == "refs/tags/" + TAG and value.get("object") == {
            "sha": TAG_OBJECT, "type": "tag", "url": API + "/git/tags/" + TAG_OBJECT}
    elif key == "tag-object":
        valid = value.get("sha") == TAG_OBJECT and value.get("tag") == TAG and value.get("object") == {
            "sha": REVISION, "type": "commit", "url": API + "/git/commits/" + REVISION}
    elif key == "commit":
        valid = value.get("sha") == REVISION and value.get("tree") == {
            "sha": TREE_OBJECT, "url": API + "/git/trees/" + TREE_OBJECT}
    else:
        raise ValueError("fixed SPDX identity metadata key required")
    if not valid:
        raise ValueError("exact original SPDX tag/commit/tree identity required")


def decoded_license(license_id, raw, pins=None):
    pins = LICENSES if pins is None else pins
    if license_id not in pins or not 0 < len(raw) <= MAX_RAW:
        raise ValueError("fixed SPDX license and bounded response required")
    pin = pins[license_id]
    row = json.loads(raw.decode("utf-8", errors="strict"))
    if (not isinstance(row, dict) or row.get("path") != "text/" + license_id + ".txt"
            or row.get("sha") != pin["oid"] or row.get("size") != pin["bytes"] or row.get("type") != "file"
            or row.get("git_url") != API + "/git/blobs/" + pin["oid"] or row.get("encoding") != "base64"
            or not isinstance(row.get("content"), str) or len(row["content"]) > MAX_RAW):
        raise ValueError("exact immutable SPDX file identity required")
    contents = base64.b64decode(row["content"].replace("\n", ""), validate=True)
    identity = INPUTS["TREE"]["check_blob"](pin["oid"], contents)
    if identity != {key: pin[key] for key in ("bytes", "sha256")} or not 0 < len(contents) <= MAX_TEXT:
        raise ValueError("original complete SPDX license bytes/digest required")
    contents.decode("utf-8", errors="strict")
    return contents, identity


def checked_headers(source):
    original, sha = INPUTS["receipt"](source / "consumer-coverage-01.json", CONSUMER_SHA)
    inputs, checkout_sha, correspondence = SOURCES["checked_inputs"](source)
    blobs = {}
    for profile in CONSUMER["PROFILE_SEALS"]:
        added, _ = CONSUMER["INTEGRATION"]["checked_profile"](source, profile, inputs, checkout_sha, correspondence, profiles=CONSUMER["PROFILE_SEALS"])
        blobs.update(added)
    headers = CONSUMER["copied_headers"](blobs)
    if headers != original.get("copiedKhronosHeaders"):
        raise ValueError("original verified eight-header attribution/include records must reproduce exactly")
    return headers, sha


def render_fragment(headers, license_texts):
    if set(license_texts) != set(LICENSES) or len(headers.get("headers", [])) != 8:
        raise ValueError("both full license texts and eight original headers required")
    parts = ["Khronos headers copied in ANGLE: notice fragment\n\n"
             "Scope: the eight verified files listed below. This fragment is not a complete Chromium NOTICE.\n"
             "Actual copyright and declarations are retained from those files.\n"
             "SPDX standard-text copyright placeholders remain verbatim; actual copyright is in each source declaration.\n"]
    bindings = []
    for row in headers["headers"]:
        license_id = row["declaredSpdx"]
        if license_id is not None and license_id not in license_texts:
            raise ValueError("every original SPDX declaration needs its exact complete license text")
        parts.append("Source: " + row["sourcePath"] + "\n" + row["originalAttributionText"] + "\n")
        if license_id is not None:
            bindings.append({"sourcePath": row["sourcePath"], "sourceSha256": row["sourceSha256"],
                             "attributionSha256": row["attributionSha256"], "copyrightYears": row["copyrightYears"],
                             "licenseId": license_id, "fullLicenseGitObjectSha1": LICENSES[license_id]["oid"],
                             "fullLicenseSha256": hashlib.sha256(license_texts[license_id]).hexdigest()})
    if len(bindings) != 7 or sum(row["fullLicenseTextInBlock"] for row in headers["headers"]) != 1:
        raise ValueError("seven SPDX and one unchanged inline permission header required")
    for license_id, contents in license_texts.items():
        pin = LICENSES[license_id]
        if INPUTS["TREE"]["check_blob"](pin["oid"], contents) != {key: pin[key] for key in ("bytes", "sha256")}:
            raise ValueError("complete canonical license text cannot change")
        parts.append("Complete SPDX standard text: " + license_id + "\n" + contents.decode("utf-8"))
    return "\n".join(parts).encode("utf-8"), bindings


def collect(source, output):
    INPUTS["checked_source"](source)
    if output.parent != source or not re.fullmatch(r"khronos-license-source-[0-9]{2}", output.name):
        raise ValueError("safe fresh task-owned Khronos license output required")
    if output.exists() or output.is_symlink():
        raise FileExistsError("never overwrite Khronos license evidence")
    headers, header_sha = checked_headers(source)
    output.mkdir()
    report = {"scope": "complete normative SPDX texts bound to eight actual ANGLE headers only",
              "browserSourceCommit": INPUTS["TREE"]["COMMIT"], "consumerReceiptSha256": header_sha,
              "spdxRepository": REPOSITORY, "releaseTag": TAG, "tagObjectSha1": TAG_OBJECT,
              "spdxRevision": REVISION, "spdxTreeSha1": TREE_OBJECT, "requests": requests(), "records": [],
              "collectionComplete": False, "fullLicenseBindingsComplete": False,
              "sourceFunctionsExecuted": False, "generatorExecuted": False, "metadataPathsAliased": False,
              "spdxDatasetIsBrowserComponent": False, "completeChromiumNotice": False, "promoted": False}
    texts, total = {}, 0
    try:
        for request in requests():
            raw = fetch(request)
            filename = request["key"] + ".json-data"
            with (output / filename).open("xb") as stream:
                stream.write(raw)
            row = {**request, "rawFile": filename, "rawBytes": len(raw), "rawSha256": hashlib.sha256(raw).hexdigest(), "verified": False}
            report["records"].append(row)
            if request["key"] in LICENSES:
                contents, identity = decoded_license(request["key"], raw)
                total += len(contents)
                if total > MAX_TOTAL:
                    raise ValueError("bounded complete SPDX selected-text total required")
                data_name = request["key"] + ".license-data"
                with (output / data_name).open("xb") as stream:
                    stream.write(contents)
                texts[request["key"]] = contents
                row.update(sourceFile=data_name, **identity)
            else:
                checked_metadata(request["key"], raw)
            row["verified"] = True
        contents, bindings = render_fragment(headers, texts)
        with (output / "khronos-header-notice.txt").open("xb") as stream:
            stream.write(contents)
        report.update(collectionComplete=True, fullLicenseBindingsComplete=True, selectedLicenseBytes=total,
                      bindings=bindings, inlinePermissionHeaders=1, spdxHeaders=7,
                      fragmentFile="khronos-header-notice.txt", fragmentBytes=len(contents),
                      fragmentSha256=hashlib.sha256(contents).hexdigest())
    except Exception as error:
        report["failureType"] = type(error).__name__
        raise
    finally:
        INPUTS["EXTERNAL"]["write_report"](output / "khronos-license-inventory.json", report)
    return report


def verify(source, name):
    INPUTS["checked_source"](source)
    if not re.fullmatch(r"khronos-license-source-[0-9]{2}", name) or SOURCE_SEAL is None:
        raise ValueError("fixed completed source seal and safe Khronos source name required")
    root = source / name
    report, sha = INPUTS["receipt"](root / "khronos-license-inventory.json", SOURCE_SEAL)
    headers, header_sha = checked_headers(source)
    identities = {"browserSourceCommit": INPUTS["TREE"]["COMMIT"], "spdxRepository": REPOSITORY,
                  "releaseTag": TAG, "tagObjectSha1": TAG_OBJECT, "spdxRevision": REVISION,
                  "spdxTreeSha1": TREE_OBJECT, "inlinePermissionHeaders": 1, "spdxHeaders": 7}
    if (any(report.get(key) != value for key, value in identities.items())
            or report.get("collectionComplete") is not True or report.get("fullLicenseBindingsComplete") is not True
            or report.get("consumerReceiptSha256") != header_sha or report.get("requests") != requests()
            or len(report.get("records", [])) != len(requests())
            or any(report.get(key) is not False for key in ("sourceFunctionsExecuted", "generatorExecuted", "metadataPathsAliased",
                "spdxDatasetIsBrowserComponent", "completeChromiumNotice", "promoted"))):
        raise ValueError("complete original non-promoted eight-header license receipt required")
    texts = {}
    for request, row in zip(requests(), report["records"]):
        if (any(row.get(key) != value for key, value in request.items()) or row.get("verified") is not True
                or row.get("rawFile") != request["key"] + ".json-data"):
            raise ValueError("original exact bounded SPDX data identity required")
        raw = INPUTS["read_data"](root / row["rawFile"], MAX_RAW)
        if (len(raw), hashlib.sha256(raw).hexdigest()) != (row.get("rawBytes"), row.get("rawSha256")):
            raise ValueError("original SPDX raw bytes changed")
        if request["key"] in LICENSES:
            contents, identity = decoded_license(request["key"], raw)
            if (row.get("sourceFile") != request["key"] + ".license-data"
                    or identity != {key: row.get(key) for key in ("bytes", "sha256")}
                    or INPUTS["read_data"](root / row["sourceFile"], MAX_TEXT) != contents):
                raise ValueError("original complete SPDX license bytes changed")
            texts[request["key"]] = contents
        else:
            checked_metadata(request["key"], raw)
    fragment, bindings = render_fragment(headers, texts)
    if (sum(map(len, texts.values())) != report.get("selectedLicenseBytes") or report.get("bindings") != bindings
            or report.get("fragmentFile") != "khronos-header-notice.txt" or report.get("fragmentBytes") != len(fragment)
            or report.get("fragmentSha256") != hashlib.sha256(fragment).hexdigest()
            or INPUTS["read_data"](root / "khronos-header-notice.txt", MAX_TOTAL) != fragment):
        raise ValueError("exact original header bindings and notice fragment required")
    return {"sourceReceiptSha256": sha, "verified": True, "licenseBytes": sum(map(len, texts.values())),
            "spdxHeaders": len(bindings), "inlinePermissionHeaders": 1, "fragmentSha256": hashlib.sha256(fragment).hexdigest(),
            "completeChromiumNotice": False, "promoted": False}


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    action = parser.add_mutually_exclusive_group(required=True)
    action.add_argument("--output-name")
    action.add_argument("--verify")
    args = parser.parse_args()
    source = args.source.absolute()
    result = verify(source, args.verify) if args.verify else collect(source, source / args.output_name)
    print(json.dumps({key: value for key, value in result.items() if key in (
        "verified", "sourceReceiptSha256", "licenseBytes", "selectedLicenseBytes", "spdxHeaders", "inlinePermissionHeaders",
        "fragmentSha256", "collectionComplete", "completeChromiumNotice", "promoted")}))
