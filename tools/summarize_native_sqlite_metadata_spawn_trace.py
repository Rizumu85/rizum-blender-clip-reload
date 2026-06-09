#!/usr/bin/env python3
"""Summarize CSP process-start SQLite/ORM metadata traces."""

from __future__ import annotations

import argparse
import json
from collections import Counter
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / "tmp_vector_probe" / "native_sqlite_metadata_spawn_summary_v1.json"
TARGETS = ["VectorObjectList", "VectorData", "TimeLapseBlob", "ExternalChunk", "extrnlid"]


def load_jsonl(path: Path) -> list[dict[str, Any]]:
    rows = []
    with path.open("r", encoding="utf-8", errors="replace") as f:
        for line in f:
            line = line.strip()
            if not line:
                continue
            try:
                rows.append(json.loads(line))
            except json.JSONDecodeError:
                rows.append({"event": "json_decode_error", "raw": line[:500]})
    return rows


def last_summary(rows: list[dict[str, Any]]) -> dict[str, Any]:
    summaries = [r for r in rows if r.get("event") == "summary"]
    return summaries[-1] if summaries else {}


def summarize_one(path: Path) -> dict[str, Any]:
    rows = load_jsonl(path)
    summary = last_summary(rows)
    target_events = [r for r in rows if r.get("event") == "target_metadata_event"]
    counts = Counter()
    first = {}
    callers = {t: Counter() for t in TARGETS}
    functions = {t: Counter() for t in TARGETS}
    descriptors = {t: set() for t in TARGETS}

    for row in target_events:
        for hit in row.get("target_hits", []) or []:
            counts[hit] += 1
            callers[hit][str(row.get("caller_rva"))] += 1
            functions[hit][str(row.get("function_rva"))] += 1
            if hit not in first:
                first[hit] = {
                    "event_index": row.get("event_index"),
                    "timestamp_ms": row.get("timestamp_ms"),
                    "function_rva": row.get("function_rva"),
                    "caller_rva": row.get("caller_rva"),
                    "args": row.get("args"),
                    "retval": row.get("retval"),
                    "target_hits": row.get("target_hits"),
                }
            args = row.get("args") or []
            if args:
                descriptors[hit].add(str(args[0]))

    total_hook_hits = {}
    total_hook_hits.update(summary.get("total_calls_per_hook") or {})
    if not total_hook_hits:
        for row in rows:
            name = row.get("name")
            if name:
                total_hook_hits[name] = total_hook_hits.get(name, 0) + 1

    return {
        "path": str(path),
        "ready": next((r for r in rows if r.get("event") == "ready"), None),
        "row_count": len(rows),
        "total_hook_hits": total_hook_hits,
        "target_string_hits": dict(counts),
        "observed": {t: counts[t] > 0 for t in TARGETS},
        "first_event_for_each_target": first,
        "caller_rva_for_each_target": {t: dict(c.most_common(20)) for t, c in callers.items()},
        "function_rva_for_each_target": {t: dict(c.most_common(20)) for t, c in functions.items()},
        "descriptor_object_pointers": {t: sorted(v) for t, v in descriptors.items() if v},
        "wrapper_saw_target_strings_directly": any(
            row.get("name") == "metadata_string_wrapper_142049220" and row.get("target_hits")
            for row in target_events
        ),
        "target_event_count": len(target_events),
        "last_summary": summary,
    }


def classify(items: list[dict[str, Any]]) -> tuple[str, str, str]:
    startup = next((i for i in items if "startup" in Path(i["path"]).name.lower()), items[0] if items else None)
    open_run = next((i for i in items if "open_vector" in Path(i["path"]).name.lower()), None)

    startup_vo = bool(startup and startup["observed"].get("VectorObjectList"))
    startup_vd = bool(startup and startup["observed"].get("VectorData"))
    open_vo = bool(open_run and open_run["observed"].get("VectorObjectList"))
    open_vd = bool(open_run and open_run["observed"].get("VectorData"))
    wrapper_direct = any(i["wrapper_saw_target_strings_directly"] for i in items)

    if (startup_vo or startup_vd) and not (open_vo or open_vd):
        return (
            "A",
            "VectorObjectList / VectorData strings appear during process startup only.",
            "Next target: descriptor consumer / table metadata object usage.",
        )
    if open_vo and open_vd:
        return (
            "B",
            "VectorObjectList / VectorData strings appear during file open.",
            "Next target: file-open row consumer caller that referenced VectorData.",
        )
    if (startup_vo or open_vo) and not (startup_vd or open_vd):
        return (
            "C",
            "VectorObjectList appears but VectorData does not.",
            "Next target: table descriptor construction and column descriptor lookup.",
        )
    if wrapper_direct:
        return (
            "E",
            "0x142049220 sees target strings directly.",
            "Next target: caller function(s) passing those strings.",
        )
    return (
        "D",
        "Neither VectorObjectList nor VectorData appears even with the supplied spawn traces.",
        "Next target: re-audit xrefs and wrapper contract, or verify spawn/phase coverage.",
    )


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("jsonl", type=Path, nargs="+")
    parser.add_argument("--output", type=Path, default=OUT)
    args = parser.parse_args()

    summaries = [summarize_one(path) for path in args.jsonl]
    classification, rationale, next_target = classify(summaries)
    result = {
        "inputs": [str(p) for p in args.jsonl],
        "per_input": summaries,
        "classification": classification,
        "classification_rationale": rationale,
        "next_single_hook_target": next_target,
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(result, indent=2, sort_keys=True), encoding="utf-8")
    print(json.dumps(result, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
