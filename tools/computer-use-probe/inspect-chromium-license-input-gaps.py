"""Reverify collected source bytes and license-reference closure, data only.

Counts raw metadata references, never shipping dependencies. Uncollected DEPS,
conditions, non-Git inputs, source obligations and binary coverage stay open.
No upstream imports/execution, network, materialization or product mutation.
"""
import argparse
import hashlib
import json
from pathlib import Path
import re
import runpy


HERE = Path(__file__).parent.absolute()
EXTERNAL = runpy.run_path(str(HERE / "inspect-chromium-external-licenses.py"))
TREE = EXTERNAL["TREE"]
EXTERNAL_SHA = "233be34e7e29176acff240deaa36b3d91aa05f2b2d2b054277a1394748fb12fc"
MEDIA_SHA = "b4fc80ba79996402e0b185077fa0fa615156492b230fe408fd8fd18c9a2186aa"
RUNTIME_SHA = "21f92338bd3d6c3622ce1963c002f3af25fd45377d648e44bb6ea398d17ddeea"
SUPPORT_SHA = "de8ba2d653bfe0aec9e938a35102719949fab3e1cc99bc6386f85de988b4a28f"
BROWSER_SHA = "332ff906798fe4d190d46815c4a2bbe0ea87d168817214c967ce3b36657e42c7"
NOTICE_TEST_SHA = "25bef4416458ae27a69f4a9b694c70f29b7f3f5180359df4de7dbe4fbb3d5382"
NOTICE_TEST_SEALS = {"webgl": "a46f35e697bc3941d2b36833295659a2276cd9c9c72da00c6e5da59b3f5643bc"}
GPU_SHA = "83f2059270865a39f3d0e33e23908baac658612b03a186c88a11bd5cbf098119"
GPU_SEALS = {"dawn": "87828aa51164c6d1d791e2ed110d757b3a6fc64e62b8df6bc11029a727f83c72"}
GPU_SUPPORT_SHA = "ea6361295cb448fde18ac19f8cc6eef48e296e6dc9d8be411b00db76673cff16"
GPU_SUPPORT_SEALS = {
    "vulkan-utility-libraries": "41a5c4365b058b9efea7b1cf0529fb360f7995b9cde9cbdaa4133aa9a972176f",
    "vulkan-validation-layers": "9f626936053c79aed0572e83efc12b678ec3eb2e9db22b1faed6e6ff377ff004",
}
# Independently observed after each original collector reached terminal exit0.
# Seals protect selected-path completeness, not only hashes of files it lists.
CORE_SEALS = {
    "angle": "ad85a4fe480e61a7dfdc35a4db911dc9f744f90b85213ae57874ddc2dc1c711e",
    "boringssl": "4b3403dd8eebd874b6e392c86f1e0e77ce341c3ae4b80e9428ef183e3b46834a",
    "ffmpeg": "c387f360ae8f63667961cb8b6e25782a6e34da9ff25d6518e55506e95840ea6e",
    "freetype": "595f0232c3fbd5388258df504cc66bd77b9e2ccc4a949f02c89f3520e365e5a2",
    "harfbuzz": "b2f7b4beb7c04639207a8e19308d0ab9d4f2e714fcde843c57ebfda0c958aa47",
    "icu": "1d08e8b960691cc680646986f8e7834a439b8b9b5bd7a96de86dd2cf163762c2",
    "pdfium": "46f8cb8f88f8872009535db49e75d8c89e7358f190b120aca7711eb55676169b",
    "skia": "465531c934ce05519b620a567d1ddce2bfd5a88f5d9ab70421b85b5260b74436",
    "swiftshader": "e6e71c8572a818e708c9d99a0a79a8b72672cb2b293cb9804a12b017a1652393",
    "webrtc": "0f519351926ad936394d16fcf69dd2a595a298b89f067d14478ebac98222a0d9",
    "v8": "55a2cefec0b6190993efc30e14dc6548de6a4f44b09a09e87aadc29e421428e4",
    "depot-tools": "4e64013cc5a62ec39924d704e7d10a1f94a3519f4b4333376e8fff5c06d49d82",
}
MEDIA_SEALS = {
    "dav1d": "2b8c93f89c5bf635044dac8ad0e2ac24ea37574875b61ab985e2737d166ebffc",
    "libaom": "2bd13f0282e1f2c6a5755a70135c893bb9cbfd4737b5368ab90581da96b750a2",
    "libjpeg-turbo": "c4eaab0e1f6985b8d86522fff7809c1677d02c6abad8ac6abd83052eda7c87a8",
    "libvpx": "5333fd3598d8b413ce43ad5ea30a5bc86446b5a74b0ecb7fcb9093e0ab299494",
    "libwebm": "8a2d0bb2c44eb23dd011e553e617144f9945fa188b098a07d8a0fda31af7e511",
    "libwebp": "fe41b34d22522a4af611b3da491318d6ea7b257962e4471a119ad454a9c03c08",
    "libyuv": "def608c1363f8fd4128fa8db0c211c4811c52497bf5b5bd074b713213d617808",
}
RUNTIME_SEALS = {
    "quiche": "cb77fd7159ac8eaae1fee89c22a7f3d923b2b7f2586704b405e468e0c6e46f93",
    "cpu-features": "d55750af07e65d484ba2b6463df67a4a53073556164983b0c2ef70ab8fb4471f",
    "cpuinfo": "78d648ad4379181ff7482ffedb27b100b061a4814a6b3f433f031171c86350bb",
    "crc32c": "bc6b9f627f44b24d3d4d567bb1dac5c75db3368590f8e8b76c6b9e570c686efb",
    "expat": "65567c30e13fb2b02b06332f1104dc6f807448f28ecba42addb4af1548d62fe1",
    "fontconfig": "035a802b3f346000408bf3140aa87e1216ca70e70f2039a8adcf8160f047ef09",
    "leveldb": "cf3f98ad696843b327d2e292ce6db85a810ba164e9c12bbe251e0bdc60200fac",
    "libgav1": "55d172ab7b77f22302b721a9cdfa8b43ff25b21db0587c8182968ea77a7e21ff",
    "libunwind": "0ad9d1a7692f6531c881b2cc0b247ad2f29d6761cc505a135fd831d669039207",
    "snappy": "ab181961f5de0dbfbcd3cfbffb5770509b39167bb0fdb40d64b3a18d40fe7d17",
    "vulkan-loader": "4cd24d96a7c187828a92568400568f5c8359c9b4b368fd2ce49da243afb188d6",
    "wayland": "67b0ebcabefaa8338d2429721a2b7a1d449ce78966a14998ee748b573b8a0747",
    "wayland-protocols": "5d97ffa482f7f4ddd559685c68be471634e831f1bca3af350680c00384f8e914",
    "wayland-kde": "807395c24d274fe19834d0823fc3c2838807b9adf0678ea3b5a52a6c6847ee90",
    "wayland-gtk": "6963574d74d233836d55efe5780560e9d6df58340b99c61792425c703ae73ad3",
    "wuffs": "2a88d86ac182f938df9f08e53d159681597691521e760ecedd21477f2d9581e4",
    "zstd": "7603741e194abc6fc75b67b2c6ef1d97b3ec4a2e00a6f0418b43c5a9ee49c2f4",
}
SUPPORT_SEALS = {
    "compiler-rt": "81ec709cfca90097354f9aafc896e19957aa125bf18f817af454c71de31eaa31",
    "libcxx": "256aa128981364f83317d25f6732c02509144624ffb7883cc176be315e4912df",
    "libcxxabi": "b508a2688f260b4eef99bcc435a643bbb2d99793ea717203c1713ba9e10226e7",
    "llvm-libc": "672efc88afc31599554bbdf3d75c62d1ad0b551060a4bb01c59050926b7619f2",
    "dragonbox": "78a3d7c563fd89ad9659a0d5f4114abe883c9ce87e2244429b5294cc20233069",
    "fast-float": "b993c38e70d804a6f754c6392ded3c8071cb8a450e14f39b0d8f01271b531ed2",
    "fxdiv": "bf305106c646acad03306947150036a7ef4bd452918850ebb9a50cfcbd77d8b4",
    "microsoft-multimedia": "2f22a0de39ef2d92d71e5980705c8b79b79a0b9894ebdf7e7e8c2001b37ee89b",
    "microsoft-webauthn": "076a1dadfc4cec662115828d8a6942f38be10d3b5e4bd74cb7b6734a5a68aa4b",
    "pthreadpool": "037c4f23e7c5b4709383564b486d9b55254598725c095736ad2ffcd6f9d0f742",
    "xnnpack": "ccf94bb03c07c113098ea2e438bf4548409363d0bec5f8191afb436a195bfd5b",
}
BROWSER_SEALS = {
    "anonymous-tokens": "6ea4a8638d294862646ae536f177743fe6d83117e5263590fcd63475fb4dbf16",
    "devtools-frontend": "956a429134b9c186630eb964ca4e76ac579190f1e9c4f844b26b866c9894cac7",
    "federated-compute": "e5d34cd51eac6fc022d84ec7d959ae9096c616a1eef448261e280e60800f2f0d",
    "ink": "2e5be130f63e35f4792025b2f1f0170ef647fbd5b3befec6be2c5fb96c08f89b",
    "libprotobuf-mutator": "fece1eb2c6bd88f0913cd0b7609f0105c4dfced2aaf3c3bb767d90ea2fe96616",
    "oak": "615b7fcd1402ff234811909deabf044ae4a4f8f1184ffbc2f265944498944a64",
    "openh264": "972e227b0e8ec5414cd178667c8202916034bd233607e2eaa90f8fd5c737a9a7",
    "openscreen": "52716181f4af383b5f66e93138163149025aa75ea9c1ff48f394ad21839353c7",
    "openxr": "a3230801c6eb5fc41609c544146cf14892cdca5e22416ab9c014591434765310",
    "sframe": "f6239a5160e4c0ae494199294b929f1b5906b1eb8c91c694ab30669ec4c5b2b1",
}


