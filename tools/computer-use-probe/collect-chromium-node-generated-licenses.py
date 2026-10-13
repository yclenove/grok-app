"""Read the exact DEPS-pinned node_modules GCS generation without extraction.

Stream/hash a bounded immutable archive, inspect every raw tar entry and keep
selected license/version bytes as inert hash-named data. Never run npm/hooks,
follow archive links, replace original receipts or claim build/binary coverage.
"""
import argparse
import ast
from collections import Counter
import hashlib
import json
from pathlib import Path, PurePosixPath
import posixpath
import re
import runpy
import time
import urllib.request


HERE = Path(__file__).parent.absolute()
GENERATED = runpy.run_path(str(HERE / "collect-chromium-generated-input-sources.py"))
INPUTS, EXTERNAL = GENERATED["INPUTS"], GENERATED["EXTERNAL"]
TAR = runpy.run_path(str(HERE / "bounded_tar_gzip.py"))
SOURCE_SHA = "9295d0afbe743857cdcc892dc4937b8d86b0937d30f1cc7567c4ceabc880cc4d"
PREFIX = "third_party/node/node_modules"
OBJECT = {"object_name": "605b17899d6503df7f2ce96d5458976b36c5e456",
          "sha256sum": "f7d15a9ba38b018602496cde9b094c34c5876c75723d3a5497682fd2e71fcdf3",
          "size_bytes": 11492257, "generation": 1791258586019361, "output_file": "node_modules.tar.gz"}
DECLARATION = {"bucket": "chromium-nodejs", "dep_type": "gcs", "condition": "non_git_source", "objects": [OBJECT]}
BASE = "https://storage.googleapis.com/"
METADATA_URL = (BASE + "storage/v1/b/chromium-nodejs/o/" + OBJECT["object_name"]
                + "?generation=" + str(OBJECT["generation"]))
ARCHIVE_URL = BASE + "chromium-nodejs/" + OBJECT["object_name"] + "?generation=" + str(OBJECT["generation"])
MAX_METADATA, MAX_SELECTED_FILE, MAX_SELECTED_TOTAL = 64 * 1024, 1024**2, 32 * 1024**2
CHUNK = 65536
HELPER_SHA = "ac100091d3d3c02993cbcad11d13cd1e25ae8adb4346746bf85886dd96094913"


def gcs_layout(contents):
    if hashlib.sha256(contents).hexdigest() != HELPER_SHA:
        raise ValueError("original recipe-bundled gclient helper SHA256 required")
    tree = ast.parse(contents.decode("utf-8"))
    classes = [node for node in tree.body if isinstance(node, ast.ClassDef) and node.name == "GcsDependency"]
    if len(classes) != 1:
        raise ValueError("exact GcsDependency source class required")
    functions = {node.name: node for node in classes[0].body if isinstance(node, ast.FunctionDef)}
    output = functions.get("output_dir")
    extraction = functions.get("_MaybeExtractArtifact")
    expression = ast.parse('os.path.join(self.root.root_dir, self.name.split(":")[0])', mode="eval").body
    call = ast.parse("tar.extractall(path=self.output_dir, filter=TarFilter)", mode="eval").body
    if (output is None or extraction is None or len(output.body) != 1 or not isinstance(output.body[0], ast.Return)
            or ast.dump(output.body[0].value) != ast.dump(expression)):
        raise ValueError("exact declared checkout root output directory implementation required")
    matches = [node for node in ast.walk(extraction) if isinstance(node, ast.Call) and ast.dump(node) == ast.dump(call)]
    if len(matches) != 1:
        raise ValueError("exact tar extraction into declared checkout root required")
    return {"helperSha256": HELPER_SHA, "outputDirectoryLine": output.body[0].lineno,
            "tarExtractLine": matches[0].lineno, "archiveLayout": "contents_of_declared_checkout_root",
            "sourceImplementationVerified": True, "remoteRuntimeMeasured": False, "helperExecuted": False}


