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
OUT_JSON = ROOT / "tmp_vector_probe" / "test_ballon_native_material_aa_probe_codex_20260605.json"


def load_module(name: str, path: Path):
    spec = importlib.util.spec_from_file_location(name, path)
    mod = importlib.util.module_from_spec(spec)
    sys.modules[name] = mod
    assert spec.loader is not None
    spec.loader.exec_module(mod)
    return mod


clip_mod = load_module("probe_clip_loader", ROOT / "clip_studio_importer" / "clip_loader.py")
verify_mod = load_module("probe_verify_one_clip", ROOT / "verify_one_clip.py")


def premul_stats(out: np.ndarray, ref: np.ndarray) -> dict[str, int | float]:
    result: dict[str, int | float] = {}
    verify_mod._add_diff_stats(result, out, REF_PATH)
    return {
        "premul_mean": result.get("premul_mean"),
        "premul_visible_px": result.get("premul_visible_px"),
        "mean": result.get("mean"),
        "visible_px": result.get("visible_px"),
    }


def sample_alpha_nearest(alpha: np.ndarray, u: np.ndarray, v: np.ndarray, clamp_u: bool, clamp_v: bool) -> np.ndarray:
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


def material_uv(mode: int, flip: int, width: int, height: int, across: np.ndarray, along: np.ndarray):
    if mode == 0:
        u = across * (width - 1)
        v = along
        clamp_u, clamp_v = False, True
    elif mode == 1:
        u = along
        v = across * (height - 1)
        clamp_u, clamp_v = True, False
    elif mode == 2:
        u = across * (width - 1)
        v = height - 1 - along
        clamp_u, clamp_v = False, True
    else:
        u = width - 1 - along
        v = across * (height - 1)
        clamp_u, clamp_v = True, False
    if flip:
        if mode in (0, 2):
            u = (width - 1) - u
        else:
            v = (height - 1) - v
    return u, v, clamp_u, clamp_v


def draw_material_segment(
    self,
    rgba: np.ndarray,
    p0: tuple[float, float],
    p1: tuple[float, float],
    color: tuple[int, int, int, int],
    radius: float,
    material_alpha: np.ndarray,
    phase0: float,
    mode: int,
    flip: int,
) -> None:
    x0, y0 = p0
    x1, y1 = p1
    dx = x1 - x0
    dy = y1 - y0
    length = float(math.hypot(dx, dy))
    if length <= 1e-6:
        return
    radius = max(float(radius), 1.0)
    inv_len = 1.0 / length
    tx = dx * inv_len
    ty = dy * inv_len
    nx = -ty
    ny = tx

    min_x = max(int(math.floor(min(x0, x1) - radius - 1.0)), 0)
    max_x = min(int(math.ceil(max(x0, x1) + radius + 1.0)), self.width - 1)
    min_y = max(int(math.floor(min(y0, y1) - radius - 1.0)), 0)
    max_y = min(int(math.ceil(max(y0, y1) + radius + 1.0)), self.height - 1)
    if max_x < min_x or max_y < min_y:
        return

    yy, xx = np.mgrid[min_y : max_y + 1, min_x : max_x + 1].astype(np.float64)
    px = xx + 0.5 - x0
    py = yy + 0.5 - y0
    along_dist = px * tx + py * ty
    across_dist = px * nx + py * ny
    inside = (along_dist >= 0.0) & (along_dist <= length) & (np.abs(across_dist) <= radius)
    if not np.any(inside):
        return

    # Native uses a row AA path with two UV streams. This probe approximates
    # that by evaluating two across-edge UVs per pixel and averaging them.
    across0 = np.clip((across_dist + radius) / (2.0 * radius), 0.0, 1.0)
    along0 = phase0 + along_dist
    h, w = material_alpha.shape
    u0, v0, clamp_u, clamp_v = material_uv(mode, flip, w, h, across0, along0)
    u1, v1, _, _ = material_uv(mode, flip, w, h, np.clip(across0 + 0.5 / max(radius, 1.0), 0.0, 1.0), along0 + 0.5)
    mask_alpha = 0.5 * (
        sample_alpha_nearest(material_alpha, u0, v0, clamp_u, clamp_v)
        + sample_alpha_nearest(material_alpha, u1, v1, clamp_u, clamp_v)
    )

    src_alpha = np.clip(np.floor(mask_alpha * float(color[3]) + 0.5), 0, 255).astype(np.uint8)
    region = rgba[min_y : max_y + 1, min_x : max_x + 1]
    write = inside & (src_alpha > region[..., 3])
    if not np.any(write):
        return
    region[..., :3][write] = color[:3]
    region[..., 3][write] = src_alpha[write]