def read_data(path, limit):
    TREE["regular_parents"](path)
    if not path.is_file() or path.stat().st_size > limit:
        raise ValueError("bounded regular source data file required")
    contents = path.read_bytes()
    if len(contents) > limit:
        raise ValueError("source data changed beyond byte budget")
    return contents


def receipt(path, expected_sha=None):
    contents = read_data(path, TREE["MAX_BLOB"])
    sha = hashlib.sha256(contents).hexdigest()
    if expected_sha is not None and sha != expected_sha:
        raise ValueError("fixed reviewed source receipt SHA256 mismatch")
    return json.loads(contents), sha


def merge_tree(index, rows, prefix):
    for row in rows:
        path = prefix + "/" + row["path"] if prefix else row["path"]
        if path in index:
            raise ValueError("overlapping checkout tree identities")
        index[path] = row


def merge_files(index, files, evidence, prefix, raw_files, references):
    for row in files:
        path = prefix + "/" + row["path"] if prefix else row["path"]
        entry = index.get(path)
        if (entry is None or entry["mode"] not in ("100644", "100755")
                or (entry["gitObjectSha1"], entry["mode"]) != (row["gitObjectSha1"], row["mode"])
                or path in raw_files):
            raise ValueError("unique regular selected file must match full pinned tree")
        contents = read_data(evidence / "blobs" / (row["gitObjectSha1"] + ".source-data"), TREE["MAX_BLOB"])
        identity = TREE["check_blob"](row["gitObjectSha1"], contents)
        if identity["bytes"] != row["bytes"] or identity["sha256"] != row["sha256"]:
            raise ValueError("source bytes differ from receipt identity")
        raw_files[path] = {"gitObjectSha1": row["gitObjectSha1"], **identity, "payload": contents}
        references.extend(TREE["references"]({**row, "path": path}, contents))


