#!/usr/bin/env python3
"""Summarize native file/image/cache route-discovery JSONL traces."""

from __future__ import annotations

import json
import sys
from collections import Counter, defaultdict
from pathlib import Path
from typing import Any


REPO_ROOT = Path(__file__).resolve().parents[1]
OUT_PATH = REPO_ROOT / "tmp_vector_probe" / "native_route_discovery_summary_v1.json"
TARGET = "vector_sizepressure"
UI_NOISE_CALLERS = {"0x32c895c", "0x1a7c19d", "0x1965d1a", "0x32d1d2a", "0x32e9286"}


def load_jsonl(path: Path) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    with path.open("r", encoding="utf-8", errors="replace") as fh:
        for line_no, line in enumerate(fh, 1):
            text = line.strip()
            if not text:
                continue
            try:
                row = json.loads(text)
            except json.JSONDecodeError:
                rows.append({"event": "parse_error", "line_no": line_no, "raw": text[:240]})
                continue
            if isinstance(row, dict):
                row["_line_no"] = line_no
                rows.append(row)
    return rows


def lower(value: Any) -> str:
    return str(value or "").lower()


def path_is_target(path: Any) -> bool:
    text = lower(path)
    return TARGET in text or text.endswith(".clip")


def path_is_temp_or_cache(path: Any) -> bool:
    text = lower(path)
    return any(token in text for token in ("\\temp", "\\tmp", "cache", "preview", "offscreen")) or text.endswith(
        (".tmp", ".db", ".sqlite", ".sqlite3", ".png")
    )


def compact(row: dict[str, Any]) -> dict[str, Any]:
    keys = (
        "event",
        "function",
        "module",
        "path",
        "handle",
        "size",
        "width",
        "height",
        "format",
        "caller_rva",
        "return_value",
        "timestamp_ms",
        "_line_no",
    )
    return {key: row.get(key) for key in keys if key in row}


def summarize_file(path: Path) -> dict[str, Any]:
    rows = load_jsonl(path)
    function_counts = Counter(row.get("function") for row in rows if row.get("function"))
    caller_counts = Counter(row.get("caller_rva") for row in rows if row.get("caller_rva"))
    api_rows = [row for row in rows if row.get("event") == "api"]
    target_io = [row for row in api_rows if path_is_target(row.get("path"))]
    cache_io = [row for row in api_rows if path_is_temp_or_cache(row.get("path"))]
    map_rows = [row for row in api_rows if row.get("function") in {"CreateFileMappingW", "MapViewOfFile"}]
    image_rows = [
        row
        for row in api_rows
        if row.get("function")
        in {
            "BitBlt",
            "StretchBlt",
            "AlphaBlend",
            "CreateDIBSection",
            "CreateCompatibleBitmap",
            "SetDIBits",
            "GetDIBits",
            "CoCreateInstance",
            "D2D1CreateFactory",
            "D3D11CreateDevice",
            "Direct3DCreate9",
            "WICCreateImagingFactory_Proxy",
            "WICConvertBitmapSource",
            "VirtualAlloc",
            "HeapAlloc",
            "RtlAllocateHeap",
            "memcpy",
            "memmove",
        }
    ]
    large_allocs = [
        row
        for row in image_rows
        if isinstance(row.get("size"), int) and row.get("size", 0) >= 512 * 1024
    ]
    non_ui_callers = Counter(
        caller for caller in caller_counts if caller and lower(caller) not in UI_NOISE_CALLERS
    )
    first_target_ts = min((row.get("timestamp_ms") for row in target_io if row.get("timestamp_ms")), default=None)
    first_image_ts = min((row.get("timestamp_ms") for row in image_rows if row.get("timestamp_ms")), default=None)
    near_visible_events = []
    if first_image_ts is not None:
        near_visible_events = [
            compact(row)
            for row in api_rows
            if isinstance(row.get("timestamp_ms"), int) and abs(row["timestamp_ms"] - first_image_ts) <= 2000
        ][:80]
    return {
        "path": str(path),
        "row_count": len(rows),
        "function_counts": dict(function_counts),
        "caller_rva_histogram": dict(caller_counts.most_common(80)),
        "target_clip_io_events": [compact(row) for row in target_io[:120]],
        "temp_or_cache_paths": sorted({str(row.get("path")) for row in cache_io if row.get("path")})[:200],
        "memory_map_events": [compact(row) for row in map_rows[:120]],
        "image_cache_events": [compact(row) for row in image_rows[:200]],
        "large_alloc_or_copy_events": [compact(row) for row in large_allocs[:120]],
        "gdi_d2d_d3d_wic_hits": {
            key: function_counts.get(key, 0)
            for key in (
                "BitBlt",
                "StretchBlt",
                "AlphaBlend",
                "CreateDIBSection",
                "CreateCompatibleBitmap",
                "SetDIBits",
                "GetDIBits",
                "CoCreateInstance",
                "D2D1CreateFactory",
                "D3D11CreateDevice",
                "Direct3DCreate9",
                "WICCreateImagingFactory_Proxy",
                "WICConvertBitmapSource",
            )
        },
        "first_target_io_timestamp_ms": first_target_ts,
        "first_image_or_cache_timestamp_ms": first_image_ts,
        "events_near_first_image_or_cache_event": near_visible_events,
        "top_20_non_ui_caller_rvas": dict(non_ui_callers.most_common(20)),
    }


def classify(per_file: list[dict[str, Any]]) -> str:
    any_clip_io = any(item["target_clip_io_events"] for item in per_file)
    any_maps = any(item["memory_map_events"] for item in per_file)
    any_cache = any(item["temp_or_cache_paths"] for item in per_file)
    any_image = any(item["image_cache_events"] for item in per_file)
    if any_clip_io and not any_image:
        return "A_or_D: clip data is read/mapped but no broad image/display producer was observed in these traces"
    if any_cache:
        return "B: temp/cache/offscreen paths were touched"
    if not any_clip_io:
        return "C: no target .clip I/O observed in supplied traces"
    if any_image:
        return "D: broad image/display path is active; inspect source bitmap/offscreen producer callers"
    if any_maps:
        return "A: clip mapping observed; next target is preview/cache blob parse route"
    return "unknown"


def main(argv: list[str]) -> int:
    if len(argv) < 2:
        print("usage: python tools\\summarize_native_route_discovery.py <trace1.jsonl> [trace2.jsonl ...]", file=sys.stderr)
        return 2
    per_file = [summarize_file(Path(arg)) for arg in argv[1:]]
    caller_totals: Counter[str] = Counter()
    function_totals: Counter[str] = Counter()
    for item in per_file:
        caller_totals.update(item["caller_rva_histogram"])
        function_totals.update(item["function_counts"])
    summary = {
        "input_files": [str(Path(arg)) for arg in argv[1:]],
        "per_file": per_file,
        "combined_function_counts": dict(function_totals.most_common(100)),
        "combined_caller_rva_histogram": dict(caller_totals.most_common(100)),
        "top_20_non_ui_caller_rvas": dict(
            Counter({k: v for k, v in caller_totals.items() if lower(k) not in UI_NOISE_CALLERS}).most_common(20)
        ),
        "classification": classify(per_file),
    }
    OUT_PATH.parent.mkdir(parents=True, exist_ok=True)
    OUT_PATH.write_text(json.dumps(summary, indent=2, sort_keys=True), encoding="utf-8")
    print(json.dumps(summary, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