def checked_sources(source):
    INPUTS["checked_source"](source)
    inputs = INPUTS["load_inputs"](source)
    catalog_bytes = INPUTS["read_data"](source / EXTERNAL["CATALOG"], INPUTS["TREE"]["MAX_BLOB"])
    catalog = EXTERNAL["checked_catalog"](catalog_bytes)
    rows = [row for row in catalog["nonGitDependencies"] if row["path"] == PREFIX]
    if len(rows) != 1 or rows[0] != {"path": PREFIX, "kind": "gcs", "condition": "non_git_source", "declaration": DECLARATION}:
        raise ValueError("exact immutable original DEPS GCS declaration required")
    root = source / "generated-input-source-01"
    report, sha = INPUTS["receipt"](root / "generated-input-inventory.json", SOURCE_SHA)
    if (report.get("sourceCommit") != INPUTS["TREE"]["COMMIT"] or report.get("collectionComplete") is not True
            or report.get("mainReceiptSha256") != inputs["mainSha256"]
            or report.get("requests") != GENERATED["requests"]() or len(report.get("records", [])) != len(GENERATED["PINS"])):
        raise ValueError("complete sealed exact generated-input source receipt required")
    blobs = {}
    for request, row in zip(GENERATED["requests"](), report["records"]):
        oid = request["gitObjectSha1"]
        if (any(row.get(key) != value for key, value in request.items()) or row.get("verified") is not True
                or row.get("rawFile") != oid + ".gitiles-data" or row.get("sourceFile") != oid + ".source-data"):
            raise ValueError("exact source identities and inert source-data names required")
        raw = INPUTS["read_data"](root / row["rawFile"], GENERATED["MAX_RAW"])
        if (len(raw), hashlib.sha256(raw).hexdigest()) != (row["rawBytes"], row["rawSha256"]):
            raise ValueError("original generated-input raw bytes changed")
        decoded, identity = GENERATED["decode"](raw, oid)
        contents = INPUTS["read_data"](root / row["sourceFile"], GENERATED["MAX_SOURCE"])
        if decoded != contents or identity != {key: row[key] for key in ("bytes", "sha256")}:
            raise ValueError("original generated-input source bytes changed")
        blobs[row["path"]] = contents
    refs = [row for row in inputs["references"] if row["sourcePath"] and row["sourcePath"].startswith(PREFIX + "/")]
    helper = INPUTS["read_data"](source / "recipe-package-01/blobs" / (HELPER_SHA + ".source-data"), INPUTS["TREE"]["MAX_BLOB"])
    return refs, blobs, sha, gcs_layout(helper)


def opener():
    no_redirect = GENERATED["CONFIG"]["CHECKOUTS"]["LOGS"]["BUILDS"]["SEARCH"]["NoRedirect"]()
    return urllib.request.build_opener(no_redirect)


def metadata(contents):
    if not 0 < len(contents) <= MAX_METADATA:
        raise ValueError("bounded nonempty original GCS metadata required")
    row = json.loads(contents)
    expected = {"kind": "storage#object", "bucket": "chromium-nodejs", "name": OBJECT["object_name"],
                "generation": str(OBJECT["generation"]), "size": str(OBJECT["size_bytes"])}
    if not isinstance(row, dict) or any(type(row.get(key)) is not type(value) or row.get(key) != value for key, value in expected.items()):
        raise ValueError("exact DEPS-pinned GCS object generation/size required")
    return row


def fetch_metadata():
    request = urllib.request.Request(METADATA_URL, method="GET", headers={"Accept": "application/json"})
    with opener().open(request, timeout=20) as response:
        if response.geturl() != METADATA_URL or response.headers.get("Content-Type", "").split(";", 1)[0] != "application/json":
            raise ValueError("exact GCS metadata endpoint/type required")
        contents = response.read(MAX_METADATA + 1)
    if not 0 < len(contents) <= MAX_METADATA:
        raise ValueError("GCS metadata byte budget")
    return contents


