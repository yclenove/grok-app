"""Verify SwiftShader MD5 and copied libbacktrace notices with source provenance.

Preserve the original incomplete audit. Literal CMake debug branches, download
pins, submodules and absence from GN are not binary shipping decisions.
"""
import argparse
import configparser
import hashlib
import json
from pathlib import Path
import re
import runpy


HERE = Path(__file__).parent.absolute()
PREVIOUS = runpy.run_path(str(HERE / "inspect-chromium-swiftshader-notices.py"))
INPUTS, INTEGRATION, SOURCES, ROOT = (PREVIOUS[key] for key in ("INPUTS", "INTEGRATION", "SOURCES", "ROOT"))
PROFILE = {"swiftshader-copied": ("swiftshader-copied-git-source-01", "b9b72145db43d7b139f327321f578c70b456d3eee8ee3c62acf47856dc549e58")}
PREVIOUS_SEAL = "2bf2c81272f462813e9740a28f616c6cecd57f939ea1620a45a3eee9cd3634a8"
LIBBACKTRACE_REVISION = "5a99ff7fed66b8ea8f09c9805c138524a7035ece"
MAX_FRAGMENT = 64 * 1024


def inline_notice(data, path):
    text = data.decode("utf-8", errors="strict")
    match = re.match(r"/\*[\s\S]*?\*/", text)
    if match is None:
        raise ValueError("one complete leading original inline notice required")
    block = match.group()
    if path.endswith(("MD5.h", "MD5.cpp")):
        required = ("Copyright (c) 2001 Alexander Peslyak", "No copyright is",
                    "public domain is deemed null and void", "Redistribution and use in source and binary forms",
                    "modification, are permitted.", "There's ABSOLUTELY NO WARRANTY, express or implied.")
        kind = "original_md5_public_domain_and_fallback_permission"
    elif path.endswith("backtrace-supported.h"):
        required = ("Copyright (C) 2012-2016 Free Software Foundation, Inc.", "Written by Ian Lance Taylor, Google.",
                    "(1) Redistributions of source code", "(2) Redistributions in binary form",
                    "(3) The name of the author", "THIS SOFTWARE IS PROVIDED BY THE AUTHOR",
                    "POSSIBILITY OF SUCH DAMAGE.")
        kind = "copied_libbacktrace_complete_inline_permission"
    else:
        raise ValueError("only compiled MD5 and copied backtrace notice paths allowed")
    if not all(value in block for value in required):
        raise ValueError("complete original special attribution, permission and disclaimer required")
    return {"sourcePath": path, "sourceBytes": len(data), "sourceSha256": hashlib.sha256(data).hexdigest(),
            "originalAttributionText": block, "attributionSha256": hashlib.sha256(block.encode()).hexdigest(),
            "noticeKind": kind, "fullPermissionIsInline": True, "wholeComponentNoticeCovered": False}


