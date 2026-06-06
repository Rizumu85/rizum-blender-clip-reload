from __future__ import annotations

import json
import math
from collections import defaultdict
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[1]
ATTRIBUTION_PATH = ROOT / "tmp_vector_probe" / "sizepressure_emitted_dab_residual_attribution_codex_v1.json"
NATIVE_TRACE_PATH = ROOT / "tmp_vector_probe" / "native_142640150_rowspan_trace_v1.jsonl"
OUT_PATH = ROOT / "tmp_vector_probe" / "native_142640150_rowspan_compare_v1.json"

PRIMARY_SUSPECTS = list(range(203, 210))
SECONDARY_SUSPECTS = list(range(75, 88))


def finite_number(value: Any) -> float | None:
    if isinstance(value, (int, float)):
        out = float(value)
        return out if math.isfinite(out) else None
    return None


def trunc_toward_zero(value: float) -> int:
    return int(value)


def read_jsonl(path: Path) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    if not path.exists():
        raise FileNotFoundError(path)
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


def importer_span(
    *,
    cx: float,
    cy: float,
    radius: float,
    row_y: int,
    clip_left: int,
    clip_right_exclusive: int,
) -> dict[str, Any] | None:
    y_center = cy - 0.5
    dy = row_y - y_center
    span_sq = radius * radius - dy * dy
    if span_sq <= 0:
        return None
    span_before_subtract = math.sqrt(span_sq)
    span = span_before_subtract - 0.4
    if span < 0:
        span = 0.0
    x0_unclipped = trunc_toward_zero(cx - span)
    x1_unclipped = trunc_toward_zero(cx + span)
    x0 = max(clip_left, x0_unclipped)
    x1 = min(clip_right_exclusive - 1, x1_unclipped)
    if x1 < x0:
        return None
    return {
        "importer_y_center": y_center,
        "importer_dy": dy,
        "importer_span_sq": span_sq,
        "importer_span_before_subtract": span_before_subtract,
        "importer_span_after_subtract": span,
        "importer_subtract_constant": 0.4,
        "importer_x0_unclipped": x0_unclipped,
        "importer_x1_unclipped": x1_unclipped,
        "importer_x0_clipped": x0,
        "importer_x1_clipped": x1,
        "importer_inclusive_or_exclusive": "inclusive_x0_x1",
    }


def span_len(x0: int | None, x1: int | None) -> int:
    if x0 is None or x1 is None or x1 < x0:
        return 0
    return x1 - x0 + 1


def overlap_len(a0: int | None, a1: int | None, b0: int | None, b1: int | None) -> int:
    if a0 is None or a1 is None or b0 is None or b1 is None:
        return 0
    lo = max(a0, b0)
    hi = min(a1, b1)
    return max(0, hi - lo + 1)


def compare_row(native_row: dict[str, Any], dab: dict[str, Any]) -> dict[str, Any]:
    row_y = int(native_row["row_y"])
    clip_left = int(native_row.get("clip_left") if native_row.get("clip_left") is not None else 0)
    clip_right = int(
        native_row.get("clip_right_exclusive")
        if native_row.get("clip_right_exclusive") is not None
        else 1024
    )
    replay = importer_span(
        cx=float(dab["point_x"]),
        cy=float(dab["point_y"]),
        radius=float(dab["radius"]),
        row_y=row_y,
        clip_left=clip_left,
        clip_right_exclusive=clip_right,
    )
    native_x0 = native_row.get("native_x0_clipped")
    native_x1 = native_row.get("native_x1_clipped")
    native_x0_i = None if native_x0 is None else int(native_x0)
    native_x1_i = None if native_x1 is None else int(native_x1)
    importer_x0 = None if replay is None else int(replay["importer_x0_clipped"])
    importer_x1 = None if replay is None else int(replay["importer_x1_clipped"])

    native_len = span_len(native_x0_i, native_x1_i)
    importer_len = span_len(importer_x0, importer_x1)
    common = overlap_len(native_x0_i, native_x1_i, importer_x0, importer_x1)
    importer_extra = max(0, importer_len - common)
    importer_missing = max(0, native_len - common)

    row = {
        "row_y": row_y,
        "native_x0": native_x0_i,
        "native_x1": native_x1_i,
        "importer_x0": importer_x0,
        "importer_x1": importer_x1,
        "delta_left": None if importer_x0 is None or native_x0_i is None else importer_x0 - native_x0_i,
        "delta_right": None if importer_x1 is None or native_x1_i is None else importer_x1 - native_x1_i,
        "native_len": native_len,
        "importer_len": importer_len,
        "overlap_len": common,
        "extra_pixels_explained": importer_extra,
        "missing_pixels_explained": importer_missing,
        "native": {
            key: native_row.get(key)
            for key in (
                "y_center_used",
                "dy",
                "span_before_subtract",
                "span_after_subtract",
                "subtract_constant",
                "native_x0_unclipped_recomputed",
                "native_x1_unclipped_recomputed",
                "coverage_or_alpha_value",
                "context_ptr",
                "plot_ptr",
            )
        },
        "importer": replay,
    }
    return row


