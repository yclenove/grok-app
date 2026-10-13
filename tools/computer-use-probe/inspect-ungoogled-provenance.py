"""Archive fixed public packaging/patch sources as data, never execute them.

Git blob identity is checked against the pinned repository tree. This is not
binary attestation, full Chromium source reproduction, SBOM or legal approval.
No credentials, latest ref, system browser, product lock or runtime mutation.
"""
import argparse
import concurrent.futures
import hashlib
import json
import re
import ssl
import stat
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path


HERE = Path(__file__).parent
SPECS = (
    ("ungoogled-chromium-windows", "989b4382364baaddb8b3180e1eb1030ca96a7273"),
    ("ungoogled-chromium-portablelinux", "025d99db1673a21992534551598b73ca79360762"),
    ("ungoogled-chromium", "ffeb45a2bb88bc56a1dfed31410aeb10e9efcd5a"),
)
MAX_BLOB = 2 * 1024**2
MAX_REPOSITORY = 16 * 1024**2
MAX_ENTRIES = 1024
SHA1 = re.compile(r"[0-9a-f]{40}\Z")


def fetch(url, limit):
    parsed = urllib.parse.urlsplit(url)
    if (parsed.scheme != "https" or parsed.hostname not in
            ("api.github.com", "raw.githubusercontent.com") or parsed.username or parsed.password):
        raise ValueError("unexpected source host")
    request = urllib.request.Request(url, headers={
        "User-Agent": "Grok-App-CU-fixed-source-audit",
        "Accept-Encoding": "identity",
    })
    # Only public, immutable read-only GETs retry; validation errors never do.
    for attempt in range(3):
        try:
            with urllib.request.urlopen(request, timeout=20) as response:
                if response.url != url or response.status != 200:
                    raise ValueError("source redirect or non-success response")
                contents = response.read(limit + 1)
            break
        except urllib.error.HTTPError as error:
            error.close()
            if error.code not in (500, 502, 503, 504) or attempt == 2:
                raise
        except (urllib.error.URLError, TimeoutError, ssl.SSLError):
            if attempt == 2:
                raise
    if len(contents) > limit:
        raise ValueError("source response budget exceeded")
    return contents


def tree_records(tree):
    if tree.get("truncated") is not False or not isinstance(tree.get("tree"), list):
        raise ValueError("complete repository tree required")
    entries = tree["tree"]
    if not 0 < len(entries) <= MAX_ENTRIES:
        raise ValueError("repository entry budget exceeded")
    seen, total = set(), 0
    for row in entries:
        name, mode, kind, sha = (row.get(key) for key in ("path", "mode", "type", "sha"))
        if (not isinstance(name, str) or len(name) > 1024 or "\\" in name or ":" in name
                or any(ord(char) < 32 for char in name)
                or any(part in ("", ".", "..") for part in name.split("/"))
                or name.casefold() in seen or not isinstance(sha, str) or not SHA1.fullmatch(sha)):
            raise ValueError("ambiguous repository path or object identity")
        seen.add(name.casefold())
        if (mode, kind) not in (("040000", "tree"), ("160000", "commit"),
                                ("100644", "blob"), ("100755", "blob")):
            raise ValueError("repository links or special objects rejected")
        if kind != "blob":
            continue
        size = row.get("size")
        if type(size) is not int or not 0 <= size <= MAX_BLOB:
            raise ValueError("repository file budget exceeded")
        total += size
        if total > MAX_REPOSITORY:
            raise ValueError("repository total byte budget exceeded")
    return entries


def blob_record(repo, commit, row):
    path = urllib.parse.quote(row["path"], safe="/")
    url = f"https://raw.githubusercontent.com/ungoogled-software/{repo}/{commit}/{path}"
    contents = fetch(url, row["size"])
    digest = hashlib.sha1(b"blob " + str(len(contents)).encode() + b"\0" + contents).hexdigest()
    if len(contents) != row["size"] or digest != row["sha"]:
        raise ValueError("pinned Git blob identity/length mismatch")
    return {"path": row["path"], "mode": row["mode"], "gitBlobSha1": digest,
            "sha256": hashlib.sha256(contents).hexdigest(), "bytes": len(contents),
            "url": url}, contents