def download(path):
    request = urllib.request.Request(ARCHIVE_URL, method="GET", headers={"Accept-Encoding": "identity"})
    digest, total, started = hashlib.sha256(), 0, time.monotonic()
    with opener().open(request, timeout=20) as response, path.open("xb") as output:
        if response.geturl() != ARCHIVE_URL or response.headers.get("Content-Encoding") not in (None, "identity"):
            raise ValueError("exact undecoded GCS archive endpoint required")
        if response.headers.get("x-goog-generation") != str(OBJECT["generation"]):
            raise ValueError("original archive generation header required")
        while True:
            if time.monotonic() - started > 60:
                raise TimeoutError("bounded GCS archive transfer duration exceeded")
            chunk = response.read(CHUNK)
            if not chunk:
                break
            total += len(chunk)
            if total > OBJECT["size_bytes"]:
                raise ValueError("archive exceeds exact declared size")
            output.write(chunk)
            digest.update(chunk)
    if (total, digest.hexdigest()) != (OBJECT["size_bytes"], OBJECT["sha256sum"]):
        raise ValueError("archive differs from exact original DEPS size/SHA256")
    return {"bytes": total, "sha256": digest.hexdigest(), "generation": str(OBJECT["generation"])}


def archive_path(name):
    if name == ".":
        return name
    if name.startswith("./"):
        name = name[2:]
    if (not name or len(name) > 2048 or "\\" in name or ":" in name or any(ord(char) < 32 for char in name)
            or any(part in ("", ".", "..") for part in name.split("/"))):
        raise ValueError("canonical bounded node archive path required")
    return name


def archive_identity(path):
    INPUTS["TREE"]["regular_parents"](path)
    if not path.is_file() or path.stat().st_size != OBJECT["size_bytes"]:
        raise ValueError("bounded exact-size original archive required")
    digest = hashlib.sha256()
    total = 0
    with path.open("rb") as stream:
        while True:
            chunk = stream.read(CHUNK)
            if not chunk:
                break
            total += len(chunk)
            if total > OBJECT["size_bytes"]:
                raise ValueError("original archive changed beyond byte budget")
            digest.update(chunk)
    if (total, digest.hexdigest()) != (OBJECT["size_bytes"], OBJECT["sha256sum"]):
        raise ValueError("original archive size/SHA256 changed")
    return {"bytes": total, "sha256": digest.hexdigest(), "generation": str(OBJECT["generation"])}


def cached_archive(source, name):
    if not re.fullmatch(r"node-generated-license-[0-9]{2}", name):
        raise ValueError("safe existing task-owned archive generation required")
    root = source / name
    report, _ = INPUTS["receipt"](root / "node-generated-inventory.json")
    if (report.get("sourceCommit") != INPUTS["TREE"]["COMMIT"] or report.get("declaration") != DECLARATION
            or report.get("archive", {}).get("file") != "node_modules.tar.gz-data"):
        raise ValueError("original same-declaration archive receipt required")
    raw = INPUTS["read_data"](root / "object-metadata.json-data", MAX_METADATA)
    if (len(raw), hashlib.sha256(raw).hexdigest()) != (report["objectMetadata"]["bytes"], report["objectMetadata"]["sha256"]):
        raise ValueError("original GCS metadata bytes changed")
    metadata(raw)
    path = root / "node_modules.tar.gz-data"
    return raw, path, archive_identity(path)


def link_target(name, target, symlink):
    if (not target or len(target) > 2048 or target.startswith("/") or "\\" in target or ":" in target
            or any(ord(char) < 32 for char in target)):
        raise ValueError("bounded relative inert archive link required")
    resolved = posixpath.normpath(posixpath.join(posixpath.dirname(name) if symlink else "", target))
    if resolved != "node_modules" and not resolved.startswith("node_modules/"):
        raise ValueError("inert archive link escapes node_modules root")
    return resolved