def build_expected_importer_rows(dab: dict[str, Any], clip_left: int, clip_right: int, clip_top: int, clip_bottom: int) -> dict[int, dict[str, Any]]:
    rows: dict[int, dict[str, Any]] = {}
    for row_y in range(clip_top, clip_bottom):
        span = importer_span(
            cx=float(dab["point_x"]),
            cy=float(dab["point_y"]),
            radius=float(dab["radius"]),
            row_y=row_y,
            clip_left=clip_left,
            clip_right_exclusive=clip_right,
        )
        if span is not None:
            rows[row_y] = span
    return rows


def main() -> int:
    attribution = json.loads(ATTRIBUTION_PATH.read_text(encoding="utf-8"))
    native_rows = [row for row in read_jsonl(NATIVE_TRACE_PATH) if "parse_error" not in row and row.get("event") == "row_span"]
    dabs = {int(dab["global_dab_index"]): dab for dab in attribution["dabs"]}

    trace_by_dab: dict[int, list[dict[str, Any]]] = defaultdict(list)
    for row in native_rows:
        trace_by_dab[int(row["global_dab_index"])].append(row)

    suspect_ids = [idx for idx in PRIMARY_SUSPECTS + SECONDARY_SUSPECTS if idx in dabs]
    comparisons = []
    for dab_id in suspect_ids:
        dab = dabs[dab_id]
        rows = sorted(trace_by_dab.get(dab_id, []), key=lambda row: (row["row_y"], row.get("jsonl_line_no", 0)))
        if rows:
            clip_left = int(rows[0].get("clip_left") if rows[0].get("clip_left") is not None else 0)
            clip_top = int(rows[0].get("clip_top") if rows[0].get("clip_top") is not None else 0)
            clip_right = int(rows[0].get("clip_right_exclusive") if rows[0].get("clip_right_exclusive") is not None else 1024)
            clip_bottom = int(rows[0].get("clip_bottom_exclusive") if rows[0].get("clip_bottom_exclusive") is not None else 1024)
        else:
            clip_left, clip_top, clip_right, clip_bottom = 0, 0, 1024, 1024

        row_comparisons = [compare_row(row, dab) for row in rows]
        native_row_set = {int(row["row_y"]) for row in rows}
        expected_rows = build_expected_importer_rows(dab, clip_left, clip_right, clip_top, clip_bottom)
        importer_only_rows = []
        for row_y, span in sorted(expected_rows.items()):
            if row_y in native_row_set:
                continue
            importer_only_rows.append(
                {
                    "row_y": row_y,
                    "native_x0": None,
                    "native_x1": None,
                    "importer_x0": span["importer_x0_clipped"],
                    "importer_x1": span["importer_x1_clipped"],
                    "extra_pixels_explained": span_len(span["importer_x0_clipped"], span["importer_x1_clipped"]),
                    "missing_pixels_explained": 0,
                    "importer": span,
                }
            )

        extra_total = sum(int(row["extra_pixels_explained"]) for row in row_comparisons) + sum(
            int(row["extra_pixels_explained"]) for row in importer_only_rows
        )
        missing_total = sum(int(row["missing_pixels_explained"]) for row in row_comparisons)
        comparisons.append(
            {
                "global_dab_index": dab_id,
                "segment_index": dab["segment_index"],
                "emitted_index_in_segment": dab["emitted_index_in_segment"],
                "owned_extra_pixels": dab.get("owned_extra_pixels"),
                "owned_missing_pixels": dab.get("owned_missing_pixels"),
                "required_radius_shrink_median": dab.get("required_radius_shrink_median"),
                "cx": dab["point_x"],
                "cy": dab["point_y"],
                "radius": dab["radius"],
                "native_rows": len(rows),
                "importer_rows": len(expected_rows),
                "importer_only_rows": importer_only_rows,
                "extra_pixels_explained": extra_total,
                "missing_pixels_explained": missing_total,
                "rows_with_delta": [
                    row for row in row_comparisons if row["delta_left"] not in (0, None) or row["delta_right"] not in (0, None)
                ],
                "rows": row_comparisons,
            }
        )

    payload = {
        "version": 1,
        "inputs": {
            "attribution": str(ATTRIBUTION_PATH),
            "native_trace": str(NATIVE_TRACE_PATH),
        },
        "summary": {
            "native_trace_rows": len(native_rows),
            "suspect_dabs": suspect_ids,
            "dabs_with_native_rows": sum(1 for item in comparisons if item["native_rows"] > 0),
            "total_extra_pixels_explained_by_span_delta": sum(int(item["extra_pixels_explained"]) for item in comparisons),
            "total_missing_pixels_explained_by_span_delta": sum(int(item["missing_pixels_explained"]) for item in comparisons),
        },
        "comparisons": comparisons,
    }
    OUT_PATH.write_text(json.dumps(payload, indent=2), encoding="utf-8")
    print(json.dumps({"wrote": str(OUT_PATH), "summary": payload["summary"]}, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
