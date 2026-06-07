"""Summarize CSP target .clip file read-content route traces."""

from __future__ import annotations

import json
import sys
from collections import Counter, defaultdict
from pathlib import Path
from typing import Any


OUT_PATH = Path("tmp_vector_probe/native_clip_file_read_content_summary_v1.json")


def load_rows(path: Path) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    for line_no, line in enumerate(path.read_text(encoding="utf-8", errors="replace").splitlines(), 1):
        if not line.strip():
            continue
        try:
            row = json.loads(line)
        except json.JSONDecodeError:
            rows.append({"event": "decode_error", "line_no": line_no, "raw": line[:200]})
            continue
        row["_line_no"] = line_no
        rows.append(row)
    return rows


def compact_read(row: dict[str, Any]) -> dict[str, Any]:
    return {
        "target_read_index": row.get("target_read_index"),
        "offset_before": row.get("offset_before"),
        "requested_size": row.get("requested_size"),
        "bytes_read": row.get("bytes_read"),
        "tracked_offset_after": row.get("tracked_offset_after"),
        "caller_rva": row.get("caller_rva"),
        "caller": row.get("caller"),
        "magic4": row.get("magic4"),
        "probable_type": row.get("probable_type"),
        "buffer_prefix_ascii": row.get("buffer_prefix_ascii"),
        "buffer_prefix_hex": row.get("buffer_prefix_hex"),
    }


def summarize_file(path: Path) -> dict[str, Any]:
    rows = load_rows(path)
    reads = [r for r in rows if r.get("event") == "api" and r.get("function") == "ReadFile"]
    creates = [r for r in rows if r.get("event") == "api" and r.get("function") == "CreateFileW"]
    closes = [r for r in rows if r.get("event") == "api" and r.get("function") == "CloseHandle"]
    seeks = [r for r in rows if r.get("event") == "api" and r.get("function") == "SetFilePointerEx"]

    size_hist = Counter(str(r.get("bytes_read")) for r in reads)
    request_hist = Counter(str(r.get("requested_size")) for r in reads)
    magic_hist = Counter(str(r.get("probable_type")) for r in reads)
    caller_hist = Counter(str(r.get("caller_rva")) for r in reads)

    reads_by_size: dict[str, list[dict[str, Any]]] = defaultdict(list)
    for row in reads:
        key = str(row.get("bytes_read"))
        if len(reads_by_size[key]) < 8:
            reads_by_size[key].append(compact_read(row))

    interesting_reads = []
    for row in reads:
        size = row.get("bytes_read") or 0
        if row.get("target_read_index", 999999) < 20 or size not in (0, 8, 65536):
            interesting_reads.append(compact_read(row))
        if len(interesting_reads) >= 80:
            break

    backtrace_samples = []
    for row in reads:
        if row.get("backtrace"):
            backtrace_samples.append({
                "target_read_index": row.get("target_read_index"),
                "caller_rva": row.get("caller_rva"),
                "bytes_read": row.get("bytes_read"),
                "backtrace": row.get("backtrace"),
            })
        if len(backtrace_samples) >= 10:
            break

    return {
        "path": str(path),
        "row_count": len(rows),
        "target_path": creates[0].get("path") if creates else None,
        "create_count": len(creates),
        "read_count": len(reads),
        "seek_count": len(seeks),
        "close_count": len(closes),
        "total_bytes_read": sum((r.get("bytes_read") or 0) for r in reads),
        "bytes_read_histogram": dict(size_hist.most_common()),
        "requested_size_histogram": dict(request_hist.most_common()),
        "probable_type_histogram": dict(magic_hist.most_common()),
        "read_caller_rva_histogram": dict(caller_hist.most_common(20)),
        "first_30_reads": [compact_read(r) for r in reads[:30]],
        "sample_reads_by_size": reads_by_size,
        "interesting_reads": interesting_reads,
        "seek_events": seeks[:20],
        "backtrace_samples": backtrace_samples,
        "summary_events": [r for r in rows if r.get("event") == "summary"][-3:],
    }


def main(argv: list[str]) -> int:
    if len(argv) < 2:
        print("usage: summarize_native_clip_file_read_content.py TRACE.jsonl [TRACE2.jsonl ...]", file=sys.stderr)
        return 2

    summaries = [summarize_file(Path(arg)) for arg in argv[1:]]
    combined = {
        "input_files": [str(Path(arg)) for arg in argv[1:]],
        "per_file": summaries,
    }
    OUT_PATH.parent.mkdir(parents=True, exist_ok=True)
    OUT_PATH.write_text(json.dumps(combined, indent=2, sort_keys=True), encoding="utf-8")
    print(OUT_PATH)
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
