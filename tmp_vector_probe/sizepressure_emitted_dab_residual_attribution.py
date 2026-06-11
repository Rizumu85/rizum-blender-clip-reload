from __future__ import annotations

import importlib.util
import json
import math
import os
import sys
from pathlib import Path
from statistics import mean, median

import numpy as np
from PIL import Image


ROOT = Path(__file__).resolve().parents[1]
CLIP_PATH = ROOT / "img" / "Vector_SizePressure.clip"
REF_PATH = ROOT / "img" / "Vector_SizePressure.png"
TRACE_PATH = ROOT / "tmp_vector_probe" / "vector_sizepressure_emitted_dabs_trace_v1.json"
OUT_PATH = ROOT / "tmp_vector_probe" / "sizepressure_emitted_dab_residual_attribution_codex_v1.json"
BRUSH_STYLE_ID = 10
VISIBLE_THRESHOLD = 1


def load_loader_module():
    mod_path = ROOT / "clip_studio_importer" / "clip_loader.py"
    spec = importlib.util.spec_from_file_location("probe_clip_loader", mod_path)
    mod = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = mod
    assert spec.loader is not None
    spec.loader.exec_module(mod)
    return mod


def hard_circle_covers(x: int, y: int, cx: float, cy: float, radius: float) -> bool:
    if radius <= 0.0:
        return False
    span_sq = radius * radius - (float(y) - (cy - 0.5)) ** 2
    if span_sq <= 0.0:
        return False
    span = max(0.0, math.sqrt(span_sq) - 0.4)
    x0 = int(cx - span)
    x1 = int(cx + span)
    return x0 <= int(x) <= x1


def required_shrink_to_exclude(x: int, y: int, dab: dict) -> float:
    radius = float(dab["radius"])
    if not hard_circle_covers(x, y, dab["point_x"], dab["point_y"], radius):
        return 0.0
    lo = 0.0
    hi = radius
    for _ in range(48):
        mid = (lo + hi) * 0.5
        if hard_circle_covers(x, y, dab["point_x"], dab["point_y"], radius - mid):
            lo = mid
        else:
            hi = mid
    return hi


def required_expand_to_cover(x: int, y: int, dab: dict) -> float:
    radius = float(dab["radius"])
    if hard_circle_covers(x, y, dab["point_x"], dab["point_y"], radius):
        return 0.0
    hi = max(1.0, radius * 0.125)
    while hi < 256.0 and not hard_circle_covers(x, y, dab["point_x"], dab["point_y"], radius + hi):
        hi *= 2.0
    lo = 0.0
    for _ in range(48):
        mid = (lo + hi) * 0.5
        if hard_circle_covers(x, y, dab["point_x"], dab["point_y"], radius + mid):
            hi = mid
        else:
            lo = mid
    return hi


def pressure_effector_primary_only(clip, brush_style_id: int, column: str, primary_scalar: float) -> float | None:
    blob = clip._brush_effector_blob(brush_style_id, column)
    if not isinstance(blob, bytes) or len(blob) != 24:
        return None
    flag, _mode, graph0, amount0, _graph1, _amount1 = clip_loader.struct.unpack(">III f I f", blob)
    if flag != 0x31:
        return None
    primary = clip._eval_brush_effector_graph(graph0, primary_scalar)
    if primary is None:
        return None
    return max(0.0, min((1.0 - float(amount0)) + float(amount0) * primary, 1.0))


def pressure_direction(values: list[float]) -> str:
    deltas = [b - a for a, b in zip(values, values[1:]) if abs(b - a) > 1e-9]
    if not deltas:
        return "flat"
    has_rising = any(delta > 0.0 for delta in deltas)
    has_falling = any(delta < 0.0 for delta in deltas)
    if has_rising and has_falling:
        return "mixed"
    return "rising" if has_rising else "falling"


