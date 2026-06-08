"""Summarize native_vector_body_route_positive_control JSONL traces."""

from __future__ import annotations

import json
import sys
from collections import Counter, defaultdict
from pathlib import Path
from typing import Any


REPO_ROOT = Path(__file__).resolve().parents[1]
OUT_PATH = REPO_ROOT / "tmp_vector_probe/native_vector_body_route_positive_control_summary_v1.json"
TARGET_EXT_ID = "extrnlid62D15CB4395245648869B4AEBAD8FBCE"
TARGET_HASH = "fnv1a32:7bece4ac"
TARGET_BODY_SIZE = 2644


def load_jsonl(path: Path) -> list[dict[str, Any]]:
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


def update_hist(dst: dict[str, int], src: dict[str, Any] | None) -> None:
    if not isinstance(src, dict):
        return
    for key, value in src.items():
        try:
            dst[str(key)] = dst.get(str(key), 0) + int(value)
        except (TypeError, ValueError):
            continue


def top(counter: Counter[str] | dict[str, int], limit: int = 20) -> list[dict[str, Any]]:
    items = counter.items() if isinstance(counter, dict) else counter.items()
    return [{"key": key, "count": count} for key, count in sorted(items, key=lambda kv: (-kv[1], kv[0]))[:limit]]


def classify(facts: dict[str, Any]) -> tuple[str, str, str]:
    file_seen = facts["file_target_seen"]
    chunk_external = facts["did_0x20575a0_caller_0x3a41d7f_occur"]
    loader_seen = facts["did_0x143a41780_occur"]
    registration_seen = facts["did_0x143a3e180_occur"]
    vector_match = facts["total_vector_external_id_matches"] > 0
    body_match = facts["total_body_size_2644_matches"] > 0
    hash_match = facts["total_fnv1a32_7bece4ac_matches"] > 0

    if file_seen and chunk_external and loader_seen and registration_seen and (vector_match or body_match or hash_match):
        return (
            "A",
            "Full positive route",
            "Next target: the immediate post-registration parser or consumer observed after 0x143A3E180.",
        )
    if file_seen and chunk_external and not loader_seen:
        return (
            "B",
            "File/chunk reader hit, but 0x143A41780 did not",
            "Next target: disassemble/caller-backtrace from 0x20575A0 caller=0x3A41D7F in this exact run.",
        )
    if file_seen and not chunk_external:
        return (
            "C",
            "File read hit, but chunk reader 0x3A41D7F did not",
            "Next target: compare trigger/cache conditions against the previous successful external-body route.",
        )
    if not file_seen:
        return (
            "D",
            "No file read hit",
            "Next target: fix trigger/PID/window before making native route conclusions.",
        )
    if loader_seen and not (vector_match or body_match or hash_match):
        return (
            "E",
            "0x143A41780 hit but no vector body match",
            "Next target: compare original vs timestamped copy and resource/cache state.",
        )
    return (
        "unclassified",
        "Observed route does not match the requested decision tree exactly",
        "Next target: inspect the first non-empty route/backtrace record in this same trace.",
    )