def selected_path(path, requested):
    name = PurePosixPath(path).name.upper()
    return (path in requested or name.startswith(("LICENSE", "LICENCE", "COPYING", "COPYRIGHT", "NOTICE"))
            or name.endswith(("-LICENSE", "-LICENCE"))
            or path in {"node_modules/.package-lock.json", "node_modules/lit/package.json", "node_modules/@azure/msal-browser/package.json"})


def inspect_archive(path, output, references, require_existing=False):
    requested = {"node_modules/" + row["sourcePath"][len(PREFIX) + 1:] for row in references}
    entries, selected, seen, folded, parent_dirs = [], [], {}, set(), set()
    total_selected, total_files = 0, 0
    with path.open("rb") as stream:
        for entry, reader in TAR["iter_tar_gzip"](stream):
            relative = archive_path(entry.name)
            name = "node_modules" if relative == "." else "node_modules/" + relative
            kind = "directory" if entry.isdir() else "symlink" if entry.issym() else "hardlink" if entry.islnk() else "file"
            if name in seen or name.casefold() in folded or (name in parent_dirs and kind != "directory"):
                raise ValueError("unique case-portable archive paths without parent type conflicts required")
            for parent in PurePosixPath(name).parents:
                parent = str(parent)
                if parent != ".":
                    if parent in seen and seen[parent] != "directory":
                        raise ValueError("archive file/link cannot own descendant source paths")
                    parent_dirs.add(parent)
            seen[name] = kind
            folded.add(name.casefold())
            row = {"archiveName": entry.name, "path": name, "kind": kind, "bytes": entry.size,
                   "linkName": entry.linkname if kind in ("symlink", "hardlink") else None}
            if kind in ("symlink", "hardlink"):
                target = entry.linkname if entry.issym() else "node_modules/" + archive_path(entry.linkname)
                row["resolvedLinkPath"] = link_target(name, target, entry.issym())
            keep = selected_path(name, requested)
            if keep and kind != "file":
                raise ValueError("selected license/version source must be a regular archive file")
            if keep and entry.size > MAX_SELECTED_FILE:
                raise ValueError("selected source file byte budget")
            pieces, digest = [], hashlib.sha256()
            while True:
                chunk = reader.read(CHUNK)
                if not chunk:
                    break
                digest.update(chunk)
                if keep:
                    pieces.append(chunk)
            if kind == "file":
                total_files += entry.size
                row["sha256"] = digest.hexdigest()
            if keep:
                contents = b"".join(pieces)
                contents.decode("utf-8", errors="strict")
                total_selected += len(contents)
                if total_selected > MAX_SELECTED_TOTAL:
                    raise ValueError("total selected source byte budget")
                filename = digest.hexdigest() + ".source-data"
                destination = output / filename
                if destination.exists():
                    if INPUTS["read_data"](destination, MAX_SELECTED_FILE) != contents:
                        raise ValueError("hash-named selected payload collision")
                elif require_existing:
                    raise ValueError("original selected archive source bytes absent")
                else:
                    with destination.open("xb") as target:
                        target.write(contents)
                selected.append({"path": name, "sourcePath": "third_party/node/" + name,
                                 "sourceFile": filename, "bytes": len(contents), "sha256": digest.hexdigest()})
            entries.append(row)
    missing = sorted(requested - {row["path"] for row in selected})
    return {"entries": entries, "entryKinds": dict(sorted(Counter(row["kind"] for row in entries).items())),
            "fileBytes": total_files, "selectedFiles": selected, "selectedBytesByPath": total_selected,
            "requestedLicensePaths": sorted(requested), "missingRequestedPaths": missing,
            "linksFollowed": False, "extracted": False}


