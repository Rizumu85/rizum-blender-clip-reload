#!/usr/bin/env python3
"""Summarize native metadata initializer dispatch traces.

The trace is deliberately read-only and conservative: it hooks confirmed
registration stubs plus the shared metadata wrapper. This summarizer keeps the
same posture and reports what was observed without inferring renderer behavior.
"""

from __future__ import annotations

import argparse
import json
from collections import Counter, defaultdict
from pathlib import Path
from typing import Any


REPO = Path(__file__).resolve().parents[1]
OUT_PATH = REPO / "tmp_vector_probe" / "native_metadata_initializer_dispatch_summary_v1.json"
TARGETS = ("VectorObjectList", "VectorData", "TimeLapseBlob", "ExternalChunk")


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
                rows.append(
                    {
                        "event": "json_decode_error",
                        "file": str(path),
                        "line": line_no,
                        "error": str(exc),
                        "raw_prefix": line[:240],
                    }
                )
                continue
            row.setdefault("_file", str(path))
            rows.append(row)
    return rows


def compact_event(row: dict[str, Any]) -> dict[str, Any]:
    keep = [
        "_file",
        "event",
        "timestamp_ms",
        "event_index",
        "process_id",
        "thread_id",
        "stub_name",
        "function_rva",
        "caller_rva",
        "dispatch_invocation_index",
        "table_entry_rva",
        "table_entry_value",
        "target_function_rva",
        "rcx",
        "rdx",
        "descriptor_global_pointer_from_rcx",
        "target_hits",
        "return_value",
    ]
    return {k: row.get(k) for k in keep if k in row}


def merge_counter_from_summary(rows: list[dict[str, Any]], key: str) -> Counter[str]:
    counter: Counter[str] = Counter()
    for row in rows:
        if row.get("event") != "summary":
            continue
        value = row.get(key)
        if isinstance(value, dict):
            for item, count in value.items():
                try:
                    counter[str(item)] += int(count)
                except (TypeError, ValueError):
                    counter[str(item)] += 1
    return counter


def first_matching(rows: list[dict[str, Any]], predicate) -> dict[str, Any] | None:
    for row in rows:
        if predicate(row):
            return compact_event(row)
    return None


