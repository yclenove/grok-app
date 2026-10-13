"""Collect selected public DEPS license inputs at exact Git pins, data only.

Not a full checkout, platform condition resolver, NOTICE generator or binary
SBOM. No upstream code/hooks/DEPS execution, arbitrary remotes, checkout-path
materialization, credentials export, product pin/seed or user-home mutation.
"""
import argparse
import hashlib
import json
from pathlib import Path
import re
import runpy


HERE = Path(__file__).parent.absolute()
TREE = runpy.run_path(str(HERE / "inspect-chromium-license-tree.py"))
CATALOG = "deps-pin-inventory-04.json"
CATALOG_SHA = "6d0e36265008ac4e5d4dd0c3569b5421a14fc16d3b82c10af7cd3608a356ed4e"
MAIN_RECEIPT_SHA = "73f557ce384aa699d6748c0f72a8ca3a91f851360922210fc79c35cb4d745a51"
PINS = {
    "angle": ("third_party/angle", "https://chromium.googlesource.com/angle/angle.git",
              "9649fea17b77e672ae19687bf0222109b6270e0d"),
    "boringssl": ("third_party/boringssl/src", "https://boringssl.googlesource.com/boringssl.git",
                 "5b83e5d1abf0ae0033ae52ab579d906b67633b2c"),
    "ffmpeg": ("third_party/ffmpeg", "https://chromium.googlesource.com/chromium/third_party/ffmpeg.git",
               "5c85ff666b3434d9323b83437f910ad8431ab7c9"),
    "freetype": ("third_party/freetype/src", "https://chromium.googlesource.com/chromium/src/third_party/freetype2.git",
                 "79d80cadfc3d992e307c15a3f7d3a2a1555235a5"),
    "harfbuzz": ("third_party/harfbuzz/src", "https://chromium.googlesource.com/external/github.com/harfbuzz/harfbuzz.git",
                 "12ee77ee00514c442bcd28f9aa9d56b708c37382"),
    "icu": ("third_party/icu", "https://chromium.googlesource.com/chromium/deps/icu.git",
            "19dfb44a62ae60ce6b0060de8c0cef5d5c527177"),
    "pdfium": ("third_party/pdfium", "https://pdfium.googlesource.com/pdfium.git",
               "f51279699cb59754d1f49aef10eb8e825fe56532"),
    "skia": ("third_party/skia", "https://skia.googlesource.com/skia.git",
             "c88b49d1d6c7b27b1b611f78e1abf924383e801b"),
    "swiftshader": ("third_party/swiftshader", "https://swiftshader.googlesource.com/SwiftShader.git",
                    "0f87bb2d3742c2992fd1a7dcb2f051eebf79299c"),
    "webrtc": ("third_party/webrtc", "https://webrtc.googlesource.com/src.git",
               "64e8deaa40d81260ea56864ae21c91909ca37301"),
    "v8": ("v8", "https://chromium.googlesource.com/v8/v8.git",
           "f428bb00301e51385e63d1baed30ae006afb71e1"),
    "depot-tools": ("third_party/depot_tools", "https://chromium.googlesource.com/chromium/tools/depot_tools.git",
                    "21ae2d24f8ee0ca3459bda416129d7791c4313f1"),
}
MEDIA_PINS = {
    "dav1d": ("third_party/dav1d/libdav1d", "https://chromium.googlesource.com/external/github.com/videolan/dav1d.git",
              "99f3354ed0b9b4bf3c7c9347318dbcc522c94af5"),
    "libaom": ("third_party/libaom/source/libaom", "https://aomedia.googlesource.com/aom.git",
               "8fbd65b2f354e1d7059f8b1899f3f0373d1b3e2a"),
    "libjpeg-turbo": ("third_party/libjpeg_turbo", "https://chromium.googlesource.com/chromium/deps/libjpeg_turbo.git",
                     "640f254ad0fa03f6b1f29f89b7dd9366f2f6e533"),
    "libvpx": ("third_party/libvpx/source/libvpx", "https://chromium.googlesource.com/webm/libvpx.git",
               "0a6f769e44989222d99ded46daa1e9763637cc19"),
    "libwebm": ("third_party/libwebm/source", "https://chromium.googlesource.com/webm/libwebm.git",
                "6184f4484a826724b5293837134ab9492261b941"),
    "libwebp": ("third_party/libwebp/src", "https://chromium.googlesource.com/webm/libwebp.git",
                "b43b2caa710c0c997c066cb32c7fea1391fad70a"),
    "libyuv": ("third_party/libyuv", "https://chromium.googlesource.com/libyuv/libyuv.git",
               "eeb06a6451ba2e21f8ff272c7b42fc62b9faecc5"),
}
RUNTIME_PINS = {
    "quiche": ("net/third_party/quiche/src", "https://quiche.googlesource.com/quiche.git",
               "541aa2a7889b7ea7651e41afc58211f2a256850a"),
    "cpu-features": ("third_party/cpu_features/src",
                     "https://chromium.googlesource.com/external/github.com/google/cpu_features.git",
                     "81d13c49649f0714dd41fb56bb246398b6584085"),
    "cpuinfo": ("third_party/cpuinfo/src", "https://chromium.googlesource.com/external/github.com/pytorch/cpuinfo.git",
                "66ee79c038d70dad9f08705b2c9b3e58f6d8f512"),
    "crc32c": ("third_party/crc32c/src", "https://chromium.googlesource.com/external/github.com/google/crc32c.git",
               "2bbb3be42e20a0e6c0f7b39dc07dc863d9ffbc07"),
    "expat": ("third_party/expat/src", "https://chromium.googlesource.com/external/github.com/libexpat/libexpat.git",
              "a851869e111c06fbcdc89edf639abf5fc2b6deba"),
    "fontconfig": ("third_party/fontconfig/src", "https://chromium.googlesource.com/external/fontconfig.git",
                   "d416ada7b20a1cdd5050b60a7a1bbdb3d9ddd2dc"),
    "leveldb": ("third_party/leveldatabase/src", "https://chromium.googlesource.com/external/leveldb.git",
                "bb74ef739973a70ca9f0d90788a1da89b788c0ad"),
    "libgav1": ("third_party/libgav1/src", "https://chromium.googlesource.com/codecs/libgav1.git",
                "085032331f60c10055d32ab52932bd7bd77e6f53"),
    "libunwind": ("third_party/libunwind/src",
                  "https://chromium.googlesource.com/external/github.com/llvm/llvm-project/libunwind.git",
                  "c4438dd4178c627697459a12c54eb1f7f8eefd12"),
    "snappy": ("third_party/snappy/src", "https://chromium.googlesource.com/external/github.com/google/snappy.git",
               "32ded457c0b1fe78ceb8397632c416568d6714a0"),
    "vulkan-loader": ("third_party/vulkan-loader/src",
                      "https://chromium.googlesource.com/external/github.com/KhronosGroup/Vulkan-Loader",
                      "6b45d76280a9525740c38c1796060eadb9267488"),
    "wayland": ("third_party/wayland/src",
                "https://chromium.googlesource.com/external/anongit.freedesktop.org/git/wayland/wayland.git",
                "87cc8a8728a923fc57938faa81ba0e74f34ecdc7"),
    "wayland-protocols": ("third_party/wayland-protocols/src",
                          "https://chromium.googlesource.com/external/anongit.freedesktop.org/git/wayland/wayland-protocols.git",
                          "ee78491a237eaff9389a0ccf8680521d074407d3"),
    "wayland-kde": ("third_party/wayland-protocols/kde",
                    "https://chromium.googlesource.com/external/github.com/KDE/plasma-wayland-protocols.git",
                    "0b07950714b3a36c9b9f71fc025fc7783e82926e"),
    "wayland-gtk": ("third_party/wayland-protocols/gtk",
                    "https://chromium.googlesource.com/external/github.com/GNOME/gtk.git",
                    "40ebed3a03aef096addc0af09fec4ec529d882a0"),
    "wuffs": ("third_party/wuffs/src", "https://skia.googlesource.com/external/github.com/google/wuffs-mirror-release-c.git",
              "7411f488fe2e2c205c3d3b3d28638b7356522930"),
    "zstd": ("third_party/zstd/src", "https://chromium.googlesource.com/external/github.com/facebook/zstd.git",
             "10da6ba6de05e29169261fa4b68eb99239f770dd"),
}
SUPPORT_PINS = {
    "compiler-rt": ("third_party/compiler-rt/src",
                    "https://chromium.googlesource.com/external/github.com/llvm/llvm-project/compiler-rt.git",
                    "5ad76d13e9a724c8c29cd61bde4f1622e20f0566"),
    "libcxx": ("third_party/libc++/src", "https://chromium.googlesource.com/external/github.com/llvm/llvm-project/libcxx.git",
               "97b436da4c33663581d394f4ee0a5977fc38c2f4"),
    "libcxxabi": ("third_party/libc++abi/src",
                  "https://chromium.googlesource.com/external/github.com/llvm/llvm-project/libcxxabi.git",
                  "0e691bb69ac90450d8d53bc8ee5968c62f9c15a7"),
    "llvm-libc": ("third_party/llvm-libc/src", "https://chromium.googlesource.com/external/github.com/llvm/llvm-project/libc.git",
                  "97e60a1e7f7180965cc84e22ddde9f1d4ffd640f"),
    "dragonbox": ("third_party/dragonbox/src", "https://chromium.googlesource.com/external/github.com/jk-jeon/dragonbox.git",
                  "beeeef91cf6fef89a4d4ba5e95d47ca64ccb3a44"),
    "fast-float": ("third_party/fast_float/src", "https://chromium.googlesource.com/external/github.com/fastfloat/fast_float.git",
                   "42f254836ee416ffcf2f4e6576260003ac33d48c"),
    "fxdiv": ("third_party/fxdiv/src", "https://chromium.googlesource.com/external/github.com/Maratyszcza/FXdiv.git",
              "63058eff77e11aa15bf531df5dd34395ec3017c8"),
    "microsoft-multimedia": ("third_party/microsoft_multimedia_redirection/src",
                             "https://chromium.googlesource.com/external/github.com/microsoft/multimedia-redirection.git",
                             "8b1d8893d126547309f1bab8d8232418aa7c7109"),
    "microsoft-webauthn": ("third_party/microsoft_webauthn/src",
                           "https://chromium.googlesource.com/external/github.com/microsoft/webauthn.git",
                           "ef82c157125a0490e05f6ea82a7adb1b8e1bad08"),
    "pthreadpool": ("third_party/pthreadpool/src", "https://chromium.googlesource.com/external/github.com/google/pthreadpool.git",
                    "15a6644ba1c45f1acc16ac1e883efc3e56c6bed2"),
    "xnnpack": ("third_party/xnnpack/src", "https://chromium.googlesource.com/external/github.com/google/XNNPACK.git",
                "5d067b8f52342ba192ad9f67ebab9e77c1d046bc"),
}
BROWSER_PINS = {
    "anonymous-tokens": ("third_party/anonymous_tokens/src",
                         "https://chromium.googlesource.com/external/github.com/google/anonymous-tokens.git",
                         "1ae8df5e40bab28959cc182c2c0d88923059e278"),
    "devtools-frontend": ("third_party/devtools-frontend/src", "https://chromium.googlesource.com/devtools/devtools-frontend",
                          "f10f5450f5e0a68436ad30a52589a2727b23e395"),
    "federated-compute": ("third_party/federated_compute/src",
                          "https://chromium.googlesource.com/external/github.com/google-parfait/federated-compute.git",
                          "a9d5262aa05a4b1ff3c23697d644023c5d913840"),
    "ink": ("third_party/ink/src", "https://chromium.googlesource.com/external/github.com/google/ink.git",
            "b6def591a73a6aa0ffce1e58dc69b39de31484ba"),
    "libprotobuf-mutator": ("third_party/libprotobuf-mutator/src",
                            "https://chromium.googlesource.com/external/github.com/google/libprotobuf-mutator.git",
                            "4373c81cd7d50a65f7fbd8dc3779c0c3b1dc9d72"),
    "oak": ("third_party/oak/src", "https://chromium.googlesource.com/external/github.com/project-oak/oak.git",
            "3ec9bd888b4bcfbe5dea1def3c642911e71e1c96"),
    "openh264": ("third_party/openh264/src", "https://chromium.googlesource.com/external/github.com/cisco/openh264",
                 "9547f96ea5c47f4d465d97b07264b997ecc4b4b6"),
    "openscreen": ("third_party/openscreen/src", "https://chromium.googlesource.com/openscreen",
                   "8978b74badf042795c07fcf99d0a53dccbd3dcf6"),
    "openxr": ("third_party/openxr/src", "https://chromium.googlesource.com/external/github.com/KhronosGroup/OpenXR-SDK",
               "75c53b6e853dc12c7b3c771edc9c9c841b15faaa"),
    "sframe": ("third_party/sframe/src", "https://chromium.googlesource.com/external/github.com/cisco/sframe",
               "6a37f061f4eda0c6594c91ecf99b5ff498c8c124"),
}
NOTICE_TEST_PINS = {"webgl": ("third_party/webgl/src",
    "https://chromium.googlesource.com/external/khronosgroup/webgl.git", "714857a28445e8f5d8d6ae1c78498578009534d8")}
