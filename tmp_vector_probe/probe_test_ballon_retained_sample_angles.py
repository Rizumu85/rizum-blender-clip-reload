from __future__ import annotations

import importlib.util
import json
import math
import sys
from pathlib import Path

import numpy as np


ROOT = Path(__file__).resolve().parents[1]
CLIP_PATH = ROOT / "img" / "Test_Ballon.clip"
OUT_JSON = ROOT / "tmp_vector_probe" / "test_ballon_retained_sample_angles_codex_20260605.json"


def load_module(name: str, path: Path):
    spec = importlib.util.spec_from_file_location(name, path)
    mod = importlib.util.module_from_spec(spec)
    sys.modules[name] = mod
    assert spec.loader is not None
    spec.loader.exec_module(mod)
    return mod


clip_mod = load_module("probe_clip_loader_angles", ROOT / "clip_loader.py")


def dist(a: tuple[float, float], b: tuple[float, float]) -> float:
    return float(math.hypot(b[0] - a[0], b[1] - a[1]))


def qpos(
    p0: tuple[float, float],
    c: tuple[float, float],
    p1: tuple[float, float],
    t: float,
) -> tuple[float, float]:
    u = 1.0 - t
    return (
        u * u * p0[0] + 2.0 * u * t * c[0] + t * t * p1[0],
        u * u * p0[1] + 2.0 * u * t * c[1] + t * t * p1[1],
    )


def qtangent(
    p0: tuple[float, float],
    c: tuple[float, float],
    p1: tuple[float, float],
    t: float,
) -> tuple[float, float]:
    dx = 2.0 * ((1.0 - t) * (c[0] - p0[0]) + t * (p1[0] - c[0]))
    dy = 2.0 * ((1.0 - t) * (c[1] - p0[1]) + t * (p1[1] - c[1]))
    n = math.hypot(dx, dy)
    if n <= 1e-9:
        dx = p1[0] - p0[0]
        dy = p1[1] - p0[1]
        n = math.hypot(dx, dy)
    if n <= 1e-9:
        return (0.0, 0.0)
    return (dx / n, dy / n)


def tangent_deg(v: tuple[float, float]) -> float:
    if abs(v[0]) <= 1e-12 and abs(v[1]) <= 1e-12:
        return 0.0
    deg = math.degrees(math.atan2(v[1], v[0]))
    if deg < 0.0:
        deg += 360.0
    return deg


def angle_delta(a: float, b: float) -> float:
    d = abs((a - b + 180.0) % 360.0 - 180.0)
    return float(d)


def polyline_tangent(points: list[tuple[float, float]], idx: int) -> tuple[float, float]:
    if len(points) < 2:
        return (0.0, 0.0)
    p0 = points[idx]
    p1 = points[(idx + 1) % len(points)]
    dx = p1[0] - p0[0]
    dy = p1[1] - p0[1]
    n = math.hypot(dx, dy)
    if n <= 1e-9:
        return (0.0, 0.0)
    return (dx / n, dy / n)


def flatten_quadratic_segments(
    points: list[tuple[float, float]],
    controls: list[tuple[float, float]],
    samples_per_segment: int,
) -> list[tuple[float, float]]:
    out: list[tuple[float, float]] = []
    for idx, p0 in enumerate(points):
        p1 = points[(idx + 1) % len(points)]
        c = controls[idx]
        for sample_idx in range(samples_per_segment):
            out.append(qpos(p0, c, p1, sample_idx / float(samples_per_segment)))
    return out


def quad_length_table(
    p0: tuple[float, float],
    c: tuple[float, float],
    p1: tuple[float, float],
    subdivisions: int = 256,
) -> tuple[list[float], list[tuple[float, float]]]:
    pts = [qpos(p0, c, p1, i / float(subdivisions)) for i in range(subdivisions + 1)]
    acc = [0.0]
    total = 0.0
    for a, b in zip(pts, pts[1:]):
        total += dist(a, b)
        acc.append(total)
    return acc, pts


def t_at_distance(acc: list[float], target: float) -> float:
    total = acc[-1]
    if total <= 1e-9:
        return 0.0
    target = min(max(target, 0.0), total)
    lo = 0
    hi = len(acc) - 1
    while lo + 1 < hi:
        mid = (lo + hi) // 2
        if acc[mid] <= target:
            lo = mid
        else:
            hi = mid
    span = acc[lo + 1] - acc[lo]
    frac = 0.0 if span <= 1e-12 else (target - acc[lo]) / span
    return (lo + frac) / float(len(acc) - 1)


def native_like_quadratic_walk(
    points: list[tuple[float, float]],
    controls: list[tuple[float, float]],
    step: float,
) -> list[dict]:
    samples: list[dict] = []
    residual = 0.0
    for seg_idx, p0 in enumerate(points):
        c = controls[seg_idx]
        p1 = points[(seg_idx + 1) % len(points)]
        acc, _ = quad_length_table(p0, c, p1)
        length = acc[-1]
        if length <= 1e-9:
            continue
        walk = max(0.0, residual)
        emitted = 0
        while walk < length and emitted < 1024:
            t = t_at_distance(acc, walk)
            tangent = qtangent(p0, c, p1, t)
            samples.append({
                "seg": seg_idx,
                "t": round(float(t), 8),
                "x": round(float(qpos(p0, c, p1, t)[0]), 6),
                "y": round(float(qpos(p0, c, p1, t)[1]), 6),
                "angle_deg": round(tangent_deg(tangent), 6),
            })
            walk += step
            emitted += 1
        residual = max(0.0, walk - length)
    return samples


