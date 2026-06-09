#!/usr/bin/env python3
"""Correlate target VectorData body ownership trace."""

from __future__ import annotations

import argparse
import json
from collections import Counter
from pathlib import Path
from typing import Any


REPO_ROOT = Path(__file__).resolve().parents[1]
OUT_PATH = REPO_ROOT / "tmp_vector_probe/native_target_body_ownership_correlation_v1.json"
TARGET_ID = "extrnlid62D15CB4395245648869B4AEBAD8FBCE"
TARGET_SIZE = 2644
TARGET_HASH = "fnv1a32:7bece4ac"


def load_jsonl(path: Path) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    with path.open("r", encoding="utf-8", errors="replace") as fh:
        for line_no, line in enumerate(fh, 1):
            line = line.strip()
            if not line:
                continue
            try:
                row = json.loads(line)
            except json.JSONDecodeError as exc:
                row = {"event": "json_decode_error", "line": line_no, "error": str(exc)}
            row["_file"] = str(path)
            rows.append(row)
    return rows


def load_truth(path: Path) -> dict[str, Any]:
    data = json.loads(path.read_text(encoding="utf-8"))
    for row in data.get("all_exta_payloads_summary", []):
        if row.get("external_id") == TARGET_ID:
            return row
    raise SystemExit(f"target id missing from {path}")


def rec(row: dict[str, Any]) -> dict[str, Any]:
    value = row.get("record")
    return value if isinstance(value, dict) else {}


def as_list(value: Any) -> list[Any]:
    return value if isinstance(value, list) else []


def all_target_rows(rows: list[dict[str, Any]]) -> list[dict[str, Any]]:
    out = []
    for row in rows:
        text = json.dumps(row, ensure_ascii=False)
        if row.get("is_target") or TARGET_ID in text or TARGET_HASH in text or f'"body_size": {TARGET_SIZE}' in text:
            out.append(row)
    return out


def compact_row(row: dict[str, Any]) -> dict[str, Any]:
    r = rec(row)
    return {
        "event": row.get("event"),
        "timestamp_ms": row.get("timestamp_ms"),
        "thread_id": row.get("thread_id"),
        "invocation_id": r.get("invocation_id"),
        "body_size": r.get("body_size"),
        "dest_ptr": r.get("dest_ptr"),
        "dest_hash": r.get("dest_hash"),
        "return_value": r.get("return_value"),
        "registers": r.get("registers"),
    }


def collect_owner_candidates(rows: list[dict[str, Any]]) -> list[dict[str, Any]]:
    candidates: list[dict[str, Any]] = []
    seen = set()
    for row in rows:
        r = rec(row)
        for phase_key in ("ownership_after_allocation", "ownership_after_body_read", "ownership_candidates"):
            for item in as_list(row.get(phase_key)) + as_list(r.get(phase_key)):
                if not isinstance(item, dict):
                    continue
                key = (
                    item.get("object_ptr"),
                    item.get("object_label"),
                    item.get("field_offset"),
                    item.get("contains_dest_ptr"),
                    item.get("equals_dest_ptr"),
                    item.get("equals_body_size"),
                )
                if key in seen:
                    continue
                seen.add(key)
                candidates.append({
                    "object_ptr": item.get("object_ptr"),
                    "object_label": item.get("object_label"),
                    "field_offset": item.get("field_offset"),
                    "evidence": phase_key,
                    "before_value": item.get("before_value"),
                    "after_value": item.get("field_ptr") or item.get("ptr"),
                    "contains_dest_ptr": bool(item.get("contains_dest_ptr")),
                    "equals_dest_ptr": bool(item.get("equals_dest_ptr")),
                    "equals_body_size": bool(item.get("equals_body_size")),
                    "nested_offsets": item.get("nested_offsets"),
                })
    return candidates


def collect_changes(rows: list[dict[str, Any]], key: str) -> list[dict[str, Any]]:
    out: list[dict[str, Any]] = []
    for row in rows:
        value = row.get(key)
        if not isinstance(value, dict):
            continue
        for label, diff in value.items():
            if not isinstance(diff, dict):
                continue
            for changed in as_list(diff.get("changed"))[:80]:
                if isinstance(changed, dict):
                    out.append({"object_label": label, **changed})
    return out


def collect_pointer_writes(rows: list[dict[str, Any]]) -> list[dict[str, Any]]:
    writes: list[dict[str, Any]] = []
    for row in rows:
        if row.get("event") == "pointer_value_write_dest_ptr":
            write = row.get("write")
            if isinstance(write, dict):
                writes.append(write)
        r = rec(row)
        for write in as_list(row.get("pointer_value_writes")) + as_list(r.get("pointer_value_writes")):
            if isinstance(write, dict):
                writes.append(write)
    unique: list[dict[str, Any]] = []
    seen = set()
    for write in writes:
        key = (write.get("pc_rva"), write.get("destination_address"), write.get("written_value"))
        if key not in seen:
            unique.append(write)
            seen.add(key)
    return unique


