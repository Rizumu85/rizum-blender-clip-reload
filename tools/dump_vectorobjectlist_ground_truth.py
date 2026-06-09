"""Dump VectorObjectList ground truth for a .clip sample."""

from __future__ import annotations

import json
import sqlite3
import struct
import sys
import tempfile
from pathlib import Path
from typing import Any


REPO_ROOT = Path(__file__).resolve().parents[1]
OUT_PATH = REPO_ROOT / "tmp_vector_probe/vectorobjectlist_ground_truth_v1.json"
TARGET_VECTOR_ID = "extrnlid62D15CB4395245648869B4AEBAD8FBCE"

sys.path.insert(0, str(REPO_ROOT))
from clip_loader import _split_clip  # noqa: E402


def fnv1a32(data: bytes) -> str:
    h = 0x811C9DC5
    for b in data:
        h ^= b
        h = (h * 0x01000193) & 0xFFFFFFFF
    return f"fnv1a32:{h:08x}"


def split_exta_body(raw_body: bytes) -> tuple[str, bytes]:
    ext_len = struct.unpack_from(">Q", raw_body, 0)[0]
    ext_id = raw_body[8 : 8 + ext_len].decode("ascii")
    payload_start = 8 + ext_len + 8
    return ext_id, raw_body[payload_start:]


def write_sqlite(sqlite_bytes: bytes) -> Path:
    fd, name = tempfile.mkstemp(suffix=".sqlite3")
    path = Path(name)
    with open(fd, "wb", closefd=True) as f:
        f.write(sqlite_bytes)
    return path


def jsonable(value: Any) -> Any:
    if isinstance(value, bytes):
        try:
            return value.decode("ascii")
        except UnicodeDecodeError:
            return {"bytes_len": len(value), "bytes_hex_prefix": value[:64].hex(" ")}
    return value


def fetch_rows(db: sqlite3.Connection, table: str) -> list[dict[str, Any]]:
    if not db.execute("SELECT 1 FROM sqlite_master WHERE type='table' AND name=?", (table,)).fetchone():
        return []
    rows = []
    for row in db.execute(f'SELECT rowid, * FROM "{table}"'):
        rows.append({key: jsonable(row[key]) for key in row.keys()})
    return rows


def vector_record_info(payload: bytes) -> dict[str, Any]:
    result: dict[str, Any] = {
        "is_saved_vector_stroke": False,
        "is_0x2081_or_0x2011": False,
    }
    for off in range(0, min(len(payload), 256), 4):
        if off + 92 > len(payload):
            break
        try:
            header_len, point_header_len, stride_a, stride_b = struct.unpack_from(">IIII", payload, off)
            flags = struct.unpack_from(">I", payload, off + 20)[0]
            point_count = struct.unpack_from(">I", payload, off + 24)[0]
        except struct.error:
            continue
        if (header_len, point_header_len, stride_a, stride_b) == (92, 76, 88, 88) and flags in (0x2081, 0x2011):
            result.update({
                "is_saved_vector_stroke": True,
                "is_0x2081_or_0x2011": True,
                "record_offset": off,
                "header_len": header_len,
                "point_header_len": point_header_len,
                "stride_a": stride_a,
                "stride_b": stride_b,
                "flags": flags,
                "flags_hex": f"0x{flags:x}",
                "point_count": point_count,
                "brush_style_id_guess": struct.unpack_from(">I", payload, off + 84)[0] if off + 88 <= len(payload) else None,
                "prefix_hex": payload[off:off + 96].hex(" "),
            })
            break
    return result


def main(argv: list[str]) -> int:
    clip_path = Path(argv[1]) if len(argv) > 1 else REPO_ROOT / "img/Vector_SizePressure.clip"
    ext_to_body, sqlite_bytes = _split_clip(str(clip_path))

    exta_payloads: dict[str, dict[str, Any]] = {}
    for idx, raw_body in enumerate(ext_to_body.values()):
        ext_id, payload = split_exta_body(raw_body)
        exta_payloads[ext_id] = {
            "external_id": ext_id,
            "exta_index": idx,
            "payload_size": len(payload),
            "payload_hash_4096": fnv1a32(payload[:4096]),
            "payload_prefix_hex": payload[:96].hex(" "),
            "payload_prefix_ascii": "".join(chr(b) if 0x20 <= b <= 0x7E else "." for b in payload[:96]),
            **vector_record_info(payload),
        }

    sqlite_path = write_sqlite(sqlite_bytes)
    try:
        db = sqlite3.connect(sqlite_path)
        db.row_factory = sqlite3.Row
        vector_rows = fetch_rows(db, "VectorObjectList")
        layer_rows = fetch_rows(db, "Layer")
        brush_rows = fetch_rows(db, "BrushStyle")
        line_style_rows = fetch_rows(db, "LineStyle")
    finally:
        try:
            sqlite_path.unlink()
        except OSError:
            pass

    enriched = []
    for row in vector_rows:
        vector_data = row.get("VectorData")
        payload = exta_payloads.get(vector_data)
        enriched.append({
            **row,
            "VectorData_equals_target": vector_data == TARGET_VECTOR_ID,
            "VectorData_payload": payload,
        })

    result = {
        "clip_path": str(clip_path),
        "target_vector_id": TARGET_VECTOR_ID,
        "vectorobjectlist_rows": enriched,
        "vectorobjectlist_columns": list(vector_rows[0].keys()) if vector_rows else [],
        "layer_rows": layer_rows,
        "brush_style_rows": brush_rows,
        "line_style_rows": line_style_rows,
        "target_payload": exta_payloads.get(TARGET_VECTOR_ID),
        "all_exta_payloads_summary": [
            {
                "external_id": ext_id,
                "exta_index": info["exta_index"],
                "payload_size": info["payload_size"],
                "payload_hash_4096": info["payload_hash_4096"],
                "is_saved_vector_stroke": info.get("is_saved_vector_stroke", False),
                "flags_hex": info.get("flags_hex"),
            }
            for ext_id, info in sorted(exta_payloads.items(), key=lambda item: item[1]["exta_index"])
        ],
    }
    OUT_PATH.write_text(json.dumps(result, indent=2, sort_keys=True), encoding="utf-8")
    print(OUT_PATH)
    print(f"VectorObjectList rows: {len(enriched)}")
    for row in enriched:
        print({
            "rowid": row.get("rowid"),
            "MainId": row.get("MainId"),
            "LayerId": row.get("LayerId"),
            "VectorData": row.get("VectorData"),
            "target": row.get("VectorData_equals_target"),
            "payload_size": (row.get("VectorData_payload") or {}).get("payload_size"),
            "hash": (row.get("VectorData_payload") or {}).get("payload_hash_4096"),
            "flags": (row.get("VectorData_payload") or {}).get("flags_hex"),
        })
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
