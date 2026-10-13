"""Verify real C++ source paths and copied Khronos header attributions.

Keep standalone metadata gaps and original platform records. Source rules and
header text do not prove the effective graph, generated registry provenance,
binary components, full NOTICE or runtime delivery.
"""
import argparse
import hashlib
import json
from pathlib import Path
import re
import runpy


HERE = Path(__file__).parent.absolute()
INTEGRATION = runpy.run_path(str(HERE / "inspect-chromium-integration-coverage.py"))
INPUTS, SOURCES, CRASHPAD, NODE = (INTEGRATION[key] for key in ("INPUTS", "SOURCES", "CRASHPAD", "NODE"))
BASELINE_SHA = "910d738a12ee5a8fbf0a9e73e68329b397c8ef528d579bdf86ae4b21de62f8da"
PROFILE_SEALS = {
    "consumers": ("consumer-source-01", "861b91404cabff20baac863fae621dbe52a03c571b1440eb222facb5d41a9ee1"),
    "platform": ("platform-header-source-01", "6a6b1b7df2dd85bafa6dee5fe4187f66039c0de51f1eb5405c17efa3168f3f76"),
}
ANGLE = INTEGRATION["ANGLE"]
LIBRARIES = {
    key: INPUTS["EXTERNAL"]["SUPPORT_PINS"][key] for key in ("libcxx", "libcxxabi", "llvm-libc")
}
HEADERS = {
    "include/EGL/egl.h": ("Apache-2.0", "2013-2020"),
    "include/EGL/eglext.h": ("Apache-2.0", "2013-2020"),
    "include/EGL/eglplatform.h": ("Apache-2.0", "2007-2020"),
    "include/GLES2/gl2.h": ("MIT", "2013-2026"),
    "include/GLES3/gl3.h": ("MIT", "2013-2026"),
    "include/GLES2/gl2platform.h": ("Apache-2.0", "2017-2020"),
    "include/GLES3/gl3platform.h": ("Apache-2.0", "2017-2020"),
    "include/KHR/khrplatform.h": (None, "2008-2018"),
}


def recheck_baseline(source, report):
    path = source / "integration-coverage-04.json"
    original, sha = INPUTS["receipt"](path, BASELINE_SHA)
    raw = INPUTS["read_data"](path, INPUTS["TREE"]["MAX_BLOB"])
    serialized = (json.dumps(report, indent=2) + "\n").encode()
    # Match either original writer convention; the sealed raw receipt is never rewritten.
    if original != report or raw not in (serialized, serialized.replace(b"\n", b"\r\n")):
        raise ValueError("independent original integration audit must reproduce exactly")
    return sha


def libraries(source, inputs):
    raw = INPUTS["read_data"](source / INPUTS["EXTERNAL"]["CATALOG"], INPUTS["TREE"]["MAX_BLOB"])
    catalog = INPUTS["EXTERNAL"]["checked_catalog"](raw)
    checkout = SOURCES["CHECKOUTS"]["inspect"](source)
    targets = checkout["targets"]
    if len(targets) != 4 or {row["target"] for row in targets} != set(CRASHPAD["ARGS"]["PLATFORMS"]):
        raise ValueError("all four original C++ checkout targets required")
    result = []
    for key, (prefix, repository, revision) in LIBRARIES.items():
        expected = {"path": prefix, "repository": repository, "revision": revision, "condition": None,
                    "gitlinkRevision": revision, "gitlinkMatched": True}
        declarations = [row for row in catalog["gitDependencies"] if row["path"] == prefix]
        entry = inputs["index"].get(prefix)
        if declarations != [expected] or entry is None or (entry["mode"], entry["gitObjectSha1"]) != ("160000", revision):
            raise ValueError("exact original C++ DEPS/Gitlink source required")
        correspondence = []
        for target in targets:
            row = SOURCES["CHECKOUTS"]["compare_pin"]("src/" + prefix, repository, revision, target["manifest"])
            if row["status"] != "exact_repository_and_revision_match":
                raise ValueError("C++ source must match all four actual manifests")
            correspondence.append({"target": target["target"], **row})
        path = prefix + "/LICENSE.TXT"
        license_file = inputs["files"].get(path)
        indexed = inputs["index"].get(path)
        if (license_file is None or indexed is None or indexed["mode"] != "100644"
                or indexed["gitObjectSha1"] != license_file["gitObjectSha1"]):
            raise ValueError("actual regular C++ license at its real verified source path required")
        identity = INPUTS["TREE"]["check_blob"](license_file["gitObjectSha1"], license_file["payload"])
        if identity != {field: license_file[field] for field in ("bytes", "sha256")}:
            raise ValueError("actual C++ license byte/digest identity required")
        result.append({"component": key, "checkoutPath": prefix, "repository": repository, "revision": revision,
                       "licensePath": path, "licenseGitObjectSha1": license_file["gitObjectSha1"],
                       "licenseSha256": license_file["sha256"], "licenseBytes": license_file["bytes"],
                       "checkoutCorrespondence": correspondence, "metadataPathsAliased": False, "binaryUsageProven": False})
    return result