def package_comparison(report, root, source_blobs):
    lock = json.loads(source_blobs["third_party/node/package-lock.json"])
    if lock.get("lockfileVersion") != 2 or not isinstance(lock.get("packages"), dict):
        raise ValueError("original package-lock v2 packages required")
    available = {row["path"]: row for row in report["selectedFiles"]}
    result = []
    for key in ("lit", "@azure/msal-browser"):
        path = "node_modules/" + key
        row = available.get(path + "/package.json")
        if row is None:
            raise ValueError("actual archive package version metadata required")
        contents = INPUTS["read_data"](root / row["sourceFile"], MAX_SELECTED_FILE)
        package = json.loads(contents)
        locked = lock["packages"].get(path)
        if not isinstance(locked, dict) or not isinstance(package, dict) or package.get("name") != key:
            raise ValueError("exact package name and original lock record required")
        result.append({"package": key, "archiveVersion": package.get("version"), "lockVersion": locked.get("version"),
                       "versionMatchesLock": package.get("version") == locked.get("version"),
                       "archiveLicenseField": package.get("license"), "lockLicenseField": locked.get("license"),
                       "originalReadmeVersionOverridden": False})
    return result


def collect(source, output, cache=None):
    if output.parent != source or not re.fullmatch(r"node-generated-license-[0-9]{2}", output.name):
        raise ValueError("safe fresh generated-node source directory required")
    if output.exists() or output.is_symlink():
        raise FileExistsError("never overwrite generated-node source evidence")
    refs, blobs, source_sha, layout = checked_sources(source)
    cached = cached_archive(source, cache) if cache is not None else None
    output.mkdir()
    report = {"scope": "fixed generated node source input only, not actual builder consumption/binary NOTICE approval",
              "sourceCommit": INPUTS["TREE"]["COMMIT"], "parentCatalogSha256": EXTERNAL["CATALOG_SHA"],
              "generatedSourceReceiptSha256": source_sha, "declaration": DECLARATION,
              "layoutEvidence": layout, "archiveCache": cache,
              "metadataUrl": METADATA_URL, "archiveUrl": ARCHIVE_URL, "collectionComplete": False,
              "conditionsEvaluated": False, "sourceFunctionsExecuted": False, "packageManagerExecuted": False,
              "actualBuilderInputMeasured": False, "binaryComponentCoverage": "not_proven", "licenseCoverage": "not_proven", "promoted": False}
    try:
        raw = cached[0] if cached else fetch_metadata()
        with (output / "object-metadata.json-data").open("xb") as stream:
            stream.write(raw)
        report["objectMetadata"] = {"bytes": len(raw), "sha256": hashlib.sha256(raw).hexdigest(), "value": metadata(raw)}
        archive = cached[1] if cached else output / "node_modules.tar.gz-data"
        report["archive"] = {"file": archive.name, "sourceRelativeFile": archive.relative_to(source).as_posix(),
                             **(cached[2] if cached else download(archive))}
        print(json.dumps({"archiveVerified": True, **report["archive"]}), flush=True)
        report["inspection"] = inspect_archive(archive, output, refs)
        report["packageComparison"] = package_comparison(report["inspection"], output, blobs)
        report["collectionComplete"] = True
    except Exception as error:
        report["failureType"] = type(error).__name__
        raise
    finally:
        EXTERNAL["write_report"](output / "node-generated-inventory.json", report)
    return report


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--output-name", required=True)
    parser.add_argument("--archive-cache")
    args = parser.parse_args()
    source = args.source.absolute()
    report = collect(source, source / args.output_name, args.archive_cache)
    print(json.dumps({"collectionComplete": report["collectionComplete"], "kinds": report["inspection"]["entryKinds"],
                      "selectedPaths": len(report["inspection"]["selectedFiles"]),
                      "missingRequestedPaths": report["inspection"]["missingRequestedPaths"],
                      "packages": report["packageComparison"], "promoted": False}))
