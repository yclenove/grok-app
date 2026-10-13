"""Read official unbranded Chromium snapshot archives; never extract or execute.

This is a source candidate audit, not runtime/redistribution/install approval.
The four archives are pinned to one upstream source commit, not per-OS latest.
"""
import base64
import hashlib
import json
import plistlib
import runpy
import sys
import zipfile
from pathlib import Path


POSITION = "1716663"
COMMIT = "f52796fc20c6e7d618374bb8bf096e7b2155e14c"
VERSION = "157.0.8101.0"
SNAPSHOTS = {
    "Win_x64": ("chrome-win", "chrome.exe", "pe-x86_64"),
    "Linux_x64": ("chrome-linux", "chrome", "elf-x86_64"),
    "Mac_Arm": ("chrome-mac", "Chromium.app/Contents/MacOS/Chromium", "macho-arm64"),
    "Mac": ("chrome-mac", "Chromium.app/Contents/MacOS/Chromium", "macho-x86_64"),
}
HERE = Path(__file__).parent
ZIP_INSPECT = runpy.run_path(str(HERE / "inspect-maintained-chromium-sources.py"))["inspect_tree"]
NATIVE = runpy.run_path(str(HERE / "inspect-maintained-node-sources.py"))


def inspect_archive(path, platform, source):
    NATIVE["regular_source"](path)
    root, executable, architecture = SNAPSHOTS[platform]
    metadata = source["archive"]
    if (source["sourceCommit"] != COMMIT or source["sourcePosition"] != POSITION
            or source["platform"] != platform
            or metadata["name"] != f"{platform}/{POSITION}/{root}.zip"
            or not str(metadata["generation"]).isdigit()):
        raise ValueError("official snapshot identity mismatch")
    if path.stat().st_size != int(metadata["size"]):
        raise ValueError("official snapshot size mismatch")
    digest = hashlib.md5()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    if base64.b64encode(digest.digest()).decode() != metadata["md5Hash"]:
        raise ValueError("official GCS object checksum mismatch")
    result = ZIP_INSPECT(path, root, executable)
    with zipfile.ZipFile(path) as archive:
        with archive.open(f"{root}/{executable}") as stream:
            headers = {executable: NATIVE["native_header"](stream.read(65536))}
        if headers[executable]["formatArchitecture"] != architecture:
            raise ValueError("browser architecture does not match snapshot target")
        if platform in ("Mac_Arm", "Mac"):
            contents = "Chromium.app/Contents/"
            info = plistlib.loads(archive.read(f"{root}/{contents}Info.plist"))
            if info.get("CFBundleShortVersionString") != VERSION:
                raise ValueError("browser plist version does not match pinned source")
            framework = (contents + "Frameworks/Chromium Framework.framework/Versions/"
                         + VERSION + "/Chromium Framework")
            with archive.open(f"{root}/{framework}") as stream:
                headers[framework] = NATIVE["native_header"](stream.read(65536))
            if headers[framework]["formatArchitecture"] != architecture:
                raise ValueError("framework architecture does not match snapshot target")
            result["macosMinimumSystemMetadata"] = {
                "plistMinimumSystemVersion": info.get("LSMinimumSystemVersion"),
                "nativeHeaderRequirements": headers,
                "scope": "metadata only, not native macOS runtime acceptance",
            }
    result.update({"platform": platform, "sourcePosition": POSITION,
                   "sourceCommit": COMMIT, "browserVersion": VERSION,
                   "officialObjectGeneration": metadata["generation"],
                   "officialObjectMd5Matched": True, "nativeHeaders": headers,
                   "executed": False, "promoted": False})
    return result


def inspect_directory(directory):
    metadata_path = directory / "source-metadata.json"
    NATIVE["regular_source"](metadata_path)
    metadata = json.loads(metadata_path.read_text(encoding="utf-8"))
    sources = metadata["archives"]
    if (metadata["sourcePosition"] != POSITION or metadata["sourceCommit"] != COMMIT
            or len(sources) != len(SNAPSHOTS)
            or {source["platform"] for source in sources} != set(SNAPSHOTS)):
        raise ValueError("all four same-source official snapshots are required")
    return {"scope": "source inventory only, not runtime/install/redistribution acceptance",
            "sourcePosition": POSITION, "sourceCommit": COMMIT, "browserVersion": VERSION,
            "channel": "upstream development snapshot; stability not certified",
            "redistributionApproval": "not_proven_complete_third_party_notices_required",
            "archives": [inspect_archive(directory / (source["platform"] + ".zip"),
                                         source["platform"], source) for source in sources]}


if __name__ == "__main__":
    print(json.dumps(inspect_directory(Path(sys.argv[1]).resolve(strict=True)), indent=2))
