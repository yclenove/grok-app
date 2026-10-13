"""Bind copied SwiftShader notices to real sources and complete standard text.

All original metadata errors and unresolved source obligations remain visible.
GN declarations and sampled C++ sources do not prove effective build or binary
coverage. This supplement must not be used as a complete shipping NOTICE.
"""
import argparse
import hashlib
import json
from pathlib import Path
import re
import runpy


HERE = Path(__file__).parent.absolute()
SUPPLEMENT = runpy.run_path(str(HERE / "inspect-chromium-notice-supplements.py"))
LLVM = runpy.run_path(str(HERE / "collect-llvm-notice-texts.py"))
CANDIDATES, INPUTS, INTEGRATION = (SUPPLEMENT[key] for key in ("CANDIDATES", "INPUTS", "INTEGRATION"))
SOURCES = INTEGRATION["SOURCES"]
ROOT = "third_party/swiftshader/"
PROFILES = {
    "swiftshader-gn": ("swiftshader-gn-git-source-01", "2596577a1b9de3fd996fd4cf4657ad5829979a0f9011434a577e29119d427db8"),
    "swiftshader-consumers": ("swiftshader-consumer-git-source-01", "182e3ed534dc20cb3ce76a100450b913988f55fd8cadac29e002473d874c8717"),
}
LLVM_SEAL = "5f47933a24b7f439c6967851a954cb01d1ca611579bcf6e5b2c1d89785fb5f15"
LICENSE_PATHS = (
    "third_party/astc-encoder/LICENSE.txt",
    "third_party/llvm-10.0/llvm/include/llvm/Support/LICENSE.TXT",
    "third_party/llvm-10.0/llvm/lib/Support/COPYRIGHT.regex",
    "third_party/llvm-16.0/llvm/include/llvm/Support/LICENSE.TXT",
    "third_party/llvm-16.0/llvm/lib/Support/BLAKE3/LICENSE",
    "third_party/llvm-16.0/llvm/lib/Support/COPYRIGHT.regex",
    "third_party/llvm-subzero/LICENSE.TXT",
    "third_party/marl/LICENSE",
    "third_party/subzero/LICENSE.TXT",
    "third_party/subzero/bloat/COPYING",
    "third_party/subzero/bloat/LICENSE.txt",
)
ERROR_DIRECTORIES = {ROOT + "third_party/" + name for name in
    ("astc-encoder", "boost", "libbacktrace", "llvm-10.0", "llvm-16.0", "llvm-subzero", "marl", "subzero")}
MAX_FRAGMENT = 256 * 1024


def gn_paths(index):
    return {path: row["gitObjectSha1"] for path, row in index.items()
            if path.startswith(ROOT) and path.endswith((".gn", ".gni")) and row["mode"] in ("100644", "100755")}


def leading_notice(data, path):
    text = data.decode("utf-8", errors="strict")
    match = re.match(r"(?://[^\n]*\n)+", text)
    if match is None:
        raise ValueError("complete original leading C++ notice required")
    block = match.group()
    if "/llvm/ADT/StringRef.h" in path:
        if (block.count("SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception") != 1
                or "Part of the LLVM Project" not in block or "See https://llvm.org/LICENSE.txt" not in block):
            raise ValueError("exact original LLVM Apache-plus-exception declaration required")
        licenses = ["Apache-2.0", "LLVM-exception"]
    else:
        if not all(value in block for value in ("The SwiftShader Authors. All Rights Reserved.",
                "Licensed under the Apache License, Version 2.0", "http://www.apache.org/licenses/LICENSE-2.0",
                'distributed under the License is distributed on an "AS IS" BASIS',
                "WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND", "limitations under the License.")):
            raise ValueError("complete original SwiftShader Apache notice required")
        licenses = ["Apache-2.0"]
    return {"sourcePath": path, "sourceBytes": len(data), "sourceSha256": hashlib.sha256(data).hexdigest(),
            "originalAttributionText": block, "attributionSha256": hashlib.sha256(block.encode()).hexdigest(),
            "fullLicenseIds": licenses, "wholeCopiedTreeCovered": False}


