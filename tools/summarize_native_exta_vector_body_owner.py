"""Summarize vector external-body ownership traces."""

from __future__ import annotations

import json
import sys
from collections import Counter
from pathlib import Path
from typing import Any


REPO_ROOT = Path(__file__).resolve().parents[1]
OUT_PATH = REPO_ROOT / "tmp_vector_probe/native_exta_vector_body_owner_summary_v1.json"
TARGET_EXT_ID = "extrnlid62D15CB4395245648869B4AEBAD8FBCE"
TARGET_BODY_SIZE = 2644


def load_jsonl(path: Path) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
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


def latest_by_invocation(rows: list[dict[str, Any]]) -> dict[int, dict[str, Any]]:
    by_invocation: dict[int, dict[str, Any]] = {}
    for row in rows:
        idx = row.get("invocation_index")
        if isinstance(idx, int):
            by_invocation[idx] = row
    return by_invocation


def load_correlation(path: Path) -> dict[str, Any]:
    if not path.exists():
        return {}
    return json.loads(path.read_text(encoding="utf-8"))


def summarize(trace_path: Path, correlation_path: Path) -> dict[str, Any]:
    rows = load_jsonl(trace_path)
    correlation = load_correlation(correlation_path)
    summaries = [r for r in rows if r.get("event") == "summary"]

    final_summary = summaries[-1] if summaries else {}
    counts = final_summary.get("counts", {})
    vector_counts = final_summary.get("vector_counts", {})

    vector_rows = [
        r for r in rows
        if r.get("target_external_id_match")
        or r.get("body_size") == TARGET_BODY_SIZE
        or r.get("does_dest_match_native_ext_body_index_19")
    ]
    vector_by_invocation = latest_by_invocation(vector_rows)
    leaves = [r for r in rows if r.get("event") == "vector_owner_function_leave"]
    post_reads = [r for r in rows if r.get("event") == "vector_owner_post_body_read"]
    caller_posts = [r for r in rows if r.get("event") == "vector_owner_caller_post_return"]

    caller_site_hist = Counter(str(r.get("caller_site")) for r in caller_posts)
    caller_rva_hist = Counter(str(r.get("caller_rva")) for r in caller_posts)
    return_values = Counter(str(r.get("function_return_value")) for r in leaves)

    vector_leaf = leaves[-1] if leaves else (next(reversed(vector_by_invocation.values())) if vector_by_invocation else {})
    vector_post_read = post_reads[-1] if post_reads else {}
    vector_caller = caller_posts[-1] if caller_posts else {}

    dest_ptr = vector_post_read.get("dest_ptr") or vector_leaf.get("dest_ptr")
    return_value = vector_leaf.get("function_return_value")
    caller_site = vector_caller.get("caller_rva") or vector_leaf.get("caller_rva")

    next_consumer_candidate_rva = None
    storage = "unknown"
    confidence = "low"
    if vector_caller:
        site = vector_caller.get("caller_rva")
        if site == "0x3a3e1ac":
            storage = "caller success from owner slot rcx+0x100; sets [rbx+0x3f4]=1"
            next_consumer_candidate_rva = "state object rooted at caller rcx+0x100"
            confidence = "medium"
        elif site == "0x3a3e1d6":
            storage = "caller success from owner slot rcx+0x250; sets [rbx+0x3f4]=1"
            next_consumer_candidate_rva = "state object rooted at caller rcx+0x250"
            confidence = "medium"
    elif vector_leaf:
        confidence = "medium" if dest_ptr else "low"
        storage = "function completed but caller post-return hook did not correlate"

    external_ids = correlation.get("vector_external_ids", [])
    correlation_note = {
        "target_external_id_present_in_correlation": TARGET_EXT_ID in external_ids,
        "correlation_vector_external_ids": external_ids,
    }

    return {
        "trace_path": str(trace_path),
        "correlation_path": str(correlation_path),
        "row_count": len(rows),
        "total_0x143a41780_invocations": counts.get("function_entry", 0),
        "function_leave_count": counts.get("function_leave", 0),
        "vector_body_invocations": len(vector_by_invocation),
        "external_id_match_count": vector_counts.get("entry_external_id_match", 0),
        "body_size_2644_match_count": vector_counts.get("body_size_match", 0),
        "dest_match_count": vector_counts.get("dest_match_vector_body_index_19", 0),
        "dest_ptr_for_vector_body": dest_ptr,
        "return_value_for_vector_body": return_value,
        "caller_site_used": caller_site,
        "caller_site_histogram": dict(caller_site_hist.most_common()),
        "caller_rva_histogram": dict(caller_rva_hist.most_common()),
        "function_return_value_histogram": dict(return_values.most_common()),
        "where_result_appears_stored_or_passed": storage,
        "next_consumer_candidate_rva": next_consumer_candidate_rva,
        "confidence_level": confidence,
        "vector_post_read": vector_post_read,
        "vector_function_leave": vector_leaf,
        "vector_caller_post_return": vector_caller,
        "correlation_note": correlation_note,
        "summary_events_tail": summaries[-3:],
    }


def main(argv: list[str]) -> int:
    if len(argv) < 2:
        print(
            "usage: summarize_native_exta_vector_body_owner.py TRACE.jsonl "
            "[native_exta_body_correlation_v1.json]",
            file=sys.stderr,
        )
        return 2
    trace_path = Path(argv[1])
    correlation_path = Path(argv[2]) if len(argv) >= 3 else REPO_ROOT / "tmp_vector_probe/native_exta_body_correlation_v1.json"
    result = summarize(trace_path, correlation_path)
    OUT_PATH.parent.mkdir(parents=True, exist_ok=True)
    OUT_PATH.write_text(json.dumps(result, indent=2, sort_keys=True), encoding="utf-8")
    print(OUT_PATH)
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