GPU_PINS = {"dawn": ("third_party/dawn", "https://dawn.googlesource.com/dawn.git",
                     "088a8d00efd1c70b58c549d355401d115904b648")}
GPU_SUPPORT_PINS = {
    "vulkan-utility-libraries": ("third_party/vulkan-utility-libraries/src",
        "https://chromium.googlesource.com/external/github.com/KhronosGroup/Vulkan-Utility-Libraries",
        "90738f6edfa19561a978fc5f97543ba37075ba9d"),
    "vulkan-validation-layers": ("third_party/vulkan-validation-layers/src",
        "https://chromium.googlesource.com/external/github.com/KhronosGroup/Vulkan-ValidationLayers",
        "c93a48d963e7f311d8ca0511b5f41dc3abe63bff"),
}
DAWN_TEST_PINS = {"googletest": ("third_party/googletest/src",
    "https://chromium.googlesource.com/external/github.com/google/googletest.git",
    "4fe3307fb2d9f86d19777c7eb0e4809e9694dde7")}
GROUPS = {"core": PINS, "media": MEDIA_PINS, "runtime": RUNTIME_PINS,
          "support": SUPPORT_PINS, "browser": BROWSER_PINS, "notice-tests": NOTICE_TEST_PINS,
          "gpu": GPU_PINS, "gpu-support": GPU_SUPPORT_PINS, "dawn-tests": DAWN_TEST_PINS}
