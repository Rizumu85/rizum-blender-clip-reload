#!/usr/bin/env python3
"""Summarize the plot-only fresh-open positive-control trace."""

from __future__ import annotations

import json
import sys
from collections import Counter
from pathlib import Path
from typing import Any


OUT_PATH = Path("tmp_vector_probe/native_plot_only_fresh_open_control_summary_v1.json")
SUSPECT_RANGES = (range(75, 88), range(203, 210))


def load_jsonl(path: Path) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    with path.open("r", encoding="utf-8") as fh:
        for line_no, line in enumerate(fh, 1):
            text = line.strip()
            if not text:
                continue
            try:
                row = json.loads(text)
            except json.JSONDecodeError as exc:
                rows.append(
                    {
                        "event": "parse_error",
                        "line_no": line_no,
                        "error": str(exc),
                        "raw": text[:240],
                    }
                )
                continue
            if isinstance(row, dict):
                row["_line_no"] = line_no
                rows.append(row)
    return rows


def compact_record(row: dict[str, Any]) -> dict[str, Any]:
    keys = (
        "raw_call_index",
        "sizepressure_call_index",
        "thread_id",
        "caller_rva",
        "style_ptr",
        "sample_ptr",
        "styleFlag_0x78_hex",
        "sample_center_x_guess",
        "sample_center_y_guess",
        "plot_ptr",
        "plot_radius",
        "paired_entry_found",
        "_line_no",
    )
    return {key: row.get(key) for key in keys if key in row}


def main(argv: list[str]) -> int:
    if len(argv) != 2:
        print(
            "usage: python tools\\summarize_native_plot_only_control.py "
            "tmp_vector_probe\\native_plot_only_fresh_open_control_<timestamp>_pid<PID>.jsonl",
            file=sys.stderr,
        )
        return 2

    trace_path = Path(argv[1])
    rows = load_jsonl(trace_path)
    plot_entries = [row for row in rows if row.get("event") == "plot_entry"]
    radius_rows = [row for row in rows if row.get("event") == "plot_radius_written"]
    parse_errors = [row for row in rows if row.get("event") == "parse_error"]

    style_hist = Counter(
        str(row.get("styleFlag_0x78_hex") or row.get("styleFlag_0x78"))
        for row in plot_entries
    )

    sizepressure_entries = [
        row
        for row in plot_entries
        if isinstance(row.get("sizepressure_call_index"), int)
    ]
    sizepressure_radius_rows = [
        row
        for row in radius_rows
        if isinstance(row.get("sizepressure_call_index"), int)
    ]

    radius_by_sp_index: dict[int, dict[str, Any]] = {}
    for row in sizepressure_radius_rows:
        radius_by_sp_index[row["sizepressure_call_index"]] = row

    entry_by_sp_index: dict[int, dict[str, Any]] = {}
    for row in sizepressure_entries:
        entry_by_sp_index[row["sizepressure_call_index"]] = row

    first_10 = [
        compact_record({**entry, **radius_by_sp_index.get(entry["sizepressure_call_index"], {})})
        for entry in sizepressure_entries[:10]
    ]

    def records_for(indices: range) -> list[dict[str, Any]]:
        out: list[dict[str, Any]] = []
        for idx in indices:
            entry = entry_by_sp_index.get(idx)
            radius = radius_by_sp_index.get(idx)
            if entry or radius:
                merged: dict[str, Any] = {}
                if entry:
                    merged.update(entry)
                if radius:
                    merged.update(radius)
                out.append(compact_record(merged))
            else:
                out.append({"sizepressure_call_index": idx, "missing": True})
        return out

    paired_radius_indices = set(radius_by_sp_index)
    entry_indices = set(entry_by_sp_index)
    unpaired_entry_indices = sorted(entry_indices - paired_radius_indices)
    unpaired_radius_indices = sorted(paired_radius_indices - entry_indices)
    suspect_indices = set()
    for suspect_range in SUSPECT_RANGES:
        suspect_indices.update(suspect_range)
    suspect_unpaired = sorted(idx for idx in unpaired_entry_indices if idx in suspect_indices)
    suspect_missing = sorted(idx for idx in suspect_indices if idx not in entry_indices)

    positive_control_passed = (
        len(sizepressure_entries) == 213
        and not suspect_unpaired
        and not suspect_missing
        and all(idx in paired_radius_indices for idx in suspect_indices)
    )

    summary = {
        "file_path": str(trace_path),
        "total_raw_plot_entries": len(plot_entries),
        "total_radius_written_records": len(radius_rows),
        "total_sizepressure_records": len(sizepressure_entries),
        "total_sizepressure_radius_records": len(sizepressure_radius_rows),
        "styleFlag_histogram": dict(style_hist),
        "first_10_sizepressure_records": first_10,
        "records_75_87": records_for(range(75, 88)),
        "records_203_209": records_for(range(203, 210)),
        "unpaired_entry_indices": unpaired_entry_indices,
        "unpaired_radius_indices": unpaired_radius_indices,
        "unpaired_entries_in_suspect_ranges": suspect_unpaired,
        "missing_entries_in_suspect_ranges": suspect_missing,
        "parse_errors": parse_errors[:20],
        "positive_control_passed": positive_control_passed,
        "positive_control_criteria": {
            "requires_213_sizepressure_records": len(sizepressure_entries) == 213,
            "requires_no_unpaired_suspect_records": not suspect_unpaired,
            "requires_suspect_ranges_present": not suspect_missing,
            "requires_suspect_radius_records": all(
                idx in paired_radius_indices for idx in suspect_indices
            ),
        },
    }

    OUT_PATH.parent.mkdir(parents=True, exist_ok=True)
    OUT_PATH.write_text(json.dumps(summary, indent=2, sort_keys=True), encoding="utf-8")
    print(json.dumps(summary, indent=2, sort_keys=True))
    return 0 if positive_control_passed else 1


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
