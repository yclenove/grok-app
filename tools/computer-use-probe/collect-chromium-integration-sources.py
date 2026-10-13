"""Collect exact Chromium/ANGLE/OpenScreen integration rules as inert data.

Each compiled Gitiles URL/blob is independently checked against sealed trees
and all four original checkout manifests. No GN, upstream source, git lazy
fetch, hooks, source alias or shipping inference is executed.
"""
import argparse
import base64
from concurrent.futures import ThreadPoolExecutor
import hashlib
import json
from pathlib import Path
import re
import runpy
import urllib.request


HERE = Path(__file__).parent.absolute()
BASE = runpy.run_path(str(HERE / "collect-chromium-crashpad-lss-sources.py"))
INPUTS, EXTERNAL, CHECKOUTS = BASE["INPUTS"], BASE["EXTERNAL"], BASE["CHECKOUTS"]
COMMIT = INPUTS["TREE"]["COMMIT"]
ROOTS = {
    "chromium": ("", "https://chromium.googlesource.com/chromium/src.git", COMMIT),
    "angle": ("third_party/angle", "https://chromium.googlesource.com/angle/angle.git", "9649fea17b77e672ae19687bf0222109b6270e0d"),
    "openscreen": ("third_party/openscreen/src", "https://chromium.googlesource.com/openscreen", "8978b74badf042795c07fcf99d0a53dccbd3dcf6"),
}
PINS = {
    "chromium": {
        "build_overrides/angle.gni": "1bb477a24f3bbe73fc6800786e8cc0ed1c93744a",
        "build/config/chrome_build.gni": "4163d9806888f00e37201be05bea8a302c8f757f",
    },
    "angle": {
        ".gn": "c3aed04534f2b9b7e966e0025b4a5632f15197f1",
        "BUILD.gn": "d6969b427ed614ec861c338cdd127ba4717dad1d",
        "dotfile_settings.gni": "d27fae199a7a7cc5f4b306f5bb2be930c0ceae8c",
        "gni/angle.gni": "34451e32dbcd986c0260061f81977ca174bfc69c",
        "third_party/BUILD.gn": "779f5ec36476bf2d5253821d10822d8564cb4d2e",
        "third_party/cpu_features/BUILD.gn": "2b048c9b0b476f7107b2b0a6f25970dec17ca64e",
    },
    "openscreen": {
        ".gn": "fe315e91e8399a7d2bce7d103f6088e54df35f3c",
        "BUILD.gn": "5659a647a957ac9cee3f5b845ee62fe46982ac95",
        "build_overrides/build.gni": "51f2cd2b907b2147073af972d23ea49d340f0345",
        "third_party/googleurl/BUILD.gn": "b0f209d461201b0bbe444ce67d3eab4ee4ddc777",
        "third_party/llvm-libc/BUILD.gn": "53fd4104978cc94909eaee33f153d136db84fc3e",
        "third_party/perfetto/BUILD.gn": "7259689cea7da3d464d7e371d7326d2cef3e9ff0",
        "third_party/quiche/BUILD.gn": "b35fd7fa81463f6df44411473f6c1417f4df4bc6",
    },
}
MAX_RAW, MAX_SOURCE, MAX_TOTAL = 2 * 1024**2, 1024**2, 8 * 1024**2
ANGLE_CACHE = "external-license-01/angle/pinned-tree.git"
ANGLE_CACHE_TREE = "c584938e2146426bdfb86c736797517c81fecc26"
SWIFTSHADER_ROOT = ("third_party/swiftshader", "https://swiftshader.googlesource.com/SwiftShader.git",
                    "0f87bb2d3742c2992fd1a7dcb2f051eebf79299c")
SWIFTSHADER_CACHE = "external-license-01/swiftshader/pinned-tree.git"
SWIFTSHADER_CACHE_TREE = "10829357f8f6de3c9e1cde6e360f164af457cfd9"
SKIA_ROOT = ("third_party/skia", "https://skia.googlesource.com/skia.git",
             "c88b49d1d6c7b27b1b611f78e1abf924383e801b")
