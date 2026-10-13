"""Positive/negative DataPack and bounded gzip inspection oracles."""
import gzip
import hashlib
import io
import runpy
import struct
import tempfile
import unittest
import zipfile
from pathlib import Path
from unittest.mock import patch

import chromium_data_pack as subject
import brotli

AUDIT = runpy.run_path(str(Path(__file__).with_name("inspect-chromium-credits.py")))


def pack(rows=(), aliases=(), version=5, encoding=0):
    header = (struct.pack("<IBxxxHH", version, encoding, len(rows), len(aliases))
              if version == 5 else struct.pack("<IIB", version, len(rows), encoding))
    offset = len(header) + (len(rows) + 1) * 6 + len(aliases) * 4
    table, payload = [], []
    for resource_id, data in rows:
        table.append(struct.pack("<HI", resource_id, offset))
        offset += len(data)
        payload.append(data)
    return (header + b"".join(table) + struct.pack("<HI", 0, offset)
            + b"".join(struct.pack("<HH", *alias) for alias in aliases)
            + b"".join(payload))


def changed(data, offset, fmt, value):
    contents = bytearray(data)
    struct.pack_into(fmt, contents, offset, value)
    return bytes(contents)


class DataPackTests(unittest.TestCase):
    def test_v4_v5_and_all_three_encodings(self):
        for version in (4, 5):
            for encoding in (0, 1, 2):
                result = subject.read_pack(pack([(1, b"abc"), (4, b""), (9, b"end")],
                                                version=version, encoding=encoding))
                self.assertEqual((result.version, result.encoding), (version, encoding))
                self.assertEqual({key: bytes(value) for key, value in result.resources.items()},
                                 {1: b"abc", 4: b"", 9: b"end"})

    def test_empty_pack_and_aliases_are_not_duplicate_payloads(self):
        self.assertEqual(subject.read_pack(pack()).resources, {})
        result = subject.read_pack(pack([(3, b"data"), (8, b"end")], [(5, 0), (10, 1)]))
        self.assertEqual(result.aliases, {5: 3, 10: 8})
        self.assertEqual(len(result.resources), 2)

    def test_truncation_unknown_version_encoding_padding_and_counts_reject(self):
        complete = pack([(1, b"abc")], [(2, 0)])
        cases = [complete[:index] for index in range(len(complete))]
        cases += [changed(complete, 0, "<I", 6), changed(complete, 4, "<B", 3),
                  changed(complete, 5, "<B", 1), changed(complete, 8, "<H", 65535),
                  struct.pack("<IIB", 4, 65536, 0)]
        for data in cases:
            with self.subTest(length=len(data)), self.assertRaises(ValueError):
                subject.read_pack(data)

    def test_unordered_zero_duplicate_ids_and_bad_offsets_reject(self):
        complete = pack([(1, b"abc"), (7, b"end")])
        cases = [changed(complete, 12, "<H", 0), changed(complete, 18, "<H", 1),
                 changed(complete, 14, "<I", 12), changed(complete, 14, "<I", 31),
                 changed(complete, 20, "<I", 29), changed(complete, 20, "<I", 100),
                 changed(complete, 24, "<H", 9), changed(complete, 26, "<I", len(complete) - 1),
                 complete + b"trailer"]
        for data in cases:
            with self.subTest(data=data), self.assertRaises(ValueError):
                subject.read_pack(data)

    def test_alias_zero_duplicate_unordered_collision_and_bad_index_reject(self):
        complete = pack([(3, b"data")], [(5, 0), (9, 0)])
        for offset, fmt, value in [(24, "<H", 0), (24, "<H", 3), (26, "<H", 1),
                                   (28, "<H", 5), (28, "<H", 4)]:
            with self.subTest(value=value), self.assertRaises(ValueError):
                subject.read_pack(changed(complete, offset, fmt, value))
        with self.assertRaises(ValueError):
            subject.read_pack(pack([], [(1, 0)]))

    def test_pack_resource_and_id_budgets_cannot_be_bypassed(self):
        contents = pack([(1, b"12345")])
        for name, limit in [("MAX_PACK_BYTES", len(contents) - 1), ("MAX_RESOURCE_BYTES", 4),
                            ("MAX_RESOURCE_IDS", 0)]:
            with patch.object(subject, name, limit), self.assertRaises(ValueError):
                subject.read_pack(contents)

    def test_raw_gzip_crc_truncation_multimember_trailing_and_bomb(self):
        original = b"complete resource" * 100
        zipped = gzip.compress(original, mtime=0)
        self.assertEqual(subject.unpack_resource(original), (original, "raw"))
        self.assertEqual(subject.unpack_resource(zipped), (original, "gzip"))
        corrupted = bytearray(zipped)
        corrupted[-8] ^= 1
        for data in [zipped[:-1], bytes(corrupted), zipped + zipped, zipped + b"trailer",
                     gzip.compress(b"x" * 10000, mtime=0)]:
            with self.subTest(length=len(data)), self.assertRaises(ValueError):
                subject.unpack_resource(data, limit=2000)
        for limit in (0, -1, subject.MAX_RESOURCE_BYTES + 1):
            with self.assertRaises(ValueError):
                subject.unpack_resource(zipped, limit=limit)

    def test_utf8_utf16_candidates_not_small_stub_or_nontext(self):
        text = ("<html><title>Credits</title><body>Chromium Copyright license "
                + "full notice " * 500 + "</body></html>")
        for encoding, codec in [(0, "utf-8"), (1, "utf-8"), (2, "utf-16-le")]:
            self.assertEqual(subject.credits_text(text.encode(codec), encoding), text)
        for data in [b"credits", b"\xff" * 5000, text.replace("license", "stub").encode(),
                     text.replace("<title>Credits</title>", "<title>Notices</title>").encode()]:
            self.assertIsNone(subject.credits_text(data, 0))

    def test_brotli_six_byte_size_large_stream_and_empty_resource(self):
        for data in (b"", b"small resource", b"complete resource " * 100000):
            compressed = b"\x1e\x9b" + len(data).to_bytes(6, "little") + brotli.compress(data)
            self.assertEqual(subject.unpack_resource(compressed), (data, "brotli"))

    def test_brotli_bad_header_size_overflow_truncation_trailer_and_bomb(self):
        original = b"x" * 100000
        compressed = b"\x1e\x9b" + len(original).to_bytes(6, "little") + brotli.compress(original)
        cases = [compressed[:index] for index in range(2, 9)]
        cases += [compressed[:-1], compressed + b"trailer", compressed + compressed[8:],
                  b"\x1e\x9b" + (len(original) + 1).to_bytes(6, "little") + compressed[8:],
                  b"\x1e\x9b" + (10).to_bytes(6, "little") + compressed[8:]]
        for data in cases:
            with self.subTest(length=len(data)), self.assertRaises(ValueError):
                subject.unpack_resource(data)
        with self.assertRaisesRegex(ValueError, "declared size"):
            subject.unpack_resource(compressed, limit=10000)

    def test_sample_stub_is_not_full_notices_and_scripts_cannot_match(self):
        data = (b"<html><title>Credits</title>This is sample credits page. "
                b"set `generate_about_credits=true` in args.gn.</html>")
        self.assertEqual(subject.credits_document(data, 0)[1], "sample_stub")
        self.assertIsNone(subject.credits_text(data, 0))
        self.assertIsNone(subject.credits_document(b"function credits() {/* Chromium copyright license */}", 0))

    def test_old_brotli_api_cannot_fall_back_to_unbounded_decompression(self):
        class OldDecoder:
            def process(self, data):
                raise AssertionError("unbounded fallback must not be called")
        contents = b"\x1e\x9b" + (1).to_bytes(6, "little") + b"compressed"
        with patch.object(brotli, "Decompressor", return_value=OldDecoder()):
            with self.assertRaisesRegex(ValueError, "bounded process API"):
                subject.unpack_resource(contents)


