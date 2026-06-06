from __future__ import annotations

import importlib.util
import json
import math
import sys
import time
from pathlib import Path

import numpy as np
from PIL import Image


ROOT = Path(__file__).resolve().parents[1]
CLIP_PATH = ROOT / "img" / "Test_Ballon.clip"
REF_PATH = ROOT / "img" / "Test_Ballon.png"
OUT_JSON = ROOT / "tmp_vector_probe" / "test_ballon_retained_quad_aa4_probe_codex_20260605.json"


def load_module(name: str, path: Path):
    spec = importlib.util.spec_from_file_location(name, path)
    mod = importlib.util.module_from_spec(spec)
    sys.modules[name] = mod
    assert spec.loader is not None
    spec.loader.exec_module(mod)
    return mod


clip_mod = load_module("probe_clip_loader_quad_aa4", ROOT / "clip_studio_importer" / "clip_loader.py")
verify_mod = load_module("probe_verify_one_clip_quad_aa4", ROOT / "verify_one_clip.py")


def premul_stats(out: np.ndarray) -> dict[str, int | float]:
    result: dict[str, int | float] = {}
    verify_mod._add_diff_stats(result, out, REF_PATH)
    return {
        "premul_mean": result.get("premul_mean"),
        "premul_visible_px": result.get("premul_visible_px"),
        "mean": result.get("mean"),
        "visible_px": result.get("visible_px"),
    }


def sample_alpha(alpha: np.ndarray, u: np.ndarray, v: np.ndarray, clamp_u: bool, clamp_v: bool) -> np.ndarray:
    h, w = alpha.shape
    if clamp_u:
        uu = np.clip(np.floor(u).astype(np.int64), 0, w - 1)
    else:
        uu = np.floor(u).astype(np.int64) % w
    if clamp_v:
        vv = np.clip(np.floor(v).astype(np.int64), 0, h - 1)
    else:
        vv = np.floor(v).astype(np.int64) % h
    return alpha[vv, uu].astype(np.float32) / 255.0


def uv_from_mode(mode: int, flip: int, w: int, h: int, across: np.ndarray, along: np.ndarray):
    if mode == 0:
        u = across * (w - 1)
        v = along
        clamp_u, clamp_v = False, True
    elif mode == 1:
        u = along
        v = across * (h - 1)
        clamp_u, clamp_v = True, False
    elif mode == 2:
        u = across * (w - 1)
        v = (h - 1) - along
        clamp_u, clamp_v = False, True
    else:
        u = (w - 1) - along
        v = across * (h - 1)
        clamp_u, clamp_v = True, False
    if flip:
        if mode in (0, 2):
            u = (w - 1) - u
        else:
            v = (h - 1) - v
    return u, v, clamp_u, clamp_v


def tangent_angle(p0: tuple[float, float], p1: tuple[float, float], offset_deg: float) -> float:
    return math.atan2(p1[1] - p0[1], p1[0] - p0[0]) + math.radians(offset_deg)


def side_points(p: tuple[float, float], radius: float, angle: float) -> tuple[tuple[float, float], tuple[float, float]]:
    ca = math.cos(angle)
    sa = math.sin(angle)
    return (
        (p[0] - ca * radius, p[1] + sa * radius),
        (p[0] + ca * radius, p[1] - sa * radius),
    )


