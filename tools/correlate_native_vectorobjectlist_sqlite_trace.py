"""Correlate native SQLite/VectorObjectList trace with Python ground truth."""

from __future__ import annotations

import json
import sys
from collections import Counter
from pathlib import Path
from typing import Any


REPO_ROOT = Path(__file__).resolve().parents[1]
OUT_PATH = REPO_ROOT / "tmp_vector_probe/native_vectorobjectlist_sqlite_correlation_v1.json"
TARGET_VECTOR_ID = "extrnlid62D15CB4395245648869B4AEBAD8FBCE"
PREVIEW_CACHE_ID = "extrnlid5943B673F7C84B779ED2D7C96E942EAE"


def load_json(path: Path) -> dict[str, Any]:
    return json.loads(path.read_text(encoding="utf-8"))


def load_jsonl(path: Path | None) -> list[dict[str, Any]]:
    if path is None or not path.exists():
        return []
    rows: list[dict[str, Any]] = []
    for line_no, line in enumerate(path.read_text(encoding="utf-8", errors="replace").splitlines(), 1):
        if not line.strip():
            continue
        try:
            row = json.loads(line)
        except json.JSONDecodeError as exc:
            row = {"event": "json_decode_error", "line_no": line_no, "error": str(exc), "raw": line[:240]}
        row["_line_no"] = line_no
        rows.append(row)
    return rows


def row_text(row: dict[str, Any]) -> str:
    return json.dumps(row, ensure_ascii=False)


def collect_id_hits(rows: list[dict[str, Any]], ext_id: str) -> list[dict[str, Any]]:
    observation_events = {
        "sqlite_prepare",
        "sqlite_column_value",
        "wrapper_extrnlid_value",
        "known_selector_ptr_return",
        "known_selector_pre_call",
        "known_selector_post_call",
        "vectorobjectlist_consumer_candidate_entry",
    }
    return [
        row for row in rows
        if row.get("event") in observation_events and ext_id in row_text(row)
    ]


def classify(target_hits: list[dict[str, Any]], preview_hits: list[dict[str, Any]], ground_truth_has_target: bool) -> tuple[str, str]:
    target_passed_loader = any(
        row.get("event") in {"wrapper_extrnlid_value", "known_selector_pre_call", "sqlite_column_value", "vectorobjectlist_consumer_candidate_entry"}
        and (row.get("target_vector_id_seen") or TARGET_VECTOR_ID in row_text(row))
        for row in target_hits
    )
    if target_hits and target_passed_loader:
        return "B", "Target VectorData id is read/observed natively; next inspect whether it is stored or passed to loader."
    if target_hits:
        return "A", "Target VectorData id is read from SQLite on open; next target is the caller that produced it."
    if ground_truth_has_target and preview_hits:
        return "E", "Only preview/cache TimeLapseBlob id is observed; identify condition that materializes VectorObjectList rows."
    if ground_truth_has_target:
        return "D", "Target VectorData id is never read; expand SQLite/wrapper trace."
    return "unclassified", "Ground truth did not contain expected target VectorData id."


def summarize_hits(hits: list[dict[str, Any]]) -> dict[str, Any]:
    return {
        "count": len(hits),
        "events": dict(Counter(str(row.get("event")) for row in hits)),
        "caller_rvas": dict(Counter(str(row.get("caller_rva")) for row in hits if row.get("caller_rva"))),
        "column_names": dict(Counter(str(row.get("column_name")) for row in hits if row.get("column_name"))),
        "sql_statements": list(dict.fromkeys(str(row.get("sql")) for row in hits if row.get("sql")))[:20],
        "first_hits": hits[:12],
    }


def main(argv: list[str]) -> int:
    if len(argv) not in {3, 4}:
        print(
            "usage: python tools/correlate_native_vectorobjectlist_sqlite_trace.py "
            "<vectorobjectlist_ground_truth_v1.json> <sqlite_trace.jsonl> [consumer_trace.jsonl]",
            file=sys.stderr,
        )
        return 2
    ground_truth_path = Path(argv[1])
    sqlite_trace_path = Path(argv[2])
    consumer_trace_path = Path(argv[3]) if len(argv) == 4 else None

    ground_truth = load_json(ground_truth_path)
    sqlite_rows = load_jsonl(sqlite_trace_path)
    consumer_rows = load_jsonl(consumer_trace_path)
    all_native_rows = sqlite_rows + consumer_rows

    ground_truth_has_target = any(
        row.get("VectorData") == TARGET_VECTOR_ID
        for row in ground_truth.get("vectorobjectlist_rows", [])
    )
    target_hits = collect_id_hits(all_native_rows, TARGET_VECTOR_ID)
    preview_hits = collect_id_hits(all_native_rows, PREVIEW_CACHE_ID)
    classification, next_target = classify(target_hits, preview_hits, ground_truth_has_target)

    result = {
        "ground_truth_path": str(ground_truth_path),
        "sqlite_trace_path": str(sqlite_trace_path),
        "consumer_trace_path": str(consumer_trace_path) if consumer_trace_path else None,
        "classification": classification,
        "next_single_hook_target": next_target,
        "ground_truth_has_target_vector_id": ground_truth_has_target,
        "target_vector_id_observed_natively": bool(target_hits),
        "preview_cache_id_observed_natively": bool(preview_hits),
        "target_vector_id_summary": summarize_hits(target_hits),
        "preview_cache_id_summary": summarize_hits(preview_hits),
        "event_counts": dict(Counter(str(row.get("event")) for row in all_native_rows)),
        "ground_truth_target_row": [
            row for row in ground_truth.get("vectorobjectlist_rows", [])
            if row.get("VectorData") == TARGET_VECTOR_ID
        ],
    }
    OUT_PATH.write_text(json.dumps(result, indent=2, sort_keys=True), encoding="utf-8")
    print(OUT_PATH)
    print(f"classification={classification}")
    print(f"target_vector_id_observed_natively={bool(target_hits)}")
    print(f"preview_cache_id_observed_natively={bool(preview_hits)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
