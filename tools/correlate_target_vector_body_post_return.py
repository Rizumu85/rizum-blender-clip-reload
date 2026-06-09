#!/usr/bin/env python3
"""Correlate target VectorData external-body post-return trace rows.

This is intentionally narrow: it consumes the read-only
native_target_vector_body_post_return trace and reports where the target
2644-byte VectorData body appears to go after 0x143A41780 succeeds.
"""

from __future__ import annotations

import argparse
import json
from collections import Counter
from pathlib import Path
from typing import Any


REPO_ROOT = Path(__file__).resolve().parents[1]
OUT_PATH = REPO_ROOT / "tmp_vector_probe/native_target_vector_body_post_return_correlation_v1.json"
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


def load_ground_truth(path: Path) -> dict[str, Any]:
    data = json.loads(path.read_text(encoding="utf-8"))
    matches = [
        row for row in data.get("all_exta_payloads_summary", [])
        if row.get("external_id") == TARGET_ID
    ]
    if not matches:
        raise SystemExit(f"target external id not found in ground truth: {path}")
    return matches[0]


def record_from(row: dict[str, Any]) -> dict[str, Any]:
    rec = row.get("record")
    return rec if isinstance(rec, dict) else {}


def flatten_offsets(value: Any) -> list[str]:
    out: list[str] = []
    if isinstance(value, list):
        out.extend(str(item) for item in value)
    elif isinstance(value, dict):
        for key, items in value.items():
            if isinstance(items, list):
                out.extend(f"{key}:{item}" for item in items)
    return out


def compact_event(row: dict[str, Any]) -> dict[str, Any]:
    rec = record_from(row)
    return {
        "event": row.get("event"),
        "timestamp_ms": row.get("timestamp_ms"),
        "thread_id": row.get("thread_id"),
        "invocation_index": rec.get("invocation_index"),
        "slot": rec.get("slot"),
        "caller_rva": rec.get("caller_rva"),
        "parent_ptr": rec.get("parent_ptr"),
        "owner_ptr": rec.get("owner_ptr"),
        "body_size": rec.get("body_size"),
        "dest_ptr": rec.get("dest_ptr"),
        "dest_hash": rec.get("dest_hash"),
        "return_eax": rec.get("return_eax"),
        "backtrace_rvas": [bt.get("rva") for bt in row.get("backtrace", [])[:12]],
    }


def changed_field_count(diff: Any) -> int:
    if not isinstance(diff, dict):
        return 0
    changed = diff.get("changed")
    return len(changed) if isinstance(changed, list) else 0


