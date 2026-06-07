"""Summarize CSP chunk-stream reader traces."""

from __future__ import annotations

import json
import sys
from collections import Counter, defaultdict
from pathlib import Path
from typing import Any


OUT_PATH = Path("tmp_vector_probe/native_clip_chunk_reader_summary_v1.json")


def load_rows(path: Path) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    for line_no, line in enumerate(path.read_text(encoding="utf-8", errors="replace").splitlines(), 1):
        if not line.strip():
            continue
        try:
            row = json.loads(line)
        except json.JSONDecodeError:
            row = {"event": "decode_error", "raw": line[:200]}
        row["_line_no"] = line_no
        rows.append(row)
    return rows


def compact(row: dict[str, Any]) -> dict[str, Any]:
    return {
        "chunk_read_index": row.get("chunk_read_index"),
        "caller_rva": row.get("caller_rva"),
        "route_label": row.get("route_label"),
        "requested_size": row.get("requested_size"),
        "return_value": row.get("return_value"),
        "signature": row.get("signature"),
        "buffer_prefix_ascii": row.get("buffer_prefix_ascii"),
        "buffer_prefix_hex": row.get("buffer_prefix_hex"),
    }


def summarize(path: Path) -> dict[str, Any]:
    rows = load_rows(path)
    reads = [r for r in rows if r.get("event") == "chunk_read"]
    caller_hist = Counter(str(r.get("caller_rva")) for r in reads)
    sig_hist = Counter(str(r.get("signature")) for r in reads)
    size_hist = Counter(str(r.get("requested_size")) for r in reads)

    by_signature: dict[str, list[dict[str, Any]]] = defaultdict(list)
    by_caller: dict[str, list[dict[str, Any]]] = defaultdict(list)
    for row in reads:
        sig = str(row.get("signature"))
        caller = str(row.get("caller_rva"))
        if len(by_signature[sig]) < 12:
            by_signature[sig].append(compact(row))
        if len(by_caller[caller]) < 12:
            by_caller[caller].append(compact(row))

    backtrace_samples = []
    for row in reads:
        if row.get("backtrace"):
            backtrace_samples.append({
                "chunk_read_index": row.get("chunk_read_index"),
                "caller_rva": row.get("caller_rva"),
                "requested_size": row.get("requested_size"),
                "signature": row.get("signature"),
                "backtrace": row.get("backtrace"),
            })
        if len(backtrace_samples) >= 12:
            break

    return {
        "path": str(path),
        "row_count": len(rows),
        "chunk_read_count": len(reads),
        "caller_rva_histogram": dict(caller_hist.most_common(40)),
        "signature_histogram": dict(sig_hist.most_common(40)),
        "requested_size_histogram": dict(size_hist.most_common(40)),
        "first_60_reads": [compact(r) for r in reads[:60]],
        "samples_by_signature": by_signature,
        "samples_by_caller": by_caller,
        "backtrace_samples": backtrace_samples,
        "summary_events": [r for r in rows if r.get("event") == "summary"][-3:],
    }


def main(argv: list[str]) -> int:
    if len(argv) < 2:
        print("usage: summarize_native_clip_chunk_reader.py TRACE.jsonl [TRACE2.jsonl ...]", file=sys.stderr)
        return 2
    result = {
        "input_files": [str(Path(arg)) for arg in argv[1:]],
        "per_file": [summarize(Path(arg)) for arg in argv[1:]],
    }
    OUT_PATH.parent.mkdir(parents=True, exist_ok=True)
    OUT_PATH.write_text(json.dumps(result, indent=2, sort_keys=True), encoding="utf-8")
    print(OUT_PATH)
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