def read_record_points(body: bytes, record) -> tuple[list[tuple[float, float]], list[tuple[float, float]]]:
    import struct

    points_start = record.off + record.header_len
    points: list[tuple[float, float]] = []
    controls: list[tuple[float, float]] = []
    for idx in range(record.point_count):
        point_off = points_start + idx * record.point_stride
        x = struct.unpack_from(">d", body, point_off)[0]
        y = struct.unpack_from(">d", body, point_off + 8)[0]
        points.append((float(x), float(y)))
        tail_off = point_off + record.point_tail_offset
        cx = struct.unpack_from(">d", body, tail_off)[0]
        cy = struct.unpack_from(">d", body, tail_off + 8)[0]
        controls.append((float(cx), float(cy)))
    return points, controls


def compare_point_lists(a: list[tuple[float, float]], b: list[dict]) -> dict:
    count = min(len(a), len(b))
    if count == 0:
        return {"paired": 0}
    deltas = [
        dist(a[i], (float(b[i]["x"]), float(b[i]["y"])))
        for i in range(count)
    ]
    return {
        "paired": count,
        "count_a": len(a),
        "count_b": len(b),
        "mean_pos_delta": round(float(np.mean(deltas)), 6),
        "max_pos_delta": round(float(np.max(deltas)), 6),
        "p95_pos_delta": round(float(np.percentile(deltas, 95)), 6),
    }


def main() -> int:
    clip = clip_mod.ClipFile(str(CLIP_PATH))
    try:
        layer = clip._layer_row(8)
        if layer is None:
            raise RuntimeError("Layer 8 not found")
        body = clip._vector_object_body(layer["MainId"])
        if body is None:
            raise RuntimeError("Layer 8 vector body not found")

        out = {
            "clip": str(CLIP_PATH),
            "layer": 8,
            "note": (
                "Diagnostic only. Quadratic walk uses a 256-subdivision arc-length table, "
                "not a proven exact native length helper."
            ),
            "records": [],
        }
        records = [
            record for record in clip._vector_object_records_100(body)
            if record.family_id == clip_mod.VECTOR_FAMILY_BALLOON
            and record.line_style_id == 4
            and (record.object_flags & clip_mod.VECTOR_OBJECT_FLAGS_CONTROL_POINT)
        ]
        for record_index, record in enumerate(records):
            style = clip._brush_style_preview(record.line_style_id)
            interval = 1.0 if style is None else float(style.interval_base)
            points, controls = read_record_points(body, record)
            preview_path = clip._vector_object_record_point_path(body, record)
            preview_step = max(clip_mod.BALLOON_NATIVE_RETAINED_DAB_MIN_STEP, float(record.width) * max(interval, 0.05))
            preview_samples = clip._closed_path_resample_points_by_distance(preview_path or [], preview_step)
            preview_angles = [tangent_deg(polyline_tangent(preview_samples, i)) for i in range(len(preview_samples))]
            native_steps = {
                "width_interval": max(clip_mod.BALLOON_NATIVE_RETAINED_DAB_MIN_STEP, float(record.width) * max(interval, 0.05)),
                "diameter_interval": max(clip_mod.BALLOON_NATIVE_RETAINED_DAB_MIN_STEP, 2.0 * float(record.width) * max(interval, 0.05)),
            }
            variants = {}
            for name, step in native_steps.items():
                native_samples = native_like_quadratic_walk(points, controls, step)
                angle_count = min(len(preview_angles), len(native_samples))
                angle_deltas = [
                    angle_delta(preview_angles[i], float(native_samples[i]["angle_deg"]))
                    for i in range(angle_count)
                ]
                variants[name] = {
                    "step": round(float(step), 6),
                    "native_count": len(native_samples),
                    "compare_to_preview_order": compare_point_lists(preview_samples, native_samples),
                    "mean_angle_delta_to_preview": None if not angle_deltas else round(float(np.mean(angle_deltas)), 6),
                    "max_angle_delta_to_preview": None if not angle_deltas else round(float(np.max(angle_deltas)), 6),
                    "first_samples": native_samples[:8],
                }
            out["records"].append({
                "record_index": record_index,
                "off": record.off,
                "point_count": record.point_count,
                "object_flags": hex(record.object_flags),
                "width": round(float(record.width), 6),
                "line_style_id": record.line_style_id,
                "preview_path_points": 0 if preview_path is None else len(preview_path),
                "preview_step": round(float(preview_step), 6),
                "preview_sample_count": len(preview_samples),
                "preview_first_samples": [
                    {
                        "x": round(float(p[0]), 6),
                        "y": round(float(p[1]), 6),
                        "angle_deg": round(float(preview_angles[i]), 6),
                    }
                    for i, p in enumerate(preview_samples[:8])
                ],
                "variants": variants,
            })
    finally:
        clip.close()

    OUT_JSON.write_text(json.dumps(out, indent=2, ensure_ascii=False), encoding="utf-8")
    print(json.dumps({
        "out": str(OUT_JSON),
        "records": len(out["records"]),
        "preview_total": sum(record["preview_sample_count"] for record in out["records"]),
        "native_width_interval_total": sum(record["variants"]["width_interval"]["native_count"] for record in out["records"]),
        "native_diameter_interval_total": sum(record["variants"]["diameter_interval"]["native_count"] for record in out["records"]),
    }, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
