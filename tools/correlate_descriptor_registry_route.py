#!/usr/bin/env python3
"""Correlate descriptor registry-route static audits and runtime traces."""

from __future__ import annotations

import argparse
import json
from collections import Counter
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / "tmp_vector_probe" / "native_descriptor_registry_route_correlation_v1.json"
STATIC_AUDIT = ROOT / "tmp_vector_probe" / "descriptor_registry_2049220_static_audit_v1.json"
LOOKUP_AUDIT = ROOT / "tmp_vector_probe" / "descriptor_registry_lookup_static_audit_v1.json"
TARGET_VECTOR_ID = "extrnlid62D15CB4395245648869B4AEBAD8FBCE"
PREVIEW_CACHE_ID = "extrnlid5943B673F7C84B779ED2D7C96E942EAE"


def load_json(path: Path) -> dict[str, Any]:
    if not path.exists():
        return {}
    return json.loads(path.read_text(encoding="utf-8"))


def load_jsonl(path: Path | None) -> list[dict[str, Any]]:
    if path is None:
        return []
    rows: list[dict[str, Any]] = []
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
        "function_rva",
        "caller_rva",
        "wrapper_call_index",
        "descriptor_ptr",
        "descriptor_static_rva",
        "string_key",
        "target_hits",
        "return_value",
        "return_value_equals_descriptor",
        "helper_name",
        "helper_rva",
    ]
    return {key: row.get(key) for key in keep if key in row}


def ground_truth_summary(path: Path) -> dict[str, Any]:
    data = load_json(path)
    target_payload = None
    preview_payload = None
    for row in data.get("all_exta_payloads_summary") or []:
        if row.get("external_id") == TARGET_VECTOR_ID:
            target_payload = row
        if row.get("external_id") == PREVIEW_CACHE_ID:
            preview_payload = row
    target_row = None
    for key in ("vector_object_list_rows", "vectorobjectlist_rows"):
        for row in data.get(key) or []:
            if TARGET_VECTOR_ID in json.dumps(row):
                target_row = row
                break
    return {
        "target_vector_id": TARGET_VECTOR_ID,
        "target_payload": target_payload,
        "target_vectorobjectlist_row": target_row,
        "preview_cache_id": PREVIEW_CACHE_ID,
        "preview_payload": preview_payload,
    }


def summarize_registration(rows: list[dict[str, Any]]) -> dict[str, Any]:
    events = Counter(str(row.get("event", "unknown")) for row in rows)
    target_events = [row for row in rows if row.get("event") == "wrapper_target_leave"]
    helper_events = [row for row in rows if row.get("event") == "helper_inside_target_wrapper"]
    descriptors = {}
    for row in rows:
        if row.get("event") == "summary" and isinstance(row.get("descriptors"), dict):
            descriptors.update(row["descriptors"])
    by_target: dict[str, list[dict[str, Any]]] = {}
    for row in target_events:
        for hit in row.get("target_hits") or []:
            by_target.setdefault(str(hit), []).append(row)
    exact_target_events = {}
    for name, meta in descriptors.items():
        wanted = (meta or {}).get("descriptor_rva")
        if not wanted:
            continue
        exact = [
            row for row in target_events
            if str(row.get("descriptor_static_rva", "")).lower() == str(wanted).lower()
        ]
        if exact:
            exact_target_events[name] = exact[0]
    helper_counts = Counter(str(row.get("helper_name", "unknown")) for row in helper_events)
    candidate_registry_pointers = []
    for row in target_events + helper_events:
        for key in ("candidate_registry_map_pointer", "candidate_registry_node_pointer", "candidate_registry_pointer"):
            value = row.get(key)
            if value:
                candidate_registry_pointers.append(value)
    return {
        "row_count": len(rows),
        "event_counts": dict(events),
        "descriptors": descriptors,
        "wrapper_target_counts_by_string_scan": {name: len(items) for name, items in by_target.items()},
        "wrapper_target_counts_by_exact_descriptor": {
            name: sum(
                1 for row in target_events
                if str(row.get("descriptor_static_rva", "")).lower() == str((meta or {}).get("descriptor_rva", "")).lower()
            )
            for name, meta in descriptors.items()
        },
        "helper_counts_inside_target_wrappers": dict(helper_counts),
        "first_wrapper_target_event_by_name": {
            name: compact(exact_target_events.get(name) or (items[0] if items else None))
            for name, items in by_target.items()
        },
        "first_wrapper_target_event_by_exact_descriptor": {
            name: compact(row) for name, row in exact_target_events.items()
        },
        "first_helper_event_by_name": {
            name: compact(next((row for row in helper_events if row.get("helper_name") == name), None))
            for name in helper_counts
        },
        "candidate_registry_map_pointers": sorted(set(candidate_registry_pointers)),
        "descriptors_share_registry": False if not candidate_registry_pointers else "unknown",
        "target_vector_id_observed": any(TARGET_VECTOR_ID in json.dumps(row) for row in rows),
        "preview_cache_id_observed": any(PREVIEW_CACHE_ID in json.dumps(row) for row in rows),
    }


