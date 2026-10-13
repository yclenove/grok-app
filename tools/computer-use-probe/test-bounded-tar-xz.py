"""Positive/negative bounded tar.xz source-reader oracles, no extraction."""
import io
import lzma
import tarfile
import unittest

from bounded_tar_xz import iter_tar_xz


def tar_bytes(entries):
    output = io.BytesIO()
    with tarfile.open(fileobj=output, mode="w", format=tarfile.GNU_FORMAT) as archive:
        for name, contents, kind in entries:
            entry = tarfile.TarInfo(name)
            entry.type = kind
            entry.size = len(contents)
            archive.addfile(entry, io.BytesIO(contents))
    return output.getvalue()


def inspect(contents, **limits):
    rows = []
    for entry, reader in iter_tar_xz(io.BytesIO(contents), **limits):
        pieces = []
        while True:
            piece = reader.read(1024)
            if not piece:
                break
            pieces.append(piece)
        rows.append((entry.name, b"".join(pieces), entry.isdir()))
    return rows


class TarXzTests(unittest.TestCase):
    def setUp(self):
        self.raw = tar_bytes([("root/", b"", tarfile.DIRTYPE),
                              ("root/file", b"payload", tarfile.REGTYPE)])
        self.compressed = lzma.compress(self.raw)

    def test_regular_directory_and_bounded_gnu_long_name(self):
        self.assertEqual(inspect(self.compressed), [("root", b"", True),
                                                     ("root/file", b"payload", False)])
        name = "root/" + "n" * 220
        compressed = lzma.compress(tar_bytes([(name, b"long name", tarfile.REGTYPE)]))
        self.assertEqual(inspect(compressed), [(name, b"long name", False)])

    def test_xz_truncation_crc_concatenation_and_trailers_reject(self):
        corrupted = bytearray(self.compressed)
        corrupted[-20] ^= 1
        for content in (b"", self.compressed[:-1], bytes(corrupted),
                        self.compressed + self.compressed, self.compressed + b"\0\0\0\0"):
            with self.subTest(length=len(content)), self.assertRaises(ValueError):
                inspect(content)

    def test_decoded_file_and_entry_budgets(self):
        self.assertEqual(len(inspect(self.compressed, max_bytes=len(self.raw))), 2)
        for limits in ({"max_bytes": len(self.raw) - 1}, {"max_file": 6}, {"max_entries": 1}):
            with self.subTest(limits=limits), self.assertRaises(ValueError):
                inspect(self.compressed, **limits)

    def test_tar_checksum_truncation_and_nonzero_trailing_data(self):
        bad_checksum = bytearray(self.raw)
        bad_checksum[0] ^= 1
        for raw in (bytes(bad_checksum), self.raw[:1024], self.raw + b"not zero"):
            with self.subTest(length=len(raw)), self.assertRaises(ValueError):
                inspect(lzma.compress(raw))

    def test_links_and_special_or_extended_headers_reject(self):
        for kind in (tarfile.SYMTYPE, tarfile.LNKTYPE, tarfile.CHRTYPE,
                     tarfile.XHDTYPE, tarfile.XGLTYPE, tarfile.GNUTYPE_SPARSE):
            raw = tar_bytes([("root/unsupported", b"", kind)])
            with self.subTest(kind=kind), self.assertRaises(ValueError):
                inspect(lzma.compress(raw))

    def test_directory_payload_and_nonzero_file_padding_reject(self):
        directory_payload = tar_bytes([("root/", b"bad", tarfile.DIRTYPE)])
        bad_padding = bytearray(self.raw)
        bad_padding[1024 + len(b"payload")] = 1
        for raw in (directory_payload, bytes(bad_padding)):
            with self.assertRaises(ValueError):
                inspect(lzma.compress(raw))

    def test_long_name_metadata_size_and_order_cannot_bypass_entry_limit(self):
        name = "root/" + "n" * 220
        raw = tar_bytes([(name, b"file", tarfile.REGTYPE)])
        with self.assertRaisesRegex(ValueError, "entry budget"):
            inspect(lzma.compress(raw), max_entries=1)
        oversized = tar_bytes([("././@LongLink", b"n" * 16384 + b"\0", tarfile.GNUTYPE_LONGNAME)])
        with self.assertRaisesRegex(ValueError, "metadata"):
            inspect(lzma.compress(oversized))

    def test_caller_must_consume_entry_and_use_bounded_reads(self):
        entries = iter_tar_xz(io.BytesIO(self.compressed))
        next(entries)
        _, reader = next(entries)
        for size in (-1, 0, 1024 * 1024 + 1):
            with self.assertRaisesRegex(ValueError, "bounded"):
                reader.read(size)
        with self.assertRaisesRegex(ValueError, "fully inspected"):
            next(entries)


if __name__ == "__main__":
    unittest.main()