def render_with_trace(clip_loader):
    had_trace = TRACE_PATH.exists()
    old_env = os.environ.get("RIZUM_CLIP_TRACE_VECTOR_DABS")
    os.environ["RIZUM_CLIP_TRACE_VECTOR_DABS"] = "1"
    clip = clip_loader.ClipFile(str(CLIP_PATH))
    try:
        out = clip.composite()
        interval_scalar = clip._brush_interval_scalar(BRUSH_STYLE_ID)
        width = None
        trace = json.loads(TRACE_PATH.read_text(encoding="utf-8"))
        for dab in trace["dabs"]:
            if dab["size_effector"]:
                width = float(dab["effective_size"]) / (
                    max(0.0, min(float(dab["size_factor"]), 4.0))
                    * max(0.0, min(float(dab["size_effector"]), 4.0))
                )
                break
        if width is None:
            raise RuntimeError("Could not infer stroke width from emitted dab trace.")
        primary_only = [
            pressure_effector_primary_only(
                clip,
                BRUSH_STYLE_ID,
                "SizeEffector",
                float(dab["primary"]),
            )
            for dab in trace["dabs"]
        ]
    finally:
        clip.close()
        if old_env is None:
            os.environ.pop("RIZUM_CLIP_TRACE_VECTOR_DABS", None)
        else:
            os.environ["RIZUM_CLIP_TRACE_VECTOR_DABS"] = old_env
    if not had_trace and TRACE_PATH.exists():
        TRACE_PATH.unlink()
    return out, trace, float(interval_scalar), float(width), primary_only


