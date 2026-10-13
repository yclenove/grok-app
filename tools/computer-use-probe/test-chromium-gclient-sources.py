"""Exact inert helper source collection, never execute upstream tests."""
from pathlib import Path
import runpy
import tempfile
import unittest


SUBJECT = runpy.run_path(str(Path(__file__).with_name("collect-chromium-gclient-sources.py")))


class GclientSourceTests(unittest.TestCase):
    def rows(self):
        return [{"path": path, "mode": mode, "gitObjectSha1": oid}
                for path, (mode, oid) in SUBJECT["FILES"].items()]

    def test_exact_complete_selected_source_and_tests_never_add_other_paths(self):
        rows = self.rows() + [{"path": "not-selected.py", "mode": "100755", "gitObjectSha1": "a" * 40}]
        picked = SUBJECT["selected"](rows)
        self.assertEqual([row["path"] for row in picked], list(SUBJECT["FILES"]))
        self.assertEqual(len(picked), 4)

    def test_missing_or_duplicate_tree_identity_rejects_before_fetch(self):
        for rows in (self.rows()[:-1], self.rows() + self.rows()[:1]):
            with self.subTest(rows=rows), self.assertRaises(ValueError):
                SUBJECT["selected"](rows)

    def test_altered_git_oid_path_or_symlink_mode_rejects(self):
        for key, value in (("gitObjectSha1", "b" * 40), ("path", "other/gclient.py"), ("mode", "120000")):
            rows = self.rows()
            rows[0] = {**rows[0], key: value}
            with self.subTest(key=key), self.assertRaises(ValueError):
                SUBJECT["selected"](rows)

    def test_foreign_source_or_unsafe_directory_rejects_before_git(self):
        for name in ("../escape", "gclient-source-bad", "gclient-source-01/inner"):
            with self.subTest(name=name), self.assertRaises(ValueError):
                SUBJECT["collect"](Path("foreign"), Path("foreign") / name)
        with self.assertRaises(ValueError):
            SUBJECT["collect"](Path("foreign"), Path("foreign/gclient-source-01"))

    def test_old_evidence_directory_is_never_overwritten(self):
        with tempfile.TemporaryDirectory(prefix="grok-cu-gclient-sources-") as name:
            root = Path(name)
            output = root / "gclient-source-01"
            output.mkdir()
            evidence = output / "old-evidence"
            evidence.write_bytes(b"retained")
            with self.assertRaises(FileExistsError):
                SUBJECT["collect"](root, output)
            self.assertEqual(evidence.read_bytes(), b"retained")


if __name__ == "__main__":
    unittest.main()
