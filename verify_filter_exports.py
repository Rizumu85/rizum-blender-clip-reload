from __future__ import annotations

import argparse
import importlib.util
import json
import sys
import time
from pathlib import Path

import numpy as np
from PIL import Image


FILTER_REF_SUFFIX = {
    1: "Brughtbesscibtrast",
    2: "LevelCorrect",
    3: "Tonecurve",
    4: "huesaturation",
    5: "Colorbalance",
    6: "reversegradient",
    7: "posterization",
    8: "threshold",
    9: "gradientmap",
}


def _load_loader(root: Path):
    mod_path = root / "clip_loader.py"
    spec = importlib.util.spec_from_file_location("clip_loader_reference", mod_path)
    mod = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = mod
    assert spec.loader is not None
    spec.loader.exec_module(mod)
    return mod


def _diff_stats(out: np.ndarray, ref: np.ndarray) -> dict:
    diff = np.abs(out.astype(np.int16) - ref.astype(np.int16))
    per_pixel = diff.max(axis=-1)
    return {
        "max": int(diff.max()),
        "mean": round(float(diff.mean()), 6),
        "diff_px": int((per_pixel > 0).sum()),
        "visible_px": int((per_pixel > 1).sum()),
        "visible_pct": round(100.0 * int((per_pixel > 1).sum()) / per_pixel.size, 6),
    }


def _render_stack(clip, layer_ids: list[int], filter_id: int) -> np.ndarray:
    out = np.zeros((clip.height, clip.width, 4), dtype=np.float32)
    paper = clip._paper_color()
    if paper is not None:
        out[..., 0] = paper[0]
        out[..., 1] = paper[1]
        out[..., 2] = paper[2]
        out[..., 3] = 1.0

    for layer_id in layer_ids:
        layer = clip._layer_row(layer_id)
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

    layer = clip._layer_row(filter_id)
    if not clip._apply_filter_layer(out, layer):
        raise ValueError(f"Filter layer {filter_id} could not be applied.")
    rgba, _ = clip._premul_to_rgba_u8(out, transparent_rgb=255)
    return rgba


def _auto_base_layers(clip, mod) -> list[int]:
    """Find visible raster layers below the first filter in the root chain."""
    root = clip._layer_row(clip.root_layer_id)
    if root is None:
        return []
    ids: list[int] = []
    for layer_id in clip._walk_chain(root["LayerFirstChildIndex"]):
        layer = clip._layer_row(layer_id)
        if layer is None or not mod._layer_is_visible(layer):
            continue
        if layer["LayerType"] == mod.LAYER_TYPE_FILTER:
            break
        if layer["LayerType"] in mod.RASTER_LAYER_TYPES:
            ids.append(int(layer_id))
    return ids


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("clip")
    parser.add_argument(
        "base_layer_ids",
        nargs="?",
        help="Optional comma-separated base layer ids. Defaults to visible raster layers below the first filter.",
    )
    parser.add_argument(
        "--base-layers",
        dest="base_layers_flag",
        help="Comma-separated base layer ids; overrides the positional form.",
    )
    parser.add_argument("--max-max", type=int, help="Fail if any filter diff max exceeds this value.")
    parser.add_argument("--max-mean", type=float, help="Fail if any filter diff mean exceeds this value.")
    parser.add_argument("--max-visible-px", type=int, help="Fail if any filter visible pixel count exceeds this value.")
    args = parser.parse_args()
    strict = (
        args.max_max is not None
        or args.max_mean is not None
        or args.max_visible_px is not None
    )

    root = Path(__file__).resolve().parent
    clip_path = Path(args.clip)

    mod = _load_loader(root)
    started = time.time()
    clip = mod.ClipFile(str(clip_path))
    try:
        base_arg = args.base_layers_flag or args.base_layer_ids
        if base_arg:
            base_layer_ids = [int(part) for part in base_arg.split(",") if part.strip()]
            base_source = "manual"
        else:
            base_layer_ids = _auto_base_layers(clip, mod)
            base_source = "auto"
        if not base_layer_ids:
            raise ValueError("No base raster layers found. Pass --base-layers explicitly.")

        paper = clip._paper_color()
        paper_rgb = None if paper is None else [int(np.clip(c * 255.0 + 0.5, 0, 255)) for c in paper]
        rows = []
        failures = []
        warnings = []
        for layer in clip._db.execute("SELECT * FROM Layer WHERE LayerType=? ORDER BY MainId", (mod.LAYER_TYPE_FILTER,)):
            info = mod._filter_info(layer)
            if info is None:
                continue
            filter_type, _payload = info
            suffix = FILTER_REF_SUFFIX.get(filter_type)
            ref_path = clip_path.with_name(f"{clip_path.stem}_{suffix}.png") if suffix else None
            row = {
                "layer_id": int(layer["MainId"]),
                "layer_name": layer["LayerName"],
                "filter_type": filter_type,
                "ref": None if ref_path is None else ref_path.name,
            }
            if ref_path is not None and ref_path.exists():
                out = _render_stack(clip, base_layer_ids, int(layer["MainId"]))
                ref = np.array(Image.open(ref_path).convert("RGBA"))
                if ref.shape == out.shape:
                    row.update(_diff_stats(out, ref))
                    if args.max_max is not None and row["max"] > args.max_max:
                        failures.append(f"{layer['LayerName']}: max {row['max']} > {args.max_max}")
                    if args.max_mean is not None and row["mean"] > args.max_mean:
                        failures.append(f"{layer['LayerName']}: mean {row['mean']} > {args.max_mean}")
                    if args.max_visible_px is not None and row["visible_px"] > args.max_visible_px:
                        failures.append(
                            f"{layer['LayerName']}: visible_px {row['visible_px']} > {args.max_visible_px}"
                        )
                else:
                    row["error"] = f"shape mismatch: output={list(out.shape)} ref={list(ref.shape)}"
                    message = f"{layer['LayerName']}: {row['error']}"
                    if strict:
                        failures.append(message)
                    else:
                        warnings.append(message)
            else:
                row["error"] = "reference PNG missing"
                message = f"{layer['LayerName']}: {row['error']}"
                if strict:
                    failures.append(message)
                else:
                    warnings.append(message)
            rows.append(row)
    finally:
        clip.close()

    result = {
        "name": clip_path.name,
        "base_layers": base_layer_ids,
        "base_source": base_source,
        "paper_rgb": paper_rgb,
        "seconds": round(time.time() - started, 3),
        "strict": strict,
        "failed": bool(failures),
        "failures": failures,
        "warnings": warnings,
        "filters": rows,
    }
    print(json.dumps(result, ensure_ascii=False, indent=2))
    return 1 if failures else 0


if __name__ == "__main__":
    raise SystemExit(main())
