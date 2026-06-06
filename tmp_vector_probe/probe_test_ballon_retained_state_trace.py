from __future__ import annotations

import importlib.util
import json
import math
import struct
import sys
import time
from pathlib import Path

import numpy as np


ROOT = Path(__file__).resolve().parents[1]
CLIP_PATH = ROOT / "img" / "Test_Ballon.clip"
REF_PATH = ROOT / "img" / "Test_Ballon.png"
OUT_JSON = ROOT / "tmp_vector_probe" / "test_ballon_retained_state_trace_probe_codex_20260605.json"


def load_module(name: str, path: Path):
    spec = importlib.util.spec_from_file_location(name, path)
    mod = importlib.util.module_from_spec(spec)
    sys.modules[name] = mod
    assert spec.loader is not None
    spec.loader.exec_module(mod)
    return mod


clip_mod = load_module("probe_clip_loader_retained_state", ROOT / "clip_studio_importer" / "clip_loader.py")
verify_mod = load_module("probe_verify_retained_state", ROOT / "verify_one_clip.py")


def premul_stats(out: np.ndarray) -> dict[str, int | float]:
    result: dict[str, int | float] = {}
    verify_mod._add_diff_stats(result, out, REF_PATH)
    return {
        "premul_mean": result.get("premul_mean"),
        "premul_visible_px": result.get("premul_visible_px"),
        "mean": result.get("mean"),
        "visible_px": result.get("visible_px"),
    }


def qpos(p0, c, p1, t: float) -> tuple[float, float]:
    u = 1.0 - t
    return (
        u * u * p0[0] + 2.0 * u * t * c[0] + t * t * p1[0],
        u * u * p0[1] + 2.0 * u * t * c[1] + t * t * p1[1],
    )


def qtangent(p0, c, p1, t: float) -> float:
    dx = 2.0 * ((1.0 - t) * (c[0] - p0[0]) + t * (p1[0] - c[0]))
    dy = 2.0 * ((1.0 - t) * (c[1] - p0[1]) + t * (p1[1] - c[1]))
    if dx * dx + dy * dy <= 1e-18:
        dx = p1[0] - p0[0]
        dy = p1[1] - p0[1]
    return math.atan2(dy, dx)


def quad_length_table(p0, c, p1, subdivisions: int = 256) -> list[float]:
    pts = [qpos(p0, c, p1, i / float(subdivisions)) for i in range(subdivisions + 1)]
    acc = [0.0]
    total = 0.0
    for a, b in zip(pts, pts[1:]):
        total += math.hypot(b[0] - a[0], b[1] - a[1])
        acc.append(total)
    return acc


def t_at_distance(acc: list[float], target: float) -> float:
    total = acc[-1]
    if total <= 1e-9:
        return 0.0
    target = min(max(target, 0.0), total)
    lo, hi = 0, len(acc) - 1
    while lo + 1 < hi:
        mid = (lo + hi) // 2
        if acc[mid] <= target:
            lo = mid
        else:
            hi = mid
    span = acc[lo + 1] - acc[lo]
    frac = 0.0 if span <= 1e-12 else (target - acc[lo]) / span
    return (lo + frac) / float(len(acc) - 1)


def read_record_points(body: bytes, record) -> tuple[list[tuple[float, float]], list[tuple[float, float]]]:
    points_start = record.off + record.header_len
    points: list[tuple[float, float]] = []
    controls: list[tuple[float, float]] = []
    for idx in range(record.point_count):
        point_off = points_start + idx * record.point_stride
        points.append((
            float(struct.unpack_from(">d", body, point_off)[0]),
            float(struct.unpack_from(">d", body, point_off + 8)[0]),
        ))
        tail_off = point_off + record.point_tail_offset
        controls.append((
            float(struct.unpack_from(">d", body, tail_off)[0]),
            float(struct.unpack_from(">d", body, tail_off + 8)[0]),
        ))
    return points, controls


def native_like_samples(points, controls, step: float) -> list[dict[str, float]]:
    samples: list[dict[str, float]] = []
    residual = 0.0
    for seg_idx, p0 in enumerate(points):
        c = controls[seg_idx]
        p1 = points[(seg_idx + 1) % len(points)]
        acc = quad_length_table(p0, c, p1)
        length = acc[-1]
        if length <= 1e-9:
            continue
        walk = max(0.0, residual)
        emitted = 0
        while walk < length and emitted < 1024:
            t = t_at_distance(acc, walk)
            x, y = qpos(p0, c, p1, t)
            samples.append({"x": x, "y": y, "angle": qtangent(p0, c, p1, t)})
            walk += step
            emitted += 1
        residual = max(0.0, walk - length)
    return samples


