"""Data-only fixed source inventory contract, not generator/native coverage."""
import hashlib
import importlib.util
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location("source_tree", Path(__file__).with_name("inspect-chromium-license-tree.py"))
SUBJECT = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(SUBJECT)
OID = "a" * 40


class TreeTests(unittest.TestCase):
    def row(self, path, mode="100644", kind="blob"):
        return {"path": path, "mode": mode, "kind": kind, "gitObjectSha1": OID}

    def test_complete_nul_tree_and_gitlink_identity(self):
        rows = SUBJECT.tree_records(f"100644 blob {OID}\tLICENSE\0".encode() +
                                    f"160000 commit {OID}\tv8\0".encode())
        self.assertEqual(len(rows), 2)
        self.assertEqual(rows[1], self.row("v8", "160000", "commit"))

    def test_empty_truncated_bad_modes_duplicate_paths_and_escape_reject(self):
        for content in (b"", b"unterminated", f"100644 blob bad\tLICENSE\0".encode(),
                        f"100644 blob {OID}\t../LICENSE\0".encode(),
                        f"100644 blob {OID}\t/a\0".encode(),
                        f"040000 tree {OID}\tx\0".encode(),
                        f"100644 blob {OID}\tx\0".encode() * 2):
            with self.subTest(content=content), self.assertRaises(ValueError):
                SUBJECT.tree_records(content)

    def test_symlinks_stay_in_index_and_never_selected_or_materialized(self):
        content = f"120000 blob {OID}\tthird_party/foo/LICENSE\0".encode()
        row = SUBJECT.tree_records(content)[0]
        self.assertFalse(SUBJECT.selected(row))

    def test_metadata_license_notices_support_and_nonstandard_refs_select(self):
        for path in ("third_party/foo/README.chromium", "v8/README.v8", "LICENSE",
                     "tools/licenses/licenses.py", "foo/LICENSE-MIT", "foo/COPYING.txt",
                     "foo/MIT-LICENSE", "foo/additional_readme_paths.json", "foo/DEPS"):
            self.assertTrue(SUBJECT.selected(self.row(path)), path)
        for path in ("foo/source.cc", "foo/README.md", "foo/package.json", "foo/NOTICED.cc"):
            if path == "foo/NOTICED.cc":
                # Name-based overcollection is honest; it never becomes a dependency declaration.
                self.assertTrue(SUBJECT.selected(self.row(path)))
            else:
                self.assertFalse(SUBJECT.selected(self.row(path)))

    def test_metadata_reference_root_relative_comma_and_not_shipped(self):
        refs = SUBJECT.references(self.row("third_party/foo/README.chromium"),
                                  b"License File: LICENSE, /LICENSE\nLicense File: NOT_SHIPPED\n")
        self.assertEqual([row["sourcePath"] for row in refs], ["third_party/foo/LICENSE", "LICENSE"])

    def test_metadata_relative_parent_remains_inside_source(self):
        refs = SUBJECT.references(self.row("third_party/foo/README.chromium"), b"License File: ../COPYING\n")
        self.assertEqual(refs[0]["sourcePath"], "third_party/COPYING")

    def test_unsafe_metadata_reference_never_becomes_local_path(self):
        for value in ("../../../outside", "C:secret", "a\\b", "a\0b"):
            with self.subTest(value=value), self.assertRaises(ValueError):
                SUBJECT.references(self.row("third_party/foo/README.chromium"),
                                   ("License File: " + value).encode())

    def test_metadata_parser_does_not_execute_python_and_nonmetadata_is_not_parsed(self):
        self.assertEqual(SUBJECT.references(self.row("DEPS"), b"raise RuntimeError('must not execute')"), [])
        self.assertEqual(SUBJECT.references(self.row("third_party/foo/README.chromium"),
                                          b"__import__('os').system('must not execute')\n"), [])

    def test_real_nonshipped_remote_license_reference_is_retained_not_followed(self):
        refs = SUBJECT.references(self.row("third_party/ninja/README.crashpad"),
                                  b"License File: https://github.com/ninja-build/ninja/blob/master/COPYING\nShipped: no\n")
        self.assertIsNone(refs[0]["sourcePath"])
        self.assertIn("not followed", refs[0]["reason"])
        with self.assertRaises(ValueError):
            SUBJECT.references(self.row("third_party/ninja/README.crashpad"),
                               b"License File: https://user:secret@example.org/LICENSE\n")

    def test_batch_object_bytes_include_newline_nul_and_header_like_data(self):
        data = b"a\n\0blob 3\n"
        oid = hashlib.sha1(b"blob " + str(len(data)).encode() + b"\0" + data).hexdigest()
        body = f"{oid} blob {len(data)}\n".encode() + data + b"\n"
        self.assertEqual(SUBJECT.batch_payloads(body, [oid]), {oid: data})
        for bad in (body[:-1], body + b"extra", body.replace(b" blob ", b" tree ", 1),
                    body.replace(data, b"x" * len(data))):
            with self.subTest(bad=bad), self.assertRaises(ValueError):
                SUBJECT.batch_payloads(bad, [oid])

    def test_actual_git_blob_hash_and_length_not_just_download_sha(self):
        data = b"license data"
        oid = hashlib.sha1(b"blob 12\0" + data).hexdigest()
        self.assertEqual(SUBJECT.check_blob(oid, data),
                         {"bytes": 12, "sha256": hashlib.sha256(data).hexdigest()})
        with self.assertRaises(ValueError):
            SUBJECT.check_blob(oid, b"other bytes")

    def test_foreign_bare_repository_or_output_reject_before_git(self):
        with tempfile.TemporaryDirectory(prefix="grok-cu-license-path-") as name:
            root = Path(name)
            with patch.object(SUBJECT, "git") as command, self.assertRaises(ValueError):
                SUBJECT.collect(root / "user-repository.git", root / "license-tree-01")
            command.assert_not_called()

    def test_existing_evidence_rejects_without_overwrite_or_git(self):
        with tempfile.TemporaryDirectory(prefix="grok-cu-license-path-") as name:
            root = Path(name)
            parent = root / ".run/maintained-notice-source-20261013-01"
            parent.mkdir(parents=True)
            output = parent / "license-tree-01"
            output.mkdir()
            receipt = output / "failure.json"
            receipt.write_text("original failure")
            with patch.object(SUBJECT, "HERE", root), patch.object(SUBJECT, "git") as command, \
                    self.assertRaises(FileExistsError):
                SUBJECT.collect(parent / "chromium-pinned-tree.git", output)
            command.assert_not_called()
            self.assertEqual(receipt.read_text(), "original failure")


if __name__ == "__main__":
    unittest.main()
