"""Read one pinned maintained Chromium proposal, never install or execute.

This is intentionally NOT a product lock or runtime migration. A positive
embedded-credits finding does not certify codecs, source coverage, security,
Playwright compatibility, the publisher or the other three native targets.
"""
import argparse
import json
import runpy
import sys
import zipfile
from pathlib import Path


HERE = Path(__file__).parent
CREDITS = runpy.run_path(str(HERE / "inspect-chromium-credits.py"))
SOURCE = runpy.run_path(str(HERE / "inspect-maintained-chromium-sources.py"))
NATIVE = runpy.run_path(str(HERE / "inspect-maintained-node-sources.py"))
RELEASE = "155.0.8059.39-1.1"
ROOT = f"ungoogled-chromium_{RELEASE}_windows_x64"
SHA256 = "7ab1bac653c531ea06cd4f08a59120e129d53f73f2cf7a00ae52a43276849f11"
ARCHIVE_BYTES = 199985084
PACKAGING_COMMIT = "989b4382364baaddb8b3180e1eb1030ca96a7273"
PATCHSET_COMMIT = "ffeb45a2bb88bc56a1dfed31410aeb10e9efcd5a"


def inspect_directory(directory):
    path = directory / "Win_x64.zip"
    # Validate the byte identity BEFORE inspecting unrelated binary members.
    credits, payloads = CREDITS["inspect_pinned_archive"](path, "Win_x64", ROOT, SHA256)
    if path.stat().st_size != ARCHIVE_BYTES:
        raise ValueError("fixed publisher archive size mismatch")
    tree = SOURCE["inspect"](path, ROOT, "chrome.exe")
    if tree["archiveSha256"] != SHA256:
        raise ValueError("source changed during proposal inventory")
    headers = {}
    with zipfile.ZipFile(path) as archive:
        for name in ("chrome.exe", "chrome.dll", "chrome_elf.dll"):
            with archive.open(f"{ROOT}/{name}") as stream:
                headers[name] = NATIVE["native_header"](stream.read(65536))
            if headers[name]["formatArchitecture"] != "pe-x86_64":
                raise ValueError("proposal binary architecture mismatch")
    credits.update({"treeInventory": tree, "nativeHeaders": headers})
    full_candidate = any(row["documentKind"] == "full_candidate"
                         for row in credits["creditsCandidates"])
    report = {
        "scope": "pinned third-party source proposal only, not runtime or redistribution approval",
        "publisher": "ungoogled-software/ungoogled-chromium-windows",
        "release": RELEASE,
        "assetId": 631109002,
        "sourcePackagingCommit": PACKAGING_COMMIT,
        "sourcePatchsetCommit": PATCHSET_COMMIT,
        "browserVersion": "155.0.8059.39",
        "upstreamSourceCommit": "3ff7ac5a9224be9156d7f8703a06e22890aafd34",
        "sourceArchive": {
            "url": "https://storage.googleapis.com/chromium-browser-official/"
                   "chromium-155.0.8059.39-lite.tar.xz?generation=1791333474509881",
            "bytes": 2021197344,
            "sha256": "d450182ea38a6485febfdfdad6f3a3f1d5d3408cc368329b94502c629ef8ec7e",
            "downloaded": False,
            "reproduced": False,
        },
        "publisherBuildFlags": {
            "source": "https://raw.githubusercontent.com/ungoogled-software/"
                      "ungoogled-chromium-windows/" + PACKAGING_COMMIT + "/flags.windows.gn",
            "isOfficialBuild": True,
            "proprietaryCodecs": True,
            "ffmpegBranding": "Chrome",
            "enableSwiftshader": False,
        },
        "archives": [credits],
        "embeddedFullNoticesGate": ("candidate_only_not_certified" if full_candidate
                                    else "failed_missing_full_notices"),
        "redistributionGate": "not_proven_codec_and_exact_patched_source_review_required",
        "nativeAcceptance": "not_run",
        "securityAcceptance": "not_audited",
        "otherTargets": {target: "not_audited" for target in ("Linux_x64", "Mac_Arm", "Mac")},
        "executed": False,
        "promoted": False,
    }
    return report, [payloads]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", type=Path)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    report, payloads = inspect_directory(args.directory)
    if args.output:
        CREDITS["save_evidence"](args.output, report, payloads)
    for row in report["archives"]:
        row["packsInspected"] = len(row.pop("packs"))
    print(json.dumps(report, indent=2))
    # Zero ONLY means a full credits candidate was found, not approval to ship.
    return 0 if report["embeddedFullNoticesGate"] == "candidate_only_not_certified" else 2


if __name__ == "__main__":
    sys.exit(main())