def classify(reference, index, raw_files):
    path = reference["sourcePath"]
    if path is None:
        return "remote_reference_not_followed"
    if path not in index:
        return "path_not_in_collected_checkout_trees"
    if index[path]["mode"] not in ("100644", "100755"):
        return "reference_is_not_regular_source_file"
    if path not in raw_files:
        return "regular_source_exists_but_bytes_not_collected"
    return "resolved_to_verified_source_bytes"


def checked_source(source):
    if (source.parent != HERE / ".run"
            or not re.fullmatch(r"maintained-notice-source-[0-9]{8}-[0-9]{2}", source.name)):
        raise ValueError("task-owned pinned source directory required")
    TREE["regular_parents"](source)
    if not source.is_dir() or source.resolve(strict=True) != source:
        raise ValueError("canonical existing pinned source directory required")


def append_external(source, name, expected_sha, pins, seals, index, raw_files, references, components):
    reviewed = {
        "external-license-02": (EXTERNAL_SHA, EXTERNAL["PINS"], CORE_SEALS),
        "external-license-03": (MEDIA_SHA, EXTERNAL["MEDIA_PINS"], MEDIA_SEALS),
        "external-license-04": (RUNTIME_SHA, EXTERNAL["RUNTIME_PINS"], RUNTIME_SEALS),
        "external-license-05": (SUPPORT_SHA, EXTERNAL["SUPPORT_PINS"], SUPPORT_SEALS),
        "external-license-06": (BROWSER_SHA, EXTERNAL["BROWSER_PINS"], BROWSER_SEALS),
        "external-license-07": (NOTICE_TEST_SHA, EXTERNAL["NOTICE_TEST_PINS"], NOTICE_TEST_SEALS),
        "external-license-08": (GPU_SHA, EXTERNAL["GPU_PINS"], GPU_SEALS),
        "external-license-09": (GPU_SUPPORT_SHA, EXTERNAL["GPU_SUPPORT_PINS"], GPU_SUPPORT_SEALS),
    }
    known = reviewed.get(name)
    if known is None or expected_sha != known[0] or pins is not known[1] or seals is not known[2]:
        raise ValueError("fixed reviewed external generation, SHA256, pins and seals required")
    external_dir = source / name
    external, sha = receipt(external_dir / "external-source-inventory.json", expected_sha)
    if (external["sourceCommit"] != TREE["COMMIT"] or external["collectionComplete"] is not True
            or external["parentCatalogSha256"] != EXTERNAL["CATALOG_SHA"]
            or set(external["requestedComponents"]) != set(pins)
            or len(external["requestedComponents"]) != len(pins) or set(seals) != set(pins)):
        raise ValueError("complete matched selected-input receipts and component seals required")
    for key in external["requestedComponents"]:
        component_dir = external_dir / key
        component, component_sha = receipt(component_dir / "license-source-inventory.json", seals[key])
        prefix, repository, revision = pins[key]
        if ((component["checkoutPath"], component["repository"], component["revision"]) != (prefix, repository, revision)
                or component["collectionComplete"] is not True):
            raise ValueError("external source receipt pin mismatch")
        if (pins is EXTERNAL["RUNTIME_PINS"] or pins is EXTERNAL["SUPPORT_PINS"] or pins is EXTERNAL["BROWSER_PINS"]
                or pins is EXTERNAL["NOTICE_TEST_PINS"] or pins is EXTERNAL["GPU_PINS"] or pins is EXTERNAL["GPU_SUPPORT_PINS"]) and (
                component["declaredCheckoutCondition"] != EXTERNAL["SOURCE_CONDITIONS"].get(key)
                or component["conditionsEvaluated"] is not False):
            raise ValueError("exact non-evaluated declared checkout condition required")
        contents = read_data(component_dir / "source-tree.nul", TREE["MAX_TREE"])
        if hashlib.sha256(contents).hexdigest() != component["treeSha256"] or len(contents) != component["treeBytes"]:
            raise ValueError("external source tree identity mismatch")
        rows = TREE["tree_records"](contents)
        if len(rows) != component["treeEntries"]:
            raise ValueError("external source tree entry count mismatch")
        merge_tree(index, rows, prefix)
        merge_files(index, component["selectedFiles"], component_dir, prefix, raw_files, references)
        components.append({"component": key, "revision": revision, "receiptSha256": component_sha})
    return sha


