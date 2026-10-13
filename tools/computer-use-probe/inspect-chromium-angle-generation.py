"""Verify pinned ANGLE generation records without executing upstream generators.

Recorded MD5s are generation bookkeeping only, never source identity or binary
proof. Git objects, SHA256s and original manifests remain the identity checks.
Registry XML bytes and reproduction of all outputs remain explicit open items.
"""
import argparse
import ast
import hashlib
import json
from pathlib import Path
import re
import runpy


HERE = Path(__file__).parent.absolute()
INTEGRATION = runpy.run_path(str(HERE / "inspect-chromium-integration-coverage.py"))
CONTEXT = runpy.run_path(str(HERE / "inspect-chromium-condition-context.py"))
INPUTS, SOURCES = INTEGRATION["INPUTS"], INTEGRATION["SOURCES"]
ANGLE = INTEGRATION["ANGLE"]
PROFILES = {
    "angle-registry": ("angle-registry-git-source-01", "fa90b0b55aed94661062b18c5bc384e47169d1e46a41be2d8c6a9cae17d0d11e"),
    "angle-generation": ("angle-generation-git-source-01", "2d2de53df0592a7833fec6f358aa92246a47d435b088d37fb1be2db2e89abd9f"),
    "angle-generated": ("angle-generated-git-source-01", "543f6524699ddc4fb884ffde9a8858e4de385163f0125d1fc59efd9363b7eea1"),
}
CONTEXT_SHA = "6a3d410ffe5e56af86b6333c8a4af433f20735332032e589ac93539506db4af2"
CATALOG_SHA = "eca26cd95153d528f8fd4df2275b208c936a2490a75edb9b09aeb27833c2ec00"
ASSEMBLY_SHA = "a2f6dca809cb846c045d6882883d2b3f771101fabefaa2cf1494a85fff5a4271"
REGISTRIES = {
    "EGL-Registry": ("7dea2ed79187cd13f76183c4b9100159b9e3e071", ("api/egl.xml",)),
    "OpenCL-Docs": ("5b4ca15f0e5a5be87b56b99f652f728e05cab587", ("xml/cl.xml",)),
    "OpenGL-Registry": ("9cb90ca4902d588bef3c830fbb1da484893bd5fb", ("xml/gl.xml", "xml/glx.xml", "xml/wgl.xml")),
}
XML_PATHS = tuple("third_party/" + name + "/src/" + path for name, (_, paths) in REGISTRIES.items() for path in paths)
GENERATORS = {
    "GL/EGL/WGL loader": "scripts/generate_loader.py",
    "GL/EGL entry points": "scripts/generate_entry_points.py",
    "GLenum value to string map": "scripts/gen_gl_enum_utils.py",
    "packed enum": "src/common/gen_packed_gl_enums.py",
    "proc table": "scripts/gen_proc_table.py",
}
MAX_RECORD = 128 * 1024


def assignment(contents, name):
    tree = ast.parse(contents.decode())
    rows = [row for row in tree.body if isinstance(row, ast.Assign)
        and len(row.targets) == 1 and isinstance(row.targets[0], ast.Name) and row.targets[0].id == name]
    if len(rows) != 1:
        raise ValueError("one literal original assignment required: " + name)
    return ast.literal_eval(rows[0].value)


def generation_rules(registry, runner):
    inputs = assignment(registry, "khronos_xml_inputs")
    extensions = assignment(registry, "angle_xml_inputs")
    generators = assignment(runner, "generators")
    if (inputs != ["../" + path for path in XML_PATHS]
            or extensions != ["gl_angle_ext.xml", "egl_angle_ext.xml", "registry_xml.py"]
            or not isinstance(generators, dict) or any(generators.get(key) != value for key, value in GENERATORS.items())):
        raise ValueError("exact pinned registry inputs and generator mappings required")
    functions = [row for row in ast.parse(runner.decode()).body if isinstance(row, ast.FunctionDef) and row.name == "md5"]
    if len(functions) != 1:
        raise ValueError("one original MD5 bookkeeping implementation required")
    expected = ast.parse("""def md5(fname):
    hash_md5 = hashlib.md5()
    with open(fname, 'rb') as f:
        if sys.platform.startswith('win') or sys.platform == 'cygwin':
            hash_md5.update(f.read().replace(b'\\r\\n', b'\\n'))
        else:
            for chunk in iter(lambda: f.read(4096), b''):
                hash_md5.update(chunk)
    return hash_md5.hexdigest()
""").body[0]
    if ast.dump(functions[0], include_attributes=False) != ast.dump(expected, include_attributes=False):
        raise ValueError("original raw and Windows CRLF-normalized MD5 rule required")
    return {"registryXmlInputs": list(XML_PATHS), "angleInputs": extensions, "generatorMappings": GENERATORS,
        "originalMd5Function": ast.get_source_segment(runner.decode(), functions[0]),
        "md5IsGenerationBookkeepingOnly": True, "sourceExecuted": False}


