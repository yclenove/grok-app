"""Read the original immutable recipe ZIP as inert bounded source evidence.

Only a fixed public GetInstanceURL RPC and its HTTPS Google Storage URL are
used. Verify the entire ZIP digest before inspection. No extraction, package
installation, source execution, cookies, user credentials or product changes.
"""
import argparse
import hashlib
import json
from pathlib import Path
import re
import runpy
import stat
from urllib.parse import urlsplit
import urllib.request
import zipfile


HERE = Path(__file__).parent.absolute()
RECIPE = runpy.run_path(str(HERE / "collect-chromium-recipe-instance.py"))
INPUTS = RECIPE["INPUTS"]
EXTERNAL = INPUTS["EXTERNAL"]
INSTANCE_SHA = "fd0a6a2ccb216b5017e05c965cf36d36912bfcb21d9a628de288308da5b006c6"
ENDPOINT = RECIPE["SERVER"] + "/prpc/cipd.Repository/GetInstanceURL"
MAX_RPC = 64 * 1024
MAX_ZIP = 64 * 1024**2
MAX_ENTRIES = 20000
MAX_FILE = 4 * 1024**2
MAX_TREE = 128 * 1024**2
MAX_SELECTED = 16 * 1024**2
SUFFIXES = ("recipes.cfg", "recipe_modules/depot_tools/api.py", "recipe_modules/depot_tools/__init__.py",
            "recipe_modules/gclient/api.py", "recipe_modules/gclient/config.py", "recipe_modules/bot_update/api.py",
            "recipes/chromium.py", "depot_tools/gclient.py", "depot_tools/gclient_eval.py",
            "bot_update/resources/bot_update.py", ".cipd/manifest.json")


def url_request():
    return {"package": RECIPE["PACKAGE"], "instance": RECIPE["object_ref"]()}


def opener():
    return urllib.request.build_opener(RECIPE["CHECKOUTS"]["LOGS"]["BUILDS"]["SEARCH"]["NoRedirect"]())


def checked_url(value):
    if not isinstance(value, str) or not 0 < len(value) <= 32768 or any(ord(char) < 33 or ord(char) > 126 for char in value):
        raise ValueError("bounded ASCII public storage URL required")
    url = urlsplit(value)
    if (url.scheme != "https" or url.netloc != "storage.googleapis.com" or url.username or url.password
            or url.fragment or not url.path.startswith("/") or url.path == "/"):
        raise ValueError("only server-returned exact HTTPS Google Storage host allowed")
    return value


def fetch_url():
    request = urllib.request.Request(ENDPOINT, method="POST", data=json.dumps(url_request()).encode(),
                                     headers={"Accept": "application/json", "Content-Type": "application/json"})
    with opener().open(request, timeout=20) as response:
        if response.geturl() != ENDPOINT:
            raise ValueError("GetInstanceURL redirect refused")
        if response.headers.get("Content-Type", "").split(";", 1)[0].strip().lower() != "application/json":
            raise ValueError("public package URL JSON response required")
        contents = response.read(MAX_RPC + 1)
    if not 0 < len(contents) <= MAX_RPC:
        raise ValueError("bounded nonempty GetInstanceURL response required")
    return contents


def parsed_url(contents):
    if not 0 < len(contents) <= MAX_RPC:
        raise ValueError("bounded GetInstanceURL raw record required")
    result = RECIPE["CHECKOUTS"]["LOGS"]["decoded_record"](contents)
    if not isinstance(result, dict):
        raise ValueError("GetInstanceURL must be a JSON object")
    return checked_url(result.get("signedUrl"))


def checked_inputs(source):
    RECIPE["checked_inputs"](source)
    root = source / "recipe-instance-01"
    report, sha = INPUTS["receipt"](root / "recipe-instance-inventory.json", INSTANCE_SHA)
    if (report.get("package") != RECIPE["PACKAGE"] or report.get("instanceVersion") != RECIPE["VERSION"]
            or report.get("instanceObject") != RECIPE["object_ref"]() or report.get("collectionComplete") is not True
            or report.get("requests") != list(RECIPE["requests"]()) or report.get("packageDownloaded") is not False
            or report.get("promoted") is not False or not isinstance(report.get("responses"), list)
            or len(report["responses"]) != 2):
        raise ValueError("complete sealed original recipe instance receipt required")
    for request, row in zip(RECIPE["requests"](), report["responses"]):
        if row.get("key") != request["key"] or row.get("evidenceFile") != request["key"] + ".public-data" or row.get("verified") is not True:
            raise ValueError("original recipe response identities required")
        contents = INPUTS["read_data"](root / row["evidenceFile"], RECIPE["MAX_RESPONSE"])
        if (len(contents), hashlib.sha256(contents).hexdigest()) != (row.get("bytes"), row.get("sha256")):
            raise ValueError("original recipe raw response changed")
        RECIPE["parse_response"](request["key"], contents)
    return sha


