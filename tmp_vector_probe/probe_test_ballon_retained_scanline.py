from __future__ import annotations

import importlib.util
import json
import math
import struct
import sys
import time
from dataclasses import dataclass
from pathlib import Path

import numpy as np


ROOT = Path(__file__).resolve().parents[1]
CLIP_PATH = ROOT / "img" / "Test_Ballon.clip"
REF_PATH = ROOT / "img" / "Test_Ballon.png"
OUT_JSON = ROOT / "tmp_vector_probe" / "test_ballon_retained_scanline_probe_codex_20260605.json"


def load_module(name: str, path: Path):
    spec = importlib.util.spec_from_file_location(name, path)
    mod = importlib.util.module_from_spec(spec)
    sys.modules[name] = mod
    assert spec.loader is not None
    spec.loader.exec_module(mod)
    return mod


clip_mod = load_module("probe_clip_loader_retained_scanline", ROOT / "clip_studio_importer" / "clip_loader.py")
verify_mod = load_module("probe_verify_retained_scanline", ROOT / "verify_one_clip.py")


@dataclass(frozen=True)
class Vertex:
    x: float
    y: float
    u: float
    v: float


@dataclass(frozen=True)
class EdgeHit:
    x: float
    u: float
    v: float


@dataclass(frozen=True)
class Edge:
    y0: float
    y1: float
    x0: float
    u0: float
    v0: float
    dxdy: float
    dudy: float
    dvdy: float

    def hit(self, y: float) -> EdgeHit:
        t = y - self.y0
        return EdgeHit(
            self.x0 + self.dxdy * t,
            self.u0 + self.dudy * t,
            self.v0 + self.dvdy * t,
        )


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


def side_points(p: tuple[float, float], radius: float, angle: float):
    ca, sa = math.cos(angle), math.sin(angle)
    return (
        (p[0] - ca * radius, p[1] + sa * radius),
        (p[0] + ca * radius, p[1] - sa * radius),
    )


def uv_vertices(
    a0: tuple[float, float],
    b0: tuple[float, float],
    b1: tuple[float, float],
    a1: tuple[float, float],
    material_alpha: np.ndarray,
    phase0: float,
    phase1: float,
    mode: int,
    flip: int,
) -> tuple[list[Vertex], bool, bool]:
    h, w = material_alpha.shape
    phase0 = float(phase0)
    phase1 = float(phase1)
    if mode == 0:
        u_left, u_right = 0.0, float(w - 1)
        if flip:
            u_left, u_right = u_right, u_left
        verts = [
            Vertex(a0[0], a0[1], u_left, phase0),
            Vertex(b0[0], b0[1], u_right, phase0),
            Vertex(b1[0], b1[1], u_right, phase1),
            Vertex(a1[0], a1[1], u_left, phase1),
        ]
        return verts, False, True
    if mode == 2:
        u_left, u_right = 0.0, float(w - 1)
        if flip:
            u_left, u_right = u_right, u_left
        verts = [
            Vertex(a0[0], a0[1], u_left, float(h - 1) - phase0),
            Vertex(b0[0], b0[1], u_right, float(h - 1) - phase0),
            Vertex(b1[0], b1[1], u_right, float(h - 1) - phase1),
            Vertex(a1[0], a1[1], u_left, float(h - 1) - phase1),
        ]
        return verts, False, True
    if mode == 1:
        v_left, v_right = 0.0, float(h - 1)
        if flip:
            v_left, v_right = v_right, v_left
        verts = [
            Vertex(a0[0], a0[1], phase0, v_left),
            Vertex(b0[0], b0[1], phase0, v_right),
            Vertex(b1[0], b1[1], phase1, v_right),
            Vertex(a1[0], a1[1], phase1, v_left),
        ]
        return verts, True, False
    v_left, v_right = 0.0, float(h - 1)
    if flip:
        v_left, v_right = v_right, v_left
    verts = [
        Vertex(a0[0], a0[1], float(w - 1) - phase0, v_left),
        Vertex(b0[0], b0[1], float(w - 1) - phase0, v_right),
        Vertex(b1[0], b1[1], float(w - 1) - phase1, v_right),
        Vertex(a1[0], a1[1], float(w - 1) - phase1, v_left),
    ]
    return verts, True, False