def load_inputs(source, include_media=False, include_runtime=False, include_support=False, include_browser=False):
    if any(type(value) is not bool for value in (include_media, include_runtime, include_support, include_browser)):
        raise ValueError("explicit boolean source group selection required")
    checked_source(source)
    main_dir = source / "license-tree-02"
    main, main_sha = receipt(main_dir / "license-source-inventory.json", EXTERNAL["MAIN_RECEIPT_SHA"])
    if main["sourceCommit"] != TREE["COMMIT"] or main["collectionComplete"] is not True:
        raise ValueError("complete matched main selected-input receipt required")
    contents = read_data(main_dir / "main-tree.nul", TREE["MAX_TREE"])
    if hashlib.sha256(contents).hexdigest() != main["fullMainTreeSha256"]:
        raise ValueError("main tree index SHA256 mismatch")
    rows = TREE["tree_records"](contents)
    if len(rows) != main["fullMainTreeEntries"]:
        raise ValueError("main tree entry count mismatch")
    index, raw_files, references, components = {}, {}, [], []
    merge_tree(index, rows, "")
    merge_files(index, main["selectedFiles"], main_dir, "", raw_files, references)
    external_sha = append_external(source, "external-license-02", EXTERNAL_SHA, EXTERNAL["PINS"], CORE_SEALS,
                                   index, raw_files, references, components)
    media_sha = (append_external(source, "external-license-03", MEDIA_SHA, EXTERNAL["MEDIA_PINS"], MEDIA_SEALS,
                                 index, raw_files, references, components) if include_media else None)
    runtime_sha = (append_external(source, "external-license-04", RUNTIME_SHA, EXTERNAL["RUNTIME_PINS"], RUNTIME_SEALS,
                                   index, raw_files, references, components) if include_runtime else None)
    support_sha = (append_external(source, "external-license-05", SUPPORT_SHA, EXTERNAL["SUPPORT_PINS"], SUPPORT_SEALS,
                                   index, raw_files, references, components) if include_support else None)
    browser_sha = (append_external(source, "external-license-06", BROWSER_SHA, EXTERNAL["BROWSER_PINS"], BROWSER_SEALS,
                                   index, raw_files, references, components) if include_browser else None)
    return {"main": main, "mainSha256": main_sha, "externalSha256": external_sha,
            "mediaSha256": media_sha, "runtimeSha256": runtime_sha, "supportSha256": support_sha,
            "browserSha256": browser_sha,
            "index": index, "files": raw_files, "references": references, "components": components}


