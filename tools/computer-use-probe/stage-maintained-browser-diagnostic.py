"""Stage an exclusive INTERNAL native compatibility diagnostic, not a CU pack.

Fixed Node22/PW1.64 and explicit Chromium157/CfT156 candidates only.
No system browser, product locks, seed, manifest, auth, model or user App data.
Full source checks precede writes; failure evidence is retained, never replaced.
"""
import argparse
import hashlib
import json
import runpy
import shutil
import stat
import tarfile
import zipfile
from pathlib import Path


HERE = Path(__file__).parent
REPO = HERE.parents[1]
NODE = runpy.run_path(str(HERE / "inspect-maintained-node-sources.py"))
PW = runpy.run_path(str(HERE / "inspect-maintained-playwright-sources.py"))
BROWSER = runpy.run_path(str(HERE / "inspect-unbranded-chromium-sources.py"))
CFT = runpy.run_path(str(HERE / "inspect-maintained-chromium-sources.py"))
TARGETS = {
    "windows": ("win-x64", "zip", "node.exe", "pe-x86_64", "Win_x64",
                "2b0ff57b049cda1bbcea2240eec20467018713c1efe1f7360c2681859b90ed71",
                "3bdd57de257651043b160c25ba4aa78a7e0f04738bf0d96a2cd4c2c23c79be2a"),
    "linux": ("linux-x64", "tar.gz", "bin/node", "elf-x86_64", "Linux_x64",
              "1084aa36196bba4c3a5e69a1ee388a6e4ff729dad09445fbcd434b28fe3c24af",
              "e3d9fbfb1866494e3515f7885d67ccddebd88d1970f110a6809204fd190609dd"),
    "linux-arm64": ("linux-arm64", "tar.gz", "bin/node", "elf-arm64", "chrome-linux-arm64",
                    "5ced2d48d1d7198739b7f86804de0171aefb6823b684b12341d3321afc3cb0b2",
                    "910a12fff6170202195f72ef3d353801c7063d301ab80b1f51b77d791fda60ce"),
}


def safe_parts(name):
    parts = name.split("/")
    reserved = {"CON", "PRN", "AUX", "NUL", *[f"COM{number}" for number in range(1, 10)],
                *[f"LPT{number}" for number in range(1, 10)]}
    if (not name or len(name) > 1024 or "\\" in name or ":" in name
            or any(ord(char) < 32 for char in name)
            or any(part in ("", ".", "..") or part.endswith((".", " "))
                   or part.split(".", 1)[0].upper() in reserved for part in parts)):
        raise ValueError("unsafe diagnostic materialization path")
    return parts


def regular_parents(path):
    for ancestor in (path, *path.parents):
        info = ancestor.lstat()
        if stat.S_ISLNK(info.st_mode) or getattr(info, "st_file_attributes", 0) & 0x400:
            raise ValueError("diagnostic paths contain links/reparse points")


def exclusive_output(path):
    path, root = path.absolute(), (HERE / ".run").absolute()
    if path.parent != root or path.name in ("", ".", ".."):
        raise ValueError("new diagnostic must be a direct task .run child")
    regular_parents(root)
    if shutil.disk_usage(root).free < 3 * 1024**3:
        raise ValueError("diagnostic staging requires at least 3GiB free")
    path.mkdir()
    return path


def copy_payload(source, path, expected_bytes, executable=False):
    if not 0 <= expected_bytes <= 600 * 1024**2:
        raise ValueError("diagnostic payload budget exceeded")
    path.parent.mkdir(parents=True, exist_ok=True)
    regular_parents(path.parent)
    actual, digest = 0, hashlib.sha256()
    with path.open("xb") as target:
        while True:
            chunk = source.read(65536)
            if not chunk:
                break
            actual += len(chunk)
            if actual > expected_bytes:
                raise ValueError("diagnostic payload exceeds declared size")
            digest.update(chunk)
            target.write(chunk)
    if actual != expected_bytes:
        raise ValueError("diagnostic payload is truncated")
    path.chmod(0o755 if executable else 0o644)
    return {"bytes": actual, "sha256": digest.hexdigest()}


