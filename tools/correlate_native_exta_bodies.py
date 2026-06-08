"""Correlate native CHNKExta body reads with importer-known resources.

The native site at 0x143A41D7A reads only the external body payload after CSP
has already consumed CHNKExta magic, external id, and body-size fields.  This
script therefore matches native payload reads against the importer payload
portion of each CHNKExta body, not against the full CHNKExta chunk body.
"""

from __future__ import annotations

import json
import sqlite3
import struct
import sys
import tempfile
import zlib
from collections import Counter, defaultdict
from pathlib import Path
from typing import Any


REPO_ROOT = Path(__file__).resolve().parents[1]
OUT_PATH = REPO_ROOT / "tmp_vector_probe/native_exta_body_correlation_v1.json"
TABLES_OF_INTEREST = [
    "VectorObjectList",
    "BrushStyle",
    "BrushPatternStyle",
    "BrushPatternImage",
    "Mipmap",
    "MipmapInfo",
    "Offscreen",
    "Layer",
]

sys.path.insert(0, str(REPO_ROOT))
from clip_loader import _parse_exta, _split_clip  # noqa: E402


def load_jsonl(path: Path) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    if not path.exists():
        return rows
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


def body_parts(raw_body: bytes) -> tuple[str, bytes]:
    ext_id_len = struct.unpack_from(">Q", raw_body, 0)[0]
    ext_id = raw_body[8 : 8 + ext_id_len].decode("ascii")
    payload_start = 8 + ext_id_len + 8
    return ext_id, raw_body[payload_start:]


def ascii_preview(data: bytes, limit: int = 96) -> str:
    return "".join(chr(b) if 0x20 <= b <= 0x7E else "." for b in data[:limit])


def hex_preview(data: bytes, limit: int = 32) -> str:
    return " ".join(f"{b:02x}" for b in data[:limit])


def contains_block_data(data: bytes) -> bool:
    return b"BlockDataBeginChunk" in data or b"\x00B\x00l\x00o\x00c\x00k\x00D\x00a\x00t\x00a\x00B\x00e\x00g\x00i\x00n\x00C\x00h\x00u\x00n\x00k" in data


def classify_payload(data: bytes) -> str:
    if not data:
        return "empty"
    preview = ascii_preview(data, 64)
    if contains_block_data(data[:512]):
        return "block-framed"
    if preview.startswith("SQLite format 3"):
        return "SQLite"
    if preview.startswith("CSFCHUNK"):
        return "CSFCHUNK"
    if len(data) >= 2 and data[0] == 0x78:
        return "zlib"
    if len(data) >= 6 and data[4] == 0x78:
        try:
            decoded = zlib.decompress(data[4:])
            if b"\x89PNG\r\n\x1a\n" in decoded[:128]:
                return "size-prefixed-zlib-png-preview"
        except zlib.error:
            pass
        return "size-prefixed-zlib"
    return preview[:24]


def decompressed_size(raw_body: bytes) -> int | None:
    try:
        _ext_id, decoded = _parse_exta(raw_body)
        if decoded:
            return len(decoded)
    except (struct.error, UnicodeDecodeError, zlib.error, ValueError):
        pass
    try:
        _ext_id, payload = body_parts(raw_body)
        if len(payload) >= 6 and payload[4] == 0x78:
            return len(zlib.decompress(payload[4:]))
    except (struct.error, UnicodeDecodeError, zlib.error, ValueError):
        pass
    return None


def write_sqlite(sqlite_bytes: bytes) -> Path:
    fd, path_text = tempfile.mkstemp(suffix=".sqlite3")
    path = Path(path_text)
    try:
        with open(fd, "wb", closefd=True) as f:
            f.write(sqlite_bytes)
    except Exception:
        path.unlink(missing_ok=True)
        raise
    return path


def table_exists(db: sqlite3.Connection, table: str) -> bool:
    row = db.execute(
        "SELECT 1 FROM sqlite_master WHERE type='table' AND name=?",
        (table,),
    ).fetchone()
    return row is not None


def stringify_value(value: Any) -> str | None:
    if value is None:
        return None
    if isinstance(value, bytes):
        try:
            return value.decode("ascii")
        except UnicodeDecodeError:
            return None
    return str(value)


def jsonable_value(value: Any) -> Any:
    if isinstance(value, bytes):
        try:
            return value.decode("ascii")
        except UnicodeDecodeError:
            return {
                "bytes_hex_prefix": value[:64].hex(" "),
                "bytes_len": len(value),
            }
    return value