SKIA_CACHE = "external-license-01/skia/pinned-tree.git"
SKIA_CACHE_TREE = "bbad6f9075fd07251b4350b5aa2e829ffbbcd743"
DAWN_ROOT = EXTERNAL["GPU_PINS"]["dawn"]
DAWN_CACHE = "external-license-08/dawn/pinned-tree.git"
DAWN_CACHE_TREE = "527b73bb36eeb7ac8e4cfa420d0740b32633ea01"
EMBEDDING_PINS = {
    "chromium": {"build_overrides/build.gni": "cd62385050b0ad022f5f1967eb64ee08f10ddf7b"},
    "angle": {
        "build_overrides/build.gni": "f4b6ee03e1b418d2a65da09a328b22247c46c72a",
        "src/libANGLE/renderer/vulkan/BUILD.gn": "4eef7114f6a1031089359ce93443e8a61ffd85b0",
    },
}
TARGET_PINS = {
    "chromium": {"url/gurl.h": "da6bbecbf7352e7114f1a83f43f8218f93aa7864"},
    "angle": {"src/common/vulkan/BUILD.gn": "aaf43796a44302744954eebae99dfa30bf254cc5"},
}
CONSUMER_PINS = {
    "chromium": {
        "build/config/c++/c++.gni": "cd07de0717f3846f1060848cfd1c21c7f4342a4d",
        "buildtools/third_party/libc++/BUILD.gn": "e459bcb42f4decfad69ea7e8ffb96db5e8395348",
        "buildtools/third_party/libc++abi/BUILD.gn": "e04e7afbbb70d5417204e1aa9a02f1bd4f11614f",
        "third_party/llvm-libc/BUILD.gn": "1c036893d34693e2e5bb3af3487d495de8ace89b",
    },
    "angle": {
        "util/BUILD.gn": "3196b4b8ae64b962fa53b27d6e2101ecd8af7855",
        "include/EGL/egl.h": "51c1b2d4eb0393a5568a78c98c106f5d5b4fb88e",
        "include/EGL/eglext.h": "f8d2ccec45d42d5d1d19ecbd53109193895de915",
        "include/GLES2/gl2.h": "fdc46fc951e560c5999f0d2e9e8a8a6c9cb5255d",
        "include/GLES3/gl3.h": "388e880fb66768924f524bbb1f4fc6ea7ce42d7c",
        "include/KHR/khrplatform.h": "dd22d9270188f2878f5dd47806497b16b34db0fe",
    },
}
PLATFORM_PINS = {"angle": {
    "include/EGL/eglplatform.h": "101fd74d221a149be3756010a80366b071bd29e9",
    "include/GLES2/gl2platform.h": "5bcce6d89e0992419294ce95a269427a2811e2f7",
    "include/GLES3/gl3platform.h": "8699212de64d4050ed97edd548e6c7cee6f56c0e",
}}
CPU_CONSUMER_PINS = {"angle": {
    "src/libGLESv2.gni": "2c3770be7ed8a122fa168e616b5a0392ffcc1be6",
    "src/gpu_info_util/SystemInfo_android.cpp": "b1ac3e133ddef1c04c42c5ec290c8023b288d5f5",
}}
CPU_TEST_PINS = {"angle": {"src/tests/BUILD.gn": "2c4f581ca7c72b9313e780f11032efe0f35087fb"}}
ANGLE_GN_PINS = {"angle": runpy.run_path(str(HERE / "chromium-angle-gn-pins.py"))["ANGLE_GN_PINS"]}
ANGLE_HEADER_PINS = {"angle": runpy.run_path(str(HERE / "chromium-angle-header-pins.py"))["ANGLE_HEADER_PINS"]}
SWIFTSHADER_GN_PINS = {"swiftshader": runpy.run_path(str(HERE / "chromium-swiftshader-gn-pins.py"))["SWIFTSHADER_GN_PINS"]}
SWIFTSHADER_CONSUMER_PINS = {"swiftshader": {
    "src/Pipeline/ComputeProgram.cpp": "a26f3f40cc4b27cbacb0b2e8e825bab59c423bcf",
    "src/Reactor/LLVMReactor.cpp": "c3ed18d826dc4958f2656c255318e70421030c19",
    "src/Reactor/SubzeroReactor.cpp": "c0c6b1d6e83b03b3c75511c27e005662c4945a05",
    "third_party/llvm-10.0/llvm/include/llvm/ADT/StringRef.h": "f06d18720c3a670b5358437977e2225795c16086",
    "third_party/llvm-16.0/llvm/include/llvm/ADT/StringRef.h": "2d2f0bedfe1f822382b5f60cf39a9fba9d80855b",
}}
SWIFTSHADER_COPIED_PINS = {"swiftshader": {
    ".gitmodules": "cb86c5cb45c0f66ab90a722c545a32407b1f4b90",
    "CMakeLists.txt": "0ef4a68cb714420acad4808bdfa32317809390b7",
    "third_party/boost/CMakeLists.txt": "d31980614417812e3b86571e58bcdc37ad482542",
    "third_party/libbacktrace/CMakeLists.txt": "b86f453bbfa22680ae1844d5b9f625f996579ff2",
    "third_party/libbacktrace/config/darwin/include/backtrace-supported.h": "7709fcab748a534505638e662c633f6d3b82acf4",
    "third_party/libbacktrace/config/darwin/include/config.h": "ee92775ba6b4a119848892564e61758d3c12496d",
    "third_party/libbacktrace/config/linux/include/backtrace-supported.h": "7709fcab748a534505638e662c633f6d3b82acf4",
    "third_party/libbacktrace/config/linux/include/config.h": "7333ecaa050cfb82f73f77a33ae5e86dcfc51915",
    "third_party/llvm-10.0/llvm/include/llvm/Support/MD5.h": "bb2bdbf1bed225e5a8d501f11849e5a7ef0b94fa",
    "third_party/llvm-10.0/llvm/lib/Support/MD5.cpp": "9b02f62912fa37a5cdd56dc043f77df15482aec0",
    "third_party/llvm-16.0/llvm/include/llvm/Support/MD5.h": "fa2f477261dd905a713b06b6bd1f45d02addca8e",
    "third_party/llvm-16.0/llvm/lib/Support/MD5.cpp": "ef7f559adf064352fdc0a064d36884c16fba2256",
    "third_party/llvm-subzero/include/llvm/Support/MD5.h": "eb181bfe8a5c8066f6088a305435d36a3ac09926",
}}
SWIFTSHADER_PROFILES = ("swiftshader-gn", "swiftshader-consumers", "swiftshader-copied")
SKIA_PINS = runpy.run_path(str(HERE / "chromium-skia-gn-pins.py"))
SKIA_GN_PINS = {"skia": SKIA_PINS["SKIA_GN_PINS"]}
SKIA_EMBEDDING_PINS = {"chromium": SKIA_PINS["CHROMIUM_SKIA_GN_PINS"]}
DAWN_PINS = runpy.run_path(str(HERE / "chromium-dawn-gn-pins.py"))
DAWN_GN_PINS = {"dawn": DAWN_PINS["DAWN_GN_PINS"]}
DAWN_EMBEDDING_PINS = {"chromium": DAWN_PINS["CHROMIUM_DAWN_PINS"]}
DAWN_SUPPORT_PINS = {"chromium": {
    "base/allocator/partition_allocator/BUILD.gn": "22c177deb9d06542920e672053ec7007af3f0c69",
    "base/allocator/partition_allocator/src/partition_alloc/pointers/raw_ptr.h": "ca958b1d1190e4ae730d862fca25d135c6ca070f",
}}
ANGLE_REGISTRY_PINS = {"angle": {
    "scripts/registry_xml.py": "41f3fd99e9796b453bc4d76f3fc62a041ea88cf9",
    "scripts/run_code_generation.py": "7f514123def6890c668dfcfa9cba7f19ff584762",
    "scripts/egl_angle_ext.xml": "e1d9b360f39a8069e3015f432899f9f23d70e802",
    "scripts/gl_angle_ext.xml": "67eca0d0344a1d2d136de08c32a6fc544ff55fbb",
}}
ANGLE_GENERATION_PINS = {"angle": {
    "scripts/code_generation_hashes/GL_EGL_WGL_loader.json": "15a86079b0c71d1789138fe19b1f5c5be676216e",
    "scripts/code_generation_hashes/GL_EGL_entry_points.json": "3b8e7ec30c3e7aaf02f3eef5411b2ae67686948e",
    "scripts/code_generation_hashes/GLenum_value_to_string_map.json": "c48e31f7af2372f2c5e2c08f765baab9b231637a",
    "scripts/code_generation_hashes/packed_enum.json": "0622fcd36ab856676d92b7231b8c7a373e14ab66",
    "scripts/code_generation_hashes/proc_table.json": "b60b295c1c736e4e6638118cd97a80dd46ab53c7",
    "scripts/generate_loader.py": "a96e106a3dad01f2a684cf694f1a146a7ef681d5",
    "scripts/generate_entry_points.py": "cc0a512406c1d01ccd88af10680a2dd6e3212ef9",
    "scripts/gen_gl_enum_utils.py": "7866853ad29982c0a0824228e4c47e23f3356da2",
    "scripts/gen_proc_table.py": "258cf349f1a013cdc1ed3d2831d65d8ef333f061",
}}
ANGLE_GENERATED_PINS = {"angle": {
    "src/common/gl_enum_utils_autogen.cpp": "ee51e1a10773a4d7cecb2249724da3a832f7f40a",
    "src/common/gl_enum_utils_autogen.h": "4a95ac550956b39d4418a44e101b421fa7c6be94",
    "src/libEGL/egl_loader_autogen.cpp": "79684bdb743274203cfe07b233f2531e6ef4a5e9",
    "src/libEGL/egl_loader_autogen.h": "7b4a293773d9ff9d2fd9b05603023b586f6955a8",
    "src/libGLESv2/egl_stubs_getprocaddress_autogen.cpp": "99f4aab543840310dc1d1c142bd0f9cff3d7c7f5",
    "src/libGLESv2/proc_table_cl_autogen.cpp": "a597c20b83904158f80be3901b62eac1824b1a66",
    "src/libOpenCL/libOpenCL_autogen.map": "55d7cc2d9a1accec009e8a269e625cc71dc720ec",
}}
PROFILES = {"integration": PINS, "embedding": EMBEDDING_PINS, "targets": TARGET_PINS,
            "consumers": CONSUMER_PINS, "platform": PLATFORM_PINS, "cpu-consumer": CPU_CONSUMER_PINS,
            "cpu-tests": CPU_TEST_PINS, "angle-gn": ANGLE_GN_PINS, "angle-headers": ANGLE_HEADER_PINS,
            "swiftshader-gn": SWIFTSHADER_GN_PINS, "swiftshader-consumers": SWIFTSHADER_CONSUMER_PINS,
            "swiftshader-copied": SWIFTSHADER_COPIED_PINS,
            "skia-gn": SKIA_GN_PINS, "skia-embedding": SKIA_EMBEDDING_PINS,
            "dawn-gn": DAWN_GN_PINS, "dawn-embedding": DAWN_EMBEDDING_PINS, "dawn-support": DAWN_SUPPORT_PINS,
            "angle-registry": ANGLE_REGISTRY_PINS, "angle-generation": ANGLE_GENERATION_PINS,
            "angle-generated": ANGLE_GENERATED_PINS}