# Preserve exact declared conditions as data, never evaluate or broaden them.
# Collecting OS-specific inputs does not prove a platform build used them.
SOURCE_CONDITIONS = {key: "checkout_linux" for key in
                     ("fontconfig", "wayland", "wayland-protocols", "wayland-kde", "wayland-gtk")}
SOURCE_CONDITIONS.update({key: "checkout_win" for key in ("microsoft-multimedia", "microsoft-webauthn")})
SOURCE_CONDITIONS["openxr"] = "checkout_openxr"
MAX_SELECTED = 12000
REVIEWED_EXTRA = {"freetype": {"docs/FTL.TXT", "docs/GPLv2.TXT", "src/bdf/README", "src/pcf/README",
                               "src/base/fthash.c", "include/freetype/internal/fthash.h", "src/gzip/zlib.h"}}


def checked_catalog(contents, pins=PINS):
    if hashlib.sha256(contents).hexdigest() != CATALOG_SHA:
        raise ValueError("fixed reviewed DEPS catalog SHA256 required")
    report = json.loads(contents)
    if (report["sourceCommit"] != TREE["COMMIT"] or report["mismatches"]
            or report["unresolved"] or report["uncataloguedGitlinks"]
            or report["hooksExecuted"] or report["conditionsEvaluated"]):
        raise ValueError("static matched parent catalog required")
    dependencies = report["gitDependencies"]
    if len({row["path"] for row in dependencies}) != len(dependencies):
        raise ValueError("unique dependency checkout identities required")
    by_path = {row["path"]: row for row in dependencies}
    for key, (path, repository, revision) in pins.items():
        row = by_path.get(path)
        if (row is None or (row["repository"], row["revision"]) != (repository, revision)
                or row["gitlinkRevision"] != revision or row["gitlinkMatched"] is not True
                or row["condition"] != SOURCE_CONDITIONS.get(key)):
            raise ValueError("selected public source differs from reviewed immutable pin")
    return report


