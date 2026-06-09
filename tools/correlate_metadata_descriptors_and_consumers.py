#!/usr/bin/env python3
"""Correlate metadata descriptor captures with static consumer evidence."""

from __future__ import annotations

import argparse
import json
from collections import Counter, defaultdict
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / "tmp_vector_probe" / "native_metadata_descriptor_consumer_correlation_v1.json"
STATIC_AUDIT = ROOT / "tmp_vector_probe" / "metadata_descriptor_static_audit_v1.json"
CONSUMER_AUDIT = ROOT / "tmp_vector_probe" / "vectorobjectlist_descriptor_consumer_static_audit_v1.json"
TARGET_VECTOR_ID = "extrnlid62D15CB4395245648869B4AEBAD8FBCE"


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
        "stub_name",
        "caller_rva",
        "wrapper_call_index",
        "rcx_descriptor_pointer",
        "rcx_descriptor_rva",
        "rdx_string_pointer",
        "rdx_string_rva",
        "string_ascii_preview",
        "target_hits",
        "return_value",
    ]
    return {key: row.get(key) for key in keep if key in row}


def extract_ground_truth(ground_truth: dict[str, Any]) -> dict[str, Any]:
    all_ids = []
    target_payload = None
    for row in ground_truth.get("all_exta_payloads_summary") or []:
        extid = row.get("external_id")
        if extid:
            all_ids.append(extid)
        if extid == TARGET_VECTOR_ID:
            target_payload = row
    vector_rows = ground_truth.get("vector_object_list_rows") or ground_truth.get("vectorobjectlist_rows") or []
    target_row = None
    for row in vector_rows:
        text = json.dumps(row)
        if TARGET_VECTOR_ID in text:
            target_row = row
            break
    return {
        "target_vector_id": TARGET_VECTOR_ID,
        "target_payload": target_payload,
        "target_vectorobjectlist_row": target_row,
        "known_external_ids_count": len(all_ids),
    }


def summarize_descriptor_trace(rows: list[dict[str, Any]]) -> dict[str, Any]:
    descriptors = {}
    stub_calls = Counter()
    target_string_counts = Counter()
    first_wrapper_by_string: dict[str, dict[str, Any]] = {}
    wrapper_events = [row for row in rows if row.get("event") == "wrapper_relevant_leave"]
    summary_rows = [row for row in rows if row.get("event") == "summary"]

    for row in rows:
        if row.get("event") == "ready" and isinstance(row.get("descriptors"), dict):
            descriptors.update(row["descriptors"])
        if row.get("event") == "stub_entry":
            stub_calls[str(row.get("stub_name"))] += 1
        if row.get("event") == "wrapper_relevant_leave":
            for hit in row.get("target_hits") or []:
                target_string_counts[str(hit)] += 1
                first_wrapper_by_string.setdefault(str(hit), compact(row))

    # Periodic summary counters are cumulative; keep max instead of summing.
    for row in summary_rows:
        for key, value in (row.get("stub_calls") or {}).items():
            stub_calls[str(key)] = max(stub_calls[str(key)], int(value))
        for key, value in (row.get("target_string_counts") or {}).items():
            target_string_counts[str(key)] = max(target_string_counts[str(key)], int(value))

    descriptor_field_diffs = {}
    for row in wrapper_events:
        for hit in row.get("target_hits") or []:
            if hit in ("VectorData", "VectorObjectList", "TimeLapseBlob"):
                descriptor_field_diffs.setdefault(hit, row.get("descriptor_field_diffs"))

    return {
        "row_count": len(rows),
        "descriptors": descriptors,
        "stub_calls": dict(stub_calls),
        "target_string_counts": dict(target_string_counts),
        "first_wrapper_by_string": first_wrapper_by_string,
        "descriptor_field_diffs": descriptor_field_diffs,
        "target_vector_id_observed_in_descriptor_trace": any(TARGET_VECTOR_ID in json.dumps(row) for row in rows),
    }


def summarize_consumer_trace(rows: list[dict[str, Any]]) -> dict[str, Any]:
    if not rows:
        return {"present": False}
    events = Counter(str(row.get("event", "unknown")) for row in rows)
    target_rows = [row for row in rows if TARGET_VECTOR_ID in json.dumps(row)]
    return {
        "present": True,
        "row_count": len(rows),
        "event_counts": dict(events),
        "target_vector_id_observed": bool(target_rows),
        "first_target_vector_id_event": compact(target_rows[0]) if target_rows else None,
    }


def rank_candidates(consumer_audit: dict[str, Any]) -> list[dict[str, Any]]:
    candidates = []
    for row in consumer_audit.get("candidate_consumers_ranked") or []:
        uses = row.get("uses") or []
        non_registration = [
            use for use in uses
            if use.get("descriptor") in ("VectorObjectList", "VectorData")
            and not str(use.get("xref_rva", "")).lower() in ("0xcdf6b", "0x13983b")
        ]
        if non_registration or row.get("mentions_generic_readers"):
            candidates.append(row)
    return candidates[:20]