def main() -> int:
    global clip_loader
    clip_loader = load_loader_module()
    out, trace, interval_scalar, width, primary_only_values = render_with_trace(clip_loader)
    ref = np.array(Image.open(REF_PATH).convert("RGBA"))
    if out.shape != ref.shape:
        raise RuntimeError(f"Shape mismatch: out={out.shape} ref={ref.shape}")

    dabs = trace["dabs"]
    segment_to_indices: dict[int, list[int]] = {}
    for global_index, dab in enumerate(dabs):
        dab["global_dab_index"] = global_index
        dab["radius"] = float(dab["effective_size"])
        segment_to_indices.setdefault(int(dab["segment_index"]), []).append(global_index)

    enriched: list[dict] = []
    for global_index, dab in enumerate(dabs):
        seg_indices = segment_to_indices[int(dab["segment_index"])]
        primary_only = primary_only_values[global_index]
        if primary_only is None:
            primary_only = float(dab["size_effector"])
        secondary_delta_size = float(dab["size_effector"]) - float(primary_only)
        radius_scale = width * max(0.0, min(float(dab["size_factor"]), 4.0))
        next_step_raw = 2.0 * max(0.1, float(dab["effective_size"])) * interval_scalar
        enriched.append(
            {
                "global_dab_index": global_index,
                "segment_index": int(dab["segment_index"]),
                "emitted_index_in_segment": int(dab["emitted_index_in_segment"]),
                "is_first_dab_after_residual": bool(
                    global_index == seg_indices[0] and float(dab["residual_before"]) > 1e-9
                ),
                "is_last_dab_in_segment": bool(global_index == seg_indices[-1]),
                "t": float(dab["t"]),
                "point_x": float(dab["point_x"]),
                "point_y": float(dab["point_y"]),
                "center_frac_x": float(dab["point_x"] - math.floor(float(dab["point_x"]))),
                "center_frac_y": float(dab["point_y"] - math.floor(float(dab["point_y"]))),
                "primary": float(dab["primary"]),
                "secondary": float(dab["secondary"]),
                "auxiliary": float(dab["auxiliary"]),
                "size_effector_full": float(dab["size_effector"]),
                "size_effector_primary_only": float(primary_only),
                "secondary_delta_size": float(secondary_delta_size),
                "secondary_delta_radius": float(secondary_delta_size * radius_scale),
                "size_factor": float(dab["size_factor"]),
                "effective_size": float(dab["effective_size"]),
                "radius": float(dab["effective_size"]),
                "radius_frac": float(dab["effective_size"] - math.floor(float(dab["effective_size"]))),
                "interval_scalar": interval_scalar,
                "next_step_raw": float(next_step_raw),
                "next_step": float(dab["next_step"]),
                "next_step_was_clamped": bool(
                    abs(float(dab["next_step"]) - 1.0) <= 1e-9 and next_step_raw < 1.0
                ),
                "residual_before": float(dab["residual_before"]),
                "residual_after": float(dab["residual_after"]),
                "owned_extra_pixels": 0,
                "owned_missing_pixels": 0,
                "required_radius_shrink_min": None,
                "required_radius_shrink_median": None,
                "required_radius_shrink_max": None,
            }
        )

    diff = np.abs(out.astype(np.int16) - ref.astype(np.int16))
    visible = diff.max(axis=-1) > VISIBLE_THRESHOLD
    out_luma = out[..., :3].astype(np.int16).sum(axis=-1)
    ref_luma = ref[..., :3].astype(np.int16).sum(axis=-1)
    extra_points = np.argwhere(visible & (out_luma < ref_luma))
    missing_points = np.argwhere(visible & (ref_luma < out_luma))

    shrink_by_dab: dict[int, list[float]] = {idx: [] for idx in range(len(enriched))}
    extra_pixels = []
    missing_pixels = []

    for y, x in extra_points:
        covers = [
            int(dab["global_dab_index"])
            for dab in enriched
            if hard_circle_covers(int(x), int(y), dab["point_x"], dab["point_y"], dab["radius"])
        ]
        owner = covers[-1] if covers else None
        shrink = None
        if owner is not None:
            shrink = required_shrink_to_exclude(int(x), int(y), enriched[owner])
            enriched[owner]["owned_extra_pixels"] += 1
            shrink_by_dab[owner].append(shrink)
        extra_pixels.append(
            {
                "x": int(x),
                "y": int(y),
                "owner_dab": owner,
                "covering_dabs": covers,
                "required_radius_shrink_px": shrink,
            }
        )

    for y, x in missing_points:
        covers = [
            int(dab["global_dab_index"])
            for dab in enriched
            if hard_circle_covers(int(x), int(y), dab["point_x"], dab["point_y"], dab["radius"])
        ]
        if covers:
            owner = covers[-1]
        else:
            owner = min(
                range(len(enriched)),
                key=lambda idx: required_expand_to_cover(int(x), int(y), enriched[idx]),
            )
        enriched[owner]["owned_missing_pixels"] += 1
        missing_pixels.append(
            {
                "x": int(x),
                "y": int(y),
                "owner_dab": int(owner),
                "covering_dabs": covers,
            }
        )

    for idx, values in shrink_by_dab.items():
        if values:
            enriched[idx]["required_radius_shrink_min"] = float(min(values))
            enriched[idx]["required_radius_shrink_median"] = float(median(values))
            enriched[idx]["required_radius_shrink_max"] = float(max(values))

    segment_summary = []
    for segment_index, indices in sorted(segment_to_indices.items()):
        seg_dabs = [enriched[idx] for idx in indices]
        radii = [dab["radius"] for dab in seg_dabs]
        ts = [dab["t"] for dab in seg_dabs]
        top_dabs = sorted(
            indices,
            key=lambda idx: enriched[idx]["owned_extra_pixels"],
            reverse=True,
        )
        top_dabs = [idx for idx in top_dabs if enriched[idx]["owned_extra_pixels"] > 0][:10]
        segment_summary.append(
            {
                "segment_index": int(segment_index),
                "sample_count": len(indices),
                "residual_in": float(seg_dabs[0]["residual_before"]),
                "residual_out": float(seg_dabs[-1]["residual_after"]),
                "extra_pixels_total": int(sum(dab["owned_extra_pixels"] for dab in seg_dabs)),
                "missing_pixels_total": int(sum(dab["owned_missing_pixels"] for dab in seg_dabs)),
                "radius_min": float(min(radii)),
                "radius_max": float(max(radii)),
                "radius_mean": float(mean(radii)),
                "t_min": float(min(ts)),
                "t_max": float(max(ts)),
                "pressure_direction": pressure_direction([dab["primary"] for dab in seg_dabs]),
                "top_dab_indices_by_extra_pixels": top_dabs,
            }
        )

    payload = {
        "version": 1,
        "clip": CLIP_PATH.name,
        "attribution_rule": "Importer-extra pixels are assigned to the last emitted hard no-pattern dab whose current span covers the pixel; covering_dabs records every covering dab.",
        "summary": {
            "emitted_dabs": len(enriched),
            "segments": len(segment_summary),
            "visible_pixels": int(visible.sum()),
            "extra_pixels": int(len(extra_points)),
            "missing_pixels": int(len(missing_points)),
            "interval_scalar": interval_scalar,
            "inferred_width": width,
        },
        "dabs": enriched,
        "segments": segment_summary,
        "extra_pixels": extra_pixels,
        "missing_pixels": missing_pixels,
    }
    OUT_PATH.parent.mkdir(parents=True, exist_ok=True)
    OUT_PATH.write_text(json.dumps(payload, indent=2), encoding="utf-8")
    print(json.dumps({"wrote": str(OUT_PATH), "summary": payload["summary"]}, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