def checked_paths(source, output, pins=PINS):
    if (source.parent != HERE / ".run"
            or not re.fullmatch(r"maintained-notice-source-[0-9]{8}-[0-9]{2}", source.name)
            or output.parent != source or not re.fullmatch(r"external-license-[0-9]{2}", output.name)):
        raise ValueError("task-owned source and fresh external generation required")
    if output.exists() or output.is_symlink():
        raise FileExistsError("never overwrite external-source evidence")
    TREE["regular_parents"](source)
    if not source.is_dir() or source.resolve(strict=True) != source:
        raise ValueError("canonical source directory required")
    catalog = source / CATALOG
    TREE["regular_parents"](catalog)
    if catalog.stat().st_size > TREE["MAX_BLOB"]:
        raise ValueError("catalog byte budget")
    report = checked_catalog(catalog.read_bytes(), pins)
    main = source / "license-tree-02" / "license-source-inventory.json"
    TREE["regular_parents"](main)
    if main.stat().st_size > TREE["MAX_BLOB"]:
        raise ValueError("main license receipt byte budget")
    contents = main.read_bytes()
    if hashlib.sha256(contents).hexdigest() != MAIN_RECEIPT_SHA:
        raise ValueError("fixed main license receipt SHA256 required")
    parent_refs = json.loads(contents)["licenseReferences"]
    return report, parent_refs