def build_edges(verts: list[Vertex]) -> list[Edge]:
    edges: list[Edge] = []
    for va, vb in zip(verts, verts[1:] + verts[:1]):
        dy = vb.y - va.y
        if abs(dy) <= 1e-8:
            continue
        if va.y <= vb.y:
            top, bottom = va, vb
        else:
            top, bottom = vb, va
            dy = bottom.y - top.y
        inv = 1.0 / dy
        edges.append(Edge(
            top.y,
            bottom.y,
            top.x,
            top.u,
            top.v,
            (bottom.x - top.x) * inv,
            (bottom.u - top.u) * inv,
            (bottom.v - top.v) * inv,
        ))
    return edges


def sample_material_scalar(alpha: np.ndarray, u: float, v: float, clamp_u: bool, clamp_v: bool) -> int:
    h, w = alpha.shape
    uu = math.floor(u)
    vv = math.floor(v)
    if clamp_u:
        uu = min(max(int(uu), 0), w - 1)
    else:
        uu = int(uu) % w
    if clamp_v:
        vv = min(max(int(vv), 0), h - 1)
    else:
        vv = int(vv) % h
    return int(alpha[vv, uu])


def write_coverage(
    rgba: np.ndarray,
    cover16: np.ndarray,
    x: int,
    y: int,
    color: tuple[int, int, int, int],
    coverage_byte_sum: int,
    opacity_scale: float,
) -> None:
    if coverage_byte_sum <= 0:
        return
    cov16 = int(round((coverage_byte_sum / 1020.0) * 32768.0 * opacity_scale))
    if cov16 <= 0:
        return
    cov16 = min(cov16, 32768)
    limit16 = int(round(max(0, min(color[3], 255)) * 128.0))
    old = int(cover16[y, x])
    delta = ((limit16 - old) * cov16) >> 15
    if delta <= 0:
        return
    new = max(0, min(old + delta, 32768))
    if new <= old:
        return
    cover16[y, x] = np.uint16(new)
    alpha8 = max(0, min((new - 1) >> 7, 255))
    rgba[y, x, :3] = color[:3]
    if alpha8 > int(rgba[y, x, 3]):
        rgba[y, x, 3] = np.uint8(alpha8)


def emit_span(
    rgba: np.ndarray,
    cover16: np.ndarray,
    material_alpha: np.ndarray,
    left: EdgeHit,
    right: EdgeHit,
    y: int,
    color: tuple[int, int, int, int],
    clamp_u: bool,
    clamp_v: bool,
    opacity_scale: float,
    aa_mode: str,
) -> None:
    if right.x < left.x:
        left, right = right, left
    if right.x - left.x <= 1e-8:
        return
    x0 = max(int(math.ceil(left.x - 0.5)), 0)
    x1 = min(int(math.floor(right.x - 0.5)), rgba.shape[1] - 1)
    if x1 < x0 or y < 0 or y >= rgba.shape[0]:
        return
    inv = 1.0 / (right.x - left.x)
    for x in range(x0, x1 + 1):
        if aa_mode == "center4":
            coords = ((0.25, 0.0), (0.75, 0.0), (0.25, 0.5), (0.75, 0.5))
        elif aa_mode == "native_step":
            coords = ((0.5, 0.0), (1.0, 0.0), (0.5, 0.5), (1.0, 0.5))
        else:
            coords = ((0.5, 0.0), (0.5, 0.0), (0.5, 0.0), (0.5, 0.0))
        total = 0
        for ox, row_bias in coords:
            t = min(max(((x + ox) - left.x) * inv, 0.0), 1.0)
            u = left.u + (right.u - left.u) * t
            v = left.v + (right.v - left.v) * t + row_bias
            total += sample_material_scalar(material_alpha, u, v, clamp_u, clamp_v)
        write_coverage(rgba, cover16, x, y, color, total, opacity_scale)


