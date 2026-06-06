from __future__ import annotations

import importlib.util
import json
import shutil
import sys
from pathlib import Path

import numpy as np
from PIL import Image, ImageDraw, ImageFont


ROOT = Path(__file__).resolve().parents[1]
OUT_DIR = ROOT / "tmp_vector_probe" / "rejected_visuals"
CLIP_PATH = ROOT / "img" / "Test_Ballon.clip"
REF_PATH = ROOT / "img" / "Test_Ballon.png"


def load_module(name: str, path: Path):
    spec = importlib.util.spec_from_file_location(name, path)
    mod = importlib.util.module_from_spec(spec)
    sys.modules[name] = mod
    assert spec.loader is not None
    spec.loader.exec_module(mod)
    return mod


def save_rgba(path: Path, rgba: np.ndarray) -> None:
    Image.fromarray(np.asarray(rgba, dtype=np.uint8), "RGBA").save(path)


def render_current_baseline() -> np.ndarray:
    clip_mod = load_module(
        "visual_baseline_clip_loader",
        ROOT / "clip_studio_importer" / "clip_loader.py",
    )
    clip = clip_mod.ClipFile(str(CLIP_PATH))
    try:
        return clip.composite()
    finally:
        clip.close()


def render_scanline_variant(scanline_mod, variant: dict) -> np.ndarray:
    scanline_mod.clip_mod.ClipFile._balloon_native_point_family_image = scanline_mod.make_renderer(
        mode=int(variant["mode"]),
        flip=int(variant["flip"]),
        angle_offset_deg=float(variant["angle_offset_deg"]),
        phase_step=float(variant["phase_step"]),
        opacity_scale=float(variant["opacity_scale"]),
        phase_wrap=str(variant["phase_wrap"]),
        radius_scale=float(variant["radius_scale"]),
        aa_mode=str(variant["aa_mode"]),
        material_source=str(variant["material_source"]),
        step_scale=float(variant["step_scale"]),
    )
    clip = scanline_mod.clip_mod.ClipFile(str(CLIP_PATH))
    try:
        return clip.composite()
    finally:
        clip.close()


def premul_rgba(rgba: np.ndarray) -> np.ndarray:
    arr = rgba.astype(np.float32)
    alpha = arr[..., 3:4] / 255.0
    out = arr.copy()
    out[..., :3] *= alpha
    return np.clip(out + 0.5, 0, 255).astype(np.uint8)


def make_diff_overlay(out: np.ndarray, ref: np.ndarray) -> np.ndarray:
    out_p = premul_rgba(out)
    ref_p = premul_rgba(ref)
    diff = np.abs(out_p.astype(np.int16) - ref_p.astype(np.int16))
    visible = diff.max(axis=-1) > 1
    extra = (out_p[..., 3].astype(np.int16) - ref_p[..., 3].astype(np.int16)) > 1
    missing = (ref_p[..., 3].astype(np.int16) - out_p[..., 3].astype(np.int16)) > 1
    changed = visible & ~(extra | missing)

    base = ref.copy()
    # Lighten reference so colored diagnostics dominate while shape remains visible.
    base = np.clip(base.astype(np.float32) * 0.55 + 180.0 * 0.45, 0, 255).astype(np.uint8)
    base[..., 3] = 255

    overlay = base.copy()
    overlay[changed] = np.array([255, 220, 0, 255], dtype=np.uint8)
    overlay[missing] = np.array([0, 220, 255, 255], dtype=np.uint8)
    overlay[extra] = np.array([255, 40, 40, 255], dtype=np.uint8)
    return overlay


def crop_union(images: list[np.ndarray], pad: int = 24) -> tuple[int, int, int, int]:
    masks = [(img[..., 3] > 0) for img in images]
    mask = np.logical_or.reduce(masks)
    ys, xs = np.where(mask)
    if len(xs) == 0:
        return (0, 0, images[0].shape[1], images[0].shape[0])
    x0 = max(int(xs.min()) - pad, 0)
    y0 = max(int(ys.min()) - pad, 0)
    x1 = min(int(xs.max()) + pad + 1, images[0].shape[1])
    y1 = min(int(ys.max()) + pad + 1, images[0].shape[0])
    return x0, y0, x1, y1