def cpp_configuration(blobs, targets):
    cpp = "build/config/c++/c++.gni"
    rules = [
        (cpp, r'^import\("//build_overrides/build.gni"\)$'),
        (cpp, r'^  use_custom_libcxx = true$'),
        (cpp, r'^  use_custom_libcxx_for_host = true$'),
        (cpp, r'^libcxx_prefix = "//third_party/libc\+\+/src"$'),
        (cpp, r'^libcxxabi_prefix = "//third_party/libc\+\+abi/src"$'),
        ("buildtools/third_party/libc++/BUILD.gn", r'^    "//third_party/libc\+\+/src/src/algorithm.cpp",$'),
        ("buildtools/third_party/libc++/BUILD.gn", r'^  deps = \[ "//third_party/llvm-libc:llvm-libc-shared" \]$'),
        ("buildtools/third_party/libc++abi/BUILD.gn", r'^import\("//build/config/c\+\+/c\+\+.gni"\)$'),
        ("buildtools/third_party/libc++abi/BUILD.gn", r'^    "//third_party/libc\+\+abi/src/src/abort_message.cpp",$'),
        ("third_party/llvm-libc/BUILD.gn", r'^  include_dirs = \[ "src" \]$'),
        ("third_party/llvm-libc/BUILD.gn", r'^  visibility = \[ "//buildtools/third_party/libc\+\+" \]$'),
    ]
    evidence = [CRASHPAD["excerpt"](blobs, path, pattern) for path, pattern in rules]
    guarded = {"use_custom_libcxx", "use_custom_libcxx_for_host", "libcxx_prefix", "libcxxabi_prefix"}
    if len(targets) != 4 or {row["target"] for row in targets} != set(CRASHPAD["ARGS"]["PLATFORMS"]):
        raise ValueError("all four original C++ build targets required")
    for row in targets:
        if guarded.intersection(row["gnArgsWritten"]):
            raise ValueError("original C++ GN override needs separate source review")
        if row.get("effectiveGnMeasured") is not False or row.get("remoteGeneratedFileMeasured") is not False:
            raise ValueError("source-only original C++ records required")
    return {"rules": evidence, "originalTargets": targets, "sourceLevelSourceRoots": [pin[0] for pin in LIBRARIES.values()],
            "standaloneDirectoriesRewritten": False, "effectiveGnMeasured": False, "binaryUsageProven": False}


def attribution(data, path, specification):
    spdx, years = specification
    text = data.decode("utf-8", errors="strict")
    blocks = [match for match in re.finditer(r"/\*[\s\S]*?\*/", text)
              if "The Khronos Group Inc." in match.group() and ("SPDX-License-Identifier:" in match.group() or "Permission is hereby granted" in match.group())]
    if len(blocks) != 1:
        raise ValueError("one exact original Khronos attribution block required")
    match = blocks[0]
    block = match.group()
    copyright_lines = re.findall(r"^\*\* Copyright (?:\(c\) )?([0-9]+(?:-[0-9]+)?) The Khronos Group Inc\.$", block, re.MULTILINE)
    identifiers = re.findall(r"^\*\* SPDX-License-Identifier: ([A-Za-z0-9.\-]+)$", block, re.MULTILINE)
    if copyright_lines != [years] or identifiers != ([spdx] if spdx is not None else []):
        raise ValueError("original per-header license identifier and copyright years required")
    if spdx is None:
        required = ("Permission is hereby granted, free of charge", "distribute, sublicense, and/or sell copies of the Materials",
                    "The above copyright notice and this permission notice shall be included",
                    'THE MATERIALS ARE PROVIDED "AS IS"', "INFRINGEMENT.", "MATERIALS OR THE USE OR OTHER DEALINGS IN THE MATERIALS.")
        if not all(value in block for value in required):
            raise ValueError("complete original inline Khronos permission/disclaimer required")
    includes = re.findall(r"^#include <((?:EGL|GLES[23]|KHR)/[A-Za-z0-9_.]+)>$", text, re.MULTILINE)
    return {"sourcePath": path, "sourceSha256": hashlib.sha256(data).hexdigest(), "sourceBytes": len(data),
            "line": text[:match.start()].count("\n") + 1, "copyrightYears": years, "declaredSpdx": spdx,
            "originalAttributionText": block, "attributionSha256": hashlib.sha256(block.encode()).hexdigest(),
            "fullLicenseTextInBlock": spdx is None, "briefSpdxHeaderNeedsFullLicense": spdx is not None,
            "includes": includes, "registryGeneratorExecuted": False, "originalRegistryCommitReproduced": False,
            "binaryUsageProven": False}