def correlate(trace_path: Path, clip_path: Path, truth_path: Path) -> dict[str, Any]:
    rows = load_jsonl(trace_path)
    truth = load_ground_truth(truth_path)
    event_counts = Counter(str(row.get("event", "unknown")) for row in rows)

    target_rows = [
        row for row in rows
        if row.get("is_target")
        or TARGET_ID in json.dumps(row, ensure_ascii=False)
        or TARGET_HASH in json.dumps(row, ensure_ascii=False)
        or f'"body_size": {TARGET_SIZE}' in json.dumps(row, ensure_ascii=False)
    ]
    records = [record_from(row) for row in target_rows if record_from(row)]

    target_id_observed = any(rec.get("external_id_ascii") == TARGET_ID for rec in records)
    body_size_match = any(rec.get("body_size") == TARGET_SIZE for rec in records)
    body_hash_match = any(rec.get("dest_hash") == TARGET_HASH for rec in records)
    dest_ptrs = [rec.get("dest_ptr") for rec in records if rec.get("dest_ptr")]
    owner_ptrs = [rec.get("owner_ptr") for rec in records if rec.get("owner_ptr")]
    parent_ptrs = [rec.get("parent_ptr") for rec in records if rec.get("parent_ptr")]

    field_offsets_dest: list[str] = []
    field_offsets_size: list[str] = []
    nested_dest_refs: list[Any] = []
    parent_changed = 0
    owner_changed = 0
    for row in target_rows:
        rec = record_from(row)
        field_offsets_dest.extend(flatten_offsets(rec.get("fields_equal_dest_ptr")))
        field_offsets_size.extend(flatten_offsets(rec.get("fields_equal_body_size")))
        nested = rec.get("nested_fields_containing_dest")
        if nested:
            nested_dest_refs.append(nested)
        parent_changed += changed_field_count(row.get("parent_diff"))
        owner_changed += changed_field_count(row.get("owner_diff"))

    post_success_calls = [
        row for row in target_rows
        if "call" in str(row.get("event", "")) and row.get("event") not in {"loader_caller_post_return"}
    ]

    if post_success_calls:
        classification = "C"
        classification_label = "passed to parser/consumer"
        next_target = "first post-success call from trace"
    elif field_offsets_dest:
        classification = "A"
        classification_label = "stored directly"
        next_target = "0x14331EB72"
    elif nested_dest_refs:
        classification = "D"
        classification_label = "registered into owner/cache wrapper"
        next_target = "0x14331EB72"
    elif body_hash_match and (parent_changed or owner_changed):
        classification = "D"
        classification_label = "owner/resource state changed without direct dest field"
        next_target = "0x14331EB72"
    elif body_hash_match:
        classification = "E"
        classification_label = "left only in temporary owner"
        next_target = None
    else:
        classification = None
        classification_label = "target body was not recovered"
        next_target = None

    next_reason = None
    if next_target == "0x14331EB72":
        next_reason = (
            "0x143A3E180 returns to the observed upstream route at 0x331EB72; "
            "hook it next only if this correlation shows storage/cache state, "
            "carrying parent+0x100 owner and parent+0x3f4 success flag."
        )

    return {
        "output_path": str(OUT_PATH),
        "trace_path": str(trace_path),
        "clip_path": str(clip_path),
        "ground_truth_path": str(truth_path),
        "ground_truth_target": truth,
        "event_counts": dict(event_counts),
        "target_id_observed": target_id_observed,
        "body_size_match": body_size_match,
        "body_hash_match": body_hash_match,
        "dest_ptr": dest_ptrs[0] if dest_ptrs else None,
        "all_dest_ptrs": sorted(set(dest_ptrs)),
        "owner_context_pointer_candidates": {
            "owner_ptrs": sorted(set(owner_ptrs)),
            "parent_ptrs": sorted(set(parent_ptrs)),
        },
        "field_offsets_that_received_dest_ptr_or_metadata": {
            "dest_ptr": sorted(set(field_offsets_dest)),
            "body_size": sorted(set(field_offsets_size)),
            "nested_dest_refs": nested_dest_refs[:20],
        },
        "post_success_calls": [compact_event(row) for row in post_success_calls],
        "target_event_excerpt": [compact_event(row) for row in target_rows[:30]],
        "body_classification": classification,
        "body_classification_label": classification_label,
        "next_single_hook_target": next_target,
        "next_single_hook_reason": next_reason,
        "stop_condition": (
            "continue to next single target if classification is A/C/D; "
            "stop route discovery as research blocker if classification is E or target is absent"
        ),
    }


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("trace_jsonl", type=Path)
    parser.add_argument(
        "--clip",
        type=Path,
        default=REPO_ROOT / "img/Vector_SizePressure.clip",
    )
    parser.add_argument(
        "--ground-truth",
        type=Path,
        default=REPO_ROOT / "tmp_vector_probe/vectorobjectlist_ground_truth_v1.json",
    )
    parser.add_argument("--output", type=Path, default=OUT_PATH)
    args = parser.parse_args()

    result = correlate(args.trace_jsonl, args.clip, args.ground_truth)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(result, indent=2, ensure_ascii=False), encoding="utf-8")
    print(json.dumps({
        "output": str(args.output),
        "target_id_observed": result["target_id_observed"],
        "body_size_match": result["body_size_match"],
        "body_hash_match": result["body_hash_match"],
        "classification": result["body_classification"],
        "next_single_hook_target": result["next_single_hook_target"],
    }, indent=2))


if __name__ == "__main__":
    main()
