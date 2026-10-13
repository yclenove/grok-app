"""Fixed source identities, bounded reads, root-tree integrity and retained failure."""
import base64
import copy
import hashlib
import json
from pathlib import Path
import runpy
import tempfile
import unittest
from unittest.mock import patch


SUBJECT = runpy.run_path(str(Path(__file__).with_name("collect-chromium-crashpad-lss-sources.py")))


def blob(contents):
    return hashlib.sha1(b"blob " + str(len(contents)).encode() + b"\0" + contents).hexdigest()


def wire(value):
    return b")]}'\n" + json.dumps(value).encode()


class CrashpadSourceTests(unittest.TestCase):
    def tree(self):
        entries = [{"name": "linux_syscall_support.h", "mode": 0o100644, "type": "blob", "id": "a" * 40},
                   {"name": "tests", "mode": 0o40000, "type": "tree", "id": "b" * 40}]
        raw = b"100644 linux_syscall_support.h\0" + bytes.fromhex("a" * 40)
        raw += b"40000 tests\0" + bytes.fromhex("b" * 40)
        oid = hashlib.sha1(b"tree " + str(len(raw)).encode() + b"\0" + raw).hexdigest()
        return {"id": oid, "entries": entries}

    def test_main_and_default_urls_are_fixed_to_exact_git_blobs(self):
        rows = SUBJECT["main_requests"]() + [SUBJECT["DEFAULT_PIN"]]
        self.assertEqual(len(rows), 5)
        for row in rows:
            self.assertIn("/+/" + SUBJECT["COMMIT"] + "/" + row["path"], row["url"])
            self.assertEqual(len(row["gitObjectSha1"]), 40)
        self.assertIn(SUBJECT["REVISION"], SUBJECT["TREE_URL"])

    def test_foreign_url_revision_path_or_unselected_file_refused_before_network(self):
        for url in ("https://example.test/file", SUBJECT["TREE_URL"].replace(SUBJECT["REVISION"], "a" * 40),
                    SUBJECT["GITILES"] + "/+/" + SUBJECT["REVISION"] + "/../LICENSE?format=TEXT",
                    SUBJECT["GITILES"] + "/+/" + SUBJECT["REVISION"] + "/OWNERS?format=TEXT"):
            with self.subTest(url=url), self.assertRaises(ValueError):
                SUBJECT["fetch"](url, "text/plain")

    def test_root_tree_hash_is_reconstructed_and_order_independent(self):
        row = self.tree()
        self.assertEqual(SUBJECT["checked_tree"](wire(row), row["id"]), row)
        row["entries"].reverse()
        self.assertEqual(SUBJECT["checked_tree"](wire(row), row["id"]), row)

    def test_omission_duplicate_unsafe_name_changed_mode_or_object_refused(self):
        original = self.tree()
        values = []
        omitted = copy.deepcopy(original)
        omitted["entries"].pop()
        values.append(omitted)
        duplicate = copy.deepcopy(original)
        duplicate["entries"].append(duplicate["entries"][0])
        values.append(duplicate)
        for key, value in (("name", "../LICENSE"), ("id", "c" * 40), ("mode", 0o100755), ("type", "tree")):
            mutated = copy.deepcopy(original)
            mutated["entries"][0][key] = value
            values.append(mutated)
        for value in values:
            with self.subTest(value=value), self.assertRaises(ValueError):
                SUBJECT["checked_tree"](wire(value), original["id"])

    def test_commit_json_wrong_revision_prefix_or_budget_refused(self):
        row = {"commit": SUBJECT["REVISION"], "tree": self.tree()["id"]}
        self.assertEqual(SUBJECT["checked_commit"](wire(row)), row)
        for raw in (wire({**row, "commit": "a" * 40}), b"{}", b")]}'\n[]", b"x" * (SUBJECT["MAX_RAW"] + 1)):
            with self.subTest(size=len(raw)), self.assertRaises(ValueError):
                SUBJECT["checked_commit"](raw)

    def test_only_regular_root_header_metadata_and_license_blobs_selected(self):
        row = {"name": "LICENSE", "mode": 0o100644, "type": "blob", "id": "a" * 40}
        self.assertEqual(SUBJECT["lss_request"](row)["path"], "LICENSE")
        for changed in ({"name": "nested/LICENSE"}, {"mode": 0o120000}, {"type": "tree"}, {"name": "OWNERS"}):
            with self.subTest(changed=changed), self.assertRaises(ValueError):
                SUBJECT["lss_request"]({**row, **changed})

    def test_valid_utf8_blob_verified_without_executing_contents(self):
        data = b"raise RuntimeError('inert only')\n"
        decoded, identity = SUBJECT["decoded_blob"](base64.b64encode(data), blob(data))
        self.assertEqual(decoded, data)
        self.assertEqual(identity, {"bytes": len(data), "sha256": hashlib.sha256(data).hexdigest()})
        with self.assertRaises(ValueError):
            SUBJECT["decoded_blob"](base64.b64encode(data + b"changed"), blob(data))

    def test_empty_invalid_oversize_or_non_utf8_blob_refused(self):
        for raw in (b"", b"invalid!", base64.b64encode(b""), b"A" * (SUBJECT["MAX_RAW"] + 1)):
            with self.subTest(size=len(raw)), self.assertRaises(ValueError):
                SUBJECT["decoded_blob"](raw, "a" * 40)
        data = b"x" * (SUBJECT["MAX_SOURCE"] + 1)
        with self.assertRaises(ValueError):
            SUBJECT["decoded_blob"](base64.b64encode(data), blob(data))
        with self.assertRaises(UnicodeDecodeError):
            SUBJECT["decoded_blob"](base64.b64encode(b"\xff"), blob(b"\xff"))

    def input_fixture(self):
        index = {path: {"mode": "100644", "gitObjectSha1": oid} for path, oid in SUBJECT["MAIN_PINS"].items()}
        index[SUBJECT["PREFIX"]] = {"mode": "160000", "gitObjectSha1": SUBJECT["REVISION"]}
        catalog = {"gitDependencies": [{"path": SUBJECT["PREFIX"], "repository": SUBJECT["REPOSITORY"],
                    "revision": SUBJECT["REVISION"], "condition": "checkout_android or checkout_linux",
                    "gitlinkRevision": SUBJECT["REVISION"], "gitlinkMatched": True}]}
        targets = [{"target": target, "manifest": {}} for target in ("windows-x64", "linux-x64", "mac-intel", "mac-arm64")]
        return index, catalog, targets

    def test_exact_linux_only_manifest_correspondence_without_shipping_exemption(self):
        index, catalog, targets = self.input_fixture()
        def compare(path, repository, revision, manifest):
            self.assertEqual((path, repository, revision), ("src/" + SUBJECT["PREFIX"], SUBJECT["REPOSITORY"], SUBJECT["REVISION"]))
            status = "exact_repository_and_revision_match" if manifest.get("linux") else "exact_manifest_entry_absent"
            return {"status": status}
        targets[1]["manifest"] = {"linux": True}
        with patch.dict(SUBJECT["INPUTS"], load_inputs=lambda *_: {"index": index, "mainSha256": "main"}, read_data=lambda *_: b"catalog"), \
                patch.dict(SUBJECT["EXTERNAL"], checked_catalog=lambda *_: catalog), \
                patch.dict(SUBJECT["CHECKOUTS"], inspect=lambda *_: {"targets": targets, "checkoutReceiptSha256": "checkout"}, compare_pin=compare):
            main, checkout, rows = SUBJECT["checked_inputs"](Path("fixture"))
            self.assertEqual((main, checkout), ("main", "checkout"))
            self.assertEqual(rows[1]["status"], "exact_repository_and_revision_match")
            targets[0]["manifest"] = {"linux": True}
            with self.assertRaisesRegex(ValueError, "four-platform"):
                SUBJECT["checked_inputs"](Path("fixture"))

    def test_main_tree_pin_disagreement_rejects_before_catalog_or_network(self):
        index, _, _ = self.input_fixture()
        index[SUBJECT["PREFIX"]]["gitObjectSha1"] = "a" * 40
        with patch.dict(SUBJECT["INPUTS"], load_inputs=lambda *_: {"index": index}), self.assertRaisesRegex(ValueError, "Gitlink"):
            SUBJECT["checked_inputs"](Path("fixture"))

    def test_redirect_content_type_or_oversize_response_rejected(self):
        url = SUBJECT["main_requests"]()[0]["url"]
        class Response:
            headers = {"Content-Type": "text/plain"}
            data = b"eA=="
            actual_url = url
            def geturl(self):
                return self.actual_url
            def read(self, size):
                self.asserted_size = size
                return self.data[:size]
            def __enter__(self):
                return self
            def __exit__(self, *_):
                pass
        class Opener:
            def open(inner, request, timeout):
                self.assertEqual(timeout, 20)
                self.assertEqual(request.full_url, url)
                return response
        response = Response()
        with patch.object(SUBJECT["urllib"].request, "build_opener", return_value=Opener()):
            self.assertEqual(SUBJECT["fetch"](url, "text/plain"), b"eA==")
            self.assertEqual(response.asserted_size, SUBJECT["MAX_RAW"] + 1)
            response.actual_url = "https://example.test/file"
            with self.assertRaises(ValueError):
                SUBJECT["fetch"](url, "text/plain")
            response.actual_url = url
            response.headers = {"Content-Type": "text/html"}
            with self.assertRaises(ValueError):
                SUBJECT["fetch"](url, "text/plain")
            response.headers = {"Content-Type": "text/plain"}
            response.data = b"x" * (SUBJECT["MAX_RAW"] + 1)
            with self.assertRaises(ValueError):
                SUBJECT["fetch"](url, "text/plain")

    def test_foreign_escaping_or_existing_output_refused_for_both_profiles(self):
        with tempfile.TemporaryDirectory(prefix="grok-cu-crashpad-") as name:
            root = Path(name)
            for function, prefix in (("collect", "crashpad-lss-source"), ("collect_defaults", "crashpad-default-source")):
                for output in (root / "other", root / ("../" + prefix + "-01")):
                    with self.subTest(function=function, output=output), self.assertRaises(ValueError):
                        SUBJECT[function](root, output)
                output = root / (prefix + "-01")
                output.mkdir()
                with self.assertRaises(FileExistsError):
                    SUBJECT[function](root, output)

    def test_late_decode_failure_retains_original_raw_and_partial_receipt(self):
        with tempfile.TemporaryDirectory(prefix="grok-cu-crashpad-") as name:
            root = Path(name)
            with patch.dict(SUBJECT["collect"].__globals__, checked_inputs=lambda *_: ("main", "checkout", []), fetch=lambda *_: b"invalid!"), \
                    self.assertRaises(ValueError):
                SUBJECT["collect"](root, root / "crashpad-lss-source-01")
            report = json.loads((root / "crashpad-lss-source-01/crashpad-lss-inventory.json").read_text())
            self.assertFalse(report["collectionComplete"])
            self.assertFalse(report["records"][0]["verified"])
            self.assertEqual((root / "crashpad-lss-source-01" / report["records"][0]["rawFile"]).read_bytes(), b"invalid!")
            for key in ("conditionsEvaluated", "generatorExecuted", "effectiveGnMeasured", "promoted"):
                self.assertFalse(report[key])


if __name__ == "__main__":
    unittest.main()
