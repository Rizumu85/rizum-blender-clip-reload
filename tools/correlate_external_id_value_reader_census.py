"""Correlate native external-id value-reader census with .clip resources."""

from __future__ import annotations

import json
import sqlite3
import struct
import sys
import tempfile
from collections import Counter
from pathlib import Path
from typing import Any


REPO_ROOT = Path(__file__).resolve().parents[1]
OUT_PATH = REPO_ROOT / "tmp_vector_probe/native_external_id_value_reader_correlation_v1.json"
PREVIEW_CACHE_ID = "extrnlid5943B673F7C84B779ED2D7C96E942EAE"
TARGET_VECTOR_ID = "extrnlid62D15CB4395245648869B4AEBAD8FBCE"

sys.path.insert(0, str(REPO_ROOT))
from clip_loader import _split_clip  # noqa: E402


def load_jsonl(path: Path) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
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


def jsonable(value: Any) -> Any:
    if isinstance(value, bytes):
        try:
            return value.decode("ascii")
        except UnicodeDecodeError:
            return {"bytes_len": len(value), "bytes_hex_prefix": value[:64].hex(" ")}
    return value


def table_columns(db: sqlite3.Connection, table: str) -> list[str]:
    return [row[1] for row in db.execute(f'PRAGMA table_info("{table}")')]


