"""Bind all copied API header notices and review omitted metadata records.

Retain the exact original errors. Actual source-path bindings and manifest
absence do not prove shipping exclusions, effective GN or a complete SBOM.
"""
import argparse
from collections import Counter
import hashlib
import json
from pathlib import Path
import re
import runpy


HERE = Path(__file__).parent.absolute()
CANDIDATES = runpy.run_path(str(HERE / "inspect-chromium-notice-candidates.py"))
SPDX = runpy.run_path(str(HERE / "collect-khronos-notice-texts.py"))
PLATFORM = runpy.run_path(str(HERE / "inspect-chromium-platform-coverage.py"))
PINS = runpy.run_path(str(HERE / "chromium-angle-header-pins.py"))
INPUTS, INTEGRATION = CANDIDATES["INPUTS"], CANDIDATES["INTEGRATION"]
SOURCES = INTEGRATION["SOURCES"]
HEADER_SEAL = "9615a5d31c9e25e10959b5b51a6a434a0db7f275a955321eb4845e57f7e9f93e"
CANDIDATE_SEAL = "1eecc988cd5ab4f9cfc983433b37fce55bb2ad8607e3e41d250f4e6c35c6d682"
ANGLE, SCREEN = INTEGRATION["ANGLE"], INTEGRATION["SCREEN"]
MAX_FRAGMENT = 256 * 1024
COPY_SOURCES = {
    ANGLE + "third_party/EGL-Registry": ("include/EGL/",),
    ANGLE + "third_party/OpenGL-Registry": ("include/GLES/", "include/GLES2/", "include/GLES3/", "include/GLX/", "include/WGL/"),
    ANGLE + "third_party/OpenCL-Docs": ("include/CL/",),
}
UNRESOLVED = {ANGLE + "third_party/" + name for name in ("OpenCL-ICD-Loader", "astc-encoder", "cpu_features")}
UNRESOLVED.add("third_party/fuchsia-sdk")


def header_paths(index):
    def selected(path):
        relative = path[len(ANGLE):] if path.startswith(ANGLE) else path
        return path.startswith(ANGLE) and relative.endswith(".h") and (
            relative.startswith("src/third_party/khronos/")
            or any(relative.startswith("include/" + name + "/") for name in PINS["API_DIRECTORIES"]))
    return {path: row["gitObjectSha1"] for path, row in index.items()
            if selected(path) and row["mode"] in ("100644", "100755")}


def original_attribution(data, path):
    text = data.decode("utf-8", errors="strict")
    comments = list(re.finditer(r"/\*[\s\S]*?\*/|(?m:^[ \t]*//[^\n]*(?:\n[ \t]*//[^\n]*)*)", text))
    notices = [match for match in comments if re.search(r"Copyright|SPDX-License-Identifier|Permission is hereby granted|Licensed under", match.group())]
    if len(notices) != 1 or text.count("Copyright") != 1:
        raise ValueError("one complete original copyright/licensing comment required: " + path)
    match = notices[0]
    block = match.group()
    identifiers = re.findall(r"SPDX-License-Identifier: ([A-Za-z0-9.\-]+)", block)
    copyright_lines = [line for line in block.splitlines() if "Copyright" in line]
    if identifiers:
        if len(identifiers) != 1 or identifiers[0] not in SPDX["LICENSES"] or "The Khronos Group Inc." not in block:
            raise ValueError("reviewed Khronos SPDX declaration required")
        kind, license_id = "spdx", identifiers[0]
    elif "Licensed under the Apache License, Version 2.0" in block:
        required = ("you may not use this file except in compliance with the License", "http://www.apache.org/licenses/LICENSE-2.0",
                    'distributed under the License is distributed on an "AS IS" BASIS', "WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND",
                    "limitations under the License.", "The Khronos Group Inc.")
        if not all(value in block for value in required):
            raise ValueError("complete original Apache short notice required")
        kind, license_id = "apache_short_notice", "Apache-2.0"
    elif "Permission is hereby granted, free of charge" in block:
        required = ("distribute, sublicense, and/or sell copies of the Materials", "The above copyright notice and this permission notice shall be included",
                    'THE MATERIALS ARE PROVIDED "AS IS"', "NONINFRINGEMENT", "MATERIALS OR THE USE OR OTHER DEALINGS IN THE MATERIALS.", "The Khronos Group Inc.")
        if not all(value in block for value in required):
            raise ValueError("complete original inline permission and disclaimer required")
        kind, license_id = "inline_permission", None
    elif ("The ANGLE Project Authors. All rights reserved." in block
          and "governed by a BSD-style license" in block and "found in the LICENSE file." in block):
        kind, license_id = "angle_root_license", None
    else:
        raise ValueError("unreviewed original header license requires separate disposition")
    return {"sourcePath": path, "sourceSha256": hashlib.sha256(data).hexdigest(), "sourceBytes": len(data),
            "line": text[:match.start()].count("\n") + 1, "originalAttributionText": block,
            "attributionSha256": hashlib.sha256(block.encode()).hexdigest(), "copyrightLines": copyright_lines,
            "noticeKind": kind, "licenseId": license_id}