def checked_cache(source, name):
    if name is None:
        return None
    if not re.fullmatch(r"external-license-[0-9]{2}", name):
        raise ValueError("safe existing external-source cache generation required")
    cache = source / name
    receipt = cache / "external-source-inventory.json"
    TREE["regular_parents"](receipt)
    if receipt.stat().st_size > TREE["MAX_BLOB"]:
        raise ValueError("external cache receipt byte budget")
    report = json.loads(receipt.read_bytes())
    if (report["sourceCommit"] != TREE["COMMIT"] or report["parentCatalogSha256"] != CATALOG_SHA
            or report["collectionComplete"] is not True):
        raise ValueError("completed matched source cache required")
    return cache


def reviewed_extra(key, extra_paths):
    if (not isinstance(extra_paths, (tuple, list, set)) or len(extra_paths) > MAX_SELECTED
            or any(not isinstance(path, str) or not re.fullmatch(r"[A-Za-z0-9_.\-/]+", path)
                or path.startswith("/") or any(part in ("", ".", "..") for part in path.split("/")) for path in extra_paths)
            or len(set(extra_paths)) != len(extra_paths)):
        raise ValueError("unique bounded canonical extra source paths required")
    return REVIEWED_EXTRA.get(key, set()) | set(extra_paths)


def selected_inputs(key, rows, parent_refs, pins=PINS, extra_paths=()):
    prefix = pins[key][0]
    refs = [row for row in parent_refs if row["sourcePath"] and row["sourcePath"].startswith(prefix + "/")]
    extra = reviewed_extra(key, extra_paths)
    by_path = {row["path"]: row for row in rows}
    requested = {row["sourcePath"][len(prefix) + 1:] for row in refs} | extra
    picked = [row for row in rows if TREE["selected"](row)
              or (row["path"] in requested and row["mode"] in ("100644", "100755"))
              or (key == "depot-tools" and row["path"].startswith("metadata/")
                  and row["mode"] in ("100644", "100755"))]
    missing = sorted(path for path in requested if path not in by_path or by_path[path]["mode"] not in ("100644", "100755"))
    return picked, refs, sorted(extra), missing