def copied_headers(blobs):
    rows = []
    for path, specification in HEADERS.items():
        source_path = ANGLE + path
        if source_path not in blobs:
            raise ValueError("all exact reviewed copied Khronos header files required")
        row = attribution(blobs[source_path], source_path, specification)
        for include in row["includes"]:
            dependency = "include/" + include
            if dependency not in HEADERS or ANGLE + dependency not in blobs:
                raise ValueError("referenced reviewed platform header must exist at its real source path")
        rows.append(row)
    return {"headers": rows, "allReviewedHeaderIncludesCollected": True,
            "allKhronosFilesReviewed": False, "originalRegistryCheckoutProven": False,
            "registryMetadataPathsAliased": False, "fullNoticeComplete": False}


def load_consumer(source):
    inputs, baseline = INTEGRATION["load_integration"](source)
    baseline_sha = recheck_baseline(source, baseline)
    _, checkout_sha, correspondence = SOURCES["checked_inputs"](source)
    blobs, seals = {}, {}
    for profile in PROFILE_SEALS:
        added, seals[profile] = INTEGRATION["checked_profile"](source, profile, inputs, checkout_sha, correspondence, profiles=PROFILE_SEALS)
        blobs.update(added)
    config = cpp_configuration(blobs, baseline["configurationEvidence"]["targets"])
    notices = copied_headers(blobs)
    licensed_libraries = libraries(source, inputs)
    INTEGRATION["append_blobs"](inputs, blobs)
    after = NODE["summarize"](inputs)
    report = {"scope": "exact C++ source paths, copied Khronos attributions and raw/default-policy audit only, not full NOTICE/binary approval",
              "sourceCommit": INPUTS["TREE"]["COMMIT"], "mainReceiptSha256": inputs["mainSha256"],
              "originalIntegrationReceiptSha256": baseline_sha, "consumerSourceSeals": seals, "checkoutReceiptSha256": checkout_sha,
              "cppConfigurationEvidence": config, "licensedCppSources": licensed_libraries, "copiedKhronosHeaders": notices,
              "before": baseline["after"], "after": after,
              "embeddedMetadataErrorsRetained": [row for row in after["shippingMetadataEntriesWithErrors"]
                  if row["directory"].startswith((ANGLE, INTEGRATION["SCREEN"]))],
              "originalRawGapsExcluded": False, "sourceFunctionsExecuted": False, "conditionsEvaluated": False,
              "generatorExecuted": False, "metadataPathsAliased": False, "shippingDecisionsMade": False,
              "effectiveGnMeasured": False, "binaryComponentCoverage": "not_proven", "licenseCoverage": "not_proven", "promoted": False}
    return inputs, report


def inspect(source, output):
    if output.parent != source or not re.fullmatch(r"consumer-coverage-[0-9]{2}\.json", output.name):
        raise ValueError("safe fresh consumer coverage output required")
    if output.exists() or output.is_symlink():
        raise FileExistsError("never overwrite consumer coverage evidence")
    _, report = load_consumer(source)
    INPUTS["EXTERNAL"]["write_report"](output, report)
    return report


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--output-name", required=True)
    args = parser.parse_args()
    source = args.source.absolute()
    report = inspect(source, source / args.output_name)
    print(json.dumps({"selectedPaths": report["after"]["verifiedSelectedFiles"], "rawGaps": len(report["after"]["unresolvedRawReferences"]),
                      "yesErrors": len(report["after"]["shippingMetadataEntriesWithErrors"]), "directoryErrors": len(report["after"]["directoryErrors"]),
                      "headers": len(report["copiedKhronosHeaders"]["headers"]), "cppLibraries": len(report["licensedCppSources"]), "promoted": False}))