def unique_pairs(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError("duplicate generation-record key")
        result[key] = value
    return result


def generation_record(data):
    if not isinstance(data, bytes) or not 0 < len(data) <= MAX_RECORD:
        raise ValueError("bounded generation record required")
    result = json.loads(data.decode(), object_pairs_hook=unique_pairs)
    if not isinstance(result, dict) or not 0 < len(result) <= 512:
        raise ValueError("bounded nonempty generation dictionary required")
    for path, digest in result.items():
        if (not re.fullmatch(r"[A-Za-z0-9_.\-/]+", path) or path.startswith("/")
                or any(part in ("", ".", "..") for part in path.split("/"))
                or not isinstance(digest, str) or not re.fullmatch(r"[0-9a-f]{32}", digest)):
            raise ValueError("canonical source path and recorded MD5 required")
    return result


def check_records(blobs, index):
    parsed, matches, unresolved, inputs = {}, [], [], {}
    for name in GENERATORS:
        path = "scripts/code_generation_hashes/" + name.replace(" ", "_").replace("/", "_") + ".json"
        record = generation_record(blobs[ANGLE + path])
        parsed[path] = {"entries": len(record), "registryInputs": [key for key in record if key in XML_PATHS]}
        for relative, recorded in record.items():
            if relative in XML_PATHS:
                if relative in inputs and inputs[relative] != recorded:
                    raise ValueError("registry input hashes disagree across original generation records")
                inputs[relative] = recorded
            full = ANGLE + relative
            if full not in blobs:
                unresolved.append({"generationRecord": path, "sourcePath": full, "recordedMd5": recorded,
                    "status": "registry_xml_bytes_not_collected" if relative in XML_PATHS else "outside_selected_source_samples"})
                continue
            data = blobs[full]
            entry = index.get(full)
            if entry is None or entry["mode"] not in ("100644", "100755") or entry["kind"] != "blob":
                raise ValueError("original regular source tree entry required")
            identity = INPUTS["TREE"]["check_blob"](entry["gitObjectSha1"], data)
            raw, normalized = hashlib.md5(data).hexdigest(), hashlib.md5(data.replace(b"\r\n", b"\n")).hexdigest()
            if recorded not in (raw, normalized):
                raise ValueError("selected source differs from original generation bookkeeping: " + full)
            matches.append({"generationRecord": path, "sourcePath": full, "gitObjectSha1": entry["gitObjectSha1"], **identity,
                "recordedMd5": recorded, "rawMd5Matches": recorded == raw, "windowsNormalizedMd5Matches": recorded == normalized,
                "generatorReproduced": False, "binaryUseProven": False})
    if set(inputs) != set(XML_PATHS):
        raise ValueError("all five original registry inputs required")
    return {"records": parsed, "selectedSourceMatches": matches, "unmeasuredRecordEntries": unresolved,
        "registryInputRecordedMd5": inputs, "allGeneratedOutputsReproduced": False}


def registry_declarations(catalog, contexts, index, files, targets):
    result = []
    if (len(contexts) != 4 or len(targets) != 4 or {row["target"] for row in contexts} != {row["target"] for row in targets}
            or len({row["target"] for row in contexts}) != 4):
        raise ValueError("four unique original recorded contexts required")
    for name, (revision, _) in REGISTRIES.items():
        relative = "third_party/" + name + "/src"
        rows = [row for row in catalog["gitDependencies"] if row["path"] == relative]
        repository = "https://chromium.googlesource.com/external/github.com/KhronosGroup/" + name
        if (len(rows) != 1 or rows[0]["repository"] != repository or rows[0]["revision"] != revision
                or rows[0]["condition"] != "not build_with_chromium" or rows[0]["gitlinkRevision"] != revision
                or rows[0]["gitlinkMatched"] is not True
                or index.get(ANGLE + relative) != {"mode": "160000", "kind": "commit", "path": relative, "gitObjectSha1": revision}):
            raise ValueError("exact original registry DEPS declaration and Gitlink required")
        metadata = ANGLE + "third_party/" + name + "/README.chromium"
        data = files[metadata]["payload"]
        original = {}
        for key, expected in (("Revision", "DEPS"), ("License", "Apache-2.0"), ("License File", "Not Included"), ("Shipped", "yes")):
            lines = [(number, line) for number, line in enumerate(data.decode().splitlines(), 1) if line.startswith(key + ": ")]
            if len(lines) != 1 or lines[0][1] != key + ": " + expected:
                raise ValueError("unchanged original registry metadata fields required")
            original[key] = {"line": lines[0][0], "text": lines[0][1]}
        analyses = []
        for context in contexts:
            flags = context["effectiveFlagsFromRecordedContextAndPinnedRules"]
            analyses.append({"target": context["target"], **CONTEXT["analyze_boolean"](rows[0]["condition"], flags)})
        comparisons = [{"target": target["target"], **SOURCES["CHECKOUTS"]["compare_pin"](
            "src/" + ANGLE + relative, repository, revision, target["manifest"])} for target in targets]
        result.append({"declaration": rows[0], "originalMetadataPath": metadata, "originalMetadataFields": original,
            "metadataSha256": hashlib.sha256(data).hexdigest(), "recordedContextAnalysis": analyses,
            "originalManifestCorrespondence": comparisons, "registrySourceBytesVerified": False,
            "copiedHeaderProvenanceProven": False, "binaryExclusionProven": False})
    return result


def inspect(source, output):
    INPUTS["checked_source"](source)
    if output.parent != source or not re.fullmatch(r"angle-generation-coverage-[0-9]{2}\.json", output.name):
        raise ValueError("safe fresh task-owned generation output required")
    if output.exists() or output.is_symlink():
        raise FileExistsError("never overwrite generation evidence")
    inputs, checkout_sha, correspondence = SOURCES["checked_inputs"](source, "angle-registry")
    blobs, profiles = {}, {}
    for name in PROFILES:
        collected, sha = INTEGRATION["checked_profile"](source, name, inputs, checkout_sha, correspondence,
            profiles=PROFILES, git_objects=True)
        blobs.update(collected)
        profiles[name] = sha
    rules = generation_rules(blobs[ANGLE + "scripts/registry_xml.py"], blobs[ANGLE + "scripts/run_code_generation.py"])
    records = check_records(blobs, inputs["index"])
    context, _ = INPUTS["receipt"](source / "condition-context-01.json", CONTEXT_SHA)
    if context["rootBooleanDefaults"] != CONTEXT["boolean_defaults"](inputs["files"]["DEPS"]["payload"]):
        raise ValueError("original root defaults and context identities required")
    catalog, _ = INPUTS["receipt"](source / "recursive-deps-angle-02.json", CATALOG_SHA)
    checkout = SOURCES["CHECKOUTS"]["inspect"](source)
    declarations = registry_declarations(catalog, context["targetContexts"], inputs["index"], inputs["files"], checkout["targets"])
    assembly, _ = INPUTS["receipt"](source / "notice-assembly-03/notice-assembly-inventory.json", ASSEMBLY_SHA)
    report = {"scope": "fixed generation records and seven selected output samples only, not full provenance or binary proof",
        "sourceCommit": INPUTS["TREE"]["COMMIT"], "profileReceipts": profiles, "generationSourceBytes": sum(map(len, blobs.values())),
        "checkoutReceiptSha256": checkout_sha, "fourManifestCorrespondence": correspondence, "generationRules": rules,
        "generationRecords": records, "registryDeclarations": declarations, "recordedContextReceiptSha256": CONTEXT_SHA,
        "originalAssemblyReceiptSha256": ASSEMBLY_SHA, "originalRawErrorsRetained": assembly["originalRawErrorsRetained"],
        "remainingSourceObligations": ["Verify all five registry XML blobs at declared revisions, with full original notices",
            "Determine copied API header provenance separately; generated loader hashes do not prove copied header identity",
            "Reproduce remaining generated outputs and corresponding complete notice coverage",
            "Recorded contexts are not measurements of builder .DEPS.git, effective GN or binary exclusions"],
        "sourceFunctionsExecuted": False, "generatorExecuted": False, "metadataPathsAliased": False,
        "originalErrorsRemoved": False, "shippingExclusionsProven": False, "registryProvenanceComplete": False,
        "completeChromiumNotice": False, "binarySbom": False, "releaseReady": False, "promoted": False}
    INPUTS["EXTERNAL"]["write_report"](output, report)
    return report


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--output-name", required=True)
    args = parser.parse_args()
    source = args.source.absolute()
    report = inspect(source, source / args.output_name)
    print(json.dumps({"generationSourceBytes": report["generationSourceBytes"],
        "selectedRecordMatches": len(report["generationRecords"]["selectedSourceMatches"]),
        "registryDeclarations": len(report["registryDeclarations"]), "releaseReady": False}))
