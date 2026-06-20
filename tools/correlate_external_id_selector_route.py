"""Correlate native external-id selector trace with .clip resources."""

from __future__ import annotations

import json
import sqlite3
import struct
import tempfile
from collections import Counter, defaultdict
from pathlib import Path
from typing import Any


REPO_ROOT = Path(__file__).resolve().parents[1]
OUT_PATH = REPO_ROOT / "tmp_vector_probe/native_external_id_selector_correlation_v1.json"
REQUESTED_96KB_ID = "extrnlid5943B673F7C84B779ED2D7C96E942EAE"
TARGET_VECTOR_ID = "extrnlid62D15CB4395245648869B4AEBAD8FBCE"

from tools.clip_container import split_clip  # noqa: E402


def load_jsonl(path: Path) -> list[dict[str, Any]]:
    rows = []
    for line_no, line in enumerate(path.read_text(encoding="utf-8", errors="replace").splitlines(), 1):
        if not line.strip():
            continue
        try:
            row = json.loads(line)
        except json.JSONDecodeError as exc:
            row = {"event": "json_decode_error", "error": str(exc), "raw": line[:240]}
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


def table_columns(db: sqlite3.Connection, table: str) -> list[dict[str, Any]]:
    return [
        {"cid": row[0], "name": row[1], "type": row[2], "notnull": row[3], "default": row[4], "pk": row[5]}
        for row in db.execute(f'PRAGMA table_info("{table}")')
    ]


def jsonable(value: Any) -> Any:
    if isinstance(value, bytes):
        try:
            return value.decode("ascii")
        except UnicodeDecodeError:
            return {"bytes_len": len(value), "bytes_hex_prefix": value[:64].hex(" ")}
    return value


