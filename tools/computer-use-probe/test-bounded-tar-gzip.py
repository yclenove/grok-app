"""Actual gzip footer, output bound and raw tar validation; no extraction."""
import gzip
import io
import tarfile
import unittest

from bounded_tar_gzip import GzipReader, iter_tar_gzip


def raw_tar(entries):
    output = io.BytesIO()
    with tarfile.open(fileobj=output, mode="w", format=tarfile.GNU_FORMAT) as archive:
        for name, data, kind, link in entries:
            row = tarfile.TarInfo(name)
            row.type, row.size, row.linkname = kind, len(data), link
            archive.addfile(row, io.BytesIO(data))
    return output.getvalue()


def inspect(contents, **limits):
    rows = []
    for entry, reader in iter_tar_gzip(io.BytesIO(contents), **limits):
        data = bytearray()
        while True:
            chunk = reader.read(1024)
            if not chunk:
                break
            data.extend(chunk)
        rows.append((entry.name, bytes(data), entry.type, entry.linkname))
    return rows


class TarGzipTests(unittest.TestCase):
    def setUp(self):
        self.rows = [("node_modules", b"", tarfile.DIRTYPE, ""),
                     ("node_modules/lit/LICENSE", b"license", tarfile.REGTYPE, "")]
        self.raw = raw_tar(self.rows)
        self.gz = gzip.compress(self.raw)

    def test_regular_directory_gnu_long_name_and_links_are_inert(self):
        self.assertEqual(inspect(self.gz), self.rows)
        rows = [("node_modules/" + "n" * 220, b"data", tarfile.REGTYPE, ""),
                ("node_modules/.bin/tool", b"", tarfile.SYMTYPE, "../pkg/tool"),
                ("node_modules/hard", b"", tarfile.LNKTYPE, "node_modules/pkg/tool")]
        self.assertEqual(inspect(gzip.compress(raw_tar(rows))), rows)

    def test_gzip_truncation_crc_isize_concatenation_and_trailer_reject(self):
        bad_crc, bad_size = bytearray(self.gz), bytearray(self.gz)
        bad_crc[-8] ^= 1
        bad_size[-4] ^= 1
        for contents in (b"", self.gz[:-1], bytes(bad_crc), bytes(bad_size), self.gz + self.gz, self.gz + b"\0"):
            with self.subTest(length=len(contents)), self.assertRaises(ValueError):
                inspect(contents)

    def test_total_file_entry_and_metadata_budgets(self):
        self.assertEqual(len(inspect(self.gz, max_bytes=len(self.raw), max_entries=2, max_file=7)), 2)
        for limits in ({"max_bytes": len(self.raw) - 1}, {"max_entries": 1}, {"max_file": 6}):
            with self.subTest(limits=limits), self.assertRaises(ValueError):
                inspect(self.gz, **limits)
        long_name = gzip.compress(raw_tar([("node_modules/" + "n" * 220, b"data", tarfile.REGTYPE, "")]))
        with self.assertRaisesRegex(ValueError, "entry budget"):
            inspect(long_name, max_entries=1)

    def test_raw_checksum_padding_footer_and_special_entries_reject(self):
        checksum, padding = bytearray(self.raw), bytearray(self.raw)
        checksum[0] ^= 1
        padding[1024 + 7] = 1
        for raw in (bytes(checksum), bytes(padding), self.raw[:1024], self.raw + b"notzero"):
            with self.subTest(length=len(raw)), self.assertRaises(ValueError):
                inspect(gzip.compress(raw))
        for kind in (tarfile.XHDTYPE, tarfile.XGLTYPE, tarfile.CHRTYPE, tarfile.GNUTYPE_SPARSE):
            with self.subTest(kind=kind), self.assertRaises(ValueError):
                inspect(gzip.compress(raw_tar([("node_modules/special", b"", kind, "")])))

    def test_directory_or_link_payload_and_unconsumed_entry_reject(self):
        for kind in (tarfile.DIRTYPE, tarfile.SYMTYPE, tarfile.LNKTYPE):
            with self.subTest(kind=kind), self.assertRaises(ValueError):
                inspect(gzip.compress(raw_tar([("node_modules/invalid", b"payload", kind, "target")])))
        iterator = iter_tar_gzip(io.BytesIO(self.gz))
        next(iterator)
        _, reader = next(iterator)
        for size in (-1, 0, 1024**2 + 1):
            with self.assertRaises(ValueError):
                reader.read(size)
        with self.assertRaisesRegex(ValueError, "fully inspected"):
            next(iterator)

    def test_tiny_readinto_output_keeps_output_bound_and_checks_footer(self):
        reader = GzipReader(io.BytesIO(self.gz), len(self.raw))
        result, output = bytearray(), bytearray(1)
        while reader.readinto(output):
            result.extend(output)
        self.assertEqual(bytes(result), self.raw)
        self.assertTrue(reader.finished)


if __name__ == "__main__":
    unittest.main()