GIT_PROFILES = ("angle-gn", "angle-headers", "skia-gn", "dawn-gn", "angle-registry",
                "angle-generation", "angle-generated") + SWIFTSHADER_PROFILES


def source_roots(profile):
    if profile not in PROFILES:
        raise ValueError("compiled integration source profile required")
    if profile == "skia-gn":
        return {"skia": SKIA_ROOT}
    if profile == "dawn-gn":
        return {"dawn": DAWN_ROOT}
    if profile in ("angle-registry", "angle-generation", "angle-generated"):
        return {"angle": ROOTS["angle"]}
    if profile in ("skia-embedding", "dawn-embedding", "dawn-support"):
        return {"chromium": ROOTS["chromium"]}
    return {"swiftshader": SWIFTSHADER_ROOT} if profile in SWIFTSHADER_PROFILES else ROOTS


def git_scope(selected):
    if selected == requests("dawn-gn"):
        return DAWN_CACHE, DAWN_ROOT, DAWN_CACHE_TREE
    if selected == requests("skia-gn"):
        return SKIA_CACHE, SKIA_ROOT, SKIA_CACHE_TREE
    if any(selected == requests(profile) for profile in SWIFTSHADER_PROFILES):
        return SWIFTSHADER_CACHE, SWIFTSHADER_ROOT, SWIFTSHADER_CACHE_TREE
    return ANGLE_CACHE, ROOTS["angle"], ANGLE_CACHE_TREE