def download(url, path):
    checked_url(url)
    digest, size = hashlib.sha256(), 0
    request = urllib.request.Request(url, method="GET", headers={"Accept": "application/octet-stream"})
    with opener().open(request, timeout=20) as response, path.open("xb") as stream:
        if response.geturl() != url:
            raise ValueError("package URL redirect refused")
        length = response.headers.get("Content-Length")
        if length is not None and (not re.fullmatch(r"[0-9]{1,12}", length) or not 0 < int(length) <= MAX_ZIP):
            raise ValueError("bounded package content length required")
        while True:
            contents = response.read(min(1024**2, MAX_ZIP - size + 1))
            if not contents:
                break
            if size + len(contents) > MAX_ZIP:
                raise ValueError("recipe ZIP byte budget exceeded; bounded partial file retained")
            stream.write(contents)
            digest.update(contents)
            size += len(contents)
    if not size or (length is not None and size != int(length)) or digest.hexdigest() != RECIPE["object_ref"]()["hexDigest"]:
        raise ValueError("original immutable recipe ZIP digest/size mismatch; raw retained")
    return {"bytes": size, "sha256": digest.hexdigest()}


def zip_records(archive):
    infos = archive.infolist()
    if not 0 < len(infos) <= MAX_ENTRIES:
        raise ValueError("recipe archive entry budget")
    seen, total, rows = set(), 0, []
    for info in infos:
        path = info.filename.rstrip("/")
        if (info.orig_filename != info.filename or not path or path in seen or len(path) > 2048 or path.startswith("/") or "\\" in path or ":" in path
                or any(part in ("", ".", "..") for part in path.split("/")) or any(ord(char) < 32 for char in path)
                or info.flag_bits & 1 or info.file_size > MAX_FILE or info.file_size < 0):
            raise ValueError("unsafe/duplicate/oversized recipe ZIP entry")
        seen.add(path)
        total += info.file_size
        if total > MAX_TREE:
            raise ValueError("recipe ZIP unpacked tree budget")
        mode = (info.external_attr >> 16) & 0xffff
        kind = ("directory" if info.is_dir() else "symlink" if stat.S_ISLNK(mode)
                else "regular" if stat.S_IFMT(mode) == 0 or stat.S_ISREG(mode) else "other")
        picked = kind == "regular" and any(path == suffix or path.endswith("/" + suffix) for suffix in SUFFIXES)
        rows.append({"path": path, "kind": kind, "bytes": info.file_size,
                     "compressedBytes": info.compress_size, "crc32": info.CRC, "selected": picked})
    return rows, total


def inspect_zip(path, output):
    with zipfile.ZipFile(path) as archive:
        rows, total = zip_records(archive)
        picked, size = [], 0
        blobs = output / "blobs"
        blobs.mkdir()
        for row in rows:
            if not row["selected"]:
                continue
            contents = archive.read(row["path"])
            size += len(contents)
            if len(contents) != row["bytes"] or size > MAX_SELECTED:
                raise ValueError("recipe selected source bytes budget")
            contents.decode("utf-8")
            sha = hashlib.sha256(contents).hexdigest()
            blob = blobs / (sha + ".source-data")
            if not blob.exists():
                with blob.open("xb") as stream:
                    stream.write(contents)
            picked.append({**row, "sha256": sha, "evidenceFile": "blobs/" + blob.name})
    if not picked:
        raise ValueError("recipe package contained no reviewed source descriptors")
    return {"entries": rows, "unpackedBytes": total, "selectedFiles": picked, "selectedBytes": size}


def collect(source, output):
    if output.parent != source or not re.fullmatch(r"recipe-package-[0-9]{2}", output.name):
        raise ValueError("safe fresh task-owned recipe package path required")
    if output.exists() or output.is_symlink():
        raise FileExistsError("never overwrite recipe package evidence")
    sha = checked_inputs(source)
    output.mkdir()
    report = {"scope": "original recipe ZIP/digest and selected inert source inputs, not executed helper or NOTICE approval",
              "instanceReceiptSha256": sha, "package": RECIPE["PACKAGE"], "instanceVersion": RECIPE["VERSION"],
              "request": {"url": ENDPOINT, "body": url_request()}, "collectionComplete": False,
              "sourceFunctionsExecuted": False, "packageInstalled": False, "archiveExtracted": False,
              "actualGclientHelperRevisionVerified": False, "licenseCoverage": "not_proven", "promoted": False}
    try:
        contents = fetch_url()
        with (output / "url.public-data").open("xb") as stream:
            stream.write(contents)
        report["urlResponse"] = {"bytes": len(contents), "sha256": hashlib.sha256(contents).hexdigest()}
        url = parsed_url(contents)
        path = output / "recipe-package.zip-data"
        report["archive"] = {"evidenceFile": path.name, **download(url, path)}
        report["inspection"] = inspect_zip(path, output)
        report["collectionComplete"] = True
    except Exception as error:
        report["failureType"] = type(error).__name__
        raise
    finally:
        EXTERNAL["write_report"](output / "recipe-package-inventory.json", report)
    return report


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--output-name", required=True)
    args = parser.parse_args()
    source = args.source.absolute()
    result = collect(source, source / args.output_name)
    print(json.dumps({"collectionComplete": result["collectionComplete"], "zipBytes": result["archive"]["bytes"],
                      "entries": len(result["inspection"]["entries"]),
                      "selectedPaths": [row["path"] for row in result["inspection"]["selectedFiles"]], "promoted": False}))