def classify(trace_summary: dict[str, Any], consumer_trace: dict[str, Any], consumer_audit: dict[str, Any]) -> tuple[str, str, str]:
    descs = trace_summary.get("descriptors") or {}
    has_descs = "VectorObjectList" in descs and "VectorData" in descs
    non_reg = consumer_audit.get("non_registration_xrefs_by_descriptor") or {}
    vector_non_reg = (non_reg.get("VectorObjectList") or []) + (non_reg.get("VectorData") or [])

    if has_descs and consumer_trace.get("target_vector_id_observed"):
        return "A", "Descriptor pointers captured and target VectorData id observed in consumer trace.", "consumer post-read storage/load path"
    if has_descs and consumer_trace.get("present") and not consumer_trace.get("target_vector_id_observed"):
        return "B", "Descriptor pointers captured and consumer trace ran, but target VectorData id was not observed.", "non-saving vector-layer/object UI action with consumer trace"
    if has_descs and not vector_non_reg:
        return "C", "Descriptor pointers captured, but static audit found only registration xrefs.", "registry insertion/lookup around 0x142049220"
    if not has_descs:
        return "E", "Descriptor capture did not recover VectorObjectList/VectorData descriptors despite static stubs.", "0x142049220 contract and stub snapshots"
    return "D", "Descriptor pointer ownership is not stable enough from current evidence.", "registry owner or descriptor factory output"


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("ground_truth", type=Path)
    parser.add_argument("descriptor_trace", type=Path)
    parser.add_argument("consumer_trace", nargs="?", type=Path)
    parser.add_argument("--output", type=Path, default=OUT)
    args = parser.parse_args()

    ground_truth = extract_ground_truth(load_json(args.ground_truth))
    static_audit = load_json(STATIC_AUDIT)
    consumer_audit = load_json(CONSUMER_AUDIT)
    descriptor_rows = load_jsonl(args.descriptor_trace)
    consumer_rows = load_jsonl(args.consumer_trace)
    descriptor_summary = summarize_descriptor_trace(descriptor_rows)
    consumer_summary = summarize_consumer_trace(consumer_rows)
    classification, reason, next_target = classify(descriptor_summary, consumer_summary, consumer_audit)

    stubs = static_audit.get("stubs") or {}
    output = {
        "output_path": str(args.output),
        "inputs": {
            "ground_truth": str(args.ground_truth),
            "descriptor_trace": str(args.descriptor_trace),
            "consumer_trace": str(args.consumer_trace) if args.consumer_trace else None,
            "static_audit": str(STATIC_AUDIT) if STATIC_AUDIT.exists() else None,
            "consumer_static_audit": str(CONSUMER_AUDIT) if CONSUMER_AUDIT.exists() else None,
        },
        "ground_truth": ground_truth,
        "VectorObjectList_descriptor_pointer_rva": {
            "static_rva": stubs.get("VectorObjectList", {}).get("descriptor_global_candidate_rva"),
            "runtime": descriptor_summary.get("descriptors", {}).get("VectorObjectList"),
        },
        "VectorData_descriptor_pointer_rva": {
            "static_rva": stubs.get("VectorData", {}).get("descriptor_global_candidate_rva"),
            "runtime": descriptor_summary.get("descriptors", {}).get("VectorData"),
        },
        "TimeLapseBlob_descriptor_pointer_rva": {
            "static_rva": stubs.get("TimeLapseBlob", {}).get("descriptor_global_candidate_rva"),
            "runtime": descriptor_summary.get("descriptors", {}).get("TimeLapseBlob"),
        },
        "descriptors_are_module_globals_or_heap": "module globals",
        "descriptor_trace_summary": descriptor_summary,
        "non_registration_xrefs_to_each_descriptor": consumer_audit.get("non_registration_xrefs_by_descriptor"),
        "candidate_consumers_ranked_by_evidence": rank_candidates(consumer_audit),
        "consumer_trace_summary": consumer_summary,
        "target_VectorData_id_observed_natively": (
            descriptor_summary.get("target_vector_id_observed_in_descriptor_trace")
            or consumer_summary.get("target_vector_id_observed", False)
        ),
        "row_MainId_LayerId_if_recoverable": None,
        "target_id_state": "A" if consumer_summary.get("target_vector_id_observed") else "D",
        "classification": classification,
        "classification_reason": reason,
        "next_single_consumer_hook_target": next_target,
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(output, indent=2, ensure_ascii=False), encoding="utf-8")
    print(json.dumps({
        "output": str(args.output),
        "classification": classification,
        "reason": reason,
        "next_single_consumer_hook_target": next_target,
    }, indent=2))


if __name__ == "__main__":
    main()
