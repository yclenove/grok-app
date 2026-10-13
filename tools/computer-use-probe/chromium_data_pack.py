"""Bounded, read-only Chromium DataPack inspection (not a browser launcher).

Format reference: Chromium tools/grit/grit/format/data_pack.py at
f52796fc20c6e7d618374bb8bf096e7b2155e14c (BSD-style Chromium license).
This independent parser rejects ambiguous tables and never renders resources.
Finding a credits candidate is not a complete redistribution compliance audit.
"""
import struct
import zlib
from dataclasses import dataclass


MAX_PACK_BYTES = 64 * 1024**2
MAX_RESOURCE_BYTES = 32 * 1024**2
MAX_RESOURCE_IDS = 65535


@dataclass(frozen=True)
class DataPack:
    version: int
    encoding: int
    resources: dict
    aliases: dict


def read_pack(data):
    """Parse canonical v4/v5 tables, returning zero-copy resource views."""
    if len(data) < 4 or len(data) > MAX_PACK_BYTES:
        raise ValueError("DataPack size outside inspection budget")
    version = struct.unpack_from("<I", data)[0]
    if version == 4 and len(data) >= 9:
        count, encoding = struct.unpack_from("<IB", data, 4)
        alias_count, header_size = 0, 9
    elif version == 5 and len(data) >= 12:
        encoding, count, alias_count = struct.unpack_from("<BxxxHH", data, 4)
        header_size = 12
        if data[5:8] != b"\0\0\0":
            raise ValueError("DataPack nonzero reserved padding")
    else:
        raise ValueError("DataPack unsupported version or truncated header")
    if encoding not in (0, 1, 2) or count + alias_count > MAX_RESOURCE_IDS:
        raise ValueError("DataPack invalid encoding or resource count")
    table_end = header_size + (count + 1) * 6 + alias_count * 4
    if table_end > len(data):
        raise ValueError("DataPack truncated index/alias table")
    entries = [struct.unpack_from("<HI", data, header_size + index * 6)
               for index in range(count + 1)]
    if entries[0][1] != table_end or entries[-1] != (0, len(data)):
        raise ValueError("DataPack first offset/end sentinel mismatch")
    view = memoryview(data)
    resources, previous = {}, 0
    for index, (resource_id, start) in enumerate(entries[:-1]):
        end = entries[index + 1][1]
        if resource_id <= previous or not table_end <= start <= end <= len(data):
            raise ValueError("DataPack unordered IDs or invalid offsets")
        if end - start > MAX_RESOURCE_BYTES:
            raise ValueError("DataPack resource exceeds inspection budget")
        resources[resource_id] = view[start:end]
        previous = resource_id
    aliases, previous = {}, 0
    alias_start = header_size + (count + 1) * 6
    for index in range(alias_count):
        alias_id, entry_index = struct.unpack_from("<HH", data, alias_start + index * 4)
        if alias_id <= previous or alias_id in resources or entry_index >= count:
            raise ValueError("DataPack unordered/duplicate alias or invalid index")
        aliases[alias_id] = entries[entry_index][0]
        previous = alias_id
    return DataPack(version, encoding, resources, aliases)


def unpack_resource(data, limit=MAX_RESOURCE_BYTES):
    """Decode gzip/Chromium Brotli; cap each output chunk and total size."""
    if not 0 < limit <= MAX_RESOURCE_BYTES or len(data) > limit:
        raise ValueError("resource size outside inspection budget")
    if data[:2] == b"\x1e\x9b":
        return unpack_brotli(data, limit), "brotli"
    if data[:2] != b"\x1f\x8b":
        return bytes(data), "raw"
    decoder = zlib.decompressobj(16 + zlib.MAX_WBITS)
    try:
        decoded = decoder.decompress(data, limit + 1)
    except zlib.error as error:
        raise ValueError("invalid gzip resource") from error
    if len(decoded) > limit or decoder.unconsumed_tail:
        raise ValueError("gzip resource exceeds inspection budget")
    if not decoder.eof or decoder.unused_data:
        raise ValueError("truncated/concatenated/trailing gzip resource")
    return decoded, "gzip"


def unpack_brotli(data, limit):
    # Chromium's header is magic + SIX size bytes, not a native uint64.
    # See pinned grit/node/base.py and ui/base/resource/resource_bundle.h.
    if len(data) < 9:
        raise ValueError("truncated Chromium Brotli header/payload")
    expected = int.from_bytes(data[2:8], "little")
    if expected > limit:
        raise ValueError("Brotli declared size exceeds inspection budget")
    try:
        import brotli
    except ImportError as error:
        raise ValueError("Brotli>=1.2 with bounded process API is required") from error
    decoder, decoded, pending = brotli.Decompressor(), bytearray(), bytes(data[8:])
    while True:
        try:
            # The extension may round its buffer up: bound each requested chunk
            # to 64KiB and reject overflow immediately, never unbounded decompress().
            chunk = decoder.process(pending, output_buffer_limit=min(65536, expected - len(decoded) + 1))
        except TypeError as error:
            raise ValueError("Brotli>=1.2 bounded process API is required") from error
        except brotli.error as error:
            raise ValueError("invalid/trailing Brotli resource") from error
        if len(decoded) + len(chunk) > expected:
            raise ValueError("Brotli actual size exceeds declared inspection budget")
        decoded.extend(chunk)
        if decoder.is_finished():
            if len(decoded) != expected:
                raise ValueError("Brotli declared/actual size mismatch")
            return bytes(decoded)
        if not chunk:
            raise ValueError("truncated/stalled Brotli resource")
        pending = b""


def credits_document(data, encoding):
    """Classify a credits document separately from upstream sample stubs."""
    codec = "utf-16-le" if encoding == 2 else "utf-8"
    try:
        text = data.decode(codec)
    except UnicodeDecodeError:
        return None
    folded = text.casefold()
    if "<html" not in folded or "<title>credits</title>" not in folded:
        return None
    if "this is sample credits page" in folded and "generate_about_credits=true" in folded:
        return text, "sample_stub"
    if (len(data) >= 4096 and "copyright" in folded
            and "license" in folded and "chromium" in folded):
        return text, "full_candidate"
    return text, "unproven_document"


def credits_text(data, encoding):
    """Conservative candidate detection only; never treat a stub as notices."""
    document = credits_document(data, encoding)
    return document[0] if document and document[1] == "full_candidate" else None