def requests(profile="integration"):
    if profile not in PROFILES:
        raise ValueError("compiled integration source profile required")
    result = []
    for group, pins in PROFILES[profile].items():
        prefix, repository, revision = source_roots(profile)[group]
        gitiles = repository[:-4] if repository.endswith(".git") else repository
        for path, oid in pins.items():
            result.append({"sourceGroup": group, "path": path, "sourcePath": prefix + "/" + path if prefix else path,
                           "gitObjectSha1": oid, "url": gitiles + "/+/" + revision + "/" + path + "?format=TEXT"})
    return result


def checked_inputs(source, profile="integration"):
    selected = requests(profile)
    inputs = INPUTS["load_inputs"](source, True, True, True, True)
    if profile == "dawn-gn":
        INPUTS["append_external"](source, "external-license-08", INPUTS["GPU_SHA"], EXTERNAL["GPU_PINS"],
            INPUTS["GPU_SEALS"], inputs["index"], inputs["files"], inputs["references"], inputs["components"])
    for row in selected:
        entry = inputs["index"].get(row["sourcePath"])
        if (entry is None or entry["mode"] not in ("100644", "100755")
                or (entry["kind"], entry["gitObjectSha1"]) != ("blob", row["gitObjectSha1"])):
            raise ValueError("each integration source must match the independent sealed source tree")
    roots = source_roots(profile)
    for group, (prefix, repository, revision) in roots.items():
        if group == "chromium":
            continue
        if group == "dawn":
            pin = EXTERNAL["GPU_PINS"][group]
        else:
            pin = EXTERNAL["BROWSER_PINS"][group] if group == "openscreen" else EXTERNAL["PINS"][group]
        if pin != (prefix, repository, revision):
            raise ValueError("independently pinned original dependency identity required")
    checkout = CHECKOUTS["inspect"](source)
    if (len(checkout["targets"]) != 4 or {row["target"] for row in checkout["targets"]}
            != {"windows-x64", "linux-x64", "mac-intel", "mac-arm64"}):
        raise ValueError("all four unique original checkout targets required")
    correspondence = []
    for target in checkout["targets"]:
        for group, (prefix, repository, revision) in roots.items():
            path = "src/" + prefix if prefix else "src"
            row = CHECKOUTS["compare_pin"](path, repository, revision, target["manifest"])
            if row["status"] != "exact_repository_and_revision_match":
                raise ValueError("all selected exact source roots must match all four original manifests")
            correspondence.append({"target": target["target"], "sourceGroup": group, **row})
    return inputs, checkout["checkoutReceiptSha256"], correspondence


