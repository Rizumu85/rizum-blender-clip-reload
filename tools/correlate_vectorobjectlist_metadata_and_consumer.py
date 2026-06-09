#!/usr/bin/env python3
"""Correlate native VectorObjectList metadata/consumer traces with ground truth."""

from __future__ import annotations

import argparse
import json
from collections import Counter, defaultdict
from pathlib import Path
from typing import Any


REPO_ROOT = Path(__file__).resolve().parents[1]
GROUND_TRUTH = REPO_ROOT / "tmp_vector_probe" / "vectorobjectlist_ground_truth_v1.json"
OUT_PATH = REPO_ROOT / "tmp_vector_probe" / "native_vectorobjectlist_metadata_consumer_correlation_v1.json"
TARGET_ID = "extrnlid62D15CB4395245648869B4AEBAD8FBCE"


def load_json(path: Path) -> Any:
    return json.loads(path.read_text(encoding="utf-8"))


def iter_jsonl(path: Path | None) -> list[dict[str, Any]]:
    if path is None:
        return []
    rows: list[dict[str, Any]] = []
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


def find_ground_truth_target(gt: dict[str, Any]) -> dict[str, Any] | None:
    for row in gt.get("vector_object_rows", []):
        if row.get("VectorData") == TARGET_ID:
            return row
    # Older dump shape used enriched_rows.
    for row in gt.get("enriched_rows", []):
        if row.get("VectorData") == TARGET_ID:
            return row
    return None


def relevant_strings(row: dict[str, Any]) -> list[str]:
    out: list[str] = []
    for hit in row.get("relevant_strings", []) or []:
        text = hit.get("text")
        if isinstance(text, str):
            out.append(text)
    for key in ("candidate_text", "text", "value", "vector_data"):
        text = row.get(key)
        if isinstance(text, str):
            out.append(text)
    return out


def summarize_metadata(rows: list[dict[str, Any]]) -> dict[str, Any]:
    observed_by_name: dict[str, list[dict[str, Any]]] = defaultdict(list)
    caller_counts: Counter[str] = Counter()
    descriptor_by_name: dict[str, set[str]] = defaultdict(set)
    wrapper_counts: Counter[str] = Counter()

    for row in rows:
        if row.get("event") != "metadata_wrapper_call":
            continue
        wrapper_counts[str(row.get("wrapper"))] += 1
        caller_counts[str(row.get("caller_rva"))] += 1
        descriptor = row.get("descriptor_ptr")
        for text in relevant_strings(row):
            for name in ("VectorObjectList", "VectorData", "TimeLapseBlob", "ExternalChunk", "extrnlid"):
                if name in text:
                    observed_by_name[name].append(row)
                    if descriptor:
                        descriptor_by_name[name].add(str(descriptor))

    return {
        "metadata_rows": sum(1 for row in rows if row.get("event") == "metadata_wrapper_call"),
        "wrapper_counts": dict(wrapper_counts),
        "caller_rvas": dict(caller_counts.most_common(30)),
        "observed": {name: len(values) for name, values in observed_by_name.items()},
        "descriptor_pointers": {name: sorted(values) for name, values in descriptor_by_name.items()},
        "first_records": {
            name: [
                {
                    "wrapper": row.get("wrapper"),
                    "caller_rva": row.get("caller_rva"),
                    "descriptor_ptr": row.get("descriptor_ptr"),
                    "relevant_strings": relevant_strings(row),
                    "timestamp_ms": row.get("timestamp_ms"),
                }
                for row in values[:5]
            ]
            for name, values in observed_by_name.items()
        },
    }


def summarize_consumer(rows: list[dict[str, Any]]) -> dict[str, Any]:
    target_rows = []
    caller_counts: Counter[str] = Counter()
    row_fields = []
    for row in rows:
        caller = row.get("caller_rva") or row.get("function_rva")
        if caller:
            caller_counts[str(caller)] += 1
        hay = json.dumps(row, sort_keys=True, ensure_ascii=False)
        if TARGET_ID in hay:
            target_rows.append(row)
        fields = {}
        for key in ("MainId", "LayerId", "VectorData", "main_id", "layer_id", "vector_data", "row_object_ptr", "dest_object_ptr"):
            if key in row:
                fields[key] = row[key]
        if fields:
            fields["event"] = row.get("event")
            fields["caller_rva"] = caller
            row_fields.append(fields)
    return {
        "consumer_rows": len(rows),
        "caller_rvas": dict(caller_counts.most_common(30)),
        "target_id_records": len(target_rows),
        "first_target_records": target_rows[:5],
        "row_fields": row_fields[:20],
    }


def classify(metadata: dict[str, Any], consumer: dict[str, Any]) -> tuple[str, str, str]:
    meta_observed = metadata["observed"].get("VectorObjectList", 0) > 0 or metadata["observed"].get("VectorData", 0) > 0
    consumer_observed = consumer["consumer_rows"] > 0
    target_observed = consumer["target_id_records"] > 0
    if consumer_observed and target_observed:
        return (
            "B",
            "VectorObjectList row consumer is identified and the target VectorData id is observed.",
            "Next target: hook the function that stores or loads the observed target external id.",
        )
    if meta_observed and consumer_observed and not target_observed:
        return (
            "D",
            "VectorObjectList metadata and a consumer trace ran, but the target row/id was not observed.",
            "Next target: find the row filter/condition around the consumer caller.",
        )
    if meta_observed:
        return (
            "A",
            "VectorObjectList/VectorData string xrefs are metadata registration only; no row consumer is confirmed.",
            "Next target: descriptor consumer for the observed VectorObjectList/VectorData descriptor pointer.",
        )
    return (
        "E",
        "No VectorObjectList/VectorData metadata or consumer evidence was observed in the supplied traces.",
        "Next target: re-audit static xrefs or attach earlier/spawn.",
    )


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("metadata_trace", type=Path, help="native_sqlite_table_metadata_trace_*.jsonl")
    parser.add_argument("consumer_trace", type=Path, nargs="?", help="optional native_vectorobjectlist_descriptor_consumer_trace_*.jsonl")
    parser.add_argument("--ground-truth", type=Path, default=GROUND_TRUTH)
    parser.add_argument("--output", type=Path, default=OUT_PATH)
    args = parser.parse_args()

    gt = load_json(args.ground_truth)
    metadata_rows = iter_jsonl(args.metadata_trace)
    consumer_rows = iter_jsonl(args.consumer_trace)
    metadata_summary = summarize_metadata(metadata_rows)
    consumer_summary = summarize_consumer(consumer_rows)
    classification, rationale, next_target = classify(metadata_summary, consumer_summary)

    result = {
        "ground_truth_path": str(args.ground_truth),
        "metadata_trace_path": str(args.metadata_trace),
        "consumer_trace_path": str(args.consumer_trace) if args.consumer_trace else None,
        "target_vector_data_id": TARGET_ID,
        "ground_truth_target_row": find_ground_truth_target(gt),
        "metadata": metadata_summary,
        "consumer": consumer_summary,
        "vectorobjectlist_table_metadata_observed": metadata_summary["observed"].get("VectorObjectList", 0) > 0,
        "vectordata_column_metadata_observed": metadata_summary["observed"].get("VectorData", 0) > 0,
        "row_consumer_identified": consumer_summary["consumer_rows"] > 0,
        "target_vectordata_id_observed_natively": consumer_summary["target_id_records"] > 0,
        "target_id_state": (
            "loaded immediately"
            if consumer_summary["target_id_records"] > 0
            else "not observed"
        ),
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
