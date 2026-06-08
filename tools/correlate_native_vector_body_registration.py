"""Correlate native vector body registration trace with Vector_SizePressure.clip."""

from __future__ import annotations

import json
import sqlite3
import struct
import sys
import tempfile
from pathlib import Path
from typing import Any


REPO_ROOT = Path(__file__).resolve().parents[1]
OUT_PATH = REPO_ROOT / "tmp_vector_probe/native_vector_body_registration_correlation_v1.json"
TARGET_EXT_ID = "extrnlid62D15CB4395245648869B4AEBAD8FBCE"
TARGET_BODY_SIZE = 2644

sys.path.insert(0, str(REPO_ROOT))
from clip_loader import _split_clip  # noqa: E402


def load_jsonl(path: Path) -> list[dict[str, Any]]:
    rows = []
    for line_no, line in enumerate(path.read_text(encoding="utf-8", errors="replace").splitlines(), 1):
        if not line.strip():
            continue
        try:
            row = json.loads(line)
        except json.JSONDecodeError:
            row = {"event": "json_decode_error", "raw": line[:200]}
        row["_line_no"] = line_no
        rows.append(row)
    return rows


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


def table_exists(db: sqlite3.Connection, table: str) -> bool:
    return db.execute(
        "SELECT 1 FROM sqlite_master WHERE type='table' AND name=?",
        (table,),
    ).fetchone() is not None


def body_parts(raw_body: bytes) -> dict[str, Any]:
    ext_id, payload = split_exta_body(raw_body)
    info: dict[str, Any] = {
        "external_id": ext_id,
        "full_exta_body_size": len(raw_body),
        "payload_size": len(payload),
        "payload_hash_4096": fnv1a32(payload[:4096]),
        "payload_prefix_hex": payload[:96].hex(" "),
        "payload_prefix_ascii": "".join(chr(b) if 0x20 <= b <= 0x7E else "." for b in payload[:96]),
        "is_actual_saved_vector_stroke_body": False,
        "records": [],
    }
    for off in range(0, max(0, len(payload) - 96), 4):
        try:
            header_len, point_header_len, stride_a, stride_b = struct.unpack_from(">IIII", payload, off)
            point_count = struct.unpack_from(">I", payload, off + 16)[0]
            flags = struct.unpack_from(">I", payload, off + 20)[0]
            width = struct.unpack_from(">d", payload, off + 64)[0]
            opacity = struct.unpack_from(">d", payload, off + 56)[0]
            brush_style_id = struct.unpack_from(">I", payload, off + 88)[0]
        except struct.error:
            continue
        if (header_len, point_header_len, stride_a, stride_b) == (92, 76, 88, 88) and flags in (0x2081, 0x2011):
            info["is_actual_saved_vector_stroke_body"] = True
            info["records"].append({
                "offset": off,
                "header_len": header_len,
                "point_header_len": point_header_len,
                "stride_a": stride_a,
                "stride_b": stride_b,
                "point_count": point_count,
                "flags_hex": f"0x{flags:x}",
                "width": width,
                "opacity": opacity,
                "brush_style_id_guess": brush_style_id,
            })
    return info


def sqlite_correlation(sqlite_bytes: bytes, ext_id: str) -> dict[str, Any]:
    db_path = write_sqlite(sqlite_bytes)
    try:
        db = sqlite3.connect(db_path)
        db.row_factory = sqlite3.Row
        result: dict[str, Any] = {
            "vector_object_list_rows": [],
            "layer_rows": [],
            "brush_style_rows": [],
            "tables_present": [],
        }
        for table in [
            "VectorObjectList",
            "Layer",
            "BrushStyle",
            "BrushPatternStyle",
            "BrushPatternImage",
            "Mipmap",
            "MipmapInfo",
            "Offscreen",
        ]:
            if table_exists(db, table):
                result["tables_present"].append(table)
        ref_values = (ext_id, ext_id.encode("ascii"))
        if table_exists(db, "VectorObjectList"):
            rows = db.execute(
                "SELECT * FROM VectorObjectList WHERE VectorData IN (?,?)",
                ref_values,
            ).fetchall()
            for row in rows:
                obj = {k: jsonable(row[k]) for k in row.keys()}
                result["vector_object_list_rows"].append(obj)
                layer_id = obj.get("LayerId")
                if layer_id is not None and table_exists(db, "Layer"):
                    layer = db.execute("SELECT * FROM Layer WHERE MainId=?", (layer_id,)).fetchone()
                    if layer is not None:
                        result["layer_rows"].append({k: jsonable(layer[k]) for k in layer.keys()})
        if table_exists(db, "BrushStyle"):
            for row in db.execute("SELECT * FROM BrushStyle").fetchall():
                obj = {k: jsonable(row[k]) for k in row.keys()}
                result["brush_style_rows"].append(obj)
        return result
    finally:
        try:
            db.close()  # type: ignore[name-defined]
        except Exception:
            pass
        db_path.unlink(missing_ok=True)