def source_license(inputs, path):
    row, entry = inputs["files"].get(path), inputs["index"].get(path)
    if row is None or entry is None or entry["mode"] not in ("100644", "100755"):
        raise ValueError("real verified regular license source required: " + path)
    identity = INPUTS["TREE"]["check_blob"](entry["gitObjectSha1"], row["payload"])
    if not row["payload"] or identity != {key: row[key] for key in ("bytes", "sha256")}:
        raise ValueError("original complete license byte/digest identity required")
    return {"sourcePath": path, "gitObjectSha1": entry["gitObjectSha1"], **identity}


def configured_mappings(source, inputs):
    crashpad, _ = INPUTS["receipt"](source / "crashpad-coverage-01.json", "4f0687a93a5f4ce3eb12bf8c078659c49e735709be105f7876ce0e3f52d5d187")
    integration, _ = INPUTS["receipt"](source / "integration-coverage-04.json", CANDIDATES["CONSUMER"]["BASELINE_SHA"])
    consumer, _ = INPUTS["receipt"](source / "consumer-coverage-01.json", CANDIDATES["CONSUMER_SHA"])
    rows = crashpad["configurationEvidence"]["sourceLevelMapping"] + integration["configurationEvidence"]["sourceLevelMapping"]
    result = {row["embeddedMetadataDirectory"]: {"sourceEvidence": row, "license": source_license(inputs, row["licensePath"])} for row in rows}
    cpp_rules = consumer["cppConfigurationEvidence"]
    for row in consumer["licensedCppSources"]:
        directories = [SCREEN + "third_party/" + {"libcxx": "libc++", "libcxxabi": "libc++abi", "llvm-libc": "llvm-libc"}[row["component"]]]
        if row["component"] == "llvm-libc":
            directories.append(ANGLE + "third_party/llvm-libc")
        license_record = source_license(inputs, row["licensePath"])
        if license_record["sha256"] != row["licenseSha256"] or license_record["bytes"] != row["licenseBytes"]:
            raise ValueError("sealed C++ license and actual byte binding differ")
        for directory in directories:
            result[directory] = {"sourceEvidence": {"library": row, "chromiumCppConfiguration": cpp_rules}, "license": license_record}
    if len(result) != 10:
        raise ValueError("ten distinct reviewed configured source license mappings required")
    return result


def omission_records(omitted, mappings, headers, declarations, targets):
    expected = set(mappings) | set(COPY_SOURCES) | UNRESOLVED
    if len(omitted) != 17 or {row["directory"] for row in omitted} != expected:
        raise ValueError("all seventeen original omitted metadata records required")
    result = []
    for row in omitted:
        path = row["directory"]
        record = {"originalOmittedMetadata": row, "originalErrorRemoved": False, "binaryExclusionProven": False}
        if path in mappings:
            record.update(disposition="configured_source_license_bound", **mappings[path])
        elif path in COPY_SOURCES:
            copied = [header for header in headers if any(header["sourcePath"].startswith(ANGLE + prefix) for prefix in COPY_SOURCES[path])]
            if not copied:
                raise ValueError("actual copied header notices required")
            record.update(disposition="copied_header_notices_bound_registry_checkout_not_proven", copiedHeaderPaths=[item["sourcePath"] for item in copied])
        else:
            declared = [item for item in declarations if item["path"].startswith(path + "/")]
            if not declared:
                raise ValueError("unresolved source declaration must remain explicit")
            record.update(disposition="unresolved_source_or_shipping_obligation", declarations=declared,
                          recordedManifestEntries=[{"target": target["target"], "entries": {
                              key: value for key, value in target["manifest"].items() if key.startswith("src/" + path + "/")}}
                              for target in targets], manifestAbsenceMeansNotShipped=False)
        result.append(record)
    return result


def render_fragment(headers, texts, angle_license):
    if set(texts) != set(SPDX["LICENSES"]) or not angle_license:
        raise ValueError("both complete standard texts and actual ANGLE root license required")
    for key, data in texts.items():
        pin = SPDX["LICENSES"][key]
        if INPUTS["TREE"]["check_blob"](pin["oid"], data) != {field: pin[field] for field in ("bytes", "sha256")}:
            raise ValueError("unchanged original complete standard license text required")
    parts = [b"PARTIAL COPIED HEADER NOTICE SUPPLEMENT - NOT FOR DISTRIBUTION\n"
             b"Scope: the exact copied API files below, not a complete Chromium NOTICE or binary SBOM.\n"]
    for row in headers:
        parts.append(("\nSource: " + row["sourcePath"] + "\n" + row["originalAttributionText"] + "\n").encode())
    for key, data in texts.items():
        parts.extend((("\nComplete SPDX standard text: " + key + "\n").encode(), data))
    parts.extend((b"\nActual ANGLE root license: third_party/angle/LICENSE\n", angle_license))
    fragment = b"\n".join(parts)
    if len(fragment) > MAX_FRAGMENT:
        raise ValueError("bounded complete copied-header notice supplement required")
    return fragment