def jsonable_row(row: sqlite3.Row) -> dict[str, Any]:
    return {k: jsonable_value(row[k]) for k in row.keys()}


def scan_table_references(db: sqlite3.Connection, ext_ids: set[str]) -> dict[str, list[dict[str, Any]]]:
    refs: dict[str, list[dict[str, Any]]] = defaultdict(list)
    for table in TABLES_OF_INTEREST:
        if not table_exists(db, table):
            continue
        rows = db.execute(f'SELECT rowid AS __rowid__, * FROM "{table}"').fetchall()
        for row in rows:
            keys = row.keys()
            main_id = row["MainId"] if "MainId" in keys else row["__rowid__"]
            for key in keys:
                if key == "__rowid__":
                    continue
                text = stringify_value(row[key])
                if text in ext_ids:
                    refs[text].append({
                        "table": table,
                        "rowid": row["__rowid__"],
                        "main_id": main_id,
                        "column": key,
                    })
    return refs


def semantic_links(db: sqlite3.Connection, ext_id: str) -> dict[str, Any]:
    result: dict[str, Any] = {
        "vector_object_rows": [],
        "offscreen_rows": [],
        "mipmap_info_rows": [],
        "mipmap_rows": [],
        "layer_rows": [],
        "brush_pattern_image_rows": [],
    }

    ref_values = (ext_id, ext_id.encode("ascii"))

    if table_exists(db, "VectorObjectList"):
        for row in db.execute(
            "SELECT * FROM VectorObjectList WHERE VectorData IN (?,?)",
            ref_values,
        ).fetchall():
            result["vector_object_rows"].append(jsonable_row(row))

    offscreen_ids: list[int] = []
    if table_exists(db, "Offscreen"):
        for row in db.execute(
            "SELECT * FROM Offscreen WHERE BlockData IN (?,?)",
            ref_values,
        ).fetchall():
            result["offscreen_rows"].append(jsonable_row(row))
            if "MainId" in row.keys():
                offscreen_ids.append(int(row["MainId"]))

    mipmap_info_ids: list[int] = []
    if offscreen_ids and table_exists(db, "MipmapInfo"):
        q = ",".join("?" for _ in offscreen_ids)
        for row in db.execute(f"SELECT * FROM MipmapInfo WHERE Offscreen IN ({q})", offscreen_ids).fetchall():
            result["mipmap_info_rows"].append(jsonable_row(row))
            if "MainId" in row.keys():
                mipmap_info_ids.append(int(row["MainId"]))

    mipmap_ids: list[int] = []
    if mipmap_info_ids and table_exists(db, "Mipmap"):
        q = ",".join("?" for _ in mipmap_info_ids)
        for row in db.execute(f"SELECT * FROM Mipmap WHERE BaseMipmapInfo IN ({q})", mipmap_info_ids).fetchall():
            result["mipmap_rows"].append(jsonable_row(row))
            if "MainId" in row.keys():
                mipmap_ids.append(int(row["MainId"]))

    if mipmap_ids and table_exists(db, "Layer"):
        cols = [r["name"] for r in db.execute('PRAGMA table_info("Layer")').fetchall()]
        candidate_cols = [c for c in cols if "Mipmap" in c or c.endswith("Mipmap")]
        for col in candidate_cols:
            q = ",".join("?" for _ in mipmap_ids)
            try:
                for row in db.execute(f'SELECT MainId, LayerName, LayerType, "{col}" FROM Layer WHERE "{col}" IN ({q})', mipmap_ids).fetchall():
                    result["layer_rows"].append(jsonable_row(row) | {"reference_column": col})
            except sqlite3.Error:
                continue

    if mipmap_ids and table_exists(db, "BrushPatternImage"):
        q = ",".join("?" for _ in mipmap_ids)
        for row in db.execute(f"SELECT * FROM BrushPatternImage WHERE Mipmap IN ({q})", mipmap_ids).fetchall():
            result["brush_pattern_image_rows"].append(jsonable_row(row))

    return result


