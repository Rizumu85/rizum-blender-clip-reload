from __future__ import annotations

import json
import math
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[1]
IMPORTER_ATTRIBUTION = ROOT / "tmp_vector_probe" / "sizepressure_emitted_dab_residual_attribution_codex_v1.json"
NATIVE_TRACE = ROOT / "tmp_vector_probe" / "native_1422d8550_size_trace_v1.jsonl"
OUT_PATH = ROOT / "tmp_vector_probe" / "native_1422d8550_size_match_v1.json"
SUSPECT_DABS = list(range(203, 210)) + list(range(75, 88))


def finite_number(value: Any) -> float | None:
    if isinstance(value, (int, float)):
        v = float(value)
        return v if math.isfinite(v) else None
    return None


def read_jsonl(path: Path) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    if not path.exists():
        return rows
    for line_no, line in enumerate(path.read_text(encoding="utf-8", errors="replace").splitlines(), 1):
        line = line.strip()
        if not line:
            continue
        try:
            row = json.loads(line)
        except json.JSONDecodeError as exc:
            rows.append({"parse_error": str(exc), "line_no": line_no, "raw": line})
            continue
        row["jsonl_line_no"] = line_no
        rows.append(row)
    return rows


def native_center(row: dict[str, Any]) -> tuple[float, float] | None:
    x = finite_number(row.get("center_x_guess_sample_0x00"))
    y = finite_number(row.get("center_y_guess_sample_0x08"))
    if x is None or y is None:
        return None
    return x, y


def native_radius(row: dict[str, Any]) -> float | None:
    return finite_number(row.get("effective_radius_plot_0x00"))


def distance(a: tuple[float, float], b: tuple[float, float]) -> float:
    return math.hypot(a[0] - b[0], a[1] - b[1])


def match_one(dab: dict[str, Any], native_rows: list[dict[str, Any]]) -> dict[str, Any]:
    importer_center = (float(dab["point_x"]), float(dab["point_y"]))
    importer_radius = float(dab["radius"])
    candidates = []
    for row in native_rows:
        nr = native_radius(row)
        nc = native_center(row)
        if nr is None:
            continue
        center_delta = None if nc is None else distance(importer_center, nc)
        radius_delta = abs(importer_radius - nr)
        if center_delta is None:
            score = radius_delta * 2.0
        else:
            score = center_delta + radius_delta * 2.0
        candidates.append(
            {
                "score": score,
                "call_index": row.get("call_index"),
                "jsonl_line_no": row.get("jsonl_line_no"),
                "native_radius": nr,
                "radius_delta": radius_delta,
                "native_center": None if nc is None else [nc[0], nc[1]],
                "center_delta": center_delta,
                "native_final_next_step": row.get("final_next_step_state_0x08"),
                "native_pre_clamp_feedback_step": row.get("pre_clamp_feedback_step_state_0x08"),
                "return_address": row.get("return_address"),
                "caller": row.get("caller"),
                "thread_id": row.get("thread_id"),
            }
        )
    candidates.sort(key=lambda item: item["score"])
    best = candidates[0] if candidates else None
    return {
        "global_dab_index": dab["global_dab_index"],
        "segment_index": dab["segment_index"],
        "emitted_index_in_segment": dab["emitted_index_in_segment"],
        "owned_extra_pixels": dab["owned_extra_pixels"],
        "required_radius_shrink_median": dab["required_radius_shrink_median"],
        "importer_center": [importer_center[0], importer_center[1]],
        "importer_radius": importer_radius,
        "importer_next_step": dab.get("next_step"),
        "best_match": best,
        "top_candidates": candidates[:5],
    }


def main() -> int:
    attribution = json.loads(IMPORTER_ATTRIBUTION.read_text(encoding="utf-8"))
    native_rows = [row for row in read_jsonl(NATIVE_TRACE) if "parse_error" not in row]
    dabs = attribution["dabs"]
    dab_by_index = {int(dab["global_dab_index"]): dab for dab in dabs}

    ordered_indices = [idx for idx in SUSPECT_DABS if idx in dab_by_index]
    ordered_indices += [
        int(dab["global_dab_index"])
        for dab in sorted(dabs, key=lambda item: item["owned_extra_pixels"], reverse=True)
        if int(dab["global_dab_index"]) not in set(ordered_indices)
    ]

    matches = [match_one(dab_by_index[idx], native_rows) for idx in ordered_indices]
    suspect_matches = [match for match in matches if match["global_dab_index"] in SUSPECT_DABS]
    top_extra_matches = sorted(matches, key=lambda item: item["owned_extra_pixels"], reverse=True)[:30]

    payload = {
        "version": 1,
        "inputs": {
            "importer_attribution": str(IMPORTER_ATTRIBUTION),
            "native_trace": str(NATIVE_TRACE),
        },
        "summary": {
            "importer_dabs": len(dabs),
            "native_rows": len(native_rows),
            "suspect_dabs": SUSPECT_DABS,
            "matched_rows_with_radius": sum(1 for row in native_rows if native_radius(row) is not None),
            "matched_rows_with_center_guess": sum(1 for row in native_rows if native_center(row) is not None),
        },
        "suspect_matches_first": suspect_matches,
        "top_extra_matches": top_extra_matches,
        "all_matches": matches,
    }
    OUT_PATH.write_text(json.dumps(payload, indent=2), encoding="utf-8")
    print(json.dumps({"wrote": str(OUT_PATH), "summary": payload["summary"]}, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
