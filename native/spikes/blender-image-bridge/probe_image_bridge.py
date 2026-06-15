from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import sys
import time

import bpy
import numpy as np
import OpenImageIO as oiio


def _repo_root() -> Path:
    return Path(__file__).resolve().parents[3]


def _script_args() -> list[str]:
    if "--" not in sys.argv:
        return []
    return sys.argv[sys.argv.index("--") + 1 :]


def _parse_args() -> argparse.Namespace:
    root = _repo_root()
    parser = argparse.ArgumentParser(
        description="Probe native RGBA bytes -> Blender generated Image upload."
    )
    parser.add_argument(
        "--plugin-dir",
        type=Path,
        default=root / "native" / "oiio" / "build-blender50",
        help="Directory containing clip.imageio.dll.",
    )
    parser.add_argument(
        "--clip",
        type=Path,
        default=root / "img" / "Test_Clipping.clip",
        help="Input .clip path for the Rust-backed OIIO adapter.",
    )
    parser.add_argument(
        "--bench-sizes",
        default="512,1024,2048",
        help="Comma-separated synthetic RGBA upload sizes; use empty string to skip.",
    )
    return parser.parse_args(_script_args())


def _configure_oiio(plugin_dir: Path) -> None:
    plugin_dir = plugin_dir.resolve()
    os.environ["OPENIMAGEIO_PLUGIN_PATH"] = str(plugin_dir)
    oiio.attribute("plugin_searchpath", str(plugin_dir))


def _read_rgba_u8(path: Path) -> np.ndarray:
    image_input = oiio.ImageInput.open(str(path.resolve()))
    if image_input is None:
        raise RuntimeError(f"OpenImageIO could not open {path}: {oiio.geterror()}")

    try:
        spec = image_input.spec()
        pixels = image_input.read_image(oiio.UINT8)
    finally:
        image_input.close()

    if pixels is None:
        raise RuntimeError(f"OpenImageIO could not read {path}: {oiio.geterror()}")

    rgba = np.asarray(pixels, dtype=np.uint8)
    if rgba.ndim == 1:
        rgba = rgba.reshape((spec.height, spec.width, spec.nchannels))
    if rgba.shape[-1] < 4:
        alpha = np.full(rgba.shape[:2] + (1,), 255, dtype=np.uint8)
        rgba = np.concatenate((rgba[..., :3], alpha), axis=2)
    elif rgba.shape[-1] > 4:
        rgba = rgba[..., :4]
    return np.ascontiguousarray(rgba)


def _upload_rgba_image(name: str, rgba: np.ndarray, source: str) -> dict[str, float | int | str]:
    height, width, channels = rgba.shape
    if channels != 4:
        raise ValueError(f"expected RGBA pixels, got shape {rgba.shape}")

    t0 = time.perf_counter()
    image = bpy.data.images.new(name, width=width, height=height, alpha=True, float_buffer=False)
    image.source = "GENERATED"
    image["clip_source"] = source
    image["native_bridge"] = "rgba-u8-generated-image"
    t1 = time.perf_counter()

    floats = (rgba.astype(np.float32) / np.float32(255.0)).ravel()
    t2 = time.perf_counter()
    image.pixels.foreach_set(floats)
    image.update()
    t3 = time.perf_counter()

    first = [round(v, 6) for v in image.pixels[:4]]
    return {
        "name": image.name,
        "width": width,
        "height": height,
        "first_pixel_float": first,
        "create_ms": round((t1 - t0) * 1000.0, 3),
        "convert_ms": round((t2 - t1) * 1000.0, 3),
        "upload_ms": round((t3 - t2) * 1000.0, 3),
    }


def _synthetic_rgba(size: int) -> np.ndarray:
    coords = np.arange(size, dtype=np.uint16)
    rgba = np.empty((size, size, 4), dtype=np.uint8)
    rgba[..., 0] = (coords[None, :] & 0xFF).astype(np.uint8)
    rgba[..., 1] = (coords[:, None] & 0xFF).astype(np.uint8)
    rgba[..., 2] = 192
    rgba[..., 3] = 255
    return rgba


def _parse_bench_sizes(value: str) -> list[int]:
    if not value.strip():
        return []
    sizes: list[int] = []
    for raw in value.split(","):
        raw = raw.strip()
        if raw:
            sizes.append(int(raw))
    return sizes


def main() -> None:
    args = _parse_args()
    _configure_oiio(args.plugin_dir)

    clip_rgba = _read_rgba_u8(args.clip)
    result: dict[str, object] = {
        "oiio_version": oiio.VERSION_STRING,
        "plugin_dir": str(args.plugin_dir.resolve()),
        "clip": str(args.clip.resolve()),
        "clip_upload": _upload_rgba_image(
            "clip_bridge_probe",
            clip_rgba,
            str(args.clip.resolve()),
        ),
        "benchmarks": [],
    }

    for size in _parse_bench_sizes(args.bench_sizes):
        rgba = _synthetic_rgba(size)
        bench = _upload_rgba_image(
            f"clip_bridge_bench_{size}",
            rgba,
            f"synthetic:{size}x{size}",
        )
        result["benchmarks"].append(bench)
        bpy.data.images.remove(bpy.data.images[bench["name"]])

    print("BRIDGE_RESULT " + json.dumps(result, sort_keys=True))


if __name__ == "__main__":
    main()
