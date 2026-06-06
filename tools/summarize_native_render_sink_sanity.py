#!/usr/bin/env python3
"""Summarize native_render_sink_sanity_trace_v1 JSONL files."""

from __future__ import annotations

import argparse
import json
from collections import Counter, defaultdict
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[1]
DEFAULT_OUT = ROOT / "tmp_vector_probe" / "native_render_sink_sanity_summary_v1.json"

COUNTER_KEYS = [
    "plot_1422D8550",
    "submit_14255DFE0",
    "submit_14260F550",
    "bridge_14260DB90",
    "dispatcher_14263F410",
    "hard_circle_142640150",
    "row_writer_14263AC30",
]

EVENT_TO_FLAG = {
    "plot_1422D8550": "hit_1422D8550",
    "dispatcher_14263F410": "hit_14263F410",
    "hard_circle_142640150": "hit_142640150",
    "row_writer_14263AC30": "hit_14263AC30",
}


def load_jsonl(path: Path) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    with path.open("r", encoding="utf-8") as f:
        for lineno, line in enumerate(f, 1):
            line = line.strip()
            if not line:
                continue
            try:
                rows.append(json.loads(line))
            except json.JSONDecodeError as exc:
                rows.append(
                    {
                        "event": "json_decode_error",
                        "line_number": lineno,
                        "error": str(exc),
                        "raw": line[:200],
                    }
                )
    return rows


def top_counter(counter: Counter[str], limit: int = 50) -> dict[str, int]:
    return dict(counter.most_common(limit))


def summarize_file(path: Path) -> dict[str, Any]:
    rows = load_jsonl(path)
    event_counts = Counter(str(row.get("event")) for row in rows)
    function_counts = Counter(str(row.get("function_key")) for row in rows if row.get("function_key"))
    row_writer_callers = Counter()
    dispatcher_helper_edges = Counter()
    first_callers: dict[str, list[str | None]] = defaultdict(list)

    for row in rows:
      event = str(row.get("event"))
      function_key = row.get("function_key")
      caller_rva = row.get("caller_rva")
      if function_key and len(first_callers[str(function_key)]) < 20:
          first_callers[str(function_key)].append(caller_rva)
      if event == "row_writer_14263AC30":
          row_writer_callers[str(caller_rva)] += 1
      if event.startswith("dispatcher_helper_"):
          function_rva = str(row.get("function_rva"))
          dispatcher_helper_edges[f"{function_rva}<-{caller_rva}"] += 1
      if event == "summary":
          for caller, count in (row.get("row_writer_callers") or {}).items():
              row_writer_callers[str(caller)] += int(count)
          for edge, count in (row.get("dispatcher_helper_callers") or {}).items():
              dispatcher_helper_edges[str(edge)] += int(count)

    flags = {flag: False for flag in EVENT_TO_FLAG.values()}
    for event, flag in EVENT_TO_FLAG.items():
        flags[flag] = event_counts[event] > 0 or function_counts[event] > 0

    return {
        "path": str(path),
        "line_count": len(rows),
        "event_counts": dict(event_counts),
        "function_counts": dict(function_counts),
        "row_writer_caller_histogram": top_counter(row_writer_callers),
        "dispatcher_helper_caller_target_histogram": top_counter(dispatcher_helper_edges),
        "was_142640150_hit": flags["hit_142640150"],
        "was_14263AC30_hit": flags["hit_14263AC30"],
        "was_1422D8550_hit": flags["hit_1422D8550"],
        "was_14263F410_hit": flags["hit_14263F410"],
        "hit_1422D8550_without_later_row_writer": flags["hit_1422D8550"] and not flags["hit_14263AC30"],
        "first_20_callers_by_function": dict(first_callers),
        "json_decode_errors": [row for row in rows if row.get("event") == "json_decode_error"],
    }


def build_summary(paths: list[Path]) -> dict[str, Any]:
    per_file = [summarize_file(path) for path in paths]
    aggregate_event_counts = Counter()
    aggregate_function_counts = Counter()
    aggregate_row_writer_callers = Counter()
    aggregate_dispatcher_edges = Counter()

    for item in per_file:
        aggregate_event_counts.update(item["event_counts"])
        aggregate_function_counts.update(item["function_counts"])
        aggregate_row_writer_callers.update(item["row_writer_caller_histogram"])
        aggregate_dispatcher_edges.update(item["dispatcher_helper_caller_target_histogram"])

    return {
        "inputs": [str(path) for path in paths],
        "per_file": per_file,
        "aggregate": {
            "event_counts": dict(aggregate_event_counts),
            "function_counts": dict(aggregate_function_counts),
            "row_writer_caller_histogram": top_counter(aggregate_row_writer_callers),
            "dispatcher_helper_caller_target_histogram": top_counter(aggregate_dispatcher_edges),
            "any_142640150_hit": any(item["was_142640150_hit"] for item in per_file),
            "any_14263AC30_hit": any(item["was_14263AC30_hit"] for item in per_file),
            "any_1422D8550_hit": any(item["was_1422D8550_hit"] for item in per_file),
            "files_with_1422D8550_without_later_row_writer": [
                item["path"] for item in per_file if item["hit_1422D8550_without_later_row_writer"]
            ],
        },
    }


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "jsonl",
        nargs="+",
        type=Path,
        help="One or more native_render_sink_sanity_*.jsonl files.",
    )
    parser.add_argument(
        "--out",
        type=Path,
        default=DEFAULT_OUT,
        help=f"Summary JSON path. Default: {DEFAULT_OUT}",
    )
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    paths = [path if path.is_absolute() else ROOT / path for path in args.jsonl]
    missing = [str(path) for path in paths if not path.exists()]
    if missing:
        raise FileNotFoundError("Missing JSONL input(s): " + ", ".join(missing))
    summary = build_summary(paths)
    out_path = args.out if args.out.is_absolute() else ROOT / args.out
    out_path.parent.mkdir(parents=True, exist_ok=True)
    out_path.write_text(json.dumps(summary, indent=2), encoding="utf-8")
    print(json.dumps({"wrote": str(out_path), "inputs": [str(path) for path in paths]}, indent=2))


if __name__ == "__main__":
    main()