def checkout_references(prefix, row, contents):
    refs = TREE["references"]({**row, "path": prefix + "/" + row["path"]}, contents)
    result = []
    for reference in refs:
        checkout = reference["sourcePath"]
        relative = checkout[len(prefix) + 1:] if checkout and checkout.startswith(prefix + "/") else None
        result.append({**reference, "checkoutPath": checkout, "repositoryPath": relative,
                       "sourcePath": None if relative is None else relative})
    return result


def write_report(path, report):
    with path.open("x", encoding="utf-8") as stream:
        json.dump(report, stream, indent=2)
        stream.write("\n")


def collect_component(key, output, parent_refs, cache=None, pins=PINS, extra_paths=()):
    reviewed_extra(key, extra_paths)
    prefix, repository, revision = pins[key]
    component = output / key
    component.mkdir()
    cached_bare = cache / key / "pinned-tree.git" if cache else None
    bare = cached_bare if cached_bare and cached_bare.exists() else component / "pinned-tree.git"
    cached = bare == cached_bare
    if cached:
        TREE["regular_parents"](bare)
        if not bare.is_dir() or bare.resolve(strict=True) != bare:
            raise ValueError("canonical existing source cache required")
    else:
        bare.mkdir()
    blobs = component / "blobs"
    blobs.mkdir()
    report = {"component": key, "checkoutPath": prefix, "repository": repository,
              "revision": revision, "collectionComplete": False, "selectedFiles": [],
              "licenseReferences": [], "missingReferences": [], "gitlinks": [],
              "cacheSource": str(bare.relative_to(output.parent)).replace("\\", "/"),
              "recursiveDependenciesCollected": False, "generatorExecuted": False,
              "licenseCoverage": "not_proven", "promoted": False}
    if any(pins is group for group in (RUNTIME_PINS, SUPPORT_PINS, BROWSER_PINS, NOTICE_TEST_PINS, GPU_PINS, GPU_SUPPORT_PINS, DAWN_TEST_PINS)):
        report["declaredCheckoutCondition"] = SOURCE_CONDITIONS.get(key)
        report["conditionsEvaluated"] = False
    seen, total = {}, 0
    try:
        if not cached:
            TREE["git"](bare, ["init", "--bare"], 4096)
            TREE["git"](bare, ["fetch", "--depth=1", "--filter=blob:none", "--no-tags",
                              "--no-write-fetch-head", repository, revision])
        if TREE["git"](bare, ["rev-parse", "--is-bare-repository"], 64) != b"true\n":
            raise ValueError("dependency cache must be bare, no checkout")
        if TREE["git"](bare, ["rev-parse", revision], 64).decode().strip() != revision:
            raise ValueError("exact dependency commit required")
        root = TREE["git"](bare, ["rev-parse", revision + "^{tree}"], 64).decode().strip()
        if not TREE["SHA1"].fullmatch(root):
            raise ValueError("dependency root-tree identity")
        contents = TREE["git"](bare, ["ls-tree", "-r", "-z", "--full-tree", revision])
        rows = TREE["tree_records"](contents)
        report.update(rootTreeSha1=root, treeEntries=len(rows), treeBytes=len(contents),
                      treeSha256=hashlib.sha256(contents).hexdigest(),
                      gitlinks=[row for row in rows if row["mode"] == "160000"])
        (component / "source-tree.nul").write_bytes(contents)
        by_path = {row["path"]: row for row in rows}
        pending, refs, extra, missing = selected_inputs(key, rows, parent_refs, pins, extra_paths)
        report.update(parentLicenseReferences=refs, reviewedExtraPaths=extra, missingParentOrExtraPaths=missing)
        if not 0 < len(pending) <= MAX_SELECTED:
            raise ValueError("dependency selected-path budget")
        paths = {row["path"] for row in pending}
        for round_index in range(2):
            oids = sorted({row["gitObjectSha1"] for row in pending} - set(seen))
            TREE["fetch_blobs"](bare, oids, repository)
            for start in range(0, len(oids), 64):
                batch = oids[start:start + 64]
                contents = TREE["git"](bare, ["cat-file", "--batch"], TREE["MAX_TOTAL"],
                                       ("\n".join(batch) + "\n").encode())
                for oid, contents in TREE["batch_payloads"](contents, batch).items():
                    identity = TREE["check_blob"](oid, contents)
                    total += identity["bytes"]
                    if total > TREE["MAX_TOTAL"]:
                        raise ValueError("dependency source-byte budget")
                    with (blobs / (oid + ".source-data")).open("xb") as stream:
                        stream.write(contents)
                    seen[oid] = identity
            for row in pending:
                contents = (blobs / (row["gitObjectSha1"] + ".source-data")).read_bytes()
                report["selectedFiles"].append({**row, **seen[row["gitObjectSha1"]],
                                                "checkoutPath": prefix + "/" + row["path"]})
                report["licenseReferences"].extend(checkout_references(prefix, row, contents))
            if round_index == 0:
                refs = {row["repositoryPath"] for row in report["licenseReferences"]
                        if row["repositoryPath"] is not None}
                pending = [by_path[path] for path in sorted(refs - paths)
                           if path in by_path and by_path[path]["mode"] in ("100644", "100755")]
                if len(paths) + len(pending) > MAX_SELECTED:
                    raise ValueError("dependency reference-path budget")
        report["missingReferences"] = [row for row in report["licenseReferences"]
                                       if row["repositoryPath"] not in by_path
                                       or by_path[row["repositoryPath"]]["mode"] not in ("100644", "100755")]
        report.update(collectionComplete=True)
    except Exception as error:
        # Error messages in our validators contain no network stderr or credentials.
        report["failureType"] = type(error).__name__
        raise
    finally:
        report.update(uniqueBlobs=len(seen), sourceBytes=total)
        write_report(component / "license-source-inventory.json", report)
    return report