class CreditsAuditTests(unittest.TestCase):
    def archive(self, entries):
        output = io.BytesIO()
        with zipfile.ZipFile(output, "w", compression=zipfile.ZIP_DEFLATED) as archive:
            for name, data in entries:
                archive.writestr(name, data)
        return output.getvalue()

    def inspect(self, entries):
        contents = self.archive(entries)
        with tempfile.TemporaryDirectory(prefix="grok-cu-credits-oracle-") as directory:
            path = Path(directory) / "Win_x64.zip"
            path.write_bytes(contents)
            with patch.dict(AUDIT["SHA256"], {"Win_x64": hashlib.sha256(contents).hexdigest()}):
                return AUDIT["inspect_archive"](path, "Win_x64")

    def test_actual_payload_preserved_and_stub_classification_is_explicit(self):
        stub = (b"<html><title>Credits</title>This is sample credits page. "
                b"generate_about_credits=true</html>")
        row, payloads = self.inspect([("chrome-win/resources.pak", pack([(1, stub)]))])
        self.assertEqual(payloads, [stub])
        candidate = row["creditsCandidates"][0]
        self.assertEqual(candidate["documentKind"], "sample_stub")
        self.assertEqual(candidate["sha256"], hashlib.sha256(stub).hexdigest())
        self.assertEqual(candidate["copyrightMentions"], 0)

    def test_source_mismatch_is_rejected_before_zip_parsing(self):
        with tempfile.TemporaryDirectory(prefix="grok-cu-credits-wrong-") as directory:
            path = Path(directory) / "Win_x64.zip"
            path.write_bytes(b"not the pinned source")
            with self.assertRaisesRegex(ValueError, "SHA256"):
                AUDIT["inspect_archive"](path, "Win_x64")

    def test_explicit_proposal_pin_supports_rootless_archives_without_weakening_snapshot(self):
        stub = b"<html><title>Credits</title>This is sample credits page. generate_about_credits=true</html>"
        contents = self.archive([("resources.pak", pack([(1, stub)]))])
        with tempfile.TemporaryDirectory(prefix="grok-cu-proposal-oracle-") as directory:
            path = Path(directory) / "proposal.zip"
            path.write_bytes(contents)
            digest = hashlib.sha256(contents).hexdigest()
            row, payloads = AUDIT["inspect_pinned_archive"](path, "Win_x64", "", digest)
            self.assertEqual(row["archiveSha256"], digest)
            self.assertEqual(payloads, [stub])
            with self.assertRaisesRegex(ValueError, "path"):
                AUDIT["inspect_pinned_archive"](path, "Win_x64", "chrome-win", digest)
            with self.assertRaisesRegex(ValueError, "SHA256"):
                AUDIT["inspect_archive"](path, "Win_x64")

    def test_proposal_pin_root_and_labels_reject_ambiguous_inputs(self):
        inspect = AUDIT["inspect_pinned_archive"]
        for label, root, digest in [("../Mac", "", "a" * 64), ("Mac", "../root", "a" * 64),
                                    ("Mac", ".", "a" * 64), ("Mac", "..", "a" * 64),
                                    ("Mac", "C:root", "a" * 64), ("Mac", "", "latest"),
                                    ("Mac", "", "A" * 64)]:
            with self.subTest(label=label, root=root), self.assertRaises(ValueError):
                inspect(Path("never-read.zip"), label, root, digest)

    def test_missing_credits_and_duplicate_case_or_traversal_paths_reject(self):
        for entries in [[("chrome-win/resources.pak", pack([(1, b"not credits")]))],
                        [("chrome-win/../resources.pak", pack())],
                        [("chrome-win/C:resources.pak", pack())],
                        [("chrome-win/resources.pak", pack()), ("chrome-win/RESOURCES.PAK", pack())]]:
            with self.subTest(entries=entries), self.assertRaises(ValueError):
                self.inspect(entries)
        # ZipInfo.filename truncates at NUL: reject its original raw name,
        # not just the apparently valid .pak prefix exposed by filename.
        contents = self.archive([("chrome-win/resources.pak#suffix", pack())])
        contents = contents.replace(b"resources.pak#suffix", b"resources.pak\x00suffix")
        with tempfile.TemporaryDirectory(prefix="grok-cu-credits-nul-") as directory:
            path = Path(directory) / "Win_x64.zip"
            path.write_bytes(contents)
            with patch.dict(AUDIT["SHA256"], {"Win_x64": hashlib.sha256(contents).hexdigest()}):
                with self.assertRaisesRegex(ValueError, "path"):
                    AUDIT["inspect_archive"](path, "Win_x64")

    def test_evidence_rejects_outside_run_and_existing_root(self):
        with tempfile.TemporaryDirectory(prefix="grok-cu-credits-output-") as directory:
            with self.assertRaises(ValueError):
                AUDIT["save_evidence"](Path(directory), {"archives": []}, [])
        with self.assertRaises(ValueError):
            AUDIT["save_evidence"](AUDIT["HERE"] / ".run", {"archives": []}, [])


if __name__ == "__main__":
    unittest.main()
