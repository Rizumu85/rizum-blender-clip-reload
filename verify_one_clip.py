from __future__ import annotations

import argparse
import importlib.util
import json
import sys
import time
from pathlib import Path

import numpy as np
from PIL import Image


def _premultiplied_diff(out: np.ndarray, ref: np.ndarray) -> tuple[int, float, int, int]:
    out_alpha = out[..., 3].astype(np.uint16)
    ref_alpha = ref[..., 3].astype(np.uint16)
    out_rgb = (out[..., :3].astype(np.uint16) * out_alpha[..., None] + 127) // 255
    ref_rgb = (ref[..., :3].astype(np.uint16) * ref_alpha[..., None] + 127) // 255
    rgb_diff = np.abs(out_rgb.astype(np.int16) - ref_rgb.astype(np.int16))
    alpha_diff = np.abs(out_alpha.astype(np.int16) - ref_alpha.astype(np.int16))
    per_pixel = np.maximum(rgb_diff.max(axis=-1), alpha_diff)
    total = int(rgb_diff.sum()) + int(alpha_diff.sum())
    return (
        int(per_pixel.max()),
        total / float(out.shape[0] * out.shape[1] * 4),
        int((per_pixel > 0).sum()),
        int((per_pixel > 1).sum()),
    )


def _masked_diff_stats(diff: np.ndarray, mask: np.ndarray) -> tuple[int | None, float | None, int, int]:
    count = int(mask.sum())
    if count == 0:
        return None, None, 0, 0
    per_pixel = diff.max(axis=-1)
    masked = diff[mask]
    return (
        int(masked.max()),
        float(masked.mean()),
        int((per_pixel[mask] > 0).sum()),
        int((per_pixel[mask] > 1).sum()),
    )


def _parse_ids(value: str | None) -> list[int]:
    if not value:
        return []
    return [int(part.strip()) for part in value.split(",") if part.strip()]


def _paper_rgb_u8(clip) -> list[int] | None:
    paper = clip._paper_color()
    if paper is None:
        return None
    return [int(np.clip(channel * 255.0 + 0.5, 0, 255)) for channel in paper]


def _render_selected(clip, layer_ids: list[int], filter_ids: list[int], use_paper: bool) -> np.ndarray:
    out = np.zeros((clip.height, clip.width, 4), dtype=np.float32)
    if use_paper:
        paper = clip._paper_color()
        if paper is not None:
            out[..., 0] = paper[0]
            out[..., 1] = paper[1]
            out[..., 2] = paper[2]
            out[..., 3] = 1.0

    for layer_id in layer_ids:
        layer = clip._layer_row(layer_id)
        if layer is None:
            raise ValueError(f"Layer {layer_id} not found.")
        rgba = clip.decode_layer(layer_id)
        if rgba is None:
            rgba = clip._text_cache_fallback_image(layer)
        if rgba is None:
            rgba = clip._balloon_fallback_image(layer)
        if rgba is None:
            rgba = clip._vector_stroke_fallback_image(layer)
        if rgba is None:
            raise ValueError(f"Layer {layer_id} has no supported render fallback.")
        mask = clip._layer_mask_for_composite(layer)
        alpha = clip._apply_mask_and_clip(layer, rgba, mask, None)
        clip._composite_image(out, layer, rgba, alpha)

    for filter_id in filter_ids:
        layer = clip._layer_row(filter_id)
        if layer is None:
            raise ValueError(f"Filter layer {filter_id} not found.")
        if not clip._apply_filter_layer(out, layer):
            raise ValueError(f"Filter layer {filter_id} could not be applied.")

    rgba, _ = clip._premul_to_rgba_u8(out, transparent_rgb=255)
    return rgba


