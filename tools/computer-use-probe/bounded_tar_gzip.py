"""Bounded single-member gzip/raw-tar reader for inert generated source data.

Links may be recorded, never followed or extracted. Extended/PAX/sparse and
special entries still reject. Gzip CRC/ISIZE, EOF and trailing bytes are checked.
"""
import io
from pathlib import Path
import runpy
import zlib


TAR = runpy.run_path(str(Path(__file__).with_name("bounded_tar_xz.py")))
CHUNK = TAR["CHUNK"]


class GzipReader(io.RawIOBase):
    def __init__(self, source, max_bytes):
        self.source, self.limit = source, max_bytes
        self.total = 0
        self.decoder = zlib.decompressobj(16 + zlib.MAX_WBITS)
        self.pending = b""
        self.finished = False

    def readable(self):
        return True

    def readinto(self, output):
        if not output or self.finished:
            return 0
        while True:
            compressed = self.pending or self.source.read(CHUNK)
            if not compressed:
                raise ValueError("truncated gzip stream")
            try:
                chunk = self.decoder.decompress(compressed, max_length=min(len(output), CHUNK))
            except zlib.error as error:
                raise ValueError("invalid gzip stream/CRC/size") from error
            self.pending = self.decoder.unconsumed_tail
            self.total += len(chunk)
            if self.total > self.limit:
                raise ValueError("gzip decoded byte budget exceeded")
            if self.decoder.eof:
                if self.decoder.unused_data or self.source.read(1):
                    raise ValueError("concatenated/trailing gzip stream")
                self.finished = True
            if chunk or self.finished:
                output[:len(chunk)] = chunk
                return len(chunk)


def iter_tar_gzip(source, *, max_bytes=256 * 1024**2, max_entries=50000, max_file=16 * 1024**2):
    stream = io.BufferedReader(GzipReader(source, max_bytes), buffer_size=CHUNK)
    yield from TAR["iter_tar_stream"](stream, max_entries=max_entries, max_file=max_file, allow_links=True)