def sqlite_references(sqlite_bytes: bytes, ext_ids: set[str]) -> dict[str, list[dict[str, Any]]]:
    path = write_sqlite(sqlite_bytes)
    refs: dict[str, list[dict[str, Any]]] = {ext_id: [] for ext_id in ext_ids}
    try:
        db = sqlite3.connect(path)
        db.row_factory = sqlite3.Row
        tables = [
            row[0]
            for row in db.execute("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
        ]
        for table in tables:
            cols = table_columns(db, table)
            text_cols = [
                col["name"]
                for col in cols
                if any(token in str(col["type"]).upper() for token in ("TEXT", "CHAR", "CLOB", "BLOB"))
                or "id" in col["name"].lower()
                or "data" in col["name"].lower()
                or "mipmap" in col["name"].lower()
                or "offscreen" in col["name"].lower()
            ]
            if not text_cols:
                continue
            select_cols = [col["name"] for col in cols]
            try:
                rows = db.execute(f'SELECT rowid, * FROM "{table}"').fetchall()
            except sqlite3.DatabaseError:
                continue
            for row in rows:
                row_dict = {key: jsonable(row[key]) for key in row.keys()}
                for col in text_cols:
                    value = row_dict.get(col)
                    value_text = value if isinstance(value, str) else json.dumps(value, ensure_ascii=False)
                    for ext_id in ext_ids:
                        if ext_id in value_text:
                            refs[ext_id].append({
                                "table": table,
                                "column": col,
                                "rowid": row_dict.get("rowid"),
                                "row": row_dict,
                            })
    finally:
        try:
            path.unlink()
        except OSError:
            pass
    return refs


def vector_record_info(payload: bytes) -> dict[str, Any]:
    if len(payload) < 92:
        return {"is_saved_vector_stroke": False}
    info: dict[str, Any] = {"is_saved_vector_stroke": False}
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
            brush_style_id = None
            width = None
            try:
                brush_style_id = struct.unpack_from(">I", payload, off + 84)[0]
                width = struct.unpack_from(">d", payload, off + 80)[0]
            except struct.error:
                pass
            info.update({
                "is_saved_vector_stroke": True,
                "record_offset": off,
                "header_len": header_len,
                "point_header_len": point_header_len,
                "stride_a": stride_a,
                "stride_b": stride_b,
                "flags_hex": f"0x{flags:x}",
                "point_count": point_count,
                "brush_style_id_guess": brush_style_id,
                "width_guess": width,
            })
            break
    return info


def infer_resource_type(ext_id: str, payload_info: dict[str, Any], refs: list[dict[str, Any]]) -> str:
    if payload_info.get("is_saved_vector_stroke"):
        return "vector body"
    hay = " ".join(
        f"{ref.get('table','')} {ref.get('column','')} {json.dumps(ref.get('row', {}), ensure_ascii=False)}"
        for ref in refs
    ).lower()
    if "vectorobjectlist" in hay or "vectordata" in hay:
        return "vector body"
    if "mipmap" in hay or "thumbnail" in hay or "timelapseblob" in hay:
        return "preview/cache"
    if "offscreen" in hay:
        return "raster/offscreen"
    if "brush" in hay or "material" in hay:
        return "brush/material"
    return "unknown"


def collect_native_ids(rows: list[dict[str, Any]]) -> dict[str, dict[str, Any]]:
    observed: dict[str, dict[str, Any]] = {}
    for row in rows:
        ext_id = row.get("external_id_ascii")
        if not isinstance(ext_id, str) or not ext_id.startswith("extrnlid"):
            continue
        bucket = observed.setdefault(ext_id, {
            "external_id": ext_id,
            "events": Counter(),
            "selector_invocation_indices": set(),
            "loader_invocation_indices": set(),
            "native_body_sizes": Counter(),
            "native_hashes": Counter(),
            "native_dest_ptrs": Counter(),
            "first_events": [],
        })
        bucket["events"][str(row.get("event"))] += 1
        if row.get("selector_invocation_index") is not None:
            bucket["selector_invocation_indices"].add(row.get("selector_invocation_index"))
        if row.get("loader_invocation_index") is not None:
            bucket["loader_invocation_indices"].add(row.get("loader_invocation_index"))
        if row.get("body_size") is not None:
            bucket["native_body_sizes"][str(row.get("body_size"))] += 1
        if row.get("dest_hash"):
            bucket["native_hashes"][str(row.get("dest_hash"))] += 1
        if row.get("dest_ptr"):
            bucket["native_dest_ptrs"][str(row.get("dest_ptr"))] += 1
        if len(bucket["first_events"]) < 8:
            bucket["first_events"].append(row)
    return observed


def main(argv: list[str]) -> int:
    if len(argv) != 3:
        print("usage: python tools/correlate_external_id_selector_route.py <selector_trace.jsonl> <clip_path>", file=sys.stderr)
        return 2
    trace_path = Path(argv[1])
    clip_path = Path(argv[2])
    rows = load_jsonl(trace_path)
    exta, sqlite_bytes = split_clip(str(clip_path))
    exta_payloads: dict[str, dict[str, Any]] = {}
    for idx, (ext_id, raw_body) in enumerate(exta.items()):
        parsed_ext_id, payload = split_exta_body(raw_body)
        exta_payloads[parsed_ext_id] = {
            "external_id": parsed_ext_id,
            "exta_index": idx,
            "full_body_size": len(raw_body),
            "payload_size": len(payload),
            "payload_hash_4096": fnv1a32(payload[:4096]),
            "payload_prefix_hex": payload[:96].hex(" "),
            "payload_prefix_ascii": "".join(chr(b) if 0x20 <= b <= 0x7E else "." for b in payload[:96]),
            **vector_record_info(payload),
        }

    native_ids = collect_native_ids(rows)
    ids_to_compare = set(native_ids) | {REQUESTED_96KB_ID, TARGET_VECTOR_ID}
    refs = sqlite_references(sqlite_bytes, ids_to_compare)
    correlated: dict[str, Any] = {}
    for ext_id in sorted(ids_to_compare):
        native = native_ids.get(ext_id, {
            "external_id": ext_id,
            "events": Counter(),
            "selector_invocation_indices": set(),
            "loader_invocation_indices": set(),
            "native_body_sizes": Counter(),
            "native_hashes": Counter(),
            "native_dest_ptrs": Counter(),
            "first_events": [],
        })
        payload_info = exta_payloads.get(ext_id)
        ref_rows = refs.get(ext_id, [])
        correlated[ext_id] = {
            "external_id": ext_id,
            "observed_in_native_trace": ext_id in native_ids,
            "native_events": dict(native["events"]),
            "selector_invocation_indices": sorted(native["selector_invocation_indices"]),
            "loader_invocation_indices": sorted(native["loader_invocation_indices"]),
            "native_body_sizes": dict(native["native_body_sizes"]),
            "native_hashes": dict(native["native_hashes"]),
            "native_dest_ptrs": dict(native["native_dest_ptrs"]),
            "matching_exta": payload_info,
            "sqlite_references": ref_rows,
            "sqlite_reference_tables": sorted({ref["table"] for ref in ref_rows}),
            "likely_resource_type": infer_resource_type(ext_id, payload_info or {}, ref_rows),
            "is_requested_96kb_id": ext_id == REQUESTED_96KB_ID,
            "is_target_vector_id": ext_id == TARGET_VECTOR_ID,
        }

    result = {
        "trace_path": str(trace_path),
        "clip_path": str(clip_path),
        "observed_external_ids": sorted(native_ids),
        "requested_96kb_id": REQUESTED_96KB_ID,
        "target_vector_id": TARGET_VECTOR_ID,
        "correlated_external_ids": correlated,
        "event_counts": dict(Counter(str(row.get("event")) for row in rows)),
    }
    OUT_PATH.write_text(json.dumps(result, indent=2, sort_keys=True), encoding="utf-8")
    print(OUT_PATH)
    for ext_id in [REQUESTED_96KB_ID, TARGET_VECTOR_ID]:
        item = correlated.get(ext_id, {})
        print(ext_id)
        print("  observed", item.get("observed_in_native_trace"))
        print("  type", item.get("likely_resource_type"))
        print("  native_body_sizes", item.get("native_body_sizes"))
        print("  native_hashes", item.get("native_hashes"))
        match = item.get("matching_exta") or {}
        print("  file_payload_size", match.get("payload_size"))
        print("  file_hash", match.get("payload_hash_4096"))
        print("  sqlite_tables", item.get("sqlite_reference_tables"))
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