def fetch(request):
    if request not in [row for profile in PROFILES for row in requests(profile)]:
        raise ValueError("only compiled fixed-revision integration source requests allowed")
    http = urllib.request.Request(request["url"], method="GET", headers={"Accept": "text/plain", "Accept-Encoding": "identity"})
    no_redirect = BASE["GENERATED"]["CONFIG"]["CHECKOUTS"]["LOGS"]["BUILDS"]["SEARCH"]["NoRedirect"]()
    with urllib.request.build_opener(no_redirect).open(http, timeout=20) as response:
        if (response.geturl() != request["url"] or response.headers.get("Content-Type", "").split(";", 1)[0] != "text/plain"
                or response.headers.get("Content-Encoding") not in (None, "identity")):
            raise ValueError("exact undecoded Gitiles TEXT endpoint and content type required")
        raw = response.read(MAX_RAW + 1)
    if not 0 < len(raw) <= MAX_RAW:
        raise ValueError("bounded nonempty original integration source response required")
    return raw


def decode(raw, oid):
    if not 0 < len(raw) <= MAX_RAW:
        raise ValueError("bounded original integration source raw bytes required")
    data = base64.b64decode(raw, validate=True)
    if not 0 < len(data) <= MAX_SOURCE:
        raise ValueError("bounded nonempty integration source bytes required")
    identity = INPUTS["TREE"]["check_blob"](oid, data)
    data.decode("utf-8", errors="strict")
    return data, identity