def sqlite_references(sqlite_bytes: bytes, ext_ids: set[str]) -> dict[str, list[dict[str, Any]]]:
    path = write_sqlite(sqlite_bytes)
    refs = {ext_id: [] for ext_id in ext_ids}
    try:
        db = sqlite3.connect(path)
        db.row_factory = sqlite3.Row
        tables = [row[0] for row in db.execute("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")]
        for table in tables:
            cols = table_columns(db, table)
            if not cols:
                continue
            try:
                rows = db.execute(f'SELECT rowid, * FROM "{table}"').fetchall()
            except sqlite3.DatabaseError:
                continue
            for row in rows:
                row_dict = {key: jsonable(row[key]) for key in row.keys()}
                row_text = json.dumps(row_dict, ensure_ascii=False)
                for ext_id in ext_ids:
                    if ext_id not in row_text:
                        continue
                    columns = [col for col in cols if ext_id in str(row_dict.get(col, ""))]
                    refs[ext_id].append({
                        "table": table,
                        "columns": columns,
                        "rowid": row_dict.get("rowid"),
                        "main_id": row_dict.get("MainId"),
                        "layer_id": row_dict.get("LayerId"),
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
            return {
                "is_saved_vector_stroke": True,
                "record_offset": off,
                "flags_hex": f"0x{flags:x}",
                "header_len": header_len,
                "point_header_len": point_header_len,
                "stride_a": stride_a,
                "stride_b": stride_b,
                "point_count": point_count,
                "brush_style_id_guess": struct.unpack_from(">I", payload, off + 84)[0] if off + 88 <= len(payload) else None,
            }
    return {"is_saved_vector_stroke": False}


def infer_type(payload_info: dict[str, Any] | None, refs: list[dict[str, Any]]) -> str:
    if payload_info and payload_info.get("is_saved_vector_stroke"):
        return "VectorObjectList.VectorData"
    tables = {ref.get("table") for ref in refs}
    if "VectorObjectList" in tables:
        return "VectorObjectList.VectorData"
    if "TimeLapseBlob" in tables:
        return "TimeLapseBlob / preview-cache"
    hay = " ".join(json.dumps(ref, ensure_ascii=False).lower() for ref in refs)
    if "mipmap" in hay or "thumbnail" in hay:
        return "preview-cache"
    if "offscreen" in hay:
        return "raster/offscreen"
    if "brush" in hay or "material" in hay:
        return "brush/material"
    return "unknown"


def collect_observed(rows: list[dict[str, Any]]) -> dict[str, dict[str, Any]]:
    observed: dict[str, dict[str, Any]] = {}
    for row in rows:
        ext_id = row.get("returned_ascii_preview") or row.get("external_id_ascii")
        if not isinstance(ext_id, str) or not ext_id.startswith("extrnlid"):
            continue
        item = observed.setdefault(ext_id, {
            "external_id": ext_id,
            "events": Counter(),
            "caller_rvas": Counter(),
            "enclosing_functions": Counter(),
            "passed_into_registration_or_loader": False,
            "first_events": [],
        })
        item["events"][str(row.get("event"))] += 1
        if row.get("caller_rva"):
            item["caller_rvas"][str(row.get("caller_rva"))] += 1
        if row.get("enclosing_function"):
            item["enclosing_functions"][str(row.get("enclosing_function"))] += 1
        if row.get("event") in {"registration_entry", "loader_entry", "selector_pre_external_call", "selector_post_external_call"}:
            item["passed_into_registration_or_loader"] = True
        if len(item["first_events"]) < 8:
            item["first_events"].append(row)
    return observed


def main(argv: list[str]) -> int:
    if len(argv) != 3:
        print("usage: python tools/correlate_external_id_value_reader_census.py <census.jsonl> <clip_path>", file=sys.stderr)
        return 2
    trace_path = Path(argv[1])
    clip_path = Path(argv[2])
    rows = load_jsonl(trace_path)
    ext_to_body, sqlite_bytes = _split_clip(str(clip_path))

    payloads: dict[str, dict[str, Any]] = {}
    for idx, raw_body in enumerate(ext_to_body.values()):
        ext_id, payload = split_exta_body(raw_body)
        payloads[ext_id] = {
            "external_id": ext_id,
            "exta_index": idx,
            "payload_size": len(payload),
            "payload_hash_4096": fnv1a32(payload[:4096]),
            "payload_prefix_hex": payload[:96].hex(" "),
            "payload_prefix_ascii": "".join(chr(b) if 0x20 <= b <= 0x7E else "." for b in payload[:96]),
            **vector_record_info(payload),
        }

    observed = collect_observed(rows)
    ids = set(observed) | {PREVIEW_CACHE_ID, TARGET_VECTOR_ID}
    refs = sqlite_references(sqlite_bytes, ids)
    correlated: dict[str, Any] = {}
    for ext_id in sorted(ids):
        obs = observed.get(ext_id, {
            "external_id": ext_id,
            "events": Counter(),
            "caller_rvas": Counter(),
            "enclosing_functions": Counter(),
            "passed_into_registration_or_loader": False,
            "first_events": [],
        })
        payload_info = payloads.get(ext_id)
        ref_rows = refs.get(ext_id, [])
        correlated[ext_id] = {
            "external_id": ext_id,
            "observed_native": ext_id in observed,
            "observed_native_count": sum(obs["events"].values()),
            "observed_native_caller_rvas": dict(obs["caller_rvas"]),
            "observed_enclosing_functions": dict(obs["enclosing_functions"]),
            "passed_into_0x143a3e180_or_0x143a41780": obs["passed_into_registration_or_loader"],
            "matching_exta_payload": payload_info,
            "sqlite_references": ref_rows,
            "sqlite_reference_tables": sorted({ref["table"] for ref in ref_rows}),
            "likely_type": infer_type(payload_info, ref_rows),
            "is_preview_cache_id": ext_id == PREVIEW_CACHE_ID,
            "is_target_vector_id": ext_id == TARGET_VECTOR_ID,
            "importer_parses_as_saved_vector_0x2081_or_0x2011": bool(payload_info and payload_info.get("is_saved_vector_stroke")),
            "first_native_events": obs["first_events"],
        }

    target = correlated[TARGET_VECTOR_ID]
    preview = correlated[PREVIEW_CACHE_ID]
    if target["observed_native"] and target["passed_into_0x143a3e180_or_0x143a41780"]:
        classification = "B"
        next_target = "Next target: registration/parser path after the successful target vector-body load."
    elif target["observed_native"]:
        classification = "A"
        next_target = "Next target: caller/enclosing function that read the target vector id."
    elif preview["observed_native"]:
        classification = "D"
        next_target = "Next target: native SQLite / VectorObjectList row consumer, not the external-id selector."
    else:
        classification = "E"
        next_target = "Next target: re-audit 0x143365840 contract and caller coverage."

    result = {
        "trace_path": str(trace_path),
        "clip_path": str(clip_path),
        "classification": classification,
        "next_single_hook_target": next_target,
        "observed_external_ids": sorted(observed),
        "target_vector_id_observed": target["observed_native"],
        "preview_cache_id_observed": preview["observed_native"],
        "correlated_external_ids": correlated,
        "event_counts": dict(Counter(str(row.get("event")) for row in rows)),
    }
    OUT_PATH.write_text(json.dumps(result, indent=2, sort_keys=True), encoding="utf-8")
    print(OUT_PATH)
    print("observed_external_ids", result["observed_external_ids"])
    print("target_vector_id_observed", result["target_vector_id_observed"])
    print("classification", classification)
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