def draw_scanline_quad(
    rgba: np.ndarray,
    cover16: np.ndarray,
    prev: dict[str, float],
    cur: dict[str, float],
    radius: float,
    material_alpha: np.ndarray,
    color: tuple[int, int, int, int],
    mode: int,
    flip: int,
    opacity_scale: float,
    aa_mode: str,
) -> None:
    p0 = (prev["x"], prev["y"])
    p1 = (cur["x"], cur["y"])
    a0, b0 = side_points(p0, radius, prev["angle"])
    a1, b1 = side_points(p1, radius, cur["angle"])
    verts, clamp_u, clamp_v = uv_vertices(a0, b0, b1, a1, material_alpha, prev["phase"], cur["phase"], mode, flip)
    edges = build_edges(verts)
    if len(edges) < 2:
        return
    min_y = max(int(math.floor(min(v.y for v in verts) - 1.0)), 0)
    max_y = min(int(math.ceil(max(v.y for v in verts) + 1.0)), rgba.shape[0] - 1)
    for y in range(min_y, max_y + 1):
        row_y = float(y) + 0.5
        hits = [
            edge.hit(row_y)
            for edge in edges
            if edge.y0 - 1e-8 <= row_y <= edge.y1 + 1e-8
        ]
        if len(hits) < 2:
            continue
        hits.sort(key=lambda hit: hit.x)
        collapsed: list[EdgeHit] = []
        for hit in hits:
            if collapsed and abs(hit.x - collapsed[-1].x) <= 1e-8:
                prev_hit = collapsed[-1]
                collapsed[-1] = EdgeHit(
                    (prev_hit.x + hit.x) * 0.5,
                    (prev_hit.u + hit.u) * 0.5,
                    (prev_hit.v + hit.v) * 0.5,
                )
            else:
                collapsed.append(hit)
        if len(collapsed) == 2:
            emit_span(rgba, cover16, material_alpha, collapsed[0], collapsed[1], y, color, clamp_u, clamp_v, opacity_scale, aa_mode)
        elif len(collapsed) >= 4:
            emit_span(rgba, cover16, material_alpha, collapsed[0], collapsed[1], y, color, clamp_u, clamp_v, opacity_scale, aa_mode)
            emit_span(rgba, cover16, material_alpha, collapsed[-2], collapsed[-1], y, color, clamp_u, clamp_v, opacity_scale, aa_mode)


def make_renderer(
    mode: int,
    flip: int,
    angle_offset_deg: float,
    phase_step: float,
    opacity_scale: float,
    phase_wrap: str,
    radius_scale: float,
    aa_mode: str,
    material_source: str,
    step_scale: float,
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
            if material_source == "preview_lane":
                material_alpha = self._brush_material_full_lane_alpha(record.line_style_id)
            else:
                material_alpha = self._brush_material_resource_alpha(record.line_style_id)
            if material_alpha is None:
                continue
            points, controls = read_record_points(body, record)
            step = max(
                clip_mod.BALLOON_NATIVE_RETAINED_DAB_MIN_STEP,
                float(record.width) * max(float(style.interval_base), 0.05),
            ) * max(float(step_scale), 0.05)
            samples = native_like_samples(points, controls, step)
            if len(samples) < 2:
                continue
            color = (line_rgb[0], line_rgb[1], line_rgb[2], alpha)
            phase = 0.0
            phase_limit = float((material_alpha.shape[0] if mode in (0, 2) else material_alpha.shape[1]) - 1)
            prev = None
            angle_offset = math.radians(angle_offset_deg)
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
                    draw_scanline_quad(
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
                        aa_mode,
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
    aa_mode: str,
    material_source: str,
    step_scale: float,
) -> dict:
    clip_mod.ClipFile._balloon_native_point_family_image = make_renderer(
        mode,
        flip,
        angle_offset_deg,
        phase_step,
        opacity_scale,
        phase_wrap,
        radius_scale,
        aa_mode,
        material_source,
        step_scale,
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
        "aa_mode": aa_mode,
        "material_source": material_source,
        "step_scale": step_scale,
    }
    result.update(premul_stats(out))
    return result


def main() -> int:
    started = time.time()
    variants = []
    for step_scale in (1.0, 2.0, 4.0, 8.0):
        for radius_scale in (0.5, 1.0):
            for material_source in ("resource", "preview_lane"):
                variants.append(render_variant(
                    mode=2,
                    flip=0,
                    angle_offset_deg=90.0,
                    phase_step=18.4 * step_scale,
                    opacity_scale=1.0,
                    phase_wrap="clamp",
                    radius_scale=radius_scale,
                    aa_mode="center4",
                    material_source=material_source,
                    step_scale=step_scale,
                ))
    variants.sort(key=lambda item: (item["premul_mean"], item["premul_visible_px"]))
    payload = {
        "seconds": round(time.time() - started, 3),
        "note": (
            "Diagnostic only. PatternStyle=10 retained/material Test_Ballon probe "
            "using explicit quad vertices, per-row edge intersections, UV interpolation, "
            "and native-like 16-bit coverage build-up. It does not edit importer semantics."
        ),
        "best": variants[:16],
        "all": variants,
    }
    OUT_JSON.write_text(json.dumps(payload, indent=2), encoding="utf-8")
    print(json.dumps({"out": str(OUT_JSON), "best": variants[:8]}, ensure_ascii=False))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