def fetched_requests(selected, parallel=False):
    if not parallel:
        for request in selected:
            yield request, fetch(request)
        return
    # Four independent read-only requests per wave. Consume and write in the
    # original order; a failed wave starts no later wave and joins its owners.
    with ThreadPoolExecutor(max_workers=4) as pool:
        for offset in range(0, len(selected), 4):
            wave = selected[offset:offset + 4]
            futures = [pool.submit(fetch, request) for request in wave]
            for request, future in zip(wave, futures):
                yield request, future.result()


def save_blob(path, contents, limit):
    if not 0 < len(contents) <= limit:
        raise ValueError("bounded nonempty Git object evidence required")
    if path.exists() or path.is_symlink():
        if INPUTS["read_data"](path, limit) != contents:
            raise ValueError("shared Git object evidence must retain identical original bytes")
        return
    with path.open("xb") as stream:
        stream.write(contents)


def decode_blob(raw, oid):
    if not 0 < len(raw) <= MAX_SOURCE:
        raise ValueError("bounded nonempty original Git blob required")
    identity = INPUTS["TREE"]["check_blob"](oid, raw)
    raw.decode("utf-8", errors="strict")
    return raw, identity


def fetched_git_requests(source, output, selected, report):
    if not any(selected == requests(profile) for profile in GIT_PROFILES):
        raise ValueError("only compiled complete source inventories allowed for Git transport")
    tree = INPUTS["TREE"]
    cache_name, (_, repository, revision), cache_tree = git_scope(selected)
    cache = source / cache_name
    tree["regular_parents"](cache)
    if not cache.is_dir() or cache.resolve(strict=True) != cache:
        raise ValueError("canonical original task-owned bare cache required")
    if (tree["git"](cache, ["rev-parse", "--is-bare-repository"], 64) != b"true\n"
            or tree["git"](cache, ["rev-parse", revision], 64).decode().strip() != revision
            or tree["git"](cache, ["rev-parse", revision + "^{tree}"], 64).decode().strip() != cache_tree):
        raise ValueError("original same-commit cache/tree required")
    report.update(gitCacheSource=cache_name, gitCacheRevision=revision, gitCacheRootTreeSha1=cache_tree,
                  gitObjectBatches=[], gitFetches=[], gitilesRequestsPerformed=False)
    local = tree["git"](cache, ["cat-file", "--batch-all-objects", "--unordered", "--batch-check=%(objectname) %(objecttype)"])
    available = {line[:40].decode("ascii") for line in local.splitlines() if line[41:] == b"blob"}
    objects = sorted({row["gitObjectSha1"] for row in selected})
    missing = [oid for oid in objects if oid not in available]
    for start in range(0, len(missing), 64):
        batch = missing[start:start + 64]
        fetch = {"repository": repository, "objects": batch, "succeeded": False}
        report["gitFetches"].append(fetch)
        tree["git"](cache, ["fetch", "--no-tags", "--no-write-fetch-head", "--filter=blob:none", repository, *batch])
        fetch["succeeded"] = True
    payloads, total = {}, 0
    for start in range(0, len(objects), 64):
        batch = objects[start:start + 64]
        raw = tree["git"](cache, ["cat-file", "--batch"], MAX_TOTAL + 8192, ("\n".join(batch) + "\n").encode())
        filename = "git-object-batch-" + format(start // 64, "02d") + ".raw-data"
        save_blob(output / filename, raw, MAX_TOTAL + 8192)
        record = {"objects": batch, "rawFile": filename, "rawBytes": len(raw),
                  "rawSha256": hashlib.sha256(raw).hexdigest(), "verified": False}
        report["gitObjectBatches"].append(record)
        contents = tree["batch_payloads"](raw, batch)
        for oid, data in contents.items():
            decode_blob(data, oid)
            total += len(data)
            if total > MAX_TOTAL:
                raise ValueError("bounded complete source Git object total required")
        payloads.update(contents)
        record["verified"] = True
    for request in selected:
        yield request, payloads[request["gitObjectSha1"]]


def checked_git_batches(root, report, selected, blobs):
    objects = sorted({row["gitObjectSha1"] for row in selected})
    batches = report.get("gitObjectBatches", [])
    cache_name, (_, _, revision), cache_tree = git_scope(selected)
    if (report.get("gitCacheSource") != cache_name or report.get("gitCacheRevision") != revision
            or report.get("gitCacheRootTreeSha1") != cache_tree or report.get("gitilesRequestsPerformed") is not False
            or len(batches) != (len(objects) + 63) // 64):
        raise ValueError("exact completed original Git object transport identity required")
    for index, record in enumerate(batches):
        expected = objects[index * 64:(index + 1) * 64]
        filename = "git-object-batch-" + format(index, "02d") + ".raw-data"
        if record.get("objects") != expected or record.get("rawFile") != filename or record.get("verified") is not True:
            raise ValueError("exact complete ordered original Git object batches required")
        raw = INPUTS["read_data"](root / filename, MAX_TOTAL + 8192)
        if (len(raw), hashlib.sha256(raw).hexdigest()) != (record.get("rawBytes"), record.get("rawSha256")):
            raise ValueError("original Git cat-file output changed")
        data = INPUTS["TREE"]["batch_payloads"](raw, expected)
        for request in selected:
            if request["gitObjectSha1"] in data and data[request["gitObjectSha1"]] != blobs[request["sourcePath"]]:
                raise ValueError("Git cat-file original bytes and source-path records differ")
    return True


def collect(source, output, profile="integration", git_objects=False):
    if type(git_objects) is not bool or (git_objects and profile not in GIT_PROFILES):
        raise ValueError("explicit Git object transport allowed only for complete compiled source inventories")
    selected = requests(profile)
    prefix = {"integration": "integration-source", "embedding": "embedding-source", "targets": "integration-target-source",
              "consumers": "consumer-source", "platform": "platform-header-source", "cpu-consumer": "cpu-consumer-source",
              "cpu-tests": "cpu-test-source", "angle-gn": "angle-gn-source", "angle-headers": "angle-header-source",
              "swiftshader-gn": "swiftshader-gn-source", "swiftshader-consumers": "swiftshader-consumer-source",
              "swiftshader-copied": "swiftshader-copied-source",
              "skia-gn": "skia-gn-source", "skia-embedding": "skia-embedding-source",
              "dawn-gn": "dawn-gn-source", "dawn-embedding": "dawn-embedding-source",
              "dawn-support": "dawn-support-source", "angle-registry": "angle-registry-source",
              "angle-generation": "angle-generation-source", "angle-generated": "angle-generated-source"}[profile]
    if git_objects:
        prefix = {"angle-gn": "angle-gn-git-source", "angle-headers": "angle-header-git-source",
                  "swiftshader-gn": "swiftshader-gn-git-source", "swiftshader-consumers": "swiftshader-consumer-git-source",
                  "swiftshader-copied": "swiftshader-copied-git-source", "skia-gn": "skia-gn-git-source",
                  "dawn-gn": "dawn-gn-git-source", "angle-registry": "angle-registry-git-source",
                  "angle-generation": "angle-generation-git-source", "angle-generated": "angle-generated-git-source"}[profile]
    if output.parent != source or not re.fullmatch(prefix + r"-[0-9]{2}", output.name):
        raise ValueError("safe fresh task-owned integration source output required")
    if output.exists() or output.is_symlink():
        raise FileExistsError("never overwrite integration source evidence")
    inputs, checkout_sha, correspondence = checked_inputs(source, profile)
    output.mkdir()
    report = {"scope": "fixed integration source and original manifest correspondence, not effective GN or binary approval",
              "sourceCommit": COMMIT, "mainReceiptSha256": inputs["mainSha256"], "checkoutReceiptSha256": checkout_sha,
              "sourceRoots": source_roots(profile), "checkoutCorrespondence": correspondence, "requests": selected, "records": [],
              "collectionComplete": False, "sourceFunctionsExecuted": False, "conditionsEvaluated": False,
              "generatorExecuted": False, "effectiveGnMeasured": False, "shippingDecisionsMade": False,
              "metadataPathsAliased": False, "licenseCoverage": "not_proven", "promoted": False}
    if git_objects:
        report["gitObjectTransport"] = True
    total = 0
    try:
        fetched = fetched_git_requests(source, output, selected, report) if git_objects else fetched_requests(selected, parallel=profile in ("angle-gn", "skia-embedding"))
        for request, raw in fetched:
            oid = request["gitObjectSha1"]
            raw_name = oid + (".git-blob-data" if git_objects else ".gitiles-data")
            save_blob(output / raw_name, raw, MAX_SOURCE if git_objects else MAX_RAW)
            row = {**request, "rawFile": raw_name, "rawBytes": len(raw), "rawSha256": hashlib.sha256(raw).hexdigest(), "verified": False}
            report["records"].append(row)
            data, identity = decode_blob(raw, oid) if git_objects else decode(raw, oid)
            total += len(data)
            if total > MAX_TOTAL:
                raise ValueError("selected integration source total byte budget")
            source_name = oid + ".source-data"
            save_blob(output / source_name, data, MAX_SOURCE)
            row.update(sourceFile=source_name, verified=True, **identity)
            print(json.dumps({"path": request["sourcePath"], "bytes": len(data), "verified": True}), flush=True)
        report["collectionComplete"] = True
    except Exception as error:
        report["failureType"] = type(error).__name__
        raise
    finally:
        report["selectedSourceBytes"] = total
        EXTERNAL["write_report"](output / "integration-source-inventory.json", report)
    return report


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--output-name", required=True)
    parser.add_argument("--profile", choices=tuple(PROFILES), default="integration")
    parser.add_argument("--git-objects", action="store_true", help="read a complete compiled source inventory from same-commit bare Git objects")
    args = parser.parse_args()
    source = args.source.absolute()
    report = collect(source, source / args.output_name, args.profile, args.git_objects)
    print(json.dumps({"sources": len(report["records"]), "bytes": report["selectedSourceBytes"],
                      "collectionComplete": report["collectionComplete"], "promoted": False}))