def draw_retained_quad(
    rgba: np.ndarray,
    cover16: np.ndarray,
    p0: tuple[float, float],
    p1: tuple[float, float],
    angle0: float,
    angle1: float,
    radius: float,
    material_alpha: np.ndarray,
    phase0: float,
    phase1: float,
    color: tuple[int, int, int, int],
    mode: int,
    flip: int,
    opacity_scale: float,
) -> None:
    a0, b0 = side_points(p0, radius, angle0)
    a1, b1 = side_points(p1, radius, angle1)
    xs = [a0[0], b0[0], b1[0], a1[0]]
    ys = [a0[1], b0[1], b1[1], a1[1]]
    min_x = max(int(math.floor(min(xs) - 1.0)), 0)
    max_x = min(int(math.ceil(max(xs) + 1.0)), rgba.shape[1] - 1)
    min_y = max(int(math.floor(min(ys) - 1.0)), 0)
    max_y = min(int(math.ceil(max(ys) + 1.0)), rgba.shape[0] - 1)
    if max_x < min_x or max_y < min_y:
        return

    dx = p1[0] - p0[0]
    dy = p1[1] - p0[1]
    length2 = dx * dx + dy * dy
    if length2 <= 1e-9:
        return
    length = math.sqrt(length2)
    inv_length = 1.0 / length
    tx = dx * inv_length
    ty = dy * inv_length
    nx = -ty
    ny = tx

    yy, xx = np.mgrid[min_y : max_y + 1, min_x : max_x + 1].astype(np.float64)
    # Four half-pixel tests approximate the material-AA branch that advances
    # both fixed-15 UV streams inside 0x14263E5D0.
    mask_sum = np.zeros_like(xx, dtype=np.float32)
    hit_sum = np.zeros_like(xx, dtype=np.float32)
    h, w = material_alpha.shape
    for ox, oy in ((0.25, 0.25), (0.75, 0.25), (0.25, 0.75), (0.75, 0.75)):
        px = xx + ox - p0[0]
        py = yy + oy - p0[1]
        along = (px * tx + py * ty) / max(length, 1e-9)
        across = (px * nx + py * ny) / max(radius * 2.0, 1e-9) + 0.5
        inside = (along >= 0.0) & (along <= 1.0) & (across >= 0.0) & (across <= 1.0)
        mat_along = phase0 + (phase1 - phase0) * np.clip(along, 0.0, 1.0)
        u, v, clamp_u, clamp_v = uv_from_mode(mode, flip, w, h, np.clip(across, 0.0, 1.0), mat_along)
        mask_sum += np.where(inside, sample_alpha(material_alpha, u, v, clamp_u, clamp_v), 0.0)
        hit_sum += inside.astype(np.float32)

    if not np.any(hit_sum > 0):
        return
    coverage = np.clip(mask_sum / 4.0 * opacity_scale, 0.0, 1.0)
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


def make_renderer(mode: int, flip: int, angle_offset: float, phase_step: float, opacity_scale: float):
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
            radius = max(1.0, float(record.width))
            step = max(
                clip_mod.BALLOON_NATIVE_RETAINED_DAB_MIN_STEP,
                radius * max(float(style.interval_base), 0.05),
            )
            points = self._closed_path_resample_points_by_distance(path, step)
            if len(points) < 2:
                continue
            color = (line_rgb[0], line_rgb[1], line_rgb[2], alpha)
            phase = 0.0
            prev_angle = tangent_angle(points[-1], points[0], angle_offset)
            for idx, (p0, p1) in enumerate(zip(points, points[1:] + [points[0]])):
                cur_angle = tangent_angle(p0, p1, angle_offset)
                draw_retained_quad(
                    rgba,
                    cover16,
                    p0,
                    p1,
                    prev_angle,
                    cur_angle,
                    radius,
                    material_alpha,
                    phase,
                    phase + phase_step,
                    color,
                    mode,
                    flip,
                    opacity_scale,
                )
                phase += phase_step
                prev_angle = cur_angle
        return rgba if rgba[..., 3].any() else None

    return _patched


def render_variant(mode: int, flip: int, angle_offset: float, phase_step: float, opacity_scale: float) -> dict:
    clip_mod.ClipFile._balloon_native_point_family_image = make_renderer(
        mode, flip, angle_offset, phase_step, opacity_scale
    )
    clip = clip_mod.ClipFile(str(CLIP_PATH))
    try:
        out = clip.composite()
    finally:
        clip.close()
    result = {
        "mode": mode,
        "flip": flip,
        "angle_offset": angle_offset,
        "phase_step": phase_step,
        "opacity_scale": opacity_scale,
    }
    result.update(premul_stats(out))
    return result


def main() -> int:
    started = time.time()
    variants = []
    for mode in (0, 2):
        for flip in (0, 1):
            for angle_offset in (90.0,):
                for phase_step in (18.4,):
                    for opacity_scale in (0.75, 1.0):
                        variants.append(render_variant(mode, flip, angle_offset, phase_step, opacity_scale))
    variants.sort(key=lambda item: (item["premul_mean"], item["premul_visible_px"]))
    payload = {
        "seconds": round(time.time() - started, 3),
        "note": (
            "Diagnostic only. Replaces Test_Ballon PatternStyle=10 outlines with "
            "approximate retained segment quads, true material resource, four "
            "subsamples, and native-like 16-bit build-up."
        ),
        "best": variants[:24],
        "all": variants,
    }
    OUT_JSON.write_text(json.dumps(payload, indent=2), encoding="utf-8")
    print(json.dumps({"out": str(OUT_JSON), "best": variants[:8]}, ensure_ascii=False))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