def make_native_material_balloon(mode: int, flip: int, step_scale: float, phase_ratio: float):
    def _patched(self, body: bytes, color_map=None):
        records = [
            record for record in self._vector_object_records_100(body)
            if record.family_id == clip_mod.VECTOR_FAMILY_BALLOON
        ]
        if len(records) < clip_mod.BALLOON_NATIVE_POINT_FAMILY_MIN_RECORDS:
            return None
        rgba = np.zeros((self.height, self.width, 4), dtype=np.uint8)
        for record in records:
            path = self._vector_object_record_point_path(body, record)
            if path is None or len(path) < 3:
                continue
            line_rgb = record.line_rgb if color_map is None else color_map(record.line_rgb)
            fill_rgb = record.fill_rgb if color_map is None else color_map(record.fill_rgb)
            alpha_u8 = int(round(255 * max(0.0, min(float(record.opacity), 1.0))))
            self._draw_polygon_rgba(rgba, path, (fill_rgb[0], fill_rgb[1], fill_rgb[2], alpha_u8))

            style = self._brush_style_preview(record.line_style_id)
            if style is None or style.pattern_style != clip_mod.BALLOON_NATIVE_RETAINED_DAB_PATTERN_STYLE:
                self._draw_polyline_rgba(
                    rgba,
                    path + [path[0]],
                    (line_rgb[0], line_rgb[1], line_rgb[2], alpha_u8),
                    radius=max(1.0, float(record.width)),
                )
                continue

            material_alpha = self._brush_material_resource_alpha(record.line_style_id)
            if material_alpha is None:
                continue
            radius = max(1.0, float(record.width))
            step = max(
                clip_mod.BALLOON_NATIVE_RETAINED_DAB_MIN_STEP,
                radius * max(float(style.interval_base), 0.05) * float(step_scale),
            )
            points = self._closed_path_resample_points_by_distance(path, step)
            if len(points) < 2:
                continue
            outline = (line_rgb[0], line_rgb[1], line_rgb[2], alpha_u8)
            phase = float(material_alpha.shape[0]) * float(phase_ratio)
            for p0, p1 in zip(points, points[1:] + [points[0]]):
                draw_material_segment(self, rgba, p0, p1, outline, radius, material_alpha, phase, mode, flip)
        return rgba if rgba[..., 3].any() else None

    return _patched


def render_variant(mode: int, flip: int, step_scale: float, phase_ratio: float) -> dict:
    clip_mod.ClipFile._balloon_native_point_family_image = make_native_material_balloon(
        mode, flip, step_scale, phase_ratio
    )
    clip = clip_mod.ClipFile(str(CLIP_PATH))
    try:
        out = clip.composite()
    finally:
        clip.close()
    ref = np.array(Image.open(REF_PATH).convert("RGBA"))
    result = {
        "mode": mode,
        "flip": flip,
        "step_scale": step_scale,
        "phase_ratio": phase_ratio,
    }
    result.update(premul_stats(out, ref))
    return result


def main() -> int:
    started = time.time()
    baseline = render_variant(0, 0, 999.0, 0.0)
    variants = []
    for step_scale in (0.75, 1.0, 1.25):
        for phase_ratio in (0.0, 0.25, 0.5, 0.75):
            for mode in range(4):
                for flip in (0, 1):
                    variants.append(render_variant(mode, flip, step_scale, phase_ratio))
    variants.sort(key=lambda item: (item["premul_mean"], item["premul_visible_px"]))
    payload = {
        "seconds": round(time.time() - started, 3),
        "note": "No-edit monkeypatch probe for Test_Ballon PatternStyle=10 retained/material outline.",
        "baseline_placeholder": baseline,
        "best": variants[:20],
        "all": variants,
    }
    OUT_JSON.write_text(json.dumps(payload, indent=2), encoding="utf-8")
    print(json.dumps({"out": str(OUT_JSON), "best": variants[:5]}, ensure_ascii=False))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