def summarize_file(path: Path) -> dict[str, Any]:
    rows = load_jsonl(path)
    events = Counter(str(row.get("event", "unknown")) for row in rows)
    stub_entries = [row for row in rows if row.get("event") == "registration_stub_entry"]
    wrapper_targets = [row for row in rows if row.get("event") == "metadata_wrapper_target_string"]
    summaries = [row for row in rows if row.get("event") == "summary"]
    ready = next((row for row in rows if row.get("event") == "ready"), None)

    stub_counts = Counter(str(row.get("stub_name", "unknown")) for row in stub_entries)
    target_string_counts = Counter()
    target_wrapper_callers: dict[str, Counter[str]] = defaultdict(Counter)
    descriptor_pointers: dict[str, set[str]] = defaultdict(set)
    first_target_events: dict[str, dict[str, Any] | None] = {}

    for row in wrapper_targets:
        caller = str(row.get("caller_rva", "unknown"))
        desc = row.get("descriptor_global_pointer_from_rcx")
        for hit in row.get("target_hits") or []:
            hit = str(hit)
            target_string_counts[hit] += 1
            target_wrapper_callers[hit][caller] += 1
            if desc:
                descriptor_pointers[hit].add(str(desc))

    # Periodic summaries can include counts even when bounded target events were
    # not written near the end of the run.
    target_string_counts.update(merge_counter_from_summary(rows, "target_string_counts"))
    merged_called_stubs = merge_counter_from_summary(rows, "called_stubs")
    for name, count in merged_called_stubs.items():
        stub_counts[name] = max(stub_counts[name], count)

    table_entries = {}
    for row in [ready, *summaries]:
        if not isinstance(row, dict):
            continue
        entries = row.get("table_entries")
        if isinstance(entries, dict):
            table_entries.update(entries)

    for target in TARGETS:
        first_target_events[target] = first_matching(
            wrapper_targets,
            lambda r, target=target: target in (r.get("target_hits") or []),
        )

    first_stub_events = {
        target: first_matching(stub_entries, lambda r, target=target: r.get("stub_name") == target)
        for target in ("VectorObjectList", "VectorData", "TimeLapseBlob")
    }

    wrapper_callers = Counter(str(row.get("caller_rva", "unknown")) for row in wrapper_targets)
    wrapper_callers.update(merge_counter_from_summary(rows, "wrapper_callers"))

    vector_object_called = stub_counts["VectorObjectList"] > 0
    vector_data_called = stub_counts["VectorData"] > 0
    time_lapse_called = stub_counts["TimeLapseBlob"] > 0
    table_has_vector = "VectorObjectList" in table_entries and "VectorData" in table_entries

    if vector_object_called and vector_data_called:
      classification = "A"
      classification_reason = "VectorObjectList and VectorData registration stubs were called."
    elif table_has_vector and time_lapse_called and not (vector_object_called or vector_data_called):
      classification = "D"
      classification_reason = "TimeLapseBlob stub was called while VectorObjectList/VectorData table entries were present but not called."
    elif table_has_vector and not (vector_object_called or vector_data_called):
      classification = "C"
      classification_reason = "VectorObjectList/VectorData entries are present in the audited table but their stubs were not observed."
    elif not events.get("registration_stub_entry") and not events.get("metadata_wrapper_target_string"):
      classification = "E"
      classification_reason = "No dispatcher/stub/target-string evidence was observed by this trace."
    else:
      classification = "F"
      classification_reason = "Static stubs exist, but target strings were absent or incomplete in runtime evidence."

    return {
        "file": str(path),
        "row_count": len(rows),
        "ready": compact_event(ready) if isinstance(ready, dict) else None,
        "event_counts": dict(events),
        "dispatcher_was_hit": False,
        "dispatcher_note": "No concrete dispatcher was statically identified; this trace hooks confirmed stubs and wrapper only.",
        "stub_counts": dict(stub_counts),
        "VectorObjectList_stub_called": vector_object_called,
        "VectorData_stub_called": vector_data_called,
        "TimeLapseBlob_stub_called": time_lapse_called,
        "target_string_counts": dict(target_string_counts),
        "wrapper_callers_top20": wrapper_callers.most_common(20),
        "target_wrapper_callers_top20": {
            target: target_wrapper_callers[target].most_common(20) for target in TARGETS
        },
        "descriptor_global_pointers": {
            target: sorted(values) for target, values in descriptor_pointers.items()
        },
        "table_entries_observed": table_entries,
        "called_vs_skipped_entries": {
            "called": dict(stub_counts),
            "table_present_but_not_called": [
                name
                for name in ("VectorObjectList", "VectorData", "TimeLapseBlob")
                if name in table_entries and stub_counts[name] == 0
            ],
        },
        "first_stub_events": first_stub_events,
        "first_target_string_events": first_target_events,
        "VectorObjectList_VectorData_appeared_only_in_table_but_skipped": (
            table_has_vector and not vector_object_called and not vector_data_called
        ),
        "TimeLapseBlob_called_from_same_dispatcher_or_table": (
            "unknown: no concrete dispatcher/table loop identified"
            if time_lapse_called
            else False
        ),
        "classification": classification,
        "classification_reason": classification_reason,
    }


def combine(per_file: list[dict[str, Any]]) -> dict[str, Any]:
    combined_stub_counts: Counter[str] = Counter()
    combined_target_counts: Counter[str] = Counter()
    files_by_classification: dict[str, list[str]] = defaultdict(list)
    for item in per_file:
        combined_stub_counts.update(item.get("stub_counts", {}))
        combined_target_counts.update(item.get("target_string_counts", {}))
        files_by_classification[str(item.get("classification"))].append(str(item.get("file")))

    if combined_stub_counts["VectorObjectList"] and combined_stub_counts["VectorData"]:
        next_target = "descriptor pointers produced by VectorObjectList/VectorData stubs and their consumer"
    elif combined_stub_counts["TimeLapseBlob"] and not (
        combined_stub_counts["VectorObjectList"] or combined_stub_counts["VectorData"]
    ):
        next_target = "compare TimeLapseBlob table group/gate against the VectorObjectList/VectorData table entries"
    elif not combined_stub_counts:
        next_target = "actual stub entries directly, or re-audit whether 0x432A448/0x4317078 are initializer entries"
    else:
        next_target = "module/feature initializer that registers the vector metadata group"

    return {
        "output_path": str(OUT_PATH),
        "inputs": [item["file"] for item in per_file],
        "files_by_classification": dict(files_by_classification),
        "combined_stub_counts": dict(combined_stub_counts),
        "combined_target_string_counts": dict(combined_target_counts),
        "next_single_hook_target": next_target,
        "per_file": per_file,
    }


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("jsonl", nargs="+", type=Path)
    parser.add_argument("--output", type=Path, default=OUT_PATH)
    args = parser.parse_args()

    per_file = [summarize_file(path) for path in args.jsonl]
    summary = combine(per_file)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(summary, indent=2, ensure_ascii=False), encoding="utf-8")
    print(json.dumps({
        "output": str(args.output),
        "inputs": len(per_file),
        "combined_stub_counts": summary["combined_stub_counts"],
        "combined_target_string_counts": summary["combined_target_string_counts"],
        "next_single_hook_target": summary["next_single_hook_target"],
    }, indent=2))


if __name__ == "__main__":
    main()