def configuration(blobs):
    rules = [
        ("src/swiftshader.gni", r"^  swiftshader_enable_astc = true$"),
        ("src/Device/BUILD.gn", r'^  if \(swiftshader_enable_astc\) \{[\s\S]*?    deps \+= \[ ":astc_encoder" \]\n    include_dirs \+= \[ "../../third_party/astc-encoder/Source" \]\n  \}'),
        ("src/Reactor/BUILD.gn", r'^    llvm_dir = "../../third_party/llvm-10.0"$'),
        ("src/Reactor/BUILD.gn", r'^    subzero_dir = "../../third_party/subzero"\n    subzero_llvm_dir = "../../third_party/llvm-subzero"$'),
        ("src/Reactor/reactor.gni", r'^  supports_subzero = current_cpu != "arm64"[^\n]*$'),
        ("src/Reactor/reactor.gni", r'^  use_swiftshader_with_subzero = supports_subzero && !is_msan$'),
        ("src/Reactor/reactor.gni", r'^    \|\| \(is_mac && !supports_subzero\)$'),
    ]
    evidence = [INTEGRATION["CRASHPAD"]["excerpt"](blobs, ROOT + path, pattern) for path, pattern in rules]
    references = []
    for path, data in sorted(blobs.items()):
        for line, value in enumerate(data.decode().splitlines(), 1):
            if re.search(r"third_party/(?:marl|boost|libbacktrace)|marl/", value):
                references.append({"sourcePath": path, "line": line, "text": value})
    return {"rules": evidence, "copiedLibraryReferences": references,
            "configuredLlvmDirectory": ROOT + "third_party/llvm-10.0",
            "llvm16DeclaredAsActiveReactor": False, "effectiveGnMeasured": False,
            "binaryExclusionsProven": False, "conditionsEvaluated": False}


def error_dispositions(errors, licenses, index, gn):
    selected = [row for row in errors if row["directory"].startswith(ROOT)]
    if len(selected) != 8 or {row["directory"] for row in selected} != ERROR_DIRECTORIES:
        raise ValueError("all eight original SwiftShader directory errors required")
    result = []
    for row in selected:
        directory = row["directory"]
        bound = [value for value in licenses if value["sourcePath"].startswith(directory + "/")]
        entries = [{"sourcePath": path, **entry} for path, entry in sorted(index.items()) if path.startswith(directory + "/")]
        result.append({"originalDirectoryError": row, "boundSourceNotices": bound,
            "disposition": "partial_copied_source_notices_bound" if bound else "unresolved_source_or_shipping_obligation",
            "originalErrorRemoved": False, "rootApacheCoversCopiedCode": False,
            "sourceTreeEntries": len(entries), "gitlinks": [entry for entry in entries if entry["mode"] == "160000"],
            "gnReferences": [value for value in gn["copiedLibraryReferences"] if directory in value["sourcePath"]
                             or directory[len(ROOT):] in value["text"]],
            "gnReferenceAbsenceMeansNotShipped": False, "completeCopiedTreeNotice": False})
    return result


def render_fragment(notices, bindings, inputs, texts, pins):
    if set(texts) != {"Apache-2.0", "NCSA", "LLVM-exception"}:
        raise ValueError("Apache, NCSA and complete LLVM exception texts required")
    for key, data in texts.items():
        pin = pins[key]
        if INPUTS["TREE"]["check_blob"](pin["oid"], data) != {field: pin[field] for field in ("bytes", "sha256")}:
            raise ValueError("unchanged full standard text required")
    parts = [b"PARTIAL SWIFTSHADER COPIED SOURCE NOTICE SUPPLEMENT - NOT FOR DISTRIBUTION\n"
             b"Scope: bound source notices below; not a complete Chromium NOTICE or binary SBOM.\n"]
    for row in notices:
        parts.append(("\nSource: " + row["sourcePath"] + "\n" + row["originalAttributionText"]).encode())
    for row in bindings:
        payload = inputs["files"][row["sourcePath"]]["payload"]
        if (len(payload), hashlib.sha256(payload).hexdigest()) != (row["bytes"], row["sha256"]):
            raise ValueError("unchanged actual complete source-notice bytes required")
        parts.extend((("\nOriginal source notice: " + row["sourcePath"] + "\n").encode(), payload))
    for key, data in texts.items():
        parts.extend((("\nComplete SPDX standard text: " + key + "\n").encode(), data))
    fragment = b"\n".join(parts)
    if len(fragment) > MAX_FRAGMENT:
        raise ValueError("bounded copied source supplement required")
    return fragment


