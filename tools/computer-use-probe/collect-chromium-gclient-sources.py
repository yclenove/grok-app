"""Collect four fixed depot_tools source/test files as inert source-data.

Uses the sealed same-parent Git tree and original task-owned bare cache.
No upstream imports/execution, gclient sync, hooks, credentials, checkout,
condition decisions, runtime promotion or product mutations.
"""
import argparse
import hashlib
import json
from pathlib import Path
import re
import runpy


HERE = Path(__file__).parent.absolute()
INPUTS = runpy.run_path(str(HERE / "inspect-chromium-license-input-gaps.py"))
EXTERNAL = INPUTS["EXTERNAL"]
TREE = INPUTS["TREE"]
FILES = {
    "gclient.py": ("100755", "1bd193b42c68c038d7c895e7d9981f374de54f3a"),
    "gclient_eval.py": ("100644", "52ab4cf9610c9739c2f25a257dea3677c0213d83"),
    "tests/gclient_test.py": ("100755", "a21e7e7118415907dc4488aba47cc35a509f63d1"),
    "tests/gclient_eval_unittest.py": ("100755", "b2fde0964aa90c7ef34043c4675ea686446d67ee"),
}
MAX_TOTAL = 8 * 1024**2


def selected(rows):
    by_path = {row["path"]: row for row in rows}
    if len(by_path) != len(rows):
        raise ValueError("unique complete helper tree required")
    result = []
    for path, identity in FILES.items():
        row = by_path.get(path)
        if row is None or (row["mode"], row["gitObjectSha1"]) != identity:
            raise ValueError("exact compiled helper path/mode/Git blob required")
        result.append(row)
    return result


def checked_output(source, output):
    if output.parent != source or not re.fullmatch(r"gclient-source-[0-9]{2}", output.name):
        raise ValueError("safe fresh helper evidence directory required")
    if output.exists() or output.is_symlink():
        raise FileExistsError("never overwrite helper source evidence")
    INPUTS["checked_source"](source)


def collect(source, output):
    checked_output(source, output)
    component = source / "external-license-02" / "depot-tools"
    receipt, seal = INPUTS["receipt"](component / "license-source-inventory.json", INPUTS["CORE_SEALS"]["depot-tools"])
    prefix, repository, revision = EXTERNAL["PINS"]["depot-tools"]
    if ((receipt["checkoutPath"], receipt["repository"], receipt["revision"]) != (prefix, repository, revision)
            or receipt["collectionComplete"] is not True
            or receipt["cacheSource"] != "external-license-02/depot-tools/pinned-tree.git"):
        raise ValueError("exact sealed original depot_tools cache required")
    contents = INPUTS["read_data"](component / "source-tree.nul", TREE["MAX_TREE"])
    if len(contents) != receipt["treeBytes"] or hashlib.sha256(contents).hexdigest() != receipt["treeSha256"]:
        raise ValueError("sealed helper full-tree identity mismatch")
    rows = TREE["tree_records"](contents)
    if len(rows) != receipt["treeEntries"]:
        raise ValueError("sealed helper tree count mismatch")
    picked = selected(rows)
    bare = component / "pinned-tree.git"
    TREE["regular_parents"](bare)
    if not bare.is_dir() or bare.resolve(strict=True) != bare:
        raise ValueError("canonical existing original helper bare cache required")
    if (TREE["git"](bare, ["rev-parse", "--is-bare-repository"], 64) != b"true\n"
            or TREE["git"](bare, ["rev-parse", revision + "^{tree}"], 64).decode().strip() != receipt["rootTreeSha1"]):
        raise ValueError("same fixed helper revision/tree and bare cache required")
    output.mkdir()
    blobs = output / "blobs"
    blobs.mkdir()
    report = {"scope": "four pinned gclient implementation/test source files only, not executed inheritance or builder inputs",
              "repository": repository, "revision": revision, "parentSourceCommit": TREE["COMMIT"],
              "depotReceiptSha256": seal, "treeSha256": receipt["treeSha256"], "rootTreeSha1": receipt["rootTreeSha1"],
              "collectionComplete": False, "selectedFiles": [], "sourceBytes": 0,
              "sourceFunctionsExecuted": False, "upstreamTestsExecuted": False, "hooksExecuted": False,
              "conditionsEvaluated": False, "builderCustomVarsKnown": False,
              "binaryComponentCoverage": "not_proven", "shippingDecisionsMade": False, "promoted": False}
    try:
        oids = [row["gitObjectSha1"] for row in picked]
        TREE["fetch_blobs"](bare, oids, repository)
        payloads = TREE["batch_payloads"](TREE["git"](bare, ["cat-file", "--batch"], MAX_TOTAL,
                                                    ("\n".join(oids) + "\n").encode()), oids)
        for row in picked:
            contents = payloads[row["gitObjectSha1"]]
            identity = TREE["check_blob"](row["gitObjectSha1"], contents)
            if report["sourceBytes"] + identity["bytes"] > MAX_TOTAL:
                raise ValueError("helper source-byte budget")
            with (blobs / (row["gitObjectSha1"] + ".source-data")).open("xb") as stream:
                stream.write(contents)
            report["sourceBytes"] += identity["bytes"]
            report["selectedFiles"].append({**row, **identity})
        report["collectionComplete"] = True
    except Exception as error:
        report["failureType"] = type(error).__name__
        raise
    finally:
        EXTERNAL["write_report"](output / "gclient-source-inventory.json", report)
    return report


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--output-name", required=True)
    args = parser.parse_args()
    source = args.source.absolute()
    report = collect(source, source / args.output_name)
    print(json.dumps({"files": len(report["selectedFiles"]), "sourceBytes": report["sourceBytes"],
                      "collectionComplete": report["collectionComplete"], "sourceFunctionsExecuted": False}))