def sample_alpha(alpha: np.ndarray, u: np.ndarray, v: np.ndarray, clamp_u: bool, clamp_v: bool) -> np.ndarray:
    h, w = alpha.shape
    uu = np.clip(np.floor(u).astype(np.int64), 0, w - 1) if clamp_u else np.floor(u).astype(np.int64) % w
    vv = np.clip(np.floor(v).astype(np.int64), 0, h - 1) if clamp_v else np.floor(v).astype(np.int64) % h
    return alpha[vv, uu].astype(np.float32) / 255.0


def uv_from_mode(mode: int, flip: int, w: int, h: int, across: np.ndarray, along: np.ndarray):
    if mode == 0:
        u, v, clamp_u, clamp_v = across * (w - 1), along, False, True
    elif mode == 1:
        u, v, clamp_u, clamp_v = along, across * (h - 1), True, False
    elif mode == 2:
        u, v, clamp_u, clamp_v = across * (w - 1), (h - 1) - along, False, True
    else:
        u, v, clamp_u, clamp_v = (w - 1) - along, across * (h - 1), True, False
    if flip:
        if mode in (0, 2):
            u = (w - 1) - u
        else:
            v = (h - 1) - v
    return u, v, clamp_u, clamp_v


def side_points(p: tuple[float, float], radius: float, angle: float):
    ca, sa = math.cos(angle), math.sin(angle)
    return (
        (p[0] - ca * radius, p[1] + sa * radius),
        (p[0] + ca * radius, p[1] - sa * radius),
    )


def draw_quad(rgba, cover16, prev, cur, radius, material_alpha, color, mode, flip, opacity_scale):
    p0 = (prev["x"], prev["y"])
    p1 = (cur["x"], cur["y"])
    a0, b0 = side_points(p0, radius, prev["angle"])
    a1, b1 = side_points(p1, radius, cur["angle"])
    xs = [a0[0], b0[0], b1[0], a1[0]]
    ys = [a0[1], b0[1], b1[1], a1[1]]
    min_x = max(int(math.floor(min(xs) - 1.0)), 0)
    max_x = min(int(math.ceil(max(xs) + 1.0)), rgba.shape[1] - 1)
    min_y = max(int(math.floor(min(ys) - 1.0)), 0)
    max_y = min(int(math.ceil(max(ys) + 1.0)), rgba.shape[0] - 1)
    if max_x < min_x or max_y < min_y:
        return

    dx, dy = p1[0] - p0[0], p1[1] - p0[1]
    length = math.hypot(dx, dy)
    if length <= 1e-9:
        return
    tx, ty = dx / length, dy / length
    nx, ny = -ty, tx
    phase0, phase1 = prev["phase"], cur["phase"]

    yy, xx = np.mgrid[min_y : max_y + 1, min_x : max_x + 1].astype(np.float64)
    mask_sum = np.zeros_like(xx, dtype=np.float32)
    h, w = material_alpha.shape
    for ox, oy in ((0.25, 0.25), (0.75, 0.25), (0.25, 0.75), (0.75, 0.75)):
        px = xx + ox - p0[0]
        py = yy + oy - p0[1]
        along = (px * tx + py * ty) / length
        across = (px * nx + py * ny) / max(2.0 * radius, 1e-9) + 0.5
        inside = (along >= 0.0) & (along <= 1.0) & (across >= 0.0) & (across <= 1.0)
        mat_along = phase0 + (phase1 - phase0) * np.clip(along, 0.0, 1.0)
        u, v, clamp_u, clamp_v = uv_from_mode(mode, flip, w, h, np.clip(across, 0.0, 1.0), mat_along)
        mask_sum += np.where(inside, sample_alpha(material_alpha, u, v, clamp_u, clamp_v), 0.0)

    coverage = np.clip((mask_sum / 4.0) * opacity_scale, 0.0, 1.0)
    cov16 = np.floor(coverage * 32768.0 + 0.5).astype(np.uint32)
    if not np.any(cov16):
        return
    limit16 = int(round(max(0, min(color[3], 255)) * 128.0))
    region16 = cover16[min_y : max_y + 1, min_x : max_x + 1]
    delta = ((limit16 - region16.astype(np.int32)) * cov16.astype(np.int32)) >> 15
    next16 = np.clip(region16.astype(np.int32) + np.maximum(delta, 0), 0, 32768).astype(np.uint16)
    write = next16 > region16
    if not np.any(write):
        return
    region16[write] = next16[write]
    region = rgba[min_y : max_y + 1, min_x : max_x + 1]
    alpha8 = np.clip((next16.astype(np.int32) - 1) >> 7, 0, 255).astype(np.uint8)
    region[..., :3][write] = color[:3]
    region[..., 3][write] = np.maximum(region[..., 3][write], alpha8[write])