def summarize_lookup(rows: list[dict[str, Any]]) -> dict[str, Any]:
    if not rows:
        return {"present": False}
    events = Counter(str(row.get("event", "unknown")) for row in rows)
    target_rows = [row for row in rows if TARGET_VECTOR_ID in json.dumps(row)]
    preview_rows = [row for row in rows if PREVIEW_CACHE_ID in json.dumps(row)]
    return {
        "present": True,
        "row_count": len(rows),
        "event_counts": dict(events),
        "target_vector_id_observed": bool(target_rows),
        "preview_cache_id_observed": bool(preview_rows),
        "first_target_event": compact(target_rows[0]) if target_rows else None,
        "first_preview_event": compact(preview_rows[0]) if preview_rows else None,
    }


def classify(static_audit: dict[str, Any], registration: dict[str, Any], lookup: dict[str, Any]) -> tuple[str, str, str]:
    wrapper = static_audit.get("wrapper_142049220") or {}
    if wrapper.get("classification") == "A":
        return (
            "A",
            "0x142049220 only initializes descriptor fields/string storage; registry insertion was not found inside it.",
            "post-call caller tail path after the VectorObjectList/VectorData registration stubs",
        )
    if registration.get("candidate_registry_map_pointers") and not lookup.get("present"):
        return ("B", "A candidate registry pointer was observed but lookup trace has not run.", "lookup helper during file open")
    if lookup.get("target_vector_id_observed"):
        return ("D", "VectorObjectList/VectorData lookup route observed target VectorData id.", "post-read storage/load path for target external id")
    return ("F", "Static/dynamic evidence has not identified the registry.", "narrow data-entry access diagnostic on descriptor fields modified by 0x142049220")


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("ground_truth", type=Path)
    parser.add_argument("registration_trace", type=Path)
    parser.add_argument("lookup_trace", nargs="?", type=Path)
    parser.add_argument("--output", type=Path, default=OUT)
    args = parser.parse_args()

    static_audit = load_json(STATIC_AUDIT)
    lookup_static = load_json(LOOKUP_AUDIT)
    registration = summarize_registration(load_jsonl(args.registration_trace))
    lookup = summarize_lookup(load_jsonl(args.lookup_trace))
    classification, reason, next_target = classify(static_audit, registration, lookup)
    output = {
        "output_path": str(args.output),
        "inputs": {
            "ground_truth": str(args.ground_truth),
            "registration_trace": str(args.registration_trace),
            "lookup_trace": str(args.lookup_trace) if args.lookup_trace else None,
            "static_audit": str(STATIC_AUDIT),
            "lookup_static_audit": str(LOOKUP_AUDIT),
        },
        "ground_truth": ground_truth_summary(args.ground_truth),
        "VectorObjectList_descriptor_pointer_rva": registration["descriptors"].get("VectorObjectList"),
        "VectorData_descriptor_pointer_rva": registration["descriptors"].get("VectorData"),
        "TimeLapseBlob_descriptor_pointer_rva": registration["descriptors"].get("TimeLapseBlob"),
        "candidate_registry_map_pointers": registration["candidate_registry_map_pointers"],
        "helper_functions_inside_0x142049220": (static_audit.get("wrapper_142049220") or {}).get("helper_calls_inside"),
        "descriptors_share_registry": registration["descriptors_share_registry"],
        "registration_trace_summary": registration,
        "lookup_static_candidates_ranked": lookup_static.get("candidate_lookup_or_consumer_functions_ranked"),
        "lookup_trace_summary": lookup,
        "VectorObjectList_descriptor_looked_up_during_file_open": False if not lookup.get("present") else "not observed",
        "VectorData_descriptor_looked_up_during_file_open": False if not lookup.get("present") else "not observed",
        "target_VectorData_external_id_observed": registration["target_vector_id_observed"] or lookup.get("target_vector_id_observed", False),
        "target_id_state": "D" if not lookup.get("target_vector_id_observed") else "A",
        "classification": classification,
        "classification_reason": reason,
        "next_single_hook_target": next_target,
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(output, indent=2, ensure_ascii=False), encoding="utf-8")
    print(json.dumps({
        "output": str(args.output),
        "classification": classification,
        "reason": reason,
        "next_single_hook_target": next_target,
    }, indent=2))


if __name__ == "__main__":
    main()
