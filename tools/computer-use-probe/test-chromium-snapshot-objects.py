"""Object-byte matching and time discrepancies never become build attestation."""
import base64
import hashlib
import json
from pathlib import Path
import runpy
from types import SimpleNamespace
import tempfile
import unittest
from unittest.mock import patch


SUBJECT = runpy.run_path(str(Path(__file__).with_name("collect-chromium-snapshot-objects.py")))
BUILD_TESTS = runpy.run_path(str(Path(__file__).with_name("test-chromium-archive-builds.py")))


class FixtureCrc:
    def __init__(self):
        self.data = b""

    def update(self, data):
        self.data += data

    def digest(self):
        if self.data != b"abc":
            raise ValueError("fixture must hash all original bytes")
        return bytes.fromhex("364b3fb7")


class SnapshotObjectTests(unittest.TestCase):
    def metadata(self, target="windows-x64"):
        return {"kind": "storage#object", "bucket": "chromium-browser-snapshots", "name": SUBJECT["object_name"](target),
                "generation": SUBJECT["GENERATIONS"][target], "size": "3",
                "md5Hash": base64.b64encode(hashlib.md5(b"abc").digest()).decode(),
                "crc32c": base64.b64encode(bytes.fromhex("364b3fb7")).decode(),
                "timeCreated": "2026-10-12T12:07:16.44Z", "updated": "2026-10-12T12:07:16.44Z"}

    def original(self):
        return {"metadata": self.metadata(), "sha256": hashlib.sha256(b"abc").hexdigest()}

    def build(self, target="windows-x64"):
        result = BUILD_TESTS["ArchiveBuildTests"]().build(target)
        result["steps"][0].update(startTime="2026-10-12T12:07:09.32652Z", endTime="2026-10-12T12:07:16.415938Z")
        return result

    def test_only_four_compiled_objects_and_explicit_original_generations(self):
        for target in SUBJECT["GENERATIONS"]:
            url = SUBJECT["object_url"](target)
            self.assertTrue(url.startswith("https://storage.googleapis.com/storage/v1/b/chromium-browser-snapshots/o/"))
            self.assertTrue(url.endswith("?generation=" + SUBJECT["GENERATIONS"][target]))
            self.assertIn("%2F1716663%2F", url)
        with self.assertRaises(ValueError):
            SUBJECT["object_url"]("arbitrary")

    def test_complete_original_object_metadata_and_checksum_lengths(self):
        for target in SUBJECT["GENERATIONS"]:
            result = SUBJECT["object_metadata"](target, json.dumps(self.metadata(target)).encode())
            self.assertEqual(result["generation"], SUBJECT["GENERATIONS"][target])
        for key, value in (("bucket", "other"), ("generation", "1791806836355025"), ("name", "../escape"),
                           ("size", True), ("size", str(SUBJECT["MAX_ZIP"] + 1)), ("md5Hash", ""),
                           ("crc32c", "AAAA"), ("timeCreated", "not UTC")):
            with self.subTest(key=key), self.assertRaises(ValueError):
                SUBJECT["object_metadata"]("windows-x64", json.dumps({**self.metadata(), key: value}).encode())

    def test_nonobject_binary_and_oversize_metadata_reject(self):
        for data in (b"", b"[]", b"not json", b"\x91", b"x" * (SUBJECT["MAX_METADATA"] + 1)):
            with self.subTest(length=len(data)), self.assertRaises((ValueError, UnicodeError)):
                SUBJECT["object_metadata"]("windows-x64", data)

    def test_streamed_sha_md5_crc_all_match_with_chunking_and_no_execution(self):
        with tempfile.TemporaryDirectory(prefix="grok-cu-snapshot-object-") as name:
            path = Path(name) / "original.zip"
            path.write_bytes(b"abc")
            with patch.dict(SUBJECT["verify_zip"].__globals__, CHUNK=1):
                result = SUBJECT["verify_zip"](path, self.original(), self.metadata(), FixtureCrc)
            self.assertTrue(result["originalSha256Matched"])
            self.assertTrue(result["objectMd5Matched"])
            self.assertTrue(result["objectCrc32cMatched"])
            self.assertTrue(result["gcsGenerationBytesMatched"])
            self.assertEqual(path.read_bytes(), b"abc")

    def test_any_size_sha_md5_crc_mismatch_keeps_original_zip(self):
        with tempfile.TemporaryDirectory(prefix="grok-cu-snapshot-object-") as name:
            path = Path(name) / "original.zip"
            path.write_bytes(b"abc")
            for variant in ("size", "sha", "md5", "crc"):
                original, current = self.original(), self.metadata()
                if variant == "size":
                    current["size"] = "4"
                elif variant == "sha":
                    original["sha256"] = "a" * 64
                else:
                    current["md5Hash" if variant == "md5" else "crc32c"] = "other"
                with self.subTest(variant=variant), self.assertRaises(ValueError):
                    SUBJECT["verify_zip"](path, original, current, FixtureCrc)
                self.assertEqual(path.read_bytes(), b"abc")

    def test_windows_stat_ctime_difference_allowed_only_when_each_identity_stable(self):
        with tempfile.TemporaryDirectory(prefix="grok-cu-snapshot-object-") as name:
            path = Path(name) / "original.zip"
            path.write_bytes(b"abc")
            actual = path.stat()
            def fstat(_):
                return SimpleNamespace(st_dev=actual.st_dev, st_ino=actual.st_ino, st_size=actual.st_size,
                                       st_mtime_ns=actual.st_mtime_ns, st_ctime_ns=actual.st_ctime_ns + 1000)
            with patch.object(SUBJECT["os"], "fstat", fstat):
                self.assertTrue(SUBJECT["verify_zip"](path, self.original(), self.metadata(), FixtureCrc)["gcsGenerationBytesMatched"])

    def test_handle_identity_change_reject_not_ignored_as_ctime_semantics(self):
        with tempfile.TemporaryDirectory(prefix="grok-cu-snapshot-object-") as name:
            path = Path(name) / "original.zip"
            path.write_bytes(b"abc")
            actual = path.stat()
            calls = []
            def fstat(_):
                calls.append(1)
                return SimpleNamespace(st_dev=actual.st_dev, st_ino=actual.st_ino, st_size=actual.st_size,
                                       st_mtime_ns=actual.st_mtime_ns, st_ctime_ns=actual.st_ctime_ns + len(calls))
            with patch.object(SUBJECT["os"], "fstat", fstat), self.assertRaisesRegex(ValueError, "changed while"):
                SUBJECT["verify_zip"](path, self.original(), self.metadata(), FixtureCrc)

    def test_original_24ms_timestamp_discrepancy_preserved_not_build_proof(self):
        result = SUBJECT["upload_times"]("windows-x64", self.build(), self.metadata())
        self.assertFalse(result["createdWithinUploadStep"])
        self.assertEqual(result["createdAfterUploadEndMs"], 24.062)
        self.assertFalse(result["timestampDifferenceIgnored"])
        self.assertFalse(result["publisherGenerationCryptographicallyBound"])

    def test_utc_fractional_width_is_data_and_microsecond_precision_explicit(self):
        self.assertEqual(SUBJECT["utc_time"]("2026-10-12T12:07:16.44Z"),
                         SUBJECT["utc_time"]("2026-10-12T12:07:16.440000Z"))
        self.assertEqual(SUBJECT["utc_time"]("2026-10-12T12:07:16.440000123Z"),
                         SUBJECT["utc_time"]("2026-10-12T12:07:16.440000Z"))
        for value in (None, "2026-10-12T12:07:16+00:00", "2026-10-12T12:07:16.1234567890Z", "2026-99-12T12:07:16Z"):
            with self.subTest(value=value), self.assertRaises(ValueError):
                SUBJECT["utc_time"](value)

    def test_foreign_source_existing_or_escaping_output_reject_before_network(self):
        with tempfile.TemporaryDirectory(prefix="grok-cu-snapshot-object-") as name:
            root = Path(name)
            output = root / "snapshot-object-records-01"
            with self.assertRaises(ValueError):
                SUBJECT["collect"](root, output, FixtureCrc)
            output.mkdir()
            with self.assertRaises(FileExistsError):
                SUBJECT["collect"](root, output, FixtureCrc)
            with self.assertRaises(ValueError):
                SUBJECT["collect"](root, root / "../snapshot-object-records-01", FixtureCrc)

    def test_late_object_mismatch_retains_metadata_and_never_promotes(self):
        with tempfile.TemporaryDirectory(prefix="grok-cu-snapshot-object-") as name:
            root = Path(name)
            output = root / "snapshot-object-records-01"
            path = root / "original.zip"
            path.write_bytes(b"abc")
            original = {**self.original(), "path": path}
            def metadata(target):
                value = self.metadata(target)
                if target != "windows-x64":
                    value["generation"] = "other"
                return json.dumps(value).encode()
            with patch.dict(SUBJECT["LOGS"], checked_builds=lambda _: ({"windows-x64": self.build()}, "fixture")), \
                    patch.dict(SUBJECT["collect"].__globals__, original_sources=lambda _: ({"windows-x64": original}, "fixture", "fixture"),
                               fetch_metadata=metadata), self.assertRaises(ValueError):
                SUBJECT["collect"](root, output, FixtureCrc)
            result = json.loads((output / "snapshot-object-inventory.json").read_bytes())
            self.assertFalse(result["collectionComplete"])
            self.assertTrue(result["records"][0]["verified"])
            self.assertFalse(result["records"][1]["verified"])
            self.assertTrue((output / "linux-x64.gcs-data").exists())
            self.assertFalse(result["promoted"])
            self.assertEqual(path.read_bytes(), b"abc")


if __name__ == "__main__":
    unittest.main()