def classify(
    owner_candidates: list[dict[str, Any]],
    pointer_writes: list[dict[str, Any]],
    post_success_calls: list[dict[str, Any]],
    target_seen: bool,
) -> tuple[str, str]:
    if not target_seen:
        return "F", "dest_ptr remains unowned in evidence because target body was not observed"

    direct = [c for c in owner_candidates if c.get("equals_dest_ptr")]
    if direct:
        return "A", "dest_ptr stored directly in owner object field"

    nested = [c for c in owner_candidates if c.get("contains_dest_ptr")]
    blob_labels = {"r13", "entry_rdx", "reserve_owner", "reserve_return_owner"}
    if any(c.get("object_label") in blob_labels or "reserve" in str(c.get("object_label")) for c in nested):
        return "B", "dest_ptr stored inside blob/vector-like struct owned by r13/rdx"

    copy_like = [
        row for row in post_success_calls
        if str(row.get("event", "")).lower().find("copy") >= 0
        or str(row.get("event", "")).lower().find("reader") >= 0
    ]
    if copy_like and any(row.get("dest_hash_after") == TARGET_HASH for row in post_success_calls):
        return "C", "dest_ptr copied to another buffer before return"

    parser_like = [
        row for row in post_success_calls
        if row.get("event") not in {"target_cleanup_call", "target_body_reader_leave", "target_reserve_leave"}
    ]
    if parser_like:
        return "D", "dest_ptr passed to immediate parser/consumer before return"

    if pointer_writes or nested:
        return "E", "dest_ptr registered in cache/map or wrapper, but exact cache role is not proven"

    return "F", "dest_ptr remains only in temporary allocation object, with no observed owner"


def correlate(trace_path: Path, truth_path: Path) -> dict[str, Any]:
    rows = load_jsonl(trace_path)
    truth = load_truth(truth_path)
    target_rows = all_target_rows(rows)
    records = [rec(row) for row in target_rows if rec(row)]
    event_counts = Counter(str(row.get("event", "unknown")) for row in rows)

    target_external_id = next((r.get("external_id_ascii") for r in records if r.get("external_id_ascii")), None)
    body_size = next((r.get("body_size") for r in records if r.get("body_size") == TARGET_SIZE), None)
    dest_ptr = next((r.get("dest_ptr") for r in records if r.get("dest_ptr")), None)
    dest_hash = next((r.get("dest_hash") for r in records if r.get("dest_hash")), None)
    owner_candidates = collect_owner_candidates(target_rows)
    post_allocation_changes = collect_changes(target_rows, "post_allocation_changes")
    post_read_changes = collect_changes(target_rows, "post_read_changes")
    pointer_writes = collect_pointer_writes(target_rows)
    post_success_calls = [
        row for row in target_rows
        if "call" in str(row.get("event", "")) or row.get("event") in {"target_body_reader_leave", "target_cleanup_call", "target_reserve_leave"}
    ]

    target_seen = bool(target_rows) and (target_external_id == TARGET_ID or body_size == TARGET_SIZE or dest_hash == TARGET_HASH)
    classification, classification_reason = classify(owner_candidates, pointer_writes, post_success_calls, target_seen)

    return {
        "output_path": str(OUT_PATH),
        "trace_path": str(trace_path),
        "ground_truth_path": str(truth_path),
        "ground_truth_target": truth,
        "event_counts": dict(event_counts),
        "target_external_id": target_external_id,
        "target_external_id_expected": TARGET_ID,
        "body_size": body_size,
        "body_size_expected": TARGET_SIZE,
        "dest_ptr": dest_ptr,
        "dest_hash": dest_hash,
        "dest_hash_expected": TARGET_HASH,
        "owner_candidates": owner_candidates,
        "post_allocation_changes": post_allocation_changes[:200],
        "post_read_changes": post_read_changes[:200],
        "pointer_value_writes": pointer_writes[:100],
        "post_success_calls": [compact_row(row) for row in post_success_calls[:100]],
        "classification": classification,
        "classification_reason": classification_reason,
        "stop_condition_reached": classification == "F",
        "failure_notes": {
            "A_failed": not any(c.get("equals_dest_ptr") for c in owner_candidates),
            "B_failed": not any(c.get("contains_dest_ptr") and ("reserve" in str(c.get("object_label")) or c.get("object_label") in {"r13", "entry_rdx"}) for c in owner_candidates),
            "C_failed": classification != "C",
            "D_failed": classification != "D",
            "E_failed": classification != "E",
        },
        "target_event_excerpt": [compact_row(row) for row in target_rows[:40]],
    }


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("trace_jsonl", type=Path)
    parser.add_argument(
        "--ground-truth",
        type=Path,
        default=REPO_ROOT / "tmp_vector_probe/vectorobjectlist_ground_truth_v1.json",
    )
    parser.add_argument("--output", type=Path, default=OUT_PATH)
    args = parser.parse_args()

    result = correlate(args.trace_jsonl, args.ground_truth)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(result, indent=2, ensure_ascii=False), encoding="utf-8")
    print(json.dumps({
        "output": str(args.output),
        "classification": result["classification"],
        "classification_reason": result["classification_reason"],
        "dest_ptr": result["dest_ptr"],
        "dest_hash": result["dest_hash"],
        "owner_candidate_count": len(result["owner_candidates"]),
        "stop_condition_reached": result["stop_condition_reached"],
    }, indent=2))


if __name__ == "__main__":
    main()