def likely_resource_type(refs: list[dict[str, Any]], links: dict[str, Any]) -> str:
    if links["vector_object_rows"]:
        return "vector object blob"
    if links["brush_pattern_image_rows"]:
        return "brush material/mipmap"
    if links["layer_rows"]:
        columns = {str(row.get("reference_column")) for row in links["layer_rows"]}
        if any("Mask" in col for col in columns):
            return "layer mask offscreen"
        return "layer render/offscreen cache"
    if links["offscreen_rows"]:
        return "offscreen/cache"
    tables = {r["table"] for r in refs}
    columns = {r["column"] for r in refs}
    if "VectorObjectList" in tables and "VectorData" in columns:
        return "vector object blob"
    if "Offscreen" in tables and "BlockData" in columns:
        return "offscreen/cache"
    if "BrushStyle" in tables or "BrushPatternStyle" in tables:
        return "brush style/resource"
    return "unknown"


def likely_resource_type_from_payload(payload_signature: str, refs: list[dict[str, Any]], links: dict[str, Any]) -> str:
    base = likely_resource_type(refs, links)
    if base != "unknown":
        return base
    if payload_signature == "size-prefixed-zlib-png-preview":
        return "preview/cache png"
    return base


def native_body_events(rows: list[dict[str, Any]]) -> list[dict[str, Any]]:
    bodies = [r for r in rows if r.get("event") == "external_body"]
    if bodies:
        return bodies
    # Older chunk-reader traces used "chunk_read" and route_label.
    return [
        r for r in rows
        if r.get("event") == "chunk_read"
        and (r.get("route_label") == "external_chunk_body" or r.get("caller_rva") == "0x3a41d7f")
    ]


def normalize_native_id(row: dict[str, Any]) -> str | None:
    value = row.get("external_id_ascii") or row.get("nearby_external_id")
    if not isinstance(value, str):
        return None
    value = value.strip(".\x00 ")
    return value or None


def match_native_event(row: dict[str, Any], payloads: dict[str, dict[str, Any]]) -> tuple[str | None, str]:
    native_id = normalize_native_id(row)
    if native_id in payloads:
        return native_id, "external_id"
    if native_id:
        return native_id, "native_external_id_not_top_level_chmk_exta"
    size = row.get("requested_size")
    prefix_hex = row.get("prefix_hex") or row.get("buffer_prefix_hex") or ""
    candidates = []
    for ext_id, info in payloads.items():
        if size is not None and int(size) != info["payload_size"]:
            continue
        if prefix_hex and info["payload_prefix_hex"].startswith(str(prefix_hex).split(" ")[:1][0]):
            candidates.append(ext_id)
        elif size is not None:
            candidates.append(ext_id)
    if len(candidates) == 1:
        return candidates[0], "payload_size"
    if candidates:
        return candidates[0], "payload_size_ambiguous_first"
    return None, "unmatched"