def inspect(source, output):
    INPUTS["checked_source"](source)
    if output.parent != source or not re.fullmatch(r"notice-supplements-[0-9]{2}", output.name):
        raise ValueError("safe fresh task-owned notice supplement output required")
    if output.exists() or output.is_symlink():
        raise FileExistsError("never overwrite supplement evidence")
    inputs, baseline, preview = CANDIDATES["load_notice"](source)
    original, sha = INPUTS["receipt"](source / "notice-candidates-02/notice-candidate-inventory.json", CANDIDATE_SEAL)
    if original != baseline or INPUTS["read_data"](source / "notice-candidates-02/partial-source-notice.txt", CANDIDATES["MAX_NOTICE"]) != preview:
        raise ValueError("entire original notice inventory and preview must reproduce")
    _, checkout_sha, correspondence = SOURCES["checked_inputs"](source, "angle-headers")
    profiles = {"angle-headers": ("angle-header-git-source-01", HEADER_SEAL)}
    blobs, header_sha = INTEGRATION["checked_profile"](source, "angle-headers", inputs, checkout_sha, correspondence, profiles=profiles, git_objects=True)
    expected = {ANGLE + path: oid for path, oid in PINS["ANGLE_HEADER_PINS"].items()}
    if header_paths(inputs["index"]) != expected or set(blobs) != set(expected):
        raise ValueError("complete independently indexed copied API header inventory required")
    headers = [original_attribution(data, path) for path, data in sorted(blobs.items())]
    spdx = SPDX["verify"](source, "khronos-license-source-01")
    texts = {key: INPUTS["read_data"](source / "khronos-license-source-01" / (key + ".license-data"), SPDX["MAX_TEXT"]) for key in SPDX["LICENSES"]}
    angle_license = source_license(inputs, ANGLE + "LICENSE")
    for row in headers:
        if row["licenseId"]:
            row["fullLicenseSha256"] = SPDX["LICENSES"][row["licenseId"]]["sha256"]
        elif row["noticeKind"] == "angle_root_license":
            row["fullLicenseSource"] = angle_license
    mappings = configured_mappings(source, inputs)
    checkout = SOURCES["CHECKOUTS"]["inspect"](source)
    omissions = omission_records(original["candidateSelection"]["invalidReadmeRecordsOmitted"], mappings, headers,
                                PLATFORM["declarations"](source), checkout["targets"])
    fragment = render_fragment(headers, texts, inputs["files"][ANGLE + "LICENSE"]["payload"])
    report = {"scope": "copied API header attributions and omitted metadata triage, not shipping or full NOTICE approval",
        "sourceCommit": INPUTS["TREE"]["COMMIT"], "originalNoticeInventorySha256": sha, "headerSourceReceiptSha256": header_sha,
        "completeSelectedHeaderTrees": True, "selectedHeaders": headers, "selectedHeaderBytes": sum(map(len, blobs.values())),
        "headerNoticeKinds": dict(sorted(Counter(row["noticeKind"] for row in headers).items())), "spdxSource": spdx,
        "omittedMetadataDispositions": omissions, "omissionDispositions": dict(sorted(Counter(row["disposition"] for row in omissions).items())),
        "originalRawErrorsRetained": original["afterWebglSources"], "fragmentFile": "copied-api-header-notices.txt",
        "fragmentBytes": len(fragment), "fragmentSha256": hashlib.sha256(fragment).hexdigest(),
        "sourceFunctionsExecuted": False, "generatorExecuted": False, "conditionsEvaluated": False,
        "shippingExclusionsProven": False, "metadataPathsAliased": False, "completeChromiumNotice": False, "promoted": False}
    output.mkdir()
    with (output / report["fragmentFile"]).open("xb") as stream:
        stream.write(fragment)
    INPUTS["EXTERNAL"]["write_report"](output / "notice-supplement-inventory.json", report)
    return report


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--output-name", required=True)
    args = parser.parse_args()
    source = args.source.absolute()
    report = inspect(source, source / args.output_name)
    print(json.dumps({key: report[key] for key in ("selectedHeaderBytes", "headerNoticeKinds", "omissionDispositions", "fragmentBytes", "fragmentSha256", "completeChromiumNotice")}))
