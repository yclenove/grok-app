"""Fixed Chromium Git tree and license-source data, never a product NOTICE.

No checkout, upstream Python/DEPS execution, source path materialization,
system browser, credentials, product pins or seed mutation. Gitlinks remain
explicit coverage gaps until their exact repositories are independently read.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import posixpath
import re
import stat
import subprocess
from urllib.parse import urlsplit


COMMIT = "f52796fc20c6e7d618374bb8bf096e7b2155e14c"
REMOTE = "https://github.com/chromium/chromium.git"
SHA1 = re.compile(r"[0-9a-f]{40}\Z")
MAX_TREE = 128 * 1024**2
MAX_BLOB = 4 * 1024**2
MAX_TOTAL = 128 * 1024**2
METADATA = {"README.chromium", "README.crashpad", "README.v8", "README.pdfium", "README.angle"}
SUPPORT = {"LICENSE", "DEPS", "tools/licenses/licenses.py", "tools/licenses/spdx_writer.py",
           "build/action_helpers.py", "build/find_depot_tools.py", "components/resources/BUILD.gn",
           "components/webui/about/resources/about_credits.tmpl",
           "components/webui/about/resources/about_credits_entry.tmpl",
           "components/webui/about/resources/about_credits_reciprocal.tmpl"}
HERE = Path(__file__).parent.absolute()


def regular_parents(path):
    for item in (path, *path.parents):
        info = item.lstat()
        if stat.S_ISLNK(info.st_mode) or getattr(info, "st_file_attributes", 0) & 0x400:
            raise ValueError("linked/reparse source/evidence path")


def tree_records(contents):
    if not contents or len(contents) > MAX_TREE or not contents.endswith(b"\0"):
        raise ValueError("complete bounded NUL Git tree required")
    rows, seen = [], set()
    for record in contents[:-1].split(b"\0"):
        header, path = record.split(b"\t", 1)
        mode, kind, oid = header.decode("ascii").split(" ")
        path = path.decode("utf-8")
        if (not SHA1.fullmatch(oid) or path in seen or "\x00" in path
                or path.startswith("/") or any(part in ("", ".", "..") for part in path.split("/"))):
            raise ValueError("ambiguous Git tree identity/path")
        if (mode, kind) not in (("100644", "blob"), ("100755", "blob"),
                                ("120000", "blob"), ("160000", "commit")):
            raise ValueError("unexpected Git tree mode")
        seen.add(path)
        rows.append({"path": path, "mode": mode, "kind": kind, "gitObjectSha1": oid})
        if len(rows) > 2_000_000:
            raise ValueError("Git tree entry budget exceeded")
    return rows


def selected(row):
    if row["mode"] not in ("100644", "100755"):
        return False
    name = PurePosixPath(row["path"]).name
    upper = name.upper()
    return (row["path"] in SUPPORT or name in METADATA
            or name == "additional_readme_paths.json" or name == "DEPS"
            or upper.startswith(("LICENSE", "LICENCE", "COPYING", "COPYRIGHT", "NOTICE"))
            or upper.endswith(("-LICENSE", "-LICENCE")))


def references(row, contents):
    if PurePosixPath(row["path"]).name not in METADATA:
        return []
    values = []
    for line in contents.decode("utf-8").splitlines():
        match = re.fullmatch(r"\s*License File:\s*(.*?)\s*", line, re.IGNORECASE)
        if not match:
            continue
        for value in match.group(1).split(","):
            value = value.strip()
            if not value or value == "NOT_SHIPPED":
                continue
            if value.startswith("https://"):
                url = urlsplit(value)
                if url.username or url.password or any(ord(char) < 32 for char in value):
                    raise ValueError("unsafe remote license reference")
                values.append({"metadata": row["path"], "value": value, "sourcePath": None,
                               "reason": "remote reference retained as data, not followed or treated as license text"})
                continue
            if "\\" in value or ":" in value or any(ord(char) < 32 for char in value):
                raise ValueError("unsupported metadata license reference")
            base = "" if value.startswith("/") else posixpath.dirname(row["path"])
            path = posixpath.normpath(posixpath.join(base, value.lstrip("/")))
            if path in (".", "..") or path.startswith(("../", "/")):
                raise ValueError("metadata reference escapes source root")
            values.append({"metadata": row["path"], "value": value, "sourcePath": path})
    return values


def check_blob(oid, contents):
    if len(contents) > MAX_BLOB or not SHA1.fullmatch(oid):
        raise ValueError("license blob identity/size budget")
    actual = hashlib.sha1(b"blob " + str(len(contents)).encode() + b"\0" + contents).hexdigest()
    if actual != oid:
        raise ValueError("license bytes differ from fixed Git blob identity")
    return {"bytes": len(contents), "sha256": hashlib.sha256(contents).hexdigest()}


def git(bare, args, limit=MAX_TREE, input_data=None):
    environment = {key: value for key, value in os.environ.items()
                   if not key.startswith("GIT_TRACE") and key != "GIT_CURL_VERBOSE"}
    environment.update(GIT_TERMINAL_PROMPT="0", GCM_INTERACTIVE="never")
    command = ["git", "--git-dir=" + str(bare), "-c", "protocol.version=2",
               "-c", "gc.auto=0", "-c", "maintenance.auto=false", *args]
    result = subprocess.run(command, input=input_data, capture_output=True, env=environment, check=False)
    if result.returncode:
        # Do not emit inherited credential/proxy configuration or raw stderr.
        raise RuntimeError(f"fixed-source git {args[0]} exited {result.returncode}")
    if len(result.stdout) > limit:
        raise ValueError("Git response budget exceeded")
    return result.stdout


def fetch_blobs(bare, oids, remote=REMOTE):
    local = git(bare, ["cat-file", "--batch-all-objects", "--unordered",
                       "--batch-check=%(objectname) %(objecttype)"])
    available = {line[:40].decode("ascii") for line in local.splitlines() if line[41:] == b"blob"}
    oids = [oid for oid in oids if oid not in available]
    for start in range(0, len(oids), 64):
        git(bare, ["fetch", "--no-tags", "--no-write-fetch-head", "--filter=blob:none",
                   remote, *oids[start:start + 64]])


def batch_payloads(contents, oids):
    offset, result = 0, {}
    for oid in oids:
        end = contents.find(b"\n", offset)
        if end < 0:
            raise ValueError("truncated Git batch header")
        header = contents[offset:end].decode("ascii").split(" ")
        if len(header) != 3 or header[:2] != [oid, "blob"] or not header[2].isdigit():
            raise ValueError("Git batch object identity/type mismatch")
        size = int(header[2])
        if size > MAX_BLOB or end + 1 + size >= len(contents):
            raise ValueError("Git batch size/truncation budget")
        payload = contents[end + 1:end + 1 + size]
        offset = end + 1 + size
        if contents[offset:offset + 1] != b"\n":
            raise ValueError("Git batch record terminator")
        offset += 1
        check_blob(oid, payload)
        result[oid] = payload
    if offset != len(contents):
        raise ValueError("unexpected trailing Git batch data")
    return result


def collect(bare, output):
    if (bare.name != "chromium-pinned-tree.git" or bare.parent.parent != HERE / ".run"
            or not re.fullmatch(r"maintained-notice-source-[0-9]{8}-[0-9]{2}", bare.parent.name)
            or output.parent != bare.parent or not re.fullmatch(r"license-tree-[0-9]{2}", output.name)):
        raise ValueError("exclusive task-owned pinned source/evidence paths required")
    if output.exists() or output.is_symlink():
        raise FileExistsError("source evidence must be new, never overwritten")
    for path in (bare, output.parent):
        regular_parents(path)
        if path.resolve(strict=True) != path or not path.is_dir():
            raise ValueError("canonical existing bare/evidence parent required")
    if git(bare, ["rev-parse", "--is-bare-repository"], 64) != b"true\n":
        raise ValueError("no source checkout is allowed")
    if git(bare, ["rev-parse", COMMIT], 64).decode().strip() != COMMIT:
        raise ValueError("fixed Chromium commit missing")
    tree = git(bare, ["rev-parse", COMMIT + "^{tree}"], 64).decode().strip()
    if not SHA1.fullmatch(tree):
        raise ValueError("fixed source tree identity")
    contents = git(bare, ["ls-tree", "-r", "-z", "--full-tree", COMMIT])
    rows = tree_records(contents)
    by_path = {row["path"]: row for row in rows}
    picked = [row for row in rows if selected(row)]
    if not 0 < len(picked) <= 12000 or not SUPPORT.issubset(by_path):
        raise ValueError("source support/selected entry budget")
    output.mkdir()
    data = output / "blobs"
    data.mkdir()
    report = {"scope": "fixed main-repository license-source inventory only, not NOTICE/SBOM/redistribution approval",
              "sourceCommit": COMMIT, "sourceTreeSha1": tree,
              "fullMainTreeEntries": len(rows), "fullMainTreeSha256": hashlib.sha256(contents).hexdigest(),
              "gitlinks": [row for row in rows if row["mode"] == "160000"],
              "selectedFiles": [], "licenseReferences": [], "missingReferences": [],
              "collectionComplete": False, "dependencyRepositoriesCollected": False,
              "generatorExecuted": False, "licenseCoverage": "not_proven", "promoted": False}
    (output / "main-tree.nul").write_bytes(contents)
    seen, total = {}, 0
    try:
        pending = picked
        for round_index in range(2):
            oids = sorted({row["gitObjectSha1"] for row in pending} - set(seen))
            fetch_blobs(bare, oids)
            for start in range(0, len(oids), 64):
                batch = oids[start:start + 64]
                contents = git(bare, ["cat-file", "--batch"], MAX_TOTAL,
                               ("\n".join(batch) + "\n").encode())
                for oid, contents in batch_payloads(contents, batch).items():
                    record = check_blob(oid, contents)
                    total += record["bytes"]
                    if total > MAX_TOTAL:
                        raise ValueError("license source total byte budget")
                    with (data / (oid + ".source-data")).open("xb") as stream:
                        stream.write(contents)
                    seen[oid] = record
            for row in pending:
                contents = (data / (row["gitObjectSha1"] + ".source-data")).read_bytes()
                report["selectedFiles"].append({**row, **seen[row["gitObjectSha1"]]})
                report["licenseReferences"].extend(references(row, contents))
            if round_index == 0:
                selected_paths = {row["path"] for row in picked}
                paths = {row["sourcePath"] for row in report["licenseReferences"] if row["sourcePath"] is not None}
                pending = [by_path[path] for path in sorted(paths - selected_paths)
                           if path in by_path and by_path[path]["mode"] in ("100644", "100755")]
        report["missingReferences"] = [row for row in report["licenseReferences"]
                                      if row["sourcePath"] not in by_path
                                      or by_path[row["sourcePath"]]["mode"] not in ("100644", "100755")]
        report.update(collectionComplete=True, uniqueBlobs=len(seen), sourceBytes=total)
    finally:
        with (output / "license-source-inventory.json").open("x", encoding="utf-8") as stream:
            json.dump(report, stream, indent=2)
            stream.write("\n")
    return report


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bare", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    result = collect(args.bare.absolute(), args.output.absolute())
    print(json.dumps({key: result[key] for key in ("sourceCommit", "sourceTreeSha1", "fullMainTreeEntries",
          "uniqueBlobs", "sourceBytes", "collectionComplete", "licenseCoverage", "promoted")}))