def _add_diff_stats(result: dict, out: np.ndarray, ref_path: Path) -> None:
    if not ref_path.exists():
        result["ref"] = None
        return
    ref = np.array(Image.open(ref_path).convert("RGBA"))
    result["ref"] = ref_path.name
    result["ref_shape"] = list(ref.shape)
    if ref.shape != out.shape:
        return

    diff = np.abs(out.astype(np.int16) - ref.astype(np.int16))
    visible = diff.max(axis=-1) > 1
    result["max"] = int(diff.max())
    result["mean"] = round(float(diff.mean()), 6)
    result["diff_px"] = int((diff.max(axis=-1) > 0).sum())
    result["visible_px"] = int(visible.sum())
    result["visible_pct"] = round(100.0 * int(visible.sum()) / visible.size, 6)

    premul_max, premul_mean, premul_diff_px, premul_visible_px = _premultiplied_diff(out, ref)
    result["premul_max"] = premul_max
    result["premul_mean"] = round(premul_mean, 6)
    result["premul_diff_px"] = premul_diff_px
    result["premul_visible_px"] = premul_visible_px
    result["premul_visible_pct"] = round(
        100.0 * premul_visible_px / visible.size,
        6,
    )

    alpha_union = (out[..., 3] > 0) | (ref[..., 3] > 0)
    au_max, au_mean, au_diff_px, au_visible_px = _masked_diff_stats(diff, alpha_union)
    result["alpha_union_px"] = int(alpha_union.sum())
    result["alpha_union_max"] = au_max
    result["alpha_union_mean"] = None if au_mean is None else round(au_mean, 6)
    result["alpha_union_diff_px"] = au_diff_px
    result["alpha_union_visible_px"] = au_visible_px

    high_alpha = (out[..., 3] > 128) | (ref[..., 3] > 128)
    ha_max, ha_mean, ha_diff_px, ha_visible_px = _masked_diff_stats(diff, high_alpha)
    result["high_alpha_px"] = int(high_alpha.sum())
    result["high_alpha_max"] = ha_max
    result["high_alpha_mean"] = None if ha_mean is None else round(ha_mean, 6)
    result["high_alpha_diff_px"] = ha_diff_px
    result["high_alpha_visible_px"] = ha_visible_px

    transparent = (out[..., 3] == 0) & (ref[..., 3] == 0)
    transparent_rgb = diff[..., :3].max(axis=-1)
    result["transparent_px"] = int(transparent.sum())
    result["transparent_rgb_diff_px"] = int(((transparent_rgb > 0) & transparent).sum())
    result["transparent_rgb_visible_px"] = int(((transparent_rgb > 1) & transparent).sum())


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("clip")
    parser.add_argument("--layers", help="Comma-separated layer ids to render over paper.")
    parser.add_argument("--filters", help="Comma-separated filter layer ids to apply after layers.")
    parser.add_argument("--ref", help="Reference PNG path. Defaults to clip_path.with_suffix('.png').")
    parser.add_argument("--no-paper", action="store_true", help="Do not initialize selected renders with the paper color.")
    args = parser.parse_args()

    root = Path(__file__).resolve().parent
    clip_path = Path(args.clip)
    mod_path = root / "clip_studio_importer" / "clip_loader.py"
    spec = importlib.util.spec_from_file_location("pkg_clip_loader", mod_path)
    mod = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = mod
    assert spec.loader is not None
    spec.loader.exec_module(mod)

    started = time.time()
    clip = mod.ClipFile(str(clip_path))
    try:
        paper_rgb = _paper_rgb_u8(clip)
        layer_ids = _parse_ids(args.layers)
        filter_ids = _parse_ids(args.filters)
        if layer_ids or filter_ids:
            out = _render_selected(clip, layer_ids, filter_ids, not args.no_paper)
            render_mode = "selected"
        else:
            out = clip.composite()
            render_mode = "full"
    finally:
        clip.close()

    result = {
        "name": clip_path.name,
        "rendered": True,
        "mode": render_mode,
        "paper_rgb": paper_rgb,
        "shape": list(out.shape),
        "seconds": round(time.time() - started, 3),
    }
    if render_mode == "selected":
        result["layers"] = layer_ids
        result["filters"] = filter_ids
        result["use_paper"] = not args.no_paper

    ref_path = Path(args.ref) if args.ref else clip_path.with_suffix(".png")
    _add_diff_stats(result, out, ref_path)

    print(json.dumps(result, ensure_ascii=False))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