def library_sources(blobs, index, gn):
    parser = configparser.ConfigParser(interpolation=None, strict=True)
    parser.read_string(blobs[ROOT + ".gitmodules"].decode())
    section = 'submodule "third_party/libbacktrace/src"'
    if (parser.get(section, "path") != "third_party/libbacktrace/src"
            or parser.get(section, "url") != "https://github.com/ianlancetaylor/libbacktrace.git"):
        raise ValueError("original exact libbacktrace submodule source declaration required")
    entry = index.get(ROOT + "third_party/libbacktrace/src")
    if entry is None or (entry["mode"], entry["kind"], entry["gitObjectSha1"]) != ("160000", "commit", LIBBACKTRACE_REVISION):
        raise ValueError("original libbacktrace gitlink commit required, not regular source bytes")
    rules = [
        ("CMakeLists.txt", r'^option_if_not_defined\(REACTOR_EMIT_DEBUG_INFO "Emit debug info for JIT functions" FALSE\)$'),
        ("CMakeLists.txt", r'^if\(REACTOR_EMIT_DEBUG_INFO\)\n    InitSubmodule\(libbacktrace \$\{THIRD_PARTY_DIR\}/libbacktrace/src\)\nendif\(\)$'),
        ("CMakeLists.txt", r'^if\(REACTOR_EMIT_DEBUG_INFO\)\n    add_subdirectory\(\$\{THIRD_PARTY_DIR\}/libbacktrace EXCLUDE_FROM_ALL\)\n    add_subdirectory\(\$\{THIRD_PARTY_DIR\}/boost EXCLUDE_FROM_ALL\)\nendif\(\)$'),
        ("third_party/boost/CMakeLists.txt", r'^set\(BOOST_VER 1\.78\.0\)$'),
        ("third_party/boost/CMakeLists.txt", r'^set\(BOOST_HASH_TARGZ "94ced8b72956591c4775ae2207a9763d3600b30d9d7446562c552f0a14a63be7"\)$'),
        ("third_party/boost/CMakeLists.txt", r'^set\(BOOST_HASH_ZIP   "f22143b5528e081123c3c5ed437e92f648fe69748e95fa6e2bd41484e2986cc3"\)$'),
        ("third_party/boost/CMakeLists.txt", r'^            file\(DOWNLOAD \$\{BOOST_URL\} \$\{BOOST_ARCHIVE\} EXPECTED_HASH SHA256=\$\{BOOST_HASH\}\)$'),
        ("third_party/libbacktrace/CMakeLists.txt", r'^elseif\(APPLE\)\n    message\(FATAL_ERROR "libbacktrace does not support mach-o yet"\)$'),
    ]
    evidence = [INTEGRATION["CRASHPAD"]["excerpt"](blobs, ROOT + path, pattern) for path, pattern in rules]
    references = [{"sourcePath": path, "line": number, "text": line}
                  for path, data in sorted(gn.items()) for number, line in enumerate(data.decode().splitlines(), 1)
                  if re.search(r"boost|libbacktrace|REACTOR_EMIT_DEBUG_INFO", line, flags=re.IGNORECASE)]
    config_files = [path for path in blobs if path.startswith(ROOT + "third_party/libbacktrace/config/")]
    expected = {ROOT + "third_party/libbacktrace/config/" + platform + "/include/" + name
                for platform in ("linux", "darwin") for name in ("config.h", "backtrace-supported.h")}
    actual = {path for path, row in index.items() if path.startswith(ROOT + "third_party/libbacktrace/config/")
              and row["mode"] in ("100644", "100755")}
    if set(config_files) != expected or actual != expected:
        raise ValueError("complete original copied backtrace configuration tree required")
    return {"sourceRules": evidence, "gnReferences": references, "completeCopiedBacktraceConfigInventory": True,
        "libbacktraceSource": {"path": ROOT + "third_party/libbacktrace/src", "repository": parser.get(section, "url"),
            "revision": LIBBACKTRACE_REVISION, "treeKind": "gitlink", "submoduleSourceCollected": False},
        "boostSource": {"version": "1.78.0", "downloadedByCmakeIfNeeded": True,
            "zipSha256": "f22143b5528e081123c3c5ed437e92f648fe69748e95fa6e2bd41484e2986cc3",
            "tarGzipSha256": "94ced8b72956591c4775ae2207a9763d3600b30d9d7446562c552f0a14a63be7",
            "originalArchiveCollected": False},
        "cmakeExecuted": False, "effectiveGnMeasured": False, "binaryExclusionsProven": False,
        "gnReferenceAbsenceMeansNotShipped": False}


def render_fragment(notices):
    parts = [b"PARTIAL SWIFTSHADER SPECIAL INLINE NOTICES - NOT FOR DISTRIBUTION\n"
             b"Scope: exact source blocks below, not complete Chromium NOTICE or binary SBOM.\n"]
    for row in notices:
        block = row["originalAttributionText"].encode()
        if hashlib.sha256(block).hexdigest() != row["attributionSha256"] or not row["fullPermissionIsInline"]:
            raise ValueError("unchanged complete original inline attribution required")
        parts.append(("\nSource: " + row["sourcePath"] + "\n").encode() + block + b"\n")
    fragment = b"\n".join(parts)
    if len(fragment) > MAX_FRAGMENT:
        raise ValueError("bounded copied inline notice supplement required")
    return fragment


