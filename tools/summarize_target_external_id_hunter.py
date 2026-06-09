#!/usr/bin/env python3
"""Summarize target external-id hunter JSONL traces."""

from __future__ import annotations

import argparse
import json
from collections import Counter
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / "tmp_vector_probe" / "native_target_external_id_hunter_summary_v1.json"
TARGET_ID = "extrnlid62D15CB4395245648869B4AEBAD8FBCE"


def load_jsonl(path: Path) -> list[dict[str, Any]]:
    rows = []
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


def compact(row: dict[str, Any] | None) -> dict[str, Any] | None:
    if not row:
        return None
    keep = [
        "_file",
        "event",
        "timestamp_ms",
        "event_index",
        "process_id",
        "thread_id",
        "caller_rva",
        "path",
        "bytes",
        "buffer",
        "view",
        "size",
        "module",
        "export_name",
        "api",
        "function_name",
        "function_rva",
        "retval",
        "hits",
    ]
    out = {key: row.get(key) for key in keep if key in row}
    if row.get("backtrace"):
        out["backtrace"] = row["backtrace"][:12]
    return out


def summarize(path: Path) -> dict[str, Any]:
    rows = load_jsonl(path)
    event_counts = Counter(str(row.get("event", "unknown")) for row in rows)
    hit_rows = [row for row in rows if row.get("hits")]
    target_rows = [
        row for row in hit_rows
        if any(str(hit.get("kind", "")).startswith("target") for hit in row.get("hits") or [])
        or TARGET_ID in json.dumps(row)
    ]
    caller_counts = Counter(str(row.get("caller_rva", "unknown")) for row in target_rows)
    event_target_counts = Counter(str(row.get("event", "unknown")) for row in target_rows)
    summaries = [row for row in rows if row.get("event") == "summary"]
    final_summary = summaries[-1] if summaries else None
    return {
        "file": str(path),
        "row_count": len(rows),
        "event_counts": dict(event_counts),
        "target_observed": bool(target_rows),
        "first_target_event": compact(target_rows[0]) if target_rows else None,
        "target_events_by_kind": dict(event_target_counts),
        "top_target_caller_rvas": caller_counts.most_common(20),
        "first_20_target_events": [compact(row) for row in target_rows[:20]],
        "final_trace_summary": final_summary,
        "route_hint": route_hint(target_rows[0]) if target_rows else "target id was not observed",
    }


def route_hint(row: dict[str, Any] | None) -> str:
    if not row:
        return "target id was not observed"
    event = row.get("event")
    if event == "ReadFile_buffer_hit":
        return "first target sighting is raw file/mapped input; next hook should be the caller/backtrace parser above ReadFile"
    if event == "sqlite_export_hit":
        return "first target sighting is SQLite text/blob API; next hook should be caller around sqlite column extraction"
    if event == "string_api_hit":
        return "first target sighting is string API; next hook should be caller comparing/copying this external id"
    if event == "known_csp_function_hit":
        return "first target sighting is known CSP route function; next hook should be that caller path"
    return f"first target sighting is {event}"


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("jsonl", nargs="+", type=Path)
    parser.add_argument("--output", type=Path, default=OUT)
    args = parser.parse_args()
    per_file = [summarize(path) for path in args.jsonl]
    out = {
        "output_path": str(args.output),
        "inputs": [str(path) for path in args.jsonl],
        "any_target_observed": any(item["target_observed"] for item in per_file),
        "per_file": per_file,
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(out, indent=2, ensure_ascii=False), encoding="utf-8")
    print(json.dumps({
        "output": str(args.output),
        "any_target_observed": out["any_target_observed"],
        "first_route_hint": next((item["route_hint"] for item in per_file if item["target_observed"]), "target id was not observed"),
    }, indent=2))


if __name__ == "__main__":
    main()