def find_vector_trace(rows: list[dict[str, Any]]) -> dict[str, Any]:
    preferred_events = [
        "vector_registration_parent_leave",
        "vector_registration_loader_leave",
        "vector_registration_loader_post_body_read",
    ]
    for event in preferred_events:
        matches = [
            r for r in rows
            if r.get("event") == event
            and (
                r.get("external_id_ascii") == TARGET_EXT_ID
                or r.get("body_size") == TARGET_BODY_SIZE
                or (r.get("vector_loader") or {}).get("external_id_ascii") == TARGET_EXT_ID
            )
        ]
        if matches:
            return matches[-1]
    return {}


def summarize(trace_path: Path, clip_path: Path) -> dict[str, Any]:
    rows = load_jsonl(trace_path)
    trace = find_vector_trace(rows)
    loader = trace.get("vector_loader") or trace
    exta, sqlite_bytes = _split_clip(str(clip_path))
    body = exta.get(TARGET_EXT_ID)
    body_info = body_parts(body) if body else {}
    sqlite_info = sqlite_correlation(sqlite_bytes, TARGET_EXT_ID)
    native_hash = loader.get("dest_hash")
    importer_hash = body_info.get("payload_hash_4096")
    hashes_match = native_hash == importer_hash if native_hash is not None else None
    vector_rows = sqlite_info.get("vector_object_list_rows", [])
    brush_style_id = None
    records = body_info.get("records") or []
    if records:
        brush_style_id = records[0].get("brush_style_id_guess")
    brush_rows = [
        r for r in sqlite_info.get("brush_style_rows", [])
        if brush_style_id is not None and r.get("MainId") == brush_style_id
    ]
    return {
        "trace_path": str(trace_path),
        "clip_path": str(clip_path),
        "native_external_id": loader.get("external_id_ascii"),
        "native_body_size": loader.get("body_size"),
        "native_dest_ptr": loader.get("dest_ptr"),
        "native_dest_hash": native_hash,
        "native_parent_function_invocation_index": trace.get("parent_function_invocation_index") or loader.get("parent_function_invocation_index"),
        "native_parent_ptr": trace.get("parent_ptr"),
        "native_owner_slot": loader.get("owner_slot_ptr") or trace.get("active_owner_slot"),
        "native_flag_0x3f4_after": trace.get("flag_0x3f4_after"),
        "matching_importer_external_id": body_info.get("external_id"),
        "matching_importer_external_body_index": list(exta).index(TARGET_EXT_ID) if TARGET_EXT_ID in exta else None,
        "importer_payload_size": body_info.get("payload_size"),
        "importer_payload_hash_4096": importer_hash,
        "hashes_match": hashes_match,
        "matching_vector_object_record": records[0] if records else None,
        "all_matching_vector_object_records": records,
        "layer_id": vector_rows[0].get("LayerId") if vector_rows else None,
        "vector_object_list_rows": vector_rows,
        "layer_rows": sqlite_info.get("layer_rows", []),
        "brush_style_id": brush_style_id,
        "brush_style_rows": brush_rows,
        "is_actual_saved_vector_stroke_body": body_info.get("is_actual_saved_vector_stroke_body", False),
        "is_wrapper_around_vector_body": False if body_info.get("is_actual_saved_vector_stroke_body") else None,
        "parsed_compact_points_width_flags_match_importer_path": bool(records),
        "body_info": body_info,
        "trace_vector_event": trace,
    }


def main(argv: list[str]) -> int:
    if len(argv) < 3:
        print("usage: correlate_native_vector_body_registration.py TRACE.jsonl CLIP_PATH", file=sys.stderr)
        return 2
    result = summarize(Path(argv[1]), Path(argv[2]))
    OUT_PATH.parent.mkdir(parents=True, exist_ok=True)
    OUT_PATH.write_text(json.dumps(result, indent=2, sort_keys=True), encoding="utf-8")
    print(OUT_PATH)
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