def summarize(paths: list[Path]) -> dict[str, Any]:
    all_rows: list[dict[str, Any]] = []
    per_file: dict[str, Any] = {}
    for path in paths:
        rows = load_jsonl(path)
        all_rows.extend(rows)
        per_file[str(path)] = {
            "row_count": len(rows),
            "event_counts": dict(Counter(str(row.get("event")) for row in rows)),
        }

    event_counts = Counter(str(row.get("event")) for row in all_rows)
    chunk_callers = Counter(
        str(row.get("caller_rva"))
        for row in all_rows
        if row.get("event") == "chunk_read" and row.get("caller_rva")
    )
    file_callers = Counter(
        str(row.get("caller_rva"))
        for row in all_rows
        if row.get("event") == "file_read" and row.get("caller_rva")
    )
    signatures = Counter(
        str(row.get("signature") or row.get("probable_type"))
        for row in all_rows
        if row.get("event") in {"chunk_read", "file_read"} and (row.get("signature") or row.get("probable_type"))
    )
    loader_external_ids = Counter(
        str(row.get("external_id_ascii"))
        for row in all_rows
        if row.get("event") in {"loader_entry", "loader_leave", "loader_post_body_read"} and row.get("external_id_ascii")
    )
    loader_body_sizes = Counter(
        str(row.get("body_size"))
        for row in all_rows
        if row.get("event") in {"loader_post_body_size", "loader_leave", "loader_post_body_read"} and row.get("body_size") is not None
    )
    loader_hashes = Counter(
        str(row.get("dest_hash"))
        for row in all_rows
        if row.get("event") in {"loader_post_body_read", "loader_leave"} and row.get("dest_hash")
    )

    summary_hist: dict[str, int] = {}
    for row in all_rows:
        if row.get("event") == "summary":
            update_hist(summary_hist, row.get("counts"))

    facts = {
        "trace_paths": [str(path) for path in paths],
        "event_counts": dict(event_counts),
        "file_target_seen": any(row.get("event") == "file_read" for row in all_rows)
        or any(bool(row.get("file_target_seen")) for row in all_rows if row.get("event") == "summary"),
        "total_target_readfile_calls": event_counts.get("file_read", 0),
        "total_chunk_reader_route_calls": event_counts.get("chunk_read", 0),
        "chunk_reader_caller_histogram": dict(chunk_callers),
        "file_read_caller_histogram": dict(file_callers),
        "signature_histogram": dict(signatures),
        "total_0x143a41780_invocations": event_counts.get("loader_entry", 0),
        "total_vector_external_id_matches": sum(
            1
            for row in all_rows
            if row.get("event") in {"loader_entry", "loader_leave", "loader_post_body_read"}
            and row.get("external_id_ascii") == TARGET_EXT_ID
        ),
        "total_body_size_2644_matches": sum(
            1
            for row in all_rows
            if row.get("event") in {"loader_post_body_size", "loader_leave", "loader_post_body_read"}
            and row.get("body_size") == TARGET_BODY_SIZE
        ),
        "total_fnv1a32_7bece4ac_matches": sum(
            1
            for row in all_rows
            if row.get("event") in {"loader_post_body_read", "loader_leave"}
            and row.get("dest_hash") == TARGET_HASH
        ),
        "total_0x143a3e180_invocations": event_counts.get("registration_entry", 0),
        "did_0x20575a0_caller_0x3a41d7f_occur": any(
            row.get("event") == "chunk_read" and row.get("caller_rva") == "0x3a41d7f"
            for row in all_rows
        )
        or any(bool(row.get("did_0x20575a0_caller_0x3a41d7f_occur")) for row in all_rows if row.get("event") == "summary"),
        "did_0x143a41780_occur": event_counts.get("loader_entry", 0) > 0,
        "did_0x143a3e180_occur": event_counts.get("registration_entry", 0) > 0,
    }
    classification, classification_label, next_target = classify(facts)

    interesting = [
        row for row in all_rows
        if row.get("event") in {
            "file_read",
            "chunk_read",
            "loader_entry",
            "loader_post_body_size",
            "loader_post_body_read",
            "loader_leave",
            "registration_entry",
            "registration_first_post_call",
            "registration_second_post_call",
        }
    ][:60]

    result = {
        "classification": classification,
        "classification_label": classification_label,
        "next_single_hook_target": next_target,
        **facts,
        "per_file": per_file,
        "summary_counts_histogram": summary_hist,
        "top_chunk_reader_callers": top(chunk_callers),
        "top_file_read_callers": top(file_callers),
        "top_signatures": top(signatures),
        "loader_external_id_histogram": dict(loader_external_ids),
        "loader_body_size_histogram": dict(loader_body_sizes),
        "loader_hash_histogram": dict(loader_hashes),
        "first_interesting_events": interesting,
    }
    return result


def main(argv: list[str]) -> int:
    if len(argv) < 2:
        print("usage: python tools/summarize_native_vector_body_route_positive_control.py <trace.jsonl> [more.jsonl ...]", file=sys.stderr)
        return 2
    paths = [Path(arg) for arg in argv[1:]]
    result = summarize(paths)
    OUT_PATH.write_text(json.dumps(result, indent=2, sort_keys=True), encoding="utf-8")
    print(OUT_PATH)
    print(f"classification={result['classification']} {result['classification_label']}")
    print(f"file_target_seen={result['file_target_seen']}")
    print(f"total_target_readfile_calls={result['total_target_readfile_calls']}")
    print(f"total_chunk_reader_route_calls={result['total_chunk_reader_route_calls']}")
    print(f"total_0x143a41780_invocations={result['total_0x143a41780_invocations']}")
    print(f"total_0x143a3e180_invocations={result['total_0x143a3e180_invocations']}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