def copy_tar(path, root, destination, selected=None):
    seen, count, total, rows = set(), 0, 0, {}
    with tarfile.open(path, "r|gz") as archive:
        for entry in archive:
            parts = safe_parts(entry.name.rstrip("/"))
            if parts[0] != root or entry.name.casefold() in seen:
                raise ValueError("unexpected/duplicate diagnostic tar path")
            seen.add(entry.name.casefold())
            count += 1
            if count > 10000 or entry.size < 0 or entry.size > 128 * 1024**2:
                raise ValueError("diagnostic tar entry budget exceeded")
            if entry.isdir():
                if entry.size:
                    raise ValueError("diagnostic tar directory has payload")
                continue
            # Node's npm links are validated but deliberately never materialized.
            if selected is not None and entry.issym():
                if "/".join(parts[1:]) in selected:
                    raise ValueError("selected diagnostic file is a link")
                continue
            if not entry.isfile() or len(parts) < 2:
                raise ValueError("diagnostic tar special objects are forbidden")
            total += entry.size
            if total > 512 * 1024**2:
                raise ValueError("diagnostic tar total budget exceeded")
            relative = "/".join(parts[1:])
            if selected is not None and relative not in selected:
                continue
            with archive.extractfile(entry) as source:
                rows[relative] = copy_payload(source, destination.joinpath(*parts[1:]), entry.size,
                                              bool(entry.mode & 0o111))
    if selected is not None and set(rows) != selected:
        raise ValueError("diagnostic selected files missing")
    return rows


def copy_zip(path, root, destination, selected=None):
    seen, total, rows = set(), 0, {}
    with zipfile.ZipFile(path) as archive:
        entries = archive.infolist()
        if len(entries) > 10000:
            raise ValueError("diagnostic ZIP entry budget exceeded")
        for entry in entries:
            parts = safe_parts(entry.filename.rstrip("/"))
            if (entry.orig_filename != entry.filename or parts[0] != root
                    or entry.filename.casefold() in seen or entry.flag_bits & 1
                    or entry.compress_type not in (0, 8)):
                raise ValueError("unexpected/ambiguous diagnostic ZIP entry")
            seen.add(entry.filename.casefold())
            kind = stat.S_IFMT(entry.external_attr >> 16)
            if entry.is_dir():
                if entry.file_size or kind not in (0, stat.S_IFDIR):
                    raise ValueError("diagnostic ZIP directory mode/payload mismatch")
                continue
            if kind not in (0, stat.S_IFREG) or len(parts) < 2:
                raise ValueError("diagnostic ZIP links/special objects rejected")
            total += entry.file_size
            if total > 1024 * 1024**2:
                raise ValueError("diagnostic ZIP tree budget exceeded")
            relative = "/".join(parts[1:])
            if selected is not None and relative not in selected:
                continue
            with archive.open(entry) as source:
                rows[relative] = copy_payload(source, destination.joinpath(*parts[1:]), entry.file_size,
                                              bool(entry.external_attr >> 16 & 0o111))
    if selected is not None and set(rows) != selected:
        raise ValueError("diagnostic selected files missing")
    return rows