def summarize(trace_path: Path, clip_path: Path) -> dict[str, Any]:
    rows = load_jsonl(trace_path)
    bodies = native_body_events(rows)
    exta_bodies, sqlite_bytes = _split_clip(str(clip_path))
    db_path = write_sqlite(sqlite_bytes)
    try:
        db = sqlite3.connect(db_path)
        db.row_factory = sqlite3.Row
        native_ids = {
            native_id
            for native_id in (normalize_native_id(row) for row in bodies)
            if native_id
        }
        top_level_ext_ids = set(exta_bodies)
        all_observed_ext_ids = top_level_ext_ids | native_ids
        refs_by_ext_id = scan_table_references(db, all_observed_ext_ids)

        payloads: dict[str, dict[str, Any]] = {}
        for ext_id, raw_body in exta_bodies.items():
            parsed_id, payload = body_parts(raw_body)
            if parsed_id != ext_id:
                raise ValueError(f"Exta id mismatch: {parsed_id!r} != {ext_id!r}")
            links = semantic_links(db, ext_id)
            refs = refs_by_ext_id.get(ext_id, [])
            payloads[ext_id] = {
                "external_id": ext_id,
                "full_chmk_exta_body_size": len(raw_body),
                "payload_size": len(payload),
                "payload_prefix_hex": hex_preview(payload, 48),
                "payload_prefix_ascii": ascii_preview(payload, 96),
                "payload_signature": classify_payload(payload),
                "compressed_or_block_framed": contains_block_data(payload[:512]) or classify_payload(payload).endswith("zlib"),
                "decompressed_size": decompressed_size(raw_body),
                "sqlite_references": refs,
                "semantic_links": links,
                "likely_resource_type": likely_resource_type_from_payload(classify_payload(payload), refs, links),
            }

        native_matches = []
        for row in bodies:
            ext_id, method = match_native_event(row, payloads)
            info = payloads.get(ext_id) if ext_id else None
            refs = refs_by_ext_id.get(ext_id, []) if ext_id else []
            links = semantic_links(db, ext_id) if ext_id else {
                "brush_pattern_image_rows": [],
                "layer_rows": [],
                "mipmap_info_rows": [],
                "mipmap_rows": [],
                "offscreen_rows": [],
                "vector_object_rows": [],
            }
            native_resource_type = (
                info["likely_resource_type"]
                if info
                else likely_resource_type(refs, links)
                if ext_id
                else None
            )
            native_matches.append({
                "ext_body_index": row.get("ext_body_index", row.get("chunk_read_index")),
                "native_requested_size": row.get("requested_size"),
                "native_return_value_raw": row.get("return_value_raw", row.get("return_value")),
                "native_prefix_hex": row.get("prefix_hex", row.get("buffer_prefix_hex")),
                "native_prefix_ascii": row.get("prefix_ascii", row.get("buffer_prefix_ascii")),
                "native_signature": row.get("signature"),
                "native_external_id_ascii": normalize_native_id(row),
                "matched_external_id": ext_id,
                "match_method": method,
                "likely_resource_type": native_resource_type,
                "sqlite_references": info["sqlite_references"] if info else refs,
                "semantic_links": info["semantic_links"] if info else links,
                "payload_size": info["payload_size"] if info else None,
                "payload_signature": info["payload_signature"] if info else None,
                "decompressed_size": info["decompressed_size"] if info else None,
            })

        resource_hist = Counter(info["likely_resource_type"] for info in payloads.values())
        native_resource_hist = Counter(m["likely_resource_type"] or "unmatched" for m in native_matches)
        vector_ext_ids = [
            ext_id for ext_id, info in payloads.items()
            if info["likely_resource_type"] == "vector object blob"
        ]
        offscreen_ext_ids = [
            ext_id for ext_id, info in payloads.items()
            if "offscreen" in info["likely_resource_type"] or "cache" in info["likely_resource_type"]
        ]

        table_counts = {}
        table_columns = {}
        for table in TABLES_OF_INTEREST:
            if not table_exists(db, table):
                continue
            table_counts[table] = db.execute(f'SELECT COUNT(*) FROM "{table}"').fetchone()[0]
            table_columns[table] = [r["name"] for r in db.execute(f'PRAGMA table_info("{table}")').fetchall()]

        return {
            "trace_path": str(trace_path),
            "clip_path": str(clip_path),
            "row_count": len(rows),
            "native_external_body_event_count": len(bodies),
            "clip_external_body_count": len(exta_bodies),
            "native_observed_external_id_count": len(native_ids),
            "native_external_ids_not_top_level_chmk_exta": sorted(native_ids - top_level_ext_ids),
            "resource_type_histogram_all_clip_exta": dict(resource_hist.most_common()),
            "resource_type_histogram_native_matches": dict(native_resource_hist.most_common()),
            "table_counts": table_counts,
            "table_columns": table_columns,
            "vector_external_ids": vector_ext_ids,
            "offscreen_external_ids": offscreen_ext_ids,
            "per_native_body": native_matches,
            "per_clip_external_body": list(payloads.values()),
            "vector_specific_narrowing": {
                "vector_is_sqlite_resident": False,
                "vector_is_external_body_resident": bool(vector_ext_ids),
                "vector_external_ids": vector_ext_ids,
                "note": (
                    "VectorObjectList rows store VectorData external ids; the saved vector blob lives in CHNKExta payloads "
                    "when vector_external_ids is non-empty. SQLite owns the reference/index, not the full vector body bytes."
                ),
            },
        }
    finally:
        try:
            db.close()  # type: ignore[name-defined]
        except Exception:
            pass
        db_path.unlink(missing_ok=True)


def main(argv: list[str]) -> int:
    if len(argv) < 3:
        print(
            "usage: correlate_native_exta_bodies.py TRACE.jsonl CLIP_PATH",
            file=sys.stderr,
        )
        return 2
    trace_path = Path(argv[1])
    clip_path = Path(argv[2])
    result = summarize(trace_path, clip_path)
    OUT_PATH.parent.mkdir(parents=True, exist_ok=True)
    OUT_PATH.write_text(json.dumps(result, indent=2, sort_keys=True), encoding="utf-8")
    print(OUT_PATH)
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