def add_label(img: Image.Image, label: str) -> Image.Image:
    bar_h = 34
    out = Image.new("RGBA", (img.width, img.height + bar_h), (245, 245, 245, 255))
    out.alpha_composite(img, (0, bar_h))
    draw = ImageDraw.Draw(out)
    try:
        font = ImageFont.truetype("arial.ttf", 14)
    except OSError:
        font = ImageFont.load_default()
    draw.text((8, 8), label, fill=(20, 20, 20, 255), font=font)
    return out


def make_sheet(items: list[tuple[str, np.ndarray]], crop: tuple[int, int, int, int]) -> Image.Image:
    x0, y0, x1, y1 = crop
    panels = []
    for label, arr in items:
        panel = Image.fromarray(arr[y0:y1, x0:x1], "RGBA")
        panels.append(add_label(panel, label))
    gap = 10
    w = sum(p.width for p in panels) + gap * (len(panels) - 1)
    h = max(p.height for p in panels)
    sheet = Image.new("RGBA", (w, h), (255, 255, 255, 255))
    x = 0
    for panel in panels:
        sheet.alpha_composite(panel, (x, 0))
        x += panel.width + gap
    return sheet


def main() -> int:
    OUT_DIR.mkdir(parents=True, exist_ok=True)
    shutil.copy2(REF_PATH, OUT_DIR / "00_csp_reference.png")
    ref = np.asarray(Image.open(REF_PATH).convert("RGBA"))

    baseline = render_current_baseline()
    save_rgba(OUT_DIR / "01_current_importer_baseline.png", baseline)
    save_rgba(OUT_DIR / "01_current_importer_baseline_diff_overlay.png", make_diff_overlay(baseline, ref))

    scanline_mod = load_module(
        "visual_rejected_scanline_probe",
        ROOT / "tmp_vector_probe" / "probe_test_ballon_retained_scanline.py",
    )
    variants = [
        (
            "02_rejected_scanline_best_preview_lane",
            {
                "mode": 2,
                "flip": 0,
                "angle_offset_deg": 90.0,
                "phase_step": 18.4,
                "opacity_scale": 1.0,
                "phase_wrap": "clamp",
                "radius_scale": 1.0,
                "aa_mode": "center4",
                "material_source": "preview_lane",
                "step_scale": 1.0,
            },
        ),
        (
            "03_rejected_scanline_true_resource",
            {
                "mode": 2,
                "flip": 0,
                "angle_offset_deg": 90.0,
                "phase_step": 18.4,
                "opacity_scale": 1.0,
                "phase_wrap": "clamp",
                "radius_scale": 1.0,
                "aa_mode": "center4",
                "material_source": "resource",
                "step_scale": 1.0,
            },
        ),
        (
            "04_rejected_scanline_sparse_step4",
            {
                "mode": 2,
                "flip": 0,
                "angle_offset_deg": 90.0,
                "phase_step": 73.6,
                "opacity_scale": 1.0,
                "phase_wrap": "clamp",
                "radius_scale": 1.0,
                "aa_mode": "center4",
                "material_source": "preview_lane",
                "step_scale": 4.0,
            },
        ),
    ]

    rendered: list[tuple[str, np.ndarray, np.ndarray]] = []
    for stem, variant in variants:
        out = render_scanline_variant(scanline_mod, variant)
        overlay = make_diff_overlay(out, ref)
        save_rgba(OUT_DIR / f"{stem}.png", out)
        save_rgba(OUT_DIR / f"{stem}_diff_overlay.png", overlay)
        rendered.append((stem, out, overlay))

    crop = crop_union([ref, baseline] + [item[1] for item in rendered])
    sheet = make_sheet(
        [
            ("CSP reference", ref),
            ("current importer", baseline),
            ("scanline preview diff", rendered[0][2]),
            ("scanline resource diff", rendered[1][2]),
            ("scanline step4 diff", rendered[2][2]),
        ],
        crop,
    )
    sheet.save(OUT_DIR / "contact_sheet_diff_overlays.png")

    payload = {
        "out_dir": str(OUT_DIR),
        "legend": {
            "red": "probe/importer has extra premultiplied pixels compared with CSP",
            "cyan": "probe/importer is missing pixels compared with CSP",
            "yellow": "both draw here, but color/alpha differs",
        },
        "crop": crop,
        "files": sorted(path.name for path in OUT_DIR.glob("*.png")),
    }
    (OUT_DIR / "README.json").write_text(json.dumps(payload, indent=2), encoding="utf-8")
    print(json.dumps(payload, ensure_ascii=False))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