def stage(target, output):
    arch, suffix, node_exe, native_arch, platform, node_sha, browser_sha = TARGETS[target]
    node_root = "node-v22.23.3-" + arch
    node_path = REPO / ".cache/computer-use/platform-source-20261012-node22" / f"{node_root}.{suffix}"
    pw_root = REPO / ".cache/computer-use/platform-source-20261012-maintenance"
    browser_root = REPO / ".cache/computer-use/platform-source-20261012-chromium-snapshot"
    node_report = NODE["inspect"](node_path, node_root, node_exe, native_arch, node_sha)
    pw_report = PW["inspect"](pw_root)
    if target == "linux-arm64":
        browser_path = pw_root / "chrome-linux-arm64.zip"
        browser_report = CFT["inspect"](browser_path, platform, "chrome")
        if browser_report["archiveBytes"] != 199750508:
            raise ValueError("fixed ARM64 browser source size mismatch")
    else:
        metadata_path = browser_root / "source-metadata.json"
        NODE["regular_source"](metadata_path)
        metadata = json.loads(metadata_path.read_text(encoding="utf-8"))
        source = next(row for row in metadata["archives"] if row["platform"] == platform)
        browser_path = browser_root / (platform + ".zip")
        browser_report = BROWSER["inspect_archive"](browser_path, platform, source)
    if browser_report["archiveSha256"] != browser_sha:
        raise ValueError("fixed diagnostic browser source SHA mismatch")
    output = exclusive_output(output)
    report = {"scope": "INTERNAL native compatibility diagnostic only, not a runtime pack",
              "target": target, "sources": [node_report, pw_report, browser_report],
              "runtimePrepared": False, "promoted": False, "nativeAcceptance": "not_run",
              "redistributionGate": ("not_proven_chrome_terms_and_notices" if target == "linux-arm64"
                                     else "failed_missing_full_notices"), "stagingComplete": False}
    try:
        reader = copy_zip if suffix == "zip" else copy_tar
        node_rows = reader(node_path, node_root, output / "node", {node_exe, "LICENSE"})
        for name, expected in ((node_exe, node_report["executable"]), ("LICENSE", node_report["license"])):
            if any(node_rows[name][key] != expected[key] for key in ("bytes", "sha256")):
                raise ValueError("diagnostic Node bytes changed after source audit")
        pw_rows = copy_tar(pw_root / "playwright-core-1.64.0.tgz", "package",
                           output / "worker/node_modules/playwright-core")
        pw_listing = "".join(f"{name}\t{row['bytes']}\t{row['sha256']}\n" for name, row in sorted(pw_rows.items()))
        if hashlib.sha256(pw_listing.encode()).hexdigest() != pw_report["treeSha256"]:
            raise ValueError("diagnostic Playwright tree changed after source audit")
        browser_rows = copy_zip(browser_path, browser_report["archiveRoot"], output / "browser")
        listing = "".join(f"{name}\t{row['bytes']}\t{row['sha256']}\n" for name, row in sorted(browser_rows.items()))
        if hashlib.sha256(listing.encode()).hexdigest() != browser_report["treeSha256"]:
            raise ValueError("diagnostic browser tree changed after source audit")
        # Freeze current domain worker/tests, excluding installed old dependencies.
        worker = REPO / "tools/computer-use-browser"
        worker_rows = {}
        for path in sorted(worker.rglob("*")):
            relative = path.relative_to(worker)
            if any(part in ("node_modules", ".run", ".cache", ".git") for part in relative.parts):
                continue
            regular_parents(path)
            if not path.is_file() or path.suffix not in (".mjs", ".html", ".json"):
                continue
            safe_parts(relative.as_posix())
            with path.open("rb") as source_file:
                worker_rows[relative.as_posix()] = copy_payload(
                    source_file, output / "worker" / relative, path.stat().st_size)
        report.update({"stagingComplete": True, "workerFiles": worker_rows,
                       "nodeExecutable": "node/" + node_exe,
                       "browserExecutable": "browser/" + browser_report["executableRelpath"]})
    finally:
        with (output / "diagnostic-inputs.json").open("x", encoding="utf-8") as evidence:
            json.dump(report, evidence, indent=2)
    print(json.dumps({key: report[key] for key in ("scope", "target", "stagingComplete",
                     "runtimePrepared", "promoted", "nativeAcceptance", "redistributionGate")}))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--target", choices=tuple(TARGETS), required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    stage(args.target, args.output)