def make_renderer(
    mode: int,
    flip: int,
    angle_offset_deg: float,
    phase_step: float,
    opacity_scale: float,
    phase_wrap: str,
    radius_scale: float,
):
    def _patched(self, body: bytes, color_map=None):
        records = [
            record for record in self._vector_object_records_100(body)
            if record.family_id == clip_mod.VECTOR_FAMILY_BALLOON
        ]
        if len(records) < clip_mod.BALLOON_NATIVE_POINT_FAMILY_MIN_RECORDS:
            return None
        rgba = np.zeros((self.height, self.width, 4), dtype=np.uint8)
        cover16 = np.zeros((self.height, self.width), dtype=np.uint16)
        for record in records:
            path = self._vector_object_record_point_path(body, record)
            if path is None or len(path) < 3:
                continue
            line_rgb = record.line_rgb if color_map is None else color_map(record.line_rgb)
            fill_rgb = record.fill_rgb if color_map is None else color_map(record.fill_rgb)
            alpha = int(round(255 * max(0.0, min(float(record.opacity), 1.0))))
            self._draw_polygon_rgba(rgba, path, (fill_rgb[0], fill_rgb[1], fill_rgb[2], alpha))
            style = self._brush_style_preview(record.line_style_id)
            if style is None or style.pattern_style != clip_mod.BALLOON_NATIVE_RETAINED_DAB_PATTERN_STYLE:
                self._draw_polyline_rgba(
                    rgba,
                    path + [path[0]],
                    (line_rgb[0], line_rgb[1], line_rgb[2], alpha),
                    radius=max(1.0, float(record.width)),
                )
                continue
            material_alpha = self._brush_material_resource_alpha(record.line_style_id)
            if material_alpha is None:
                continue
            points, controls = read_record_points(body, record)
            step = max(
                clip_mod.BALLOON_NATIVE_RETAINED_DAB_MIN_STEP,
                float(record.width) * max(float(style.interval_base), 0.05),
            )
            samples = native_like_samples(points, controls, step)
            if len(samples) < 2:
                continue
            color = (line_rgb[0], line_rgb[1], line_rgb[2], alpha)
            angle_offset = math.radians(angle_offset_deg)
            phase = 0.0
            phase_limit = float(material_alpha.shape[0] - 1)
            prev = None
            for sample in samples:
                cur = {
                    "x": sample["x"],
                    "y": sample["y"],
                    "angle": sample["angle"] + angle_offset,
                    "phase": phase,
                }
                phase += phase_step
                if phase_wrap == "mod":
                    phase %= max(phase_limit, 1.0)
                else:
                    phase = min(phase_limit, phase)
                if prev is not None:
                    draw_quad(
                        rgba,
                        cover16,
                        prev,
                        cur,
                        max(0.25, float(record.width) * radius_scale),
                        material_alpha,
                        color,
                        mode,
                        flip,
                        opacity_scale,
                    )
                prev = cur
        return rgba if rgba[..., 3].any() else None

    return _patched


def render_variant(
    mode: int,
    flip: int,
    angle_offset_deg: float,
    phase_step: float,
    opacity_scale: float,
    phase_wrap: str,
    radius_scale: float,
) -> dict:
    clip_mod.ClipFile._balloon_native_point_family_image = make_renderer(
        mode, flip, angle_offset_deg, phase_step, opacity_scale, phase_wrap, radius_scale
    )
    clip = clip_mod.ClipFile(str(CLIP_PATH))
    try:
        out = clip.composite()
    finally:
        clip.close()
    result = {
        "mode": mode,
        "flip": flip,
        "angle_offset_deg": angle_offset_deg,
        "phase_step": phase_step,
        "opacity_scale": opacity_scale,
        "phase_wrap": phase_wrap,
        "radius_scale": radius_scale,
    }
    result.update(premul_stats(out))
    return result


def main() -> int:
    started = time.time()
    variants = []
    for mode in (0, 2):
        for flip in (0, 1):
            for angle_offset_deg in (90.0,):
                for phase_step in (18.4,):
                    for opacity_scale in (1.0,):
                        for phase_wrap in ("clamp", "mod"):
                            for radius_scale in (0.5, 1.0):
                                variants.append(render_variant(
                                    mode,
                                    flip,
                                    angle_offset_deg,
                                    phase_step,
                                    opacity_scale,
                                    phase_wrap,
                                    radius_scale,
                                ))
    variants.sort(key=lambda item: (item["premul_mean"], item["premul_visible_px"]))
    payload = {
        "seconds": round(time.time() - started, 3),
        "note": (
            "Diagnostic only. Uses native-like quadratic sample order, suppresses "
            "the first retained submit per record, then draws prev->cur material "
            "AA quads with a simple phase state."
        ),
        "best": variants[:24],
        "all": variants,
    }
    OUT_JSON.write_text(json.dumps(payload, indent=2), encoding="utf-8")
    print(json.dumps({"out": str(OUT_JSON), "best": variants[:8]}, ensure_ascii=False))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
