"""Original library and test-image permission texts cannot be confused or lost."""
import copy
import hashlib
from pathlib import Path
import runpy
import tempfile
import unittest
from unittest.mock import Mock, patch


SUBJECT = runpy.run_path(str(Path(__file__).with_name("inspect-chromium-angle-library-notices.py")))


class AngleLibraryNoticeTests(unittest.TestCase):
    def notices(self):
        result = []
        for key, (prefix, _, _) in SUBJECT["PINS"].items():
            paths = [SUBJECT["LICENSES"][key]] + (sorted(SUBJECT["TEST_IMAGES"]) if key == "astc-encoder" else [])
            for path in paths:
                data = (key + ": original permission\r\noriginal disclaimer\r\n").encode()
                if key == "cpu-features":
                    data += b"For files in the ndk_compat folder:\nCopyright (C) 2010 The Android Open Source Project\nBSD conditions and disclaimer\n"
                result.append(({"sourcePath": prefix + "/" + path, "bytes": len(data), "sha256": hashlib.sha256(data).hexdigest(),
                    "role": "declared_library_license" if path == SUBJECT["LICENSES"][key] else "test_image_license"}, data))
        return result

    def metadata(self, key):
        prefix, repository, revision = SUBJECT["PINS"][key]
        declaration = {"path": prefix[len("third_party/angle/"):], "repository": repository,
            "revision": revision, "gitlinkRevision": revision, "gitlinkMatched": True}
        metadata = {"directory": prefix[:-4], "shippedMetadata": "yes",
            "declaredLicensePaths": [prefix + "/" + SUBJECT["LICENSES"][key]], "errors": ["original missing text"]}
        return declaration, metadata

    def test_entire_original_permission_crlf_and_android_bsd_appendix_survive(self):
        notices = self.notices()
        fragment = SUBJECT["render_fragment"](notices)
        self.assertTrue(all(data in fragment for _, data in notices))
        self.assertIn(b"Copyright (C) 2010 The Android Open Source Project", fragment)
        self.assertEqual(fragment.count(b"Role: test_image_license"), 2)
        self.assertEqual(fragment.count(b"Role: declared_library_license"), 3)

    def test_missing_duplicate_or_redirected_permission_paths_refuse(self):
        notices = self.notices()
        changed = copy.deepcopy(notices)
        changed[0][0]["sourcePath"] = "third_party/unrelated/LICENSE"
        wrong_role = copy.deepcopy(notices)
        next(row for row, _ in wrong_role if row["role"] == "test_image_license")["role"] = "declared_library_license"
        for value in (notices[:-1], notices[:-1] + notices[:1], changed, wrong_role):
            with self.assertRaises(ValueError):
                SUBJECT["render_fragment"](value)

    def test_truncated_bsd_tail_or_other_changed_notice_bytes_refuse(self):
        for mode in ("empty", "bsd-tail", "wrong-hash"):
            notices = self.notices()
            row, data = notices[-1]
            if mode == "empty":
                notices[-1] = row, b""
            elif mode == "bsd-tail":
                notices[-1] = row, data.split(b"For files")[0]
            else:
                row["sha256"] = "0" * 64
            with self.subTest(mode=mode), self.assertRaises(ValueError):
                SUBJECT["render_fragment"](notices)

    def test_total_byte_budget_includes_labels_and_separators(self):
        notices = self.notices()
        fragment = SUBJECT["render_fragment"](notices)
        with patch.dict(SUBJECT["render_fragment"].__globals__, MAX_FRAGMENT=len(fragment)):
            self.assertEqual(SUBJECT["render_fragment"](notices), fragment)
        with patch.dict(SUBJECT["render_fragment"].__globals__, MAX_FRAGMENT=len(fragment) - 1), self.assertRaises(ValueError):
            SUBJECT["render_fragment"](notices)

    def test_original_declared_paths_and_pins_are_accepted_without_mutation(self):
        for key in SUBJECT["PINS"]:
            declaration, metadata = self.metadata(key)
            original = copy.deepcopy((declaration, metadata))
            SUBJECT["checked_metadata"](key, declaration, metadata)
            self.assertEqual((declaration, metadata), original)

    def test_wrong_gitlink_license_path_removed_error_or_shipped_flag_refuses(self):
        key = "cpu-features"
        for field, value in (("gitlinkRevision", "0" * 40), ("declaredLicensePaths", ["LICENSE"]),
                ("errors", []), ("shippedMetadata", "no")):
            declaration, metadata = self.metadata(key)
            (declaration if field == "gitlinkRevision" else metadata)[field] = value
            with self.subTest(field=field), self.assertRaises(ValueError):
                SUBJECT["checked_metadata"](key, declaration, metadata)

    def test_unsafe_or_existing_output_refuses_before_loading_sources(self):
        with tempfile.TemporaryDirectory(prefix="grok-cu-angle-notices-") as name:
            source = Path(name)
            existing = source / "angle-library-notices-01"
            existing.mkdir()
            loader = Mock(side_effect=AssertionError("must not load sources"))
            with patch.dict(SUBJECT["INPUTS"], checked_source=lambda _source: None), patch.dict(SUBJECT["REGISTRY"], checked_inputs=loader):
                with self.assertRaises(FileExistsError):
                    SUBJECT["inspect"](source, existing)
                for output in (source / "../angle-library-notices-02", source / "angle-library-notices-bad"):
                    with self.assertRaises(ValueError):
                        SUBJECT["inspect"](source, output)
            loader.assert_not_called()


if __name__ == "__main__":
    unittest.main()