def inspect(source, output):
    INPUTS["checked_source"](source)
    if output.parent != source or not re.fullmatch(r"swiftshader-copied-notices-[0-9]{2}", output.name):
        raise ValueError("safe fresh task-owned copied notice output required")
    if output.exists() or output.is_symlink():
        raise FileExistsError("never overwrite copied notice evidence")
    inputs, checkout_sha, correspondence = SOURCES["checked_inputs"](source, "swiftshader-copied")
    blobs, sha = INTEGRATION["checked_profile"](source, "swiftshader-copied", inputs, checkout_sha, correspondence, profiles=PROFILE, git_objects=True)
    original, original_sha = INPUTS["receipt"](source / "swiftshader-notices-02/swiftshader-notice-inventory.json", PREVIOUS_SEAL)
    previous_text = INPUTS["read_data"](source / "swiftshader-notices-02/swiftshader-copied-notices.txt", PREVIOUS["MAX_FRAGMENT"])
    if (len(previous_text), hashlib.sha256(previous_text).hexdigest()) != (original["fragmentBytes"], original["fragmentSha256"]):
        raise ValueError("unchanged complete original SwiftShader supplement required")
    for row in original["originalSourceNoticeBindings"]:
        if PREVIOUS["SUPPLEMENT"]["source_license"](inputs, row["sourcePath"]) != row:
            raise ValueError("all original source-notice bindings must still reproduce")
    gn, gn_sha = INTEGRATION["checked_profile"](source, "swiftshader-gn", inputs, checkout_sha, correspondence,
                                               profiles=PREVIOUS["PROFILES"], git_objects=True)
    if PREVIOUS["gn_paths"](inputs["index"]) != {row["sourcePath"]: row["gitObjectSha1"] for row in SOURCES["requests"]("swiftshader-gn")}:
        raise ValueError("entire original GN tree required before reporting source references")
    notices = [inline_notice(data, path) for path, data in sorted(blobs.items()) if path.endswith(("MD5.h", "MD5.cpp", "backtrace-supported.h"))]
    if len(notices) != 7:
        raise ValueError("five MD5 and two copied backtrace original notices required")
    libraries = library_sources(blobs, inputs["index"], gn)
    fragment = render_fragment(notices)
    report = {"scope": "special inline copied-source notices and inert library provenance, not shipping approval",
        "sourceCommit": INPUTS["TREE"]["COMMIT"], "originalSwiftShaderNoticeReceiptSha256": original_sha,
        "copiedSourceReceiptSha256": sha, "gnSourceReceiptSha256": gn_sha, "checkoutCorrespondence": correspondence,
        "selectedSourcePaths": len(blobs), "selectedSourceBytes": sum(map(len, blobs.values())), "inlineNotices": notices,
        "librarySourceEvidence": libraries, "originalDirectoryErrorDispositionsRetained": original["originalDirectoryErrorDispositions"],
        "originalRawErrorsRetained": original["originalRawErrorsRetained"],
        "remainingObligations": ["Boost original download and libbacktrace gitlink checkout/shipping provenance",
            "Other copied/generated source attributions and original ANGLE obligations",
            "Complete four-platform NOTICE/SBOM and source-offer/media/security policy"],
        "fragmentFile": "swiftshader-special-inline-notices.txt", "fragmentBytes": len(fragment), "fragmentSha256": hashlib.sha256(fragment).hexdigest(),
        "sourceFunctionsExecuted": False, "cmakeExecuted": False, "effectiveGnMeasured": False,
        "shippingExclusionsProven": False, "completeChromiumNotice": False, "promoted": False}
    output.mkdir()
    with (output / report["fragmentFile"]).open("xb") as stream:
        stream.write(fragment)
    INPUTS["EXTERNAL"]["write_report"](output / "swiftshader-copied-inventory.json", report)
    return report


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--output-name", required=True)
    args = parser.parse_args()
    source = args.source.absolute()
    report = inspect(source, source / args.output_name)
    print(json.dumps({key: report[key] for key in ("selectedSourcePaths", "selectedSourceBytes", "fragmentBytes", "fragmentSha256", "completeChromiumNotice")}))