def inspect(source, output):
    INPUTS["checked_source"](source)
    if output.parent != source or not re.fullmatch(r"swiftshader-notices-[0-9]{2}", output.name):
        raise ValueError("safe fresh task-owned SwiftShader notice output required")
    if output.exists() or output.is_symlink():
        raise FileExistsError("never overwrite SwiftShader notice evidence")
    inputs, baseline, preview = CANDIDATES["load_notice"](source)
    original, sha = INPUTS["receipt"](source / "notice-candidates-02/notice-candidate-inventory.json", SUPPLEMENT["CANDIDATE_SEAL"])
    if original != baseline or INPUTS["read_data"](source / "notice-candidates-02/partial-source-notice.txt", CANDIDATES["MAX_NOTICE"]) != preview:
        raise ValueError("entire original candidate report and preview must reproduce")
    blobs, seals, correspondence = {}, {}, {}
    for profile in PROFILES:
        _, checkout_sha, matched = SOURCES["checked_inputs"](source, profile)
        added, seals[profile] = INTEGRATION["checked_profile"](source, profile, inputs, checkout_sha, matched, profiles=PROFILES, git_objects=True)
        blobs.update(added)
        correspondence[profile] = matched
    expected = {ROOT + path: oid for path, oid in SOURCES["SWIFTSHADER_GN_PINS"]["swiftshader"].items()}
    if gn_paths(inputs["index"]) != expected or not set(expected).issubset(blobs):
        raise ValueError("complete original sealed SwiftShader GN inventory required")
    gn = configuration({path: blobs[path] for path in expected})
    cpp = {path: data for path, data in blobs.items() if path not in expected}
    if set(cpp) != {row["sourcePath"] for row in SOURCES["requests"]("swiftshader-consumers")}:
        raise ValueError("all five original consumer samples required")
    notices = [leading_notice(data, path) for path, data in sorted(cpp.items())]
    bindings = [SUPPLEMENT["source_license"](inputs, ROOT + path) for path in LICENSE_PATHS]
    support = inputs["files"][ROOT + LICENSE_PATHS[1]]["payload"]
    if b"Copyright (C) 2004 eXtensible Systems, Inc." not in support or b"Illinois Open Source" not in support:
        raise ValueError("original LLVM10 Support attribution and old license declaration required")
    texts, llvm_receipt = LLVM["verify"](source, "llvm-license-source-01", LLVM_SEAL)
    spdx = SUPPLEMENT["SPDX"]
    apache_receipt = spdx["verify"](source, "khronos-license-source-01")
    texts = {"Apache-2.0": INPUTS["read_data"](source / "khronos-license-source-01/Apache-2.0.license-data", spdx["MAX_TEXT"]), **texts}
    fragment = render_fragment(notices, bindings, inputs, texts, {"Apache-2.0": spdx["LICENSES"]["Apache-2.0"], **LLVM["LICENSES"]})
    errors = error_dispositions(original["candidateSelection"]["directoryErrorsRetained"], bindings, inputs["index"], gn)
    report = {"scope": "SwiftShader copied source notices, source-level configuration and retained metadata gaps",
        "sourceCommit": INPUTS["TREE"]["COMMIT"], "swiftshaderSource": list(SOURCES["SWIFTSHADER_ROOT"]),
        "originalNoticeInventorySha256": sha, "profileReceiptSha256": seals, "checkoutCorrespondence": correspondence,
        "completeGnInventory": True, "gnPaths": len(expected), "gnSourceBytes": sum(len(blobs[path]) for path in expected),
        "configurationEvidence": gn, "sampledConsumerNotices": notices, "sampledConsumerBytes": sum(map(len, cpp.values())),
        "originalSourceNoticeBindings": bindings, "llvmStandardSource": llvm_receipt, "apacheStandardSource": apache_receipt,
        "llvm10SupportBinding": {"originalSourcePath": ROOT + LICENSE_PATHS[1], "fullStandardId": "NCSA"},
        "llvmSourceBindings": [{"originalSourcePath": row["sourcePath"], "fullStandardIds": row["fullLicenseIds"]}
                               for row in notices if "LLVM-exception" in row["fullLicenseIds"]],
        "originalDirectoryErrorDispositions": errors, "originalRawErrorsRetained": original["afterWebglSources"],
        "remainingObligations": ["Boost and libbacktrace checkout/licensing/shipping provenance",
            "Complete copied-source attribution coverage, including LLVM alternate notices",
            "Effective build and binary correspondence; source rules do not prove exclusions"],
        "fragmentFile": "swiftshader-copied-notices.txt", "fragmentBytes": len(fragment), "fragmentSha256": hashlib.sha256(fragment).hexdigest(),
        "sourceFunctionsExecuted": False, "generatorExecuted": False, "conditionsEvaluated": False,
        "effectiveGnMeasured": False, "shippingExclusionsProven": False, "metadataPathsAliased": False,
        "completeChromiumNotice": False, "promoted": False}
    output.mkdir()
    with (output / report["fragmentFile"]).open("xb") as stream:
        stream.write(fragment)
    INPUTS["EXTERNAL"]["write_report"](output / "swiftshader-notice-inventory.json", report)
    return report


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--output-name", required=True)
    args = parser.parse_args()
    source = args.source.absolute()
    report = inspect(source, source / args.output_name)
    print(json.dumps({key: report[key] for key in ("gnPaths", "gnSourceBytes", "sampledConsumerBytes", "fragmentBytes", "fragmentSha256", "completeChromiumNotice")}))
