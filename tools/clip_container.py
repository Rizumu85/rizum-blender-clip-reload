from __future__ import annotations

import logging
import struct
from dataclasses import dataclass
from typing import Iterable


log = logging.getLogger(__name__)

CSF_MAGIC = b"CSFCHUNK"


@dataclass(frozen=True)
class ClipChunk:
    type: bytes
    body: bytes


def walk_chunks(data: bytes) -> Iterable[ClipChunk]:
    if data[:8] != CSF_MAGIC:
        raise ValueError("Not a CLIP file (missing CSFCHUNK magic).")
    pos = 8 + 16
    while pos < len(data):
        ctype = data[pos : pos + 8]
        csize = struct.unpack_from(">Q", data, pos + 8)[0]
        body = data[pos + 16 : pos + 16 + csize]
        yield ClipChunk(ctype, body)
        pos += 16 + csize


def read_exta_id(body: bytes) -> str:
    length = struct.unpack_from(">Q", body, 0)[0]
    return body[8 : 8 + length].decode("ascii")


def split_clip(path: str):
    """Index CHNKExta chunks by external id and return CHNKSQLi bytes.

    This is intentionally only a container helper for old reverse-analysis
    scripts. It is not a renderer or Python compositor path.
    """
    with open(path, "rb") as f:
        data = f.read()

    sqlite_bytes = None
    ext_to_body: dict[str, bytes] = {}
    for chunk in walk_chunks(data):
        if chunk.type == b"CHNKSQLi":
            sqlite_bytes = chunk.body
        elif chunk.type == b"CHNKExta":
            try:
                ext_id = read_exta_id(chunk.body)
            except (struct.error, UnicodeDecodeError) as exc:
                log.warning("Skipping unreadable Exta header: %s", exc)
                continue
            ext_to_body[ext_id] = chunk.body
    if sqlite_bytes is None:
        raise ValueError("CLIP file has no CHNKSQLi chunk.")
    return ext_to_body, sqlite_bytes