def collect(source, output, keys, cache_name=None, pins=PINS):
    group = next((name for name, known in GROUPS.items() if pins is known), None)
    if group is None:
        raise ValueError("only reviewed compiled public source groups allowed")
    if not keys or len(keys) != len(set(keys)) or any(key not in pins for key in keys):
        raise ValueError("unique allowlisted public components required")
    catalog, parent_refs = checked_paths(source, output, pins)
    cache = checked_cache(source, cache_name)
    output.mkdir()
    # Preserve the exact local collector used, never execute this snapshot.
    (output / "collector.py.source-data").write_bytes(Path(__file__).read_bytes())
    report = {"scope": "selected fixed public dependency license inputs only, not full NOTICE/SBOM",
              "sourceCommit": TREE["COMMIT"], "parentCatalogSha256": CATALOG_SHA,
              "reviewedGroup": group,
              "requestedComponents": keys, "components": [], "collectionComplete": False,
              "parentGitDeclarations": len(catalog["gitDependencies"]),
              "parentNonGitDeclarations": len(catalog["nonGitDependencies"]),
              "conditionsEvaluated": False, "hooksExecuted": False, "generatorExecuted": False,
              "licenseCoverage": "not_proven", "promoted": False}
    try:
        for key in keys:
            result = collect_component(key, output, parent_refs, cache, pins)
            report["components"].append({"component": key, "revision": result["revision"],
                                          "treeEntries": result["treeEntries"],
                                          "selectedFiles": len(result["selectedFiles"]),
                                          "uniqueBlobs": result["uniqueBlobs"],
                                          "sourceBytes": result["sourceBytes"],
                                          "missingReferences": len(result["missingReferences"]),
                                          "missingParentOrExtraPaths": len(result["missingParentOrExtraPaths"]),
                                          "collectionComplete": result["collectionComplete"]})
            print(json.dumps(report["components"][-1]), flush=True)
        report["collectionComplete"] = True
    finally:
        write_report(output / "external-source-inventory.json", report)
    return report


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--output-name", required=True)
    parser.add_argument("--group", choices=tuple(GROUPS), default="core")
    parser.add_argument("--component", action="append", choices=sorted({key for pins in GROUPS.values() for key in pins}))
    parser.add_argument("--cache-name", help="reuse a completed task-owned bare source cache, not its receipts")
    args = parser.parse_args()
    source = args.source.absolute()
    pins = GROUPS[args.group]
    collect(source, source / args.output_name, args.component or list(pins), args.cache_name, pins)
