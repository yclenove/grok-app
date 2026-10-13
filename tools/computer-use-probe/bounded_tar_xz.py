"""Bounded, streaming, non-extracting reader for pinned tar.xz proposals.

Only regular files/directories and bounded GNU long names are accepted.
No filesystem writes or fallback to an unbounded LZMA/metadata decoder.
"""
import io
import lzma
import tarfile


CHUNK = 65536
MAX_METADATA = 16384


class XzReader(io.RawIOBase):
    def __init__(self, source, max_bytes):
        self.source = source
        self.limit = max_bytes
        self.total = 0
        self.decoder = lzma.LZMADecompressor(format=lzma.FORMAT_XZ, memlimit=128 * 1024**2)
        self.finished = False

    def readable(self):
        return True

    def readinto(self, output):
        if not output or self.finished:
            return 0
        while True:
            compressed = self.source.read(CHUNK) if self.decoder.needs_input else b""
            if self.decoder.needs_input and not compressed:
                raise ValueError("truncated XZ stream")
            try:
                chunk = self.decoder.decompress(compressed, max_length=min(len(output), CHUNK))
            except lzma.LZMAError as error:
                raise ValueError("invalid XZ stream or decoder memory budget") from error
            self.total += len(chunk)
            if self.total > self.limit:
                raise ValueError("XZ decoded byte budget exceeded")
            if self.decoder.eof:
                if self.decoder.unused_data or self.source.read(1):
                    raise ValueError("concatenated/trailing XZ stream")
                self.finished = True
            if chunk or self.finished:
                output[:len(chunk)] = chunk
                return len(chunk)


def exact(stream, size):
    if not 0 <= size <= MAX_METADATA:
        raise ValueError("bounded tar metadata read required")
    chunks = bytearray()
    while len(chunks) < size:
        chunk = stream.read(size - len(chunks))
        if not chunk:
            raise ValueError("truncated tar header/payload")
        chunks.extend(chunk)
    return bytes(chunks)


class EntryReader:
    def __init__(self, stream, size):
        self.stream = stream
        self.remaining = size

    def read(self, size):
        if not 0 < size <= 1024 * 1024:
            raise ValueError("bounded tar entry reads are required")
        if not self.remaining:
            return b""
        chunk = self.stream.read(min(size, self.remaining))
        if not chunk:
            raise ValueError("truncated tar entry")
        self.remaining -= len(chunk)
        return chunk


def iter_tar_stream(stream, *, max_entries=1024, max_file=512 * 1024**2, allow_links=False):
    pending_name = None
    count = 0
    while True:
        header = exact(stream, 512)
        if header == b"\0" * 512:
            if pending_name is not None or exact(stream, 512) != b"\0" * 512:
                raise ValueError("tar requires two end blocks and no pending metadata")
            trailing = 0
            while True:
                chunk = stream.read(CHUNK)
                if not chunk:
                    return
                trailing += len(chunk)
                if any(chunk) or trailing > 1024 * 1024:
                    raise ValueError("tar trailing data/padding budget exceeded")
        count += 1
        if count > max_entries:
            raise ValueError("tar entry budget exceeded")
        fields = [header[:100], header[157:257]]
        if header[257:263] == b"ustar\0":
            fields.append(header[345:500])
        if any(b"\0" in field and any(field[field.index(b"\0"):]) for field in fields):
            raise ValueError("ambiguous NUL-terminated tar field")
        try:
            entry = tarfile.TarInfo.frombuf(header, "utf-8", "strict")
        except (tarfile.HeaderError, UnicodeError) as error:
            raise ValueError("invalid tar checksum/header") from error
        if not 0 <= entry.size <= max_file:
            raise ValueError("tar file byte budget exceeded")
        if entry.type == tarfile.GNUTYPE_LONGNAME:
            if pending_name is not None or not 0 < entry.size <= MAX_METADATA:
                raise ValueError("tar long-name metadata budget/ordering violation")
            raw = exact(stream, entry.size)
            if not raw.endswith(b"\0") or b"\0" in raw[:-1]:
                raise ValueError("ambiguous tar long-name metadata")
            pending_name = raw[:-1].decode("utf-8", "strict")
            if any(exact(stream, (-entry.size) % 512)):
                raise ValueError("nonzero tar metadata padding")
            continue
        allowed = (tarfile.REGTYPE, tarfile.AREGTYPE, tarfile.DIRTYPE)
        if allow_links:
            allowed += (tarfile.SYMTYPE, tarfile.LNKTYPE)
        if entry.type not in allowed:
            raise ValueError("unsupported tar links/special/extended metadata")
        if (entry.isdir() or entry.issym() or entry.islnk()) and entry.size:
            raise ValueError("tar directory/link payload is forbidden")
        if pending_name is not None:
            entry.name, pending_name = pending_name, None
        content = EntryReader(stream, entry.size)
        yield entry, content
        if content.remaining:
            raise ValueError("tar entry was not fully inspected")
        if any(exact(stream, (-entry.size) % 512)):
            raise ValueError("nonzero tar file padding")


def iter_tar_xz(source, *, max_bytes=1024 * 1024**2, max_entries=1024, max_file=512 * 1024**2):
    reader = XzReader(source, max_bytes)
    stream = io.BufferedReader(reader, buffer_size=CHUNK)
    yield from iter_tar_stream(stream, max_entries=max_entries, max_file=max_file)