def inspect(source, output, include_media=False, include_runtime=False, include_support=False, include_browser=False):
    if (output.parent != source or not re.fullmatch(r"combined-license-inputs-[0-9]{2}\.json", output.name)):
        raise ValueError("safe fresh closure receipt required")
    if output.exists() or output.is_symlink():
        raise FileExistsError("never overwrite closure evidence")
    inputs = load_inputs(source, include_media, include_runtime, include_support, include_browser)
    main, main_sha, external_sha = inputs["main"], inputs["mainSha256"], inputs["externalSha256"]
    index, raw_files, references, components = (inputs[key] for key in ("index", "files", "references", "components"))
    classified = [{**row, "resolution": classify(row, index, raw_files)} for row in references]
    resolved = [row for row in classified if row["resolution"] == "resolved_to_verified_source_bytes"]
    missing = [row for row in classified if row["resolution"] != "resolved_to_verified_source_bytes"]
    prior_resolved = [row for row in main["missingReferences"]
                      if classify(row, index, raw_files) == "resolved_to_verified_source_bytes"]
    report = {"scope": "verified raw license-reference closure only, not shipping dependency count or full NOTICE/SBOM",
              "sourceCommit": TREE["COMMIT"], "mainReceiptSha256": main_sha,
              "externalReceiptSha256": external_sha, "components": components,
              "verifiedSelectedFiles": len(raw_files),
              "verifiedSourceBytesByPath": sum(row["bytes"] for row in raw_files.values()),
              "rawLicenseReferences": len(references), "resolvedRawReferences": len(resolved),
              "unresolvedRawReferences": missing,
              "previousMainMissingResolved": prior_resolved,
              "remainingParentGitSourceDeclarations": 273 - len(components),
              "parentNonGitDeclarations": 157, "recursiveDependenciesCollected": False,
              "conditionsEvaluated": False, "generatorExecuted": False,
              "binaryComponentCoverage": "not_proven", "licenseCoverage": "not_proven", "promoted": False}
    if include_media:
        report["mediaReceiptSha256"] = inputs["mediaSha256"]
    if include_runtime:
        report["runtimeReceiptSha256"] = inputs["runtimeSha256"]
    if include_support:
        report["supportReceiptSha256"] = inputs["supportSha256"]
    if include_browser:
        report["browserReceiptSha256"] = inputs["browserSha256"]
    EXTERNAL["write_report"](output, report)
    return report


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--output-name", required=True)
    parser.add_argument("--include-media", action="store_true")
    parser.add_argument("--include-runtime", action="store_true")
    parser.add_argument("--include-support", action="store_true")
    parser.add_argument("--include-browser", action="store_true")
    args = parser.parse_args()
    source = args.source.absolute()
    report = inspect(source, source / args.output_name, args.include_media, args.include_runtime,
                     args.include_support, args.include_browser)
    print(json.dumps({"verifiedSelectedFiles": report["verifiedSelectedFiles"],
                      "rawLicenseReferences": report["rawLicenseReferences"],
                      "resolvedRawReferences": report["resolvedRawReferences"],
                      "unresolvedRawReferences": len(report["unresolvedRawReferences"]),
                      "previousMainMissingResolved": len(report["previousMainMissingResolved"]),
                      "licenseCoverage": report["licenseCoverage"], "promoted": report["promoted"]}))