def collect_repository(repo, commit):
    base = f"https://api.github.com/repos/ungoogled-software/{repo}/git/"
    metadata = json.loads(fetch(base + "commits/" + commit, MAX_BLOB))
    tree_sha = metadata.get("tree", {}).get("sha")
    if metadata.get("sha") != commit or not isinstance(tree_sha, str) or not SHA1.fullmatch(tree_sha):
        raise ValueError("fixed commit/tree identity mismatch")
    tree_bytes = fetch(base + "trees/" + tree_sha + "?recursive=1", MAX_BLOB)
    tree = json.loads(tree_bytes)
    if tree.get("sha") != tree_sha:
        raise ValueError("repository resolved tree mismatch")
    entries = tree_records(tree)
    blobs = [row for row in entries if row["type"] == "blob"]
    # All original download owners join before returning, including failures.
    with concurrent.futures.ThreadPoolExecutor(max_workers=4) as pool:
        futures = [pool.submit(blob_record, repo, commit, row) for row in blobs]
        try:
            data = [future.result() for future in futures]
        except BaseException:
            # Do not drain all queued requests after a terminal validation error.
            # The context still joins any already-running original GET owners.
            for future in futures:
                future.cancel()
            raise
    return {"repository": "ungoogled-software/" + repo, "commit": commit,
            "treeSha1": tree_sha, "treeResponseSha256": hashlib.sha256(tree_bytes).hexdigest(),
            "sourceFiles": len(data), "sourceBytes": sum(len(content) for _, content in data),
            "gitlinks": [row for row in entries if row["type"] == "commit"],
            "files": [record for record, _ in data]}, data, tree_bytes


def new_evidence_directory(path):
    absolute = path.absolute()
    root = (HERE / ".run").absolute()
    if absolute.parent != root or absolute.name in ("", ".", ".."):
        raise ValueError("evidence must be a new direct task .run child")
    for ancestor in (root, *root.parents):
        info = ancestor.lstat()
        if stat.S_ISLNK(info.st_mode) or getattr(info, "st_file_attributes", 0) & 0x400:
            raise ValueError("evidence parent links/reparse points rejected")
    absolute.mkdir()
    return absolute


def save_repository(output, index, data, tree_bytes):
    destination = output / str(index)
    destination.mkdir()
    with (destination / "tree-response.json").open("xb") as target:
        target.write(tree_bytes)
    for position, (item, contents) in enumerate(data):
        filename = f"{position:04d}-{item['sha256']}.source-data"
        # Repository filenames never become local paths or executable files.
        with (destination / filename).open("xb") as target:
            target.write(contents)
        item["evidence"] = f"{index}/{filename}"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    output = new_evidence_directory(args.output)
    report = {"scope": "fixed packaging and common patch sources only; not full binary inputs",
              "repositories": [], "sourceReproduction": "not_run",
              "binaryAttestation": "not_proven", "licenseCoverage": "not_proven",
              "securityAcceptance": "not_audited", "nativeAcceptance": "not_run",
              "executedSources": False, "promoted": False, "collectionComplete": False}
    try:
        for index, (repo, commit) in enumerate(SPECS):
            record, data, tree_bytes = collect_repository(repo, commit)
            save_repository(output, index, data, tree_bytes)
            report["repositories"].append(record)
            print(json.dumps({key: record[key] for key in
                              ("repository", "commit", "sourceFiles", "sourceBytes", "gitlinks")}))
        report["collectionComplete"] = True
    finally:
        with (output / "provenance-inventory.json").open("x", encoding="utf-8") as target:
            json.dump(report, target, indent=2)


if __name__ == "__main__":
    main()
