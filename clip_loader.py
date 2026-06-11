"""
clip_loader.py 鈥?minimal Clip Studio Paint (.clip) reader and compositor.

Scope (MVP): full-color raster layers, Normal blend mode, opacity, visibility,
folder traversal. Skips and warns on non-Normal blend modes, grayscale layers,
non-zero layer offsets, vector / 3D / text layers.

Designed to run with stdlib + numpy only. No OpenCV / Pillow dependency.

Output convention: a single (H, W, 4) uint8 RGBA NumPy array, straight alpha,
matching the convention CSP uses when exporting flattened PNGs.

v0.5 optimizations:
  鈥?Lazy CHNKExta parsing 鈥?only the few blobs we actually use get zlib-decompressed
  鈥?Vectorized tile assembly (numpy reshape/transpose, no Python per-tile loop)
  鈥?save_png defaults to zlib level 1 (PNG cache; size cost ~10鈥?0%, much faster)

Verified bit-equivalent (alpha-aware) against CSP's PNG export on:
  - Illustration.clip   (512x512, 1 raster layer)
  - Illustration4K.clip (4096x4096, 3 raster layers)
"""

from __future__ import annotations

import logging
import math
import os
import sqlite3
import struct
import tempfile
import zlib
from dataclasses import dataclass
from typing import Iterable, NamedTuple, Optional

import numpy as np


log = logging.getLogger("clip_loader")

CSF_MAGIC = b"CSFCHUNK"
TILE = 256
MASK_TILE_BYTES = TILE * TILE
PER_TILE_BYTES = MASK_TILE_BYTES * 5   # 1B alpha + 4B BGRA per pixel
LAYER_TYPE_RASTER = 1
LAYER_TYPE_GROUP = 2
LAYER_TYPE_RASTER_MASKED = 3
LAYER_TYPE_LAYER_FOLDER = 0
LAYER_TYPE_FOLDER = 256
LAYER_TYPE_PAPER = 1584
LAYER_TYPE_FILTER = 4098
COMPOSITE_NORMAL = 0
RASTER_LAYER_TYPES = {LAYER_TYPE_RASTER, LAYER_TYPE_RASTER_MASKED}


class _BalloonFallbackTuning(NamedTuple):
    bbox_expand: int
    point_weight: float
    outline_extra: int
    ellipse_power: float

FRAME_LINE_FALLBACK_BLACK_ALPHA = 158
FRAME_FALLBACK_BBOX_INSET = 2
TEXT_BALLOON_FALLBACK_INSET = (3, 3, 2, 2)
TEXT_BALLOON_FALLBACK_POWER = 2.6
VECTOR_OBJECT_FALLBACK_DEFAULT_WIDTH = 2.5
VECTOR_OBJECT_FLAGS_CONTROL_POINT = 0x20
VECTOR_OBJECT_FLAGS_SPLINE = 0x80
BALLOON_NATIVE_POINT_FAMILY_MIN_RECORDS = 2
BALLOON_NATIVE_POINT_FAMILY_SAMPLES = 32
BALLOON_NATIVE_RETAINED_PATTERN_ALPHA = 0.10
BALLOON_NATIVE_RETAINED_PATTERN_FEATHER = 0.25
BALLOON_NATIVE_RETAINED_DAB_PATTERN_STYLE = 10
BALLOON_NATIVE_RETAINED_DAB_MIN_STEP = 0.25
VECTOR_NORMAL_TYPE_BALLOON = 1
VECTOR_NORMAL_TYPE_FRAME = 3
VECTOR_FAMILY_BALLOON = 0x130
VECTOR_FAMILY_FRAME = 0x410
VECTOR_STROKE_FLAGS_FILLED_CURVE = 0x41
VECTOR_STROKE_FLAGS_NATIVE_AA = 0x2011
VECTOR_STROKE_FLAGS_LEGACY = 0x2081
VECTOR_STROKE_MAX_POINTS = 2000
VECTOR_STROKE_MAX_WIDTH = 200.0
VECTOR_STROKE_RADIUS_SCALE = 0.95
VECTOR_FALLBACK_SAMPLE_STEP = 5.0
VECTOR_OPACITY_PRESSURE_SAMPLE_STEP = 2.0
VECTOR_FILLED_CURVE_DARK_RGB_THRESHOLD = 8
VECTOR_FILLED_CURVE_MAX_SOLID_WIDTH = 16.0
VECTOR_FILLED_CURVE_ELLIPSE_INSET = (9, 5, 21, 5)
VECTOR_FILLED_CURVE_ELLIPSE_POWER = 3.6
VECTOR_FILLED_CURVE_RADIUS_SCALE = 0.60
VECTOR_FILLED_CURVE_RADIUS_SCALE_BY_AA = {
    1: 0.72,
    2: 0.62,
    3: 0.60,
}
VECTOR_FILLED_CURVE_HARD_RADIUS_SCALE = 0.95
VECTOR_FILLED_CURVE_HARD_SAMPLE_SHIFT = 0.25
VECTOR_FILLED_CURVE_AA_SAMPLE_SHIFT = -0.5
VECTOR_FILLED_CURVE_AA_SAMPLE_SCALE = 2.0
VECTOR_FILLED_CURVE_FEATHER_BASE = 0.80
VECTOR_FILLED_CURVE_FEATHER_PER_AA = 0.15
VECTOR_FILLED_CURVE_FEATHER_BY_AA = {
    1: 0.50,
    2: 1.10,
    3: 1.25,
}
VECTOR_LEGACY_AA_FEATHER = 0.20
VECTOR_NATIVE_AA_FEATHER_BY_LEVEL = {1: 0.75, 2: 1.25, 3: 1.75}
VECTOR_HARDNESS_FEATHER_SCALE = 2.3
VECTOR_HARDNESS_RADIUS_SOFTEN_SCALE = 0.092
VECTOR_INTERVAL_RADIUS_SOFTEN_MAX = 0.08
VECTOR_AUTO_INTERVAL_RADIUS_SCALE = 0.96
VECTOR_AUTO_INTERVAL_FEATHER_SCALE = 2.0
VECTOR_FLOW_RADIUS_SOFTEN_MAX = 0.12
VECTOR_FLOW_DYNAMIC_RADIUS_SOFTEN_MAX = 0.12
VECTOR_TEXTURE_DENSITY_PREVIEW_SCALE = 0.70
VECTOR_TEXTURE_PREVIEW_GAMMA = 0.50
VECTOR_TEXTURE_FLAG_0X200_DENSITY_GAIN = 10.0
VECTOR_MATERIAL_STAMP_WIDTH_SCALE = 1.20
VECTOR_MATERIAL_STAMP_GAP_WIDTH_SCALE = 0.75
VECTOR_MATERIAL_STAMP_HEIGHT_SCALE = 2.00
VECTOR_MATERIAL_STAMP_GAP_HEIGHT_SCALE = 2.00
VECTOR_MATERIAL_STAMP_ALPHA = 0.40
VECTOR_MATERIAL_STAMP_GAP_ALPHA = 1.00
VECTOR_MATERIAL_STAMP_GAP_ANCHOR_Y = 0.50
VECTOR_MATERIAL_STAMP_MIN_STEP = 1.0
VECTOR_MATERIAL_NATIVE_MIP_SCALE = 4.0
VECTOR_SIMPLE_SIZE_RADIUS_SCALE = 0.75
VECTOR_SIMPLE_SIZE_AA_FEATHER = 0.60
VECTOR_PRESSURE_SIZE_RADIUS_SCALE = 1.05
VECTOR_PRESSURE_OPACITY_RADIUS_SCALE = 0.99
VECTOR_EXPERIMENTAL_DAB_ENV = "RIZUM_CLIP_EXPERIMENTAL_VECTOR_DAB"
VECTOR_EXPERIMENTAL_NATIVE_RANDOM_OPACITY_ENV = "RIZUM_CLIP_EXPERIMENTAL_VECTOR_NATIVE_RANDOM_OPACITY"
VECTOR_EXPERIMENTAL_ADAPTIVE_SPACING_ENV = "RIZUM_CLIP_EXPERIMENTAL_VECTOR_ADAPTIVE_SPACING"
VECTOR_ADAPTIVE_SPACING_MIN_STEP = 0.5
BALLOON_FALLBACK_BODY_BBOX_INSET = (2, 3, 2, 2)
BALLOON_FALLBACK_POINT_BBOX_EXPAND = 2
BALLOON_FALLBACK_DEFAULT_TUNING = _BalloonFallbackTuning(
    bbox_expand=0,
    point_weight=0.55,
    outline_extra=1,
    ellipse_power=2.0,
)
BALLOON_FALLBACK_TUNING_BY_INDEX = {
    2: _BalloonFallbackTuning(bbox_expand=1, point_weight=0.55, outline_extra=-1, ellipse_power=2.2),
    3: _BalloonFallbackTuning(bbox_expand=3, point_weight=0.45, outline_extra=2, ellipse_power=2.2),
}


class _VectorObjectHeader(NamedTuple):
    bbox: tuple[int, int, int, int]
    line_rgb: tuple[int, int, int]
    fill_rgb: tuple[int, int, int]
    opacity: float
    width: float
    line_style_id: int
    fill_style_id: int
    family_id: int
    extra_id: int
    point_bbox: Optional[tuple[int, int, int, int]]


class _VectorObjectRecord(NamedTuple):
    off: int
    header_len: int
    point_stride: int
    point_tail_offset: int
    point_count: int
    object_flags: int
    line_rgb: tuple[int, int, int]
    fill_rgb: tuple[int, int, int]
    opacity: float
    width: float
    line_style_id: int
    fill_style_id: int
    family_id: int
    extra_id: int


@dataclass(frozen=True)
class _BrushStylePreview:
    style_flag: int = 0
    pattern_style: int = 0
    texture_pattern: int = 0
    texture_flag: int = 0
    flow_base: float = 1.0
    hardness: float = 1.0
    interval_base: float = 1.0
    auto_interval_type: int = 0
    thickness_base: float = 1.0
    rotation_base: float = 0.0
    spray_flag: int = 0
    texture_composite: int = 0
    texture_density_base: float = 1.0
    texture_scale: float = 1.0
    texture_rotate: float = 0.0
    texture_offset_x: float = 0.0
    texture_offset_y: float = 0.0
    texture_brightness: float = 0.0
    texture_contrast: float = 0.0

    @property
    def direct_max_accum(self) -> bool:
        return bool(self.style_flag & 0x1000)

    @property
    def retained_state(self) -> bool:
        return bool(self.style_flag & 0x20)


# --------------------------------------------------------------------------- #
# Chunk container
# --------------------------------------------------------------------------- #

@dataclass
class _Chunk:
    type: bytes
    body: bytes


def _walk_chunks(data: bytes) -> Iterable[_Chunk]:
    if data[:8] != CSF_MAGIC:
        raise ValueError("Not a CLIP file (missing CSFCHUNK magic).")
    pos = 8 + 16
    while pos < len(data):
        ctype = data[pos:pos + 8]
        csize = struct.unpack_from(">Q", data, pos + 8)[0]
        body = data[pos + 16 : pos + 16 + csize]
        yield _Chunk(ctype, body)
        pos += 16 + csize


def _read_exta_id(body: bytes) -> str:
    """Read just the external_id from an Exta body, without parsing blocks."""
    L = struct.unpack_from(">Q", body, 0)[0]
    return body[8:8 + L].decode("ascii")


def _split_clip(path: str):
    """Walk the .clip file once. Index every CHNKExta by external_id but DO NOT
    decompress its blocks yet 鈥?that happens lazily in `_parse_exta`.

    Returns (ext_to_body: dict[str, bytes], sqlite_bytes: bytes).
    """
    with open(path, "rb") as f:
        data = f.read()

    sqlite_bytes = None
    ext_to_body: dict[str, bytes] = {}
    for chunk in _walk_chunks(data):
        if chunk.type == b"CHNKSQLi":
            sqlite_bytes = chunk.body
        elif chunk.type == b"CHNKExta":
            try:
                ext_id = _read_exta_id(chunk.body)
            except (struct.error, UnicodeDecodeError) as exc:
                log.warning("Skipping unreadable Exta header: %s", exc)
                continue
            ext_to_body[ext_id] = chunk.body
    if sqlite_bytes is None:
        raise ValueError("CLIP file has no CHNKSQLi chunk.")
    return ext_to_body, sqlite_bytes


def _parse_exta(
    body: bytes,
    empty_fill: int = 0,
    expected_len: Optional[int] = None,
) -> tuple[str, bytes]:
    """Decompress the blocks of an Exta body. Returns (external_id, tile_blob).

    Two block-name framings exist:
      Case A (no payload): size_01 = name_len, size_02 = "Bl" UTF-16BE marker
        (0x0042006C). Rewind 4 so size_02 becomes the first 4 name bytes.
      Case B (with payload): size_01 = outer_payload_size, size_02 = name_len.

    For BlockDataBeginChunk / BlockStatus / BlockCheckSum we derive block_end
    from inner fields, matching csp_tool's verified-correct behaviour.
    """
    pos = 0
    L = struct.unpack_from(">Q", body, pos)[0]
    pos += 8
    ext_id = body[pos:pos + L].decode("ascii")
    pos += L
    pos += 8  # external data size 鈥?informational

    if expected_len is not None:
        if empty_fill:
            out = bytearray([empty_fill]) * expected_len
        else:
            out = bytearray(expected_len)
        out_len = 0
    else:
        out = bytearray()
        out_len = 0
    BL_MARKER = 0x0042006C  # "Bl" in UTF-16BE 鈥?prefix of every "Block*" name

    while pos < len(body):
        if pos + 8 > len(body):
            break
        block_start = pos
        s1 = struct.unpack_from(">L", body, pos)[0]; pos += 4
        s2 = struct.unpack_from(">L", body, pos)[0]; pos += 4

        if s2 == BL_MARKER:
            name_len = s1
            outer_payload = 0
            pos = block_start + 4
        else:
            name_len = s2
            outer_payload = s1

        if name_len == 0 or name_len >= 256:
            log.warning("Bad name_len=%d at exta offset %d (ext_id=%s)",
                        name_len, block_start, ext_id)
            break

        if pos + name_len * 2 > len(body):
            log.warning("Block name truncated at offset %d", block_start)
            break
        name = body[pos:pos + name_len * 2].decode("utf-16-be")
        pos += name_len * 2

        inner_start = pos
        block_end = inner_start + outer_payload

        if name == "BlockDataBeginChunk":
            pos += 4
            uncomp = struct.unpack_from(">L", body, pos)[0]; pos += 4
            pos += 4
            pos += 4
            exist = struct.unpack_from(">L", body, pos)[0]; pos += 4
            if exist > 0:
                inner_len = struct.unpack_from(">L", body, pos)[0]; pos += 4
                tail_len = struct.unpack_from("<L", body, pos)[0]; pos += 4
                zdata = body[pos:pos + tail_len]
                decoded = zlib.decompress(zdata)
                if len(decoded) != uncomp:
                    log.warning("Tile uncompressed size mismatch: %d vs %d",
                                len(decoded), uncomp)
                if expected_len is not None:
                    write_len = min(len(decoded), expected_len - out_len)
                    if write_len > 0:
                        out[out_len:out_len + write_len] = decoded[:write_len]
                        out_len += write_len
                    if out_len >= expected_len:
                        break
                else:
                    out += decoded
                block_end = inner_start + 24 + inner_len
            else:
                if expected_len is not None:
                    out_len = min(out_len + uncomp, expected_len)
                    if out_len >= expected_len:
                        break
                else:
                    out += bytes([empty_fill]) * uncomp
                block_end = inner_start + 20
        elif name in ("BlockStatus", "BlockCheckSum"):
            block_end = inner_start + 24
            if inner_start + 12 <= len(body):
                _header_len, item_count, item_size = struct.unpack_from(">LLL", body, inner_start)
                variable_end = inner_start + 12 + item_count * item_size
                if item_count >= 0 and 0 <= item_size <= 64 and variable_end <= len(body):
                    block_end = variable_end
        elif name == "BlockDataEndChunk":
            pass
        else:
            log.warning("Unknown block name %r in Exta %s", name, ext_id)
            break

        pos = block_end
    if expected_len is not None:
        return ext_id, bytes(out[:out_len])
    return ext_id, bytes(out)


# --------------------------------------------------------------------------- #
# Tile data 鈫?RGBA image  (vectorized 鈥?no per-tile Python loop)
# --------------------------------------------------------------------------- #

def _tiles_to_rgba(tile_blob: bytes, width: int, height: int) -> np.ndarray:
    """Decode a layer's tile blob into a (H, W, 4) uint8 RGBA array.

    Tile layout (per tile, after zlib decompression):
      [TILE*TILE bytes alpha plane] [TILE*TILE*4 bytes BGRA plane]
    The standalone alpha plane is the authoritative alpha; the BGRA plane's A
    channel is ignored. Tiles are laid out row-major.
    """
    total_tiles = len(tile_blob) // PER_TILE_BYTES
    if len(tile_blob) % PER_TILE_BYTES != 0:
        raise ValueError(
            f"Tile blob size not a multiple of per-tile bytes: {len(tile_blob)}"
        )
    cols = (width + TILE - 1) // TILE
    rows = (height + TILE - 1) // TILE
    expected = cols * rows
    if total_tiles != expected:
        for try_cols in range(cols, cols + 10):
            if total_tiles % try_cols == 0:
                cols = try_cols
                rows = total_tiles // try_cols
                break
        else:
            cols = total_tiles
            rows = 1

    arr = np.frombuffer(tile_blob, dtype=np.uint8)

    # (rows, cols, alpha_bytes + bgra_bytes)
    grouped = arr.reshape(rows, cols, PER_TILE_BYTES)
    alpha_planes = grouped[:, :, : TILE * TILE].reshape(rows, cols, TILE, TILE)
    bgra_planes = grouped[:, :, TILE * TILE :].reshape(rows, cols, TILE, TILE, 4)

    # Reorder (rows, cols, TILE, TILE) 鈫?(rows, TILE, cols, TILE) 鈫?(rows*TILE, cols*TILE)
    alpha_full = alpha_planes.transpose(0, 2, 1, 3).reshape(rows * TILE, cols * TILE)
    bgra_full = bgra_planes.transpose(0, 2, 1, 3, 4).reshape(rows * TILE, cols * TILE, 4)

    # Compose RGBA: take BGR from bgra_full, swap to RGB, alpha from alpha_full.
    canvas = np.empty((rows * TILE, cols * TILE, 4), dtype=np.uint8)
    canvas[:, :, 0] = bgra_full[:, :, 2]   # R
    canvas[:, :, 1] = bgra_full[:, :, 1]   # G
    canvas[:, :, 2] = bgra_full[:, :, 0]   # B
    canvas[:, :, 3] = alpha_full
    return canvas[:height, :width].copy()



def _tiles_to_alpha(tile_blob: bytes, width: int, height: int) -> np.ndarray:
    """Decode a single-channel mask blob into a (H, W) uint8 array.

    Layout: each tile is just TILE*TILE bytes of grayscale (alpha) data, no
    BGRA plane. CSP uses this for layer masks (and grayscale layers, which we
    don't otherwise support yet).
    """
    total_tiles = len(tile_blob) // MASK_TILE_BYTES
    if len(tile_blob) % MASK_TILE_BYTES != 0:
        raise ValueError(
            f"Mask tile blob size not a multiple of per-tile bytes: {len(tile_blob)}"
        )
    cols = (width + TILE - 1) // TILE
    rows = (height + TILE - 1) // TILE
    expected = cols * rows
    if total_tiles != expected:
        for try_cols in range(cols, cols + 10):
            if total_tiles % try_cols == 0:
                cols = try_cols
                rows = total_tiles // try_cols
                break
        else:
            cols = total_tiles
            rows = 1
    arr = np.frombuffer(tile_blob, dtype=np.uint8)
    grouped = arr.reshape(rows, cols, TILE, TILE)
    full = grouped.transpose(0, 2, 1, 3).reshape(rows * TILE, cols * TILE)
    return full[:height, :width].copy()


def _tiles_to_gray_rgba(tile_blob: bytes, width: int, height: int) -> np.ndarray:
    """Decode CSP grayscale tiles into RGBA.

    Grayscale layer tiles store two 8-bit planes per tile: alpha, then gray.
    """
    per_tile = MASK_TILE_BYTES * 2
    total_tiles = len(tile_blob) // per_tile
    if len(tile_blob) % per_tile != 0:
        raise ValueError(
            f"Grayscale tile blob size not a multiple of per-tile bytes: {len(tile_blob)}"
        )
    cols = (width + TILE - 1) // TILE
    rows = (height + TILE - 1) // TILE
    expected = cols * rows
    if total_tiles != expected:
        for try_cols in range(cols, cols + 10):
            if total_tiles % try_cols == 0:
                cols = try_cols
                rows = total_tiles // try_cols
                break
        else:
            cols = total_tiles
            rows = 1

    grouped = np.frombuffer(tile_blob, dtype=np.uint8).reshape(rows, cols, per_tile)
    alpha_planes = grouped[:, :, :MASK_TILE_BYTES].reshape(rows, cols, TILE, TILE)
    gray_planes = grouped[:, :, MASK_TILE_BYTES:].reshape(rows, cols, TILE, TILE)
    alpha_full = alpha_planes.transpose(0, 2, 1, 3).reshape(rows * TILE, cols * TILE)
    gray_full = gray_planes.transpose(0, 2, 1, 3).reshape(rows * TILE, cols * TILE)

    rgba = np.empty((rows * TILE, cols * TILE, 4), dtype=np.uint8)
    rgba[..., 0] = gray_full
    rgba[..., 1] = gray_full
    rgba[..., 2] = gray_full
    rgba[..., 3] = alpha_full
    return rgba[:height, :width].copy()


def _tiles_to_mono_rgba(tile_blob: bytes, width: int, height: int) -> np.ndarray:
    """Decode CSP monochrome tiles into RGBA.

    Monochrome layer tiles store two 1-bit planes per tile: alpha, then white.
    Pixels where alpha is set and white is clear render as black.
    """
    per_tile = MASK_TILE_BYTES // 4
    total_tiles = len(tile_blob) // per_tile
    if len(tile_blob) % per_tile != 0:
        raise ValueError(
            f"Monochrome tile blob size not a multiple of per-tile bytes: {len(tile_blob)}"
        )
    cols = (width + TILE - 1) // TILE
    rows = (height + TILE - 1) // TILE
    expected = cols * rows
    if total_tiles != expected:
        for try_cols in range(cols, cols + 10):
            if total_tiles % try_cols == 0:
                cols = try_cols
                rows = total_tiles // try_cols
                break
        else:
            cols = total_tiles
            rows = 1

    grouped = np.frombuffer(tile_blob, dtype=np.uint8).reshape(rows, cols, per_tile)
    half = per_tile // 2
    alpha_bits = np.unpackbits(grouped[:, :, :half].reshape(-1), bitorder="big")
    white_bits = np.unpackbits(grouped[:, :, half:].reshape(-1), bitorder="big")
    alpha = alpha_bits.reshape(rows, cols, TILE, TILE).transpose(0, 2, 1, 3)
    white = white_bits.reshape(rows, cols, TILE, TILE).transpose(0, 2, 1, 3)
    alpha = alpha.reshape(rows * TILE, cols * TILE)[:height, :width].astype(bool)
    white = white.reshape(rows * TILE, cols * TILE)[:height, :width].astype(bool)
    white = white & alpha

    rgba = np.zeros((height, width, 4), dtype=np.uint8)
    rgba[alpha, 3] = 255
    rgba[white, :3] = 255
    return rgba


def _resize_rgba_bilinear(rgba: np.ndarray, width: int, height: int) -> np.ndarray:
    """Resize a small RGBA bitmap without adding a Pillow dependency."""
    src_h, src_w = rgba.shape[:2]
    if width <= 0 or height <= 0 or src_w <= 0 or src_h <= 0:
        return np.zeros((0, 0, 4), dtype=np.uint8)
    if src_w == width and src_h == height:
        return rgba.copy()

    xs = np.linspace(0.0, float(src_w - 1), width, dtype=np.float32)
    ys = np.linspace(0.0, float(src_h - 1), height, dtype=np.float32)
    x0 = np.floor(xs).astype(np.int32)
    y0 = np.floor(ys).astype(np.int32)
    x1 = np.minimum(x0 + 1, src_w - 1)
    y1 = np.minimum(y0 + 1, src_h - 1)
    wx = (xs - x0).reshape(1, width, 1)
    wy = (ys - y0).reshape(height, 1, 1)

    top = rgba[y0[:, None], x0[None, :]].astype(np.float32) * (1.0 - wx) + rgba[
        y0[:, None], x1[None, :]
    ].astype(np.float32) * wx
    bottom = rgba[y1[:, None], x0[None, :]].astype(np.float32) * (1.0 - wx) + rgba[
        y1[:, None], x1[None, :]
    ].astype(np.float32) * wx
    out = top * (1.0 - wy) + bottom * wy
    return np.clip(np.floor(out + 0.5), 0, 255).astype(np.uint8)


# --------------------------------------------------------------------------- #
# Blend modes
#
# CSP stores blend mode as an integer in Layer.LayerComposite. The mapping
# from integer to named mode is NOT publicly documented; we collect it
# empirically by saving test samples in CSP and observing which int comes out.
# Update _BLEND_MAPPING below as new ones are observed.
#
# Blend functions take and return STRAIGHT (non-premultiplied) RGB in [0, 1].
# --------------------------------------------------------------------------- #

# Empirical mapping: CSP integer 鈫?blend mode name. Filled in by observing
# IllustrationBlendModes.clip (one labelled layer per CSP UI mode). Modes with
# no integer assigned yet behave like NORMAL with a warning.
_BLEND_MAPPING = {
    0: "NORMAL",
    # Confirmed by IllustrationBlendModes*.clip inspection (2026-04-28/29):
    1: "DARKEN",
    2: "MULTIPLY",
    3: "COLOR_BURN",
    4: "LINEAR_BURN",
    5: "SUBTRACT",
    6: "DARKER_COLOR",
    7: "LIGHTEN",
    8: "SCREEN",
    9: "COLOR_DODGE",
    10: "GLOW_DODGE",
    11: "ADD",
    12: "ADD_GLOW",
    13: "LIGHTER_COLOR",
    14: "OVERLAY",
    15: "SOFT_LIGHT",
    16: "HARD_LIGHT",
    17: "VIVID_LIGHT",
    18: "LINEAR_LIGHT",
    19: "PIN_LIGHT",
    20: "HARD_MIX",
    21: "DIFFERENCE",
    22: "EXCLUSION",
    23: "HUE",
    24: "SATURATION",
    25: "COLOR",
    26: "BRIGHTNESS",
    30: "THROUGH",
    36: "DIVIDE",
    # Gaps (1, 4, 6, 7, 10, 12, 13, 17鈥?0, 22+) likely belong to:
}


# --- HSL helpers (Photoshop / W3C non-separable blend math) --- #

def _lum(c: np.ndarray) -> np.ndarray:
    """Photoshop luminosity: 0.3 R + 0.59 G + 0.11 B."""
    return 0.3 * c[..., 0] + 0.59 * c[..., 1] + 0.11 * c[..., 2]


def _set_lum(c: np.ndarray, l: np.ndarray) -> np.ndarray:
    """Translate c so its luminosity equals l, then clip into [0,1]."""
    diff = (l - _lum(c))[..., None]
    out = c + diff
    L = _lum(out)[..., None]
    mn = out.min(axis=-1, keepdims=True)
    mx = out.max(axis=-1, keepdims=True)
    # Clip values that fall below 0 toward L
    below = mn < 0
    out_below = L + (out - L) * (L / np.maximum(L - mn, 1e-6))
    out = np.where(below, out_below, out)
    # Clip values above 1 toward L
    above = mx > 1
    out_above = L + (out - L) * ((1.0 - L) / np.maximum(mx - L, 1e-6))
    out = np.where(above, out_above, out)
    return out


def _sat(c: np.ndarray) -> np.ndarray:
    return c.max(axis=-1) - c.min(axis=-1)


def _set_sat(c: np.ndarray, s: np.ndarray) -> np.ndarray:
    """Set saturation channel-wise; preserves min/max ordering of c."""
    out = np.zeros_like(c)
    cmax = c.max(axis=-1, keepdims=True)
    cmin = c.min(axis=-1, keepdims=True)
    span = cmax - cmin
    s_b = s[..., None]
    # For pixels where span > 0, scale relative position.
    rel = np.where(span > 0, (c - cmin) / np.maximum(span, 1e-6), 0.0)
    out = rel * s_b
    return out


def _blend_func(mode: str, s: np.ndarray, d: np.ndarray) -> np.ndarray:
    """Pure blend function: returns blended RGB given straight src/dst RGB."""
    if mode in ("HUE", "SATURATION", "BRIGHTNESS", "LUMINOSITY"):
        s = np.floor(np.clip(s, 0.0, 1.0) * 255.0 + 0.5) / 255.0
        d = np.floor(np.clip(d, 0.0, 1.0) * 255.0 + 0.5) / 255.0
    if mode == "NORMAL":
        return s
    if mode == "MULTIPLY":
        return s * d
    if mode == "SCREEN":
        return 1.0 - (1.0 - s) * (1.0 - d)
    if mode == "OVERLAY":
        return np.where(d < 0.5, 2.0 * s * d, 1.0 - 2.0 * (1.0 - s) * (1.0 - d))
    if mode == "HARD_LIGHT":
        return np.where(s < 0.5, 2.0 * s * d, 1.0 - 2.0 * (1.0 - s) * (1.0 - d))
    if mode == "SOFT_LIGHT":
        return np.where(
            s < 0.5,
            d - (1.0 - 2.0 * s) * d * (1.0 - d),
            d + (2.0 * s - 1.0) * (np.where(d < 0.25,
                                             ((16.0 * d - 12.0) * d + 4.0) * d,
                                             np.sqrt(d)) - d),
        )
    if mode == "ADD" or mode == "ADD_GLOW":
        # CSP's "Add" and "Add (Glow)" are both straight saturating addition;
        # the visual difference comes from light-source gamma which we don't
        # model here. Behaviourally they should match for opaque pixels.
        return np.minimum(s + d, 1.0)
    if mode == "SUBTRACT":
        return np.maximum(d - s, 0.0)
    if mode == "DIFFERENCE":
        return np.abs(s - d)
    if mode == "EXCLUSION":
        return s + d - 2.0 * s * d
    if mode == "LIGHTEN":
        return np.maximum(s, d)
    if mode == "DARKEN":
        return np.minimum(s, d)
    if mode == "DIVIDE":
        return np.minimum(d / np.maximum(s, 1e-6), 1.0)
    if mode == "COLOR_DODGE" or mode == "GLOW_DODGE":
        s_u8 = np.clip(np.floor(s * 255.0 + 0.5), 0, 255).astype(np.int32)
        d_u8 = np.clip(np.floor(d * 255.0 + 0.5), 0, 255).astype(np.int32)
        out = np.where(s_u8 >= 255, 255,
                       np.minimum(255, (d_u8 * 255) // np.maximum(255 - s_u8, 1)))
        return out.astype(np.float32) / 255.0
    if mode == "COLOR_BURN":
        s_u8 = np.clip(np.floor(s * 255.0 + 0.5), 0, 255).astype(np.int32)
        d_u8 = np.clip(np.floor(d * 255.0 + 0.5), 0, 255).astype(np.int32)
        out = np.where(s_u8 <= 0, 0,
                       255 - np.minimum(255, ((255 - d_u8) * 255) // np.maximum(s_u8, 1)))
        return out.astype(np.float32) / 255.0
    if mode == "LINEAR_BURN":
        return np.maximum(s + d - 1.0, 0.0)
    if mode == "LINEAR_LIGHT":
        # 2*s + d - 1, clamped
        return np.clip(2.0 * s + d - 1.0, 0.0, 1.0)
    if mode == "VIVID_LIGHT":
        # s<0.5: color-burn(d, 2*s) ; s>=0.5: color-dodge(d, 2*(s-0.5))
        burn = np.where(d >= 1, 1.0,
                        np.where(2.0 * s <= 0, 0.0,
                                 1.0 - np.minimum((1.0 - d) / np.maximum(2.0 * s, 1e-6), 1.0)))
        dodge_s = 2.0 * (s - 0.5)
        dodge = np.where(d <= 0, 0.0,
                         np.where(dodge_s >= 1, 1.0,
                                  np.minimum(d / np.maximum(1.0 - dodge_s, 1e-6), 1.0)))
        return np.where(s < 0.5, burn, dodge)
    if mode == "PIN_LIGHT":
        # s<0.5: darken(d, 2s) ; s>=0.5: lighten(d, 2(s-0.5))
        return np.where(s < 0.5, np.minimum(d, 2.0 * s), np.maximum(d, 2.0 * (s - 0.5)))
    if mode == "HARD_MIX":
        burn = np.where(d >= 1, 1.0,
                        np.where(2.0 * s <= 0, 0.0,
                                 1.0 - np.minimum((1.0 - d) / np.maximum(2.0 * s, 1e-6), 1.0)))
        dodge_s = 2.0 * (s - 0.5)
        dodge = np.where(d <= 0, 0.0,
                         np.where(dodge_s >= 1, 1.0,
                                  np.minimum(d / np.maximum(1.0 - dodge_s, 1e-6), 1.0)))
        return (np.where(s < 0.5, burn, dodge) >= (127.0 / 255.0)).astype(np.float32)
    if mode == "DARKER_COLOR":
        # Pick whichever pixel has lower luminosity, channel-wise replace
        s_lum = _lum(s)[..., None]
        d_lum = _lum(d)[..., None]
        return np.where(s_lum < d_lum, s, d)
    if mode == "LIGHTER_COLOR":
        s_lum = _lum(s)[..., None]
        d_lum = _lum(d)[..., None]
        return np.where(s_lum > d_lum, s, d)
    if mode == "HUE":
        return _set_lum(_set_sat(s, _sat(d)), _lum(d))
    if mode == "SATURATION":
        return _set_lum(_set_sat(d, _sat(s)), _lum(d))
    if mode == "COLOR":
        return _set_lum(s, _lum(d))
    if mode == "LUMINOSITY" or mode == "BRIGHTNESS":
        # CSP labels this "Brightness"; W3C/PSD call it "Luminosity".
        return _set_lum(d, _lum(s))
    # Unknown mode 鈫?fall back to Normal silently.
    return s


def _blend_add_u8(dst_rgba: np.ndarray, src_rgba: np.ndarray, param3: int = 0) -> np.ndarray:
    """CSP RenderBlendModeCall internal mode 0x201 for 8-bit straight RGBA."""
    dst = dst_rgba.astype(np.int32, copy=True)
    src = src_rgba.astype(np.int32, copy=False)
    sa = src[..., 3]
    da = dst[..., 3]
    out = dst.copy()

    if not np.any(sa):
        return out.astype(np.uint8)

    empty = da == 0
    if param3 == 0:
        out = np.where((empty & (sa > 0))[..., None], src, out)
    active = (sa > 0) & ((da > 0) | (param3 == 0))
    if not np.any(active):
        return np.clip(out, 0, 255).astype(np.uint8)

    work_da = np.where(param3 > 1, 255, da)
    summed = np.minimum(dst[..., :3] + src[..., :3], 255)
    rgb = summed.copy()

    partial_src = active & (sa < 255)
    if np.any(partial_src):
        inv_sa = 255 - sa
        partial_transparent_dst = partial_src & (work_da < 255)
        if np.any(partial_transparent_dst):
            b = (work_da * inv_sa) // 255
            denom = np.maximum(b + sa, 1)
            rgb = np.where(
                partial_transparent_dst[..., None],
                (b[..., None] * dst[..., :3] + summed * sa[..., None]) // denom[..., None],
                rgb,
            )
        partial_opaque_dst = partial_src & (work_da >= 255)
        if np.any(partial_opaque_dst):
            rgb = np.where(
                partial_opaque_dst[..., None],
                (inv_sa[..., None] * dst[..., :3] + summed * sa[..., None]) // 255,
                rgb,
            )

    out_a = dst[..., 3].copy()
    if param3 == 0:
        b = ((255 - sa) * work_da) // 255
        out_a = np.minimum(b + sa, 255)

    tail = active & (work_da <= 254)
    if np.any(tail):
        inv_da = 255 - work_da
        src_opaque = tail & (sa == 255)
        if np.any(src_opaque):
            rgb = np.where(
                src_opaque[..., None],
                (inv_da[..., None] * src[..., :3] + rgb * work_da[..., None]) // 255,
                rgb,
            )
        src_partial = tail & (sa < 255)
        if np.any(src_partial):
            b = (inv_da * sa) // 255
            denom = np.maximum(work_da + b, 1)
            rgb = np.where(
                src_partial[..., None],
                (b[..., None] * src[..., :3] + rgb * work_da[..., None]) // denom[..., None],
                rgb,
            )

    out[..., :3] = np.where(active[..., None], np.minimum(rgb, 255), out[..., :3])
    out[..., 3] = np.where(active, out_a, out[..., 3])
    return np.clip(out, 0, 255).astype(np.uint8)


def _blend_add_glow_u8(dst_rgba: np.ndarray, src_rgba: np.ndarray, param3: int = 0) -> np.ndarray:
    """CSP RenderBlendModeCall internal mode 0x206 for 8-bit straight RGBA."""
    dst = dst_rgba.astype(np.int32, copy=True)
    src = src_rgba.astype(np.int32, copy=False)
    sa = src[..., 3]
    da = dst[..., 3]
    out = dst.copy()

    if not np.any(sa):
        return out.astype(np.uint8)

    empty = da == 0
    if param3 == 0:
        out = np.where((empty & (sa > 0))[..., None], src, out)
    active = (sa > 0) & ((da > 0) | (param3 == 0))
    if not np.any(active):
        return np.clip(out, 0, 255).astype(np.uint8)

    work_da = np.where(param3 > 1, 255, da)
    summed = dst[..., :3] + src[..., :3]
    rgb = summed.copy()

    partial_src = active & (sa < 255)
    if np.any(partial_src):
        b = (work_da * (255 - sa)) // 255
        denom = np.maximum(b + sa, 1)
        rgb = np.where(
            partial_src[..., None],
            (b[..., None] * dst[..., :3] + summed * sa[..., None]) // denom[..., None],
            rgb,
        )
    rgb = np.minimum(rgb, 255)

    out_a = dst[..., 3].copy()
    if param3 == 0:
        b = ((255 - sa) * work_da) // 255
        out_a = np.minimum(b + sa, 255)

    tail = active & (work_da <= 254)
    if np.any(tail):
        inv_da = 255 - work_da
        src_opaque = tail & (sa == 255)
        if np.any(src_opaque):
            rgb = np.where(
                src_opaque[..., None],
                (inv_da[..., None] * src[..., :3] + rgb * work_da[..., None]) // 255,
                rgb,
            )
        src_partial = tail & (sa < 255)
        if np.any(src_partial):
            b = (inv_da * sa) // 255
            denom = np.maximum(work_da + b, 1)
            rgb = np.where(
                src_partial[..., None],
                (b[..., None] * src[..., :3] + rgb * work_da[..., None]) // denom[..., None],
                rgb,
            )

    out[..., :3] = np.where(active[..., None], np.minimum(rgb, 255), out[..., :3])
    out[..., 3] = np.where(active, out_a, out[..., 3])
    return np.clip(out, 0, 255).astype(np.uint8)


def _alpha_bbox(alpha: np.ndarray):
    """Return (y0, y1, x0, x1) tightly enclosing all alpha>0 pixels, else None.

    For sparse layers (typical of CSP files where ~99% of pixels are transparent)
    this lets the compositor skip the empty regions entirely.
    """
    if not alpha.any():
        return None
    rows = np.any(alpha, axis=1)
    cols = np.any(alpha, axis=0)
    y0 = int(np.argmax(rows))
    y1 = len(rows) - int(np.argmax(rows[::-1]))
    x0 = int(np.argmax(cols))
    x1 = len(cols) - int(np.argmax(cols[::-1]))
    return y0, y1, x0, x1


def _layer_is_visible(layer: sqlite3.Row) -> bool:
    """CSP stores layer visibility as a bit field; bit 0 is the eye state."""
    return (int(layer["LayerVisibility"] or 0) & 1) != 0


def _clip_color_component(value) -> int:
    """CSP may store color components as a repeated-byte 32-bit value."""
    value = int(value or 0)
    if value < 0:
        value &= 0xFFFFFFFF
    if value > 255:
        value = (value >> 24) & 0xFF
    return min(max(value, 0), 255)


def _offscreen_init_fill(attribute: bytes) -> int:
    """Return the byte fill color for omitted single-channel offscreen chunks."""
    marker = "InitColor".encode("utf-16-be")
    pos = attribute.find(marker)
    if pos < 0:
        return 0
    pos += len(marker)
    if pos + 12 > len(attribute):
        return 0
    payload_len = struct.unpack_from(">L", attribute, pos)[0]
    if payload_len < 8:
        return 0
    color = struct.unpack_from(">L", attribute, pos + 8)[0]
    return color & 0xFF


def _filter_info(layer: sqlite3.Row) -> tuple[int, bytes] | None:
    blob = layer["FilterLayerInfo"]
    if not blob or len(blob) < 8:
        return None
    filter_type, payload_len = struct.unpack_from(">II", blob, 0)
    payload = blob[8:8 + min(payload_len, max(0, len(blob) - 8))]
    return filter_type, payload


def _linear_lut(start_x: int, start_y: int, end_x: int, end_y: int) -> np.ndarray:
    lut = np.empty(256, dtype=np.uint8)
    if start_x > 0:
        lut[:min(start_x, 256)] = np.clip(start_y, 0, 255)
    span = end_x - start_x
    if span != 0:
        slope = (end_y - start_y) / span
    else:
        slope = 0.0
    lo = max(start_x, 0)
    hi = min(end_x, 256)
    if lo < hi:
        x = np.arange(lo, hi, dtype=np.float32)
        lut[lo:hi] = np.clip(np.floor(x * slope + (start_y - start_x * slope) + 0.5), 0, 255).astype(np.uint8)
    if end_x < 256:
        lut[max(end_x, 0):] = np.clip(end_y, 0, 255)
    return lut


def _brightness_lut(amount: int) -> np.ndarray:
    amount = min(max(amount, -127), 127)
    x = np.arange(256, dtype=np.int16) + amount
    return np.clip(x, 0, 255).astype(np.uint8)


def _contrast_lut(amount: int) -> np.ndarray:
    if amount == 0 or not (-127 < amount < 128):
        return np.arange(256, dtype=np.uint8)
    if amount > 0:
        return _linear_lut(amount, 0, 255 - amount, 255)
    return _linear_lut(0, -amount, 255, 255 + amount)


def _apply_level_adjust(rgb_u8: np.ndarray, payload: bytes) -> np.ndarray | None:
    if len(payload) < 10:
        return None

    groups = [
        struct.unpack_from(">5H", payload, off)
        for off in range(0, len(payload) - 9, 10)
    ]
    if not groups:
        return None

    def make_lut(group: tuple[int, int, int, int, int]) -> np.ndarray:
        in_low, mid, in_high, out_low, out_high = (
            value * 255.0 / 65535.0 for value in group
        )
        if in_high <= in_low:
            return np.arange(256, dtype=np.uint8)

        x = np.arange(256, dtype=np.float32)
        t = np.clip((x - in_low) / (in_high - in_low), 0.0, 1.0)
        mid_t = np.clip((mid - in_low) / (in_high - in_low), 1e-4, 0.9999)
        gamma = np.log(0.5) / np.log(mid_t)
        y = out_low + np.power(t, 1.0 / max(gamma, 1e-4)) * (out_high - out_low)
        return np.clip(np.floor(y + 0.5), 0, 255).astype(np.uint8)

    # SQLite FilterLayerInfo stores compact 16-bit level records. The sample
    # payload's first record is the master curve; channel records are identity
    # unless CSP writes per-channel values later.
    master = make_lut(groups[0])
    out = rgb_u8.copy()
    for channel in range(3):
        out[..., channel] = master[out[..., channel]]
    return out


_TONE_CURVE_COMPACT_STRIDE = 0x82  # uint16 count + 32 uint16 point pairs.
_IDENTITY_TONE_CURVE_POINTS = ((0, 0), (65535, 65535))
_GRADIENT_STOP_DENOMINATOR = 32768.0 * 256.0 / 255.0


def _tone_curve_compact_curves(payload: bytes) -> list[tuple[tuple[int, int], ...]] | None:
    if len(payload) % _TONE_CURVE_COMPACT_STRIDE != 0:
        return None
    curves: list[tuple[tuple[int, int], ...]] = []
    for off in range(0, len(payload), _TONE_CURVE_COMPACT_STRIDE):
        count = struct.unpack_from(">H", payload, off)[0]
        if count > 32:
            return None
        points = tuple(
            struct.unpack_from(">HH", payload, off + 2 + i * 4)
            for i in range(count)
        )
        curves.append(points)
    return curves


def _tone_curve_bspline_lut(points: tuple[tuple[int, int], ...]) -> np.ndarray | None:
    count = len(points)
    if count < 2:
        return np.arange(256, dtype=np.uint8)
    if any(not (0 <= x <= 65535 and 0 <= y <= 65535) for x, y in points):
        return None
    if count == 2 and points == _IDENTITY_TONE_CURVE_POINTS:
        return np.arange(256, dtype=np.uint8)

    # CSP stores the SQLite payload in compact 16-bit coordinates, but the PSD
    # `curv` export and runtime LUT behavior match a byte-domain point table.
    pts = [
        (
            float(min(255, int(np.ceil(x / 257.0)))),
            float(min(255, int(np.ceil(y / 257.0)))),
        )
        for x, y in points
    ]
    table = np.arange(256, dtype=np.float64)
    step_x = abs(pts[-1][0] - pts[0][0]) / 255.0
    if step_x <= 0.0:
        return None

    if count == 2:
        x0, y0 = pts[0]
        x1, y1 = pts[1]
        sample_x = x0
        for idx in range(256):
            if x1 == x0:
                table[idx] = y0
            else:
                table[idx] = ((y1 - y0) / (x1 - x0)) * (sample_x - x0) + y0
            sample_x += step_x
    else:
        have_previous = False
        previous_x = 0.0
        previous_y = 0.0
        base_x = 0.0
        for curve_idx in range(1, count - 1):
            x_prev, y_prev = pts[curve_idx - 1]
            x_mid, y_mid = pts[curve_idx]
            x_next, y_next = pts[curve_idx + 1]
            if curve_idx == 1:
                x_prev -= x_mid - x_prev
                y_prev -= y_mid - y_prev
            if curve_idx == count - 2:
                x_next -= x_mid - x_next
                y_next -= y_mid - y_next

            segment_previous_x = previous_x
            for sample_idx in range(258):
                t = sample_idx / 257.0
                w_prev = (1.0 - t) * (1.0 - t) * 0.5
                w_next = t * t * 0.5
                w_mid = (t - t * t) + 0.5
                x = x_prev * w_prev + x_mid * w_mid + x_next * w_next
                y = y_prev * w_prev + y_mid * w_mid + y_next * w_next

                next_base_x = x
                if have_previous:
                    lo = min(x, segment_previous_x)
                    hi = max(x, segment_previous_x)
                    next_base_x = base_x
                    sample_offset = 0.0
                    while sample_offset <= hi - lo + 1e-9:
                        sample_x = sample_offset + lo
                        out_idx = int(((sample_x - base_x) / step_x) + 0.5)
                        if 0 <= out_idx < 256:
                            if x == segment_previous_x:
                                sample_y = previous_y
                            else:
                                sample_y = (
                                    ((y - previous_y) / (x - segment_previous_x))
                                    * (sample_x - segment_previous_x)
                                    + previous_y
                                )
                            table[out_idx] = sample_y
                        sample_offset += step_x
                have_previous = True
                segment_previous_x = x
                base_x = next_base_x
                previous_y = y
            previous_x = segment_previous_x

    lut = np.clip(np.floor(table + 0.5), 0, 255).astype(np.uint8)
    if points[0][1] < 1:
        lut[0] = 0
    if points[-1][1] > 254:
        lut[-1] = 255
    return lut


def _apply_tone_curve(rgb_u8: np.ndarray, payload: bytes) -> np.ndarray | None:
    curves = _tone_curve_compact_curves(payload)
    if curves:
        # CSP's compact payload stores 16-bit tone-curve points. Per-channel
        # RGB curves are applied first, then the master curve is applied.
        luts = [_tone_curve_bspline_lut(curve) for curve in curves[:4]]
        if any(lut is None for lut in luts):
            return None
        out = rgb_u8.copy()
        if len(luts) >= 4:
            for channel, lut in enumerate(luts[1:4]):
                out[..., channel] = lut[out[..., channel]]
        out = luts[0][out]
        return out
    return None


def _rgb_to_hsv_u8(rgb_u8: np.ndarray) -> tuple[np.ndarray, np.ndarray, np.ndarray]:
    rgb = rgb_u8.astype(np.float32) / 255.0
    r = rgb[..., 0]
    g = rgb[..., 1]
    b = rgb[..., 2]
    mx = rgb.max(axis=-1)
    mn = rgb.min(axis=-1)
    delta = mx - mn
    h = np.zeros_like(mx)
    nonzero = delta > 1e-6
    rmax = nonzero & (mx == r)
    gmax = nonzero & (mx == g)
    bmax = nonzero & (mx == b)
    h = np.where(rmax, ((g - b) / np.maximum(delta, 1e-6)) % 6.0, h)
    h = np.where(gmax, ((b - r) / np.maximum(delta, 1e-6)) + 2.0, h)
    h = np.where(bmax, ((r - g) / np.maximum(delta, 1e-6)) + 4.0, h)
    h /= 6.0
    s = np.where(mx > 1e-6, delta / np.maximum(mx, 1e-6), 0.0)
    return h, s, mx


def _hsv_to_rgb_u8(h: np.ndarray, s: np.ndarray, v: np.ndarray) -> np.ndarray:
    h = (h % 1.0) * 6.0
    i = np.floor(h).astype(np.int32)
    f = h - i
    p = v * (1.0 - s)
    q = v * (1.0 - s * f)
    t = v * (1.0 - s * (1.0 - f))
    i = i % 6
    r = np.select([i == 0, i == 1, i == 2, i == 3, i == 4], [v, q, p, p, t], default=v)
    g = np.select([i == 0, i == 1, i == 2, i == 3, i == 4], [t, v, v, q, p], default=p)
    b = np.select([i == 0, i == 1, i == 2, i == 3, i == 4], [p, p, t, v, v], default=q)
    return np.clip(np.floor(np.stack([r, g, b], axis=-1) * 255.0 + 0.5), 0, 255).astype(np.uint8)


def _apply_hsl_adjust(rgb_u8: np.ndarray, hue: int, saturation: int, luminosity: int) -> np.ndarray:
    h, s, v = _rgb_to_hsv_u8(rgb_u8)
    h = h + hue / 360.0
    if saturation >= 0:
        s = s + (1.0 - s) * min(saturation, 100) / 100.0
    else:
        s = s * (1.0 + max(saturation, -100) / 100.0)
    if luminosity >= 0:
        v = v + (1.0 - v) * min(luminosity, 100) / 100.0
    else:
        v = v * (1.0 + max(luminosity, -100) / 100.0)
    return _hsv_to_rgb_u8(h, np.clip(s, 0.0, 1.0), np.clip(v, 0.0, 1.0))


def _color_balance_make_low(a: int, b: int, c: int) -> float:
    if c <= b:
        c = b
    value = c - a
    if c <= a:
        value = 0
    return float(value)


def _color_balance_make_mid(a: int, b: int, c: int) -> float:
    return 0.5 - ((((a * 2) - b) - c) * 0.3) / 400.0


def _color_balance_make_high(a: int, b: int, c: int) -> float:
    if b <= c:
        c = b
    if c < a:
        return float((c - a) + 255)
    return 255.0


def _color_balance_make_normal_level(a: int, b: int, c: int) -> tuple[float, float, float]:
    mid = 0.5 - (b / 100.0) * 0.2
    low = 0.0
    high = 255.0
    if c < 0:
        mid -= (c / 100.0) * 0.08
    elif c > 0:
        mid -= (c / 100.0) * 0.12
        high = 255.0 - (c / 100.0) * 96.0

    if a > 0:
        mid -= (a / 100.0) * 0.08
    elif a < 0:
        low = -(a / 100.0) * 96.0
        mid -= (a / 100.0) * 0.12
    return mid, low, high


def _color_balance_level_lut(level: tuple[float, float, float]) -> np.ndarray:
    mid, low, high = level
    start = int(low)
    end = int(high)
    mid_index = int((high - low) * mid + low)
    start = min(max(start, 0), 255)
    end = min(max(end, 0), 255)
    if start >= end:
        return np.arange(256, dtype=np.uint8)

    lut = np.empty(256, dtype=np.uint8)
    lut[:start + 1] = 0
    lut[end:] = 255
    mid_t = np.clip((mid_index - start) / (end - start), 1e-6, 0.999999)
    gamma = np.log(0.5) / np.log(mid_t)
    x = np.arange(start + 1, end, dtype=np.float64)
    t = np.clip((x - start) / (end - start), 0.0, 1.0)
    lut[start + 1:end] = np.clip(
        np.floor(np.power(t, gamma) * 255.0 + 0.5),
        0,
        255,
    ).astype(np.uint8)
    return lut


def _apply_color_balance(rgb_u8: np.ndarray, payload: bytes) -> np.ndarray | None:
    if len(payload) < 40:
        return None
    vals = struct.unpack_from(">iiiiiiiiii", payload, 0)
    preserve_luminosity = vals[0] != 0
    r_shadow, r_mid, r_high = vals[1], vals[4], vals[7]
    g_shadow, g_mid, g_high = vals[2], vals[5], vals[8]
    b_shadow, b_mid, b_high = vals[3], vals[6], vals[9]

    if preserve_luminosity:
        levels = (
            (
                _color_balance_make_mid(r_mid, g_mid, b_mid),
                _color_balance_make_low(r_shadow, g_shadow, b_shadow),
                _color_balance_make_high(r_high, g_high, b_high),
            ),
            (
                _color_balance_make_mid(g_mid, b_mid, r_mid),
                _color_balance_make_low(g_shadow, b_shadow, r_shadow),
                _color_balance_make_high(g_high, b_high, r_high),
            ),
            (
                _color_balance_make_mid(b_mid, r_mid, g_mid),
                _color_balance_make_low(b_shadow, r_shadow, g_shadow),
                _color_balance_make_high(b_high, r_high, g_high),
            ),
        )
    else:
        levels = (
            _color_balance_make_normal_level(r_shadow, r_mid, r_high),
            _color_balance_make_normal_level(g_shadow, g_mid, g_high),
            _color_balance_make_normal_level(b_shadow, b_mid, b_high),
        )

    luts = tuple(_color_balance_level_lut(level) for level in levels)
    return np.stack(
        [luts[channel][rgb_u8[..., channel]] for channel in range(3)],
        axis=-1,
    )


def _gradient_map(rgb_u8: np.ndarray, payload: bytes) -> np.ndarray | None:
    if len(payload) < 28:
        return None
    header = struct.unpack_from(">7i", payload, 0)
    count = header[3]
    if count <= 0:
        return None
    nodes = []
    pos = 28
    for _ in range(count):
        if pos + 28 > len(payload):
            break
        raw = struct.unpack_from(">7I", payload, pos)
        r = (raw[0] >> 16) & 0xFFFF
        g = (raw[1] >> 16) & 0xFFFF
        b = (raw[2] >> 16) & 0xFFFF
        # SQLite stores stops in the compact 15-bit UI domain. CSP's 256-entry
        # runtime LUT compares i / 255.0 against the converted stop position,
        # which empirically matches a 255/256 table-edge scale.
        stop = raw[5] / _GRADIENT_STOP_DENOMINATOR
        color = np.array(
            [
                min(255, int(np.floor(r / 256.0 + 0.5))),
                min(255, int(np.floor(g / 256.0 + 0.5))),
                min(255, int(np.floor(b / 256.0 + 0.5))),
            ],
            dtype=np.float32,
        )
        nodes.append((stop, color))
        pos += 28
    if not nodes:
        return None
    nodes.sort(key=lambda item: item[0])
    rgb = rgb_u8.astype(np.float32)
    lum_u8 = np.floor(rgb[..., 0] * 0.3 + rgb[..., 1] * 0.59 + rgb[..., 2] * 0.11)
    lum = np.clip(lum_u8, 0, 255) / 255.0
    out = np.zeros_like(rgb_u8, dtype=np.float32)
    first_pos, first_color = nodes[0]
    last_pos, last_color = nodes[-1]
    out[...] = first_color
    for (p0, c0), (p1, c1) in zip(nodes, nodes[1:]):
        mask = (lum >= p0) & (lum <= p1)
        t = np.clip((lum - p0) / max(p1 - p0, 1e-6), 0.0, 1.0)[..., None]
        out = np.where(mask[..., None], c0 * (1.0 - t) + c1 * t, out)
    out = np.where((lum >= last_pos)[..., None], last_color, out)
    out = np.where((lum <= first_pos)[..., None], first_color, out)
    return np.clip(np.floor(out + 0.5), 0, 255).astype(np.uint8)


# --------------------------------------------------------------------------- #
# High-level reader
# --------------------------------------------------------------------------- #

class ClipFile:
    def __init__(self, path: str):
        self.path = path
        self._exta_bodies, sqlite_bytes = _split_clip(path)   # raw, NOT decompressed
        self._tile_blob_cache: dict[tuple[str, int, Optional[int]], bytes] = {}
        self._brush_effector_blob_cache: dict[tuple[int, str], Optional[bytes]] = {}
        self._brush_effector_graph_cache: dict[int, Optional[list[tuple[float, float]]]] = {}
        self._brush_style_preview_cache: dict[int, Optional[_BrushStylePreview]] = {}
        self._brush_material_stamp_cache: dict[int, Optional[np.ndarray]] = {}
        self._brush_material_full_lane_cache: dict[int, Optional[np.ndarray]] = {}
        self._brush_material_resource_alpha_cache: dict[int, Optional[np.ndarray]] = {}

        fd, self._db_path = tempfile.mkstemp(suffix=".sqlite3")
        os.close(fd)
        with open(self._db_path, "wb") as f:
            f.write(sqlite_bytes)
        self._db = sqlite3.connect(self._db_path, check_same_thread=False)
        self._db.row_factory = sqlite3.Row

        canv = self._db.execute(
            "SELECT MainId, CanvasWidth, CanvasHeight, CanvasRootFolder FROM Canvas LIMIT 1"
        ).fetchone()
        self.canvas_id: int = canv["MainId"]
        self.width: int = int(canv["CanvasWidth"])
        self.height: int = int(canv["CanvasHeight"])
        self.root_layer_id: int = canv["CanvasRootFolder"]
        self._has_vector_object_list: bool = self._db.execute(
            "SELECT 1 FROM sqlite_master WHERE type='table' AND name='VectorObjectList'"
        ).fetchone() is not None

    def close(self):
        try:
            self._db.close()
        finally:
            try:
                os.unlink(self._db_path)
            except OSError:
                pass

    # ----- layer hierarchy ----- #

    def _layer_row(self, layer_id: int) -> sqlite3.Row:
        return self._db.execute(
            "SELECT * FROM Layer WHERE MainId=?", (layer_id,)
        ).fetchone()

    def _walk_chain(self, first_id: int) -> list[int]:
        ids = []
        cur = first_id
        seen = set()
        while cur and cur not in seen:
            seen.add(cur)
            ids.append(cur)
            row = self._layer_row(cur)
            if row is None:
                break
            cur = row["LayerNextIndex"]
        return ids

    def composite_layers_in_order(self) -> list[sqlite3.Row]:
        """Return visible raster layers in render order (bottom-most first).

        FirstChild 鈫?NextIndex chain is already bottom-up (verified against
        Illustration4K.png ground truth: walking as-is gives premultiplied diff
        < 0.01 max; reversing inflates max diff to 0.99).
        """
        root = self._layer_row(self.root_layer_id)
        if root is None or root["LayerType"] != LAYER_TYPE_FOLDER:
            raise ValueError("Root layer not found or not a folder.")
        out = []

        def visit_chain(first_id: int):
            for lid in self._walk_chain(first_id):
                row = self._layer_row(lid)
                if not _layer_is_visible(row):
                    continue
                if row["LayerType"] == LAYER_TYPE_PAPER:
                    continue
                if row["LayerType"] == LAYER_TYPE_LAYER_FOLDER:
                    visit_chain(row["LayerFirstChildIndex"])
                    continue
                if row["LayerType"] not in RASTER_LAYER_TYPES:
                    log.warning("Skipping layer %d (%r): unsupported type %d",
                                lid, row["LayerName"], row["LayerType"])
                    continue
                if row["LayerOffsetX"] or row["LayerOffsetY"]:
                    log.warning("Layer %d (%r) has offset (%d,%d); not yet handled.",
                                lid, row["LayerName"],
                                row["LayerOffsetX"], row["LayerOffsetY"])
                out.append(row)

        visit_chain(root["LayerFirstChildIndex"])
        return out

    def _paper_color(self) -> Optional[tuple[float, float, float]]:
        root = self._layer_row(self.root_layer_id)
        if root is None or root["LayerType"] != LAYER_TYPE_FOLDER:
            return None
        for lid in self._walk_chain(root["LayerFirstChildIndex"]):
            row = self._layer_row(lid)
            if not _layer_is_visible(row) or row["LayerType"] != LAYER_TYPE_PAPER:
                continue
            keys = set(row.keys())
            if {"DrawColorMainRed", "DrawColorMainGreen", "DrawColorMainBlue"} <= keys:
                rgb = (
                    _clip_color_component(row["DrawColorMainRed"]),
                    _clip_color_component(row["DrawColorMainGreen"]),
                    _clip_color_component(row["DrawColorMainBlue"]),
                )
                draw_enabled = bool(row["DrawColorEnable"]) if "DrawColorEnable" in keys else False
                if rgb != (0, 0, 0) or draw_enabled:
                    return tuple(c / 255.0 for c in rgb)

            thumb = self._db.execute(
                "SELECT ThumbnailMainColorRed, ThumbnailMainColorGreen, ThumbnailMainColorBlue "
                "FROM LayerThumbnail WHERE LayerId=?",
                (lid,),
            ).fetchone()
            if thumb is not None:
                rgb = (
                    _clip_color_component(thumb["ThumbnailMainColorRed"]),
                    _clip_color_component(thumb["ThumbnailMainColorGreen"]),
                    _clip_color_component(thumb["ThumbnailMainColorBlue"]),
                )
                if rgb != (0, 0, 0):
                    return tuple(c / 255.0 for c in rgb)

            return tuple(
                _clip_color_component(row[name]) / 255.0
                for name in ("LayerPaletteRed", "LayerPaletteGreen", "LayerPaletteBlue")
            )
        return None

    # ----- raster decode (lazy) ----- #

    def _mipmap_offscreen_id(self, mipmap_id: int) -> Optional[int]:
        if not mipmap_id:
            return None
        mipmap = self._db.execute(
            "SELECT BaseMipmapInfo FROM Mipmap WHERE MainId=?",
            (mipmap_id,),
        ).fetchone()
        if mipmap is None:
            return None
        mipmap_info = self._db.execute(
            "SELECT Offscreen FROM MipmapInfo WHERE MainId=?",
            (mipmap["BaseMipmapInfo"],),
        ).fetchone()
        if mipmap_info is None:
            return None
        return int(mipmap_info["Offscreen"])

    def _resolve_mipmap_external_id(self, mipmap_id: int) -> Optional[str]:
        offscreen_id = self._mipmap_offscreen_id(mipmap_id)
        if offscreen_id is None:
            return None
        offscreen = self._db.execute(
            "SELECT BlockData FROM Offscreen WHERE MainId=?",
            (offscreen_id,),
        ).fetchone()
        if offscreen is None:
            return None
        ext_id = offscreen["BlockData"]
        if isinstance(ext_id, bytes):
            return ext_id.decode("ascii")
        return str(ext_id)

    def _resolve_external_id(self, layer_id: int) -> Optional[str]:
        row = self._layer_row(layer_id)
        if row is None:
            return None
        return self._resolve_mipmap_external_id(row["LayerRenderMipmap"])

    def _layer_pixel_size(self, layer_id: int) -> tuple[int, int]:
        row = self._db.execute(
            "SELECT ThumbnailCanvasWidth, ThumbnailCanvasHeight "
            "FROM LayerThumbnail WHERE LayerId=?",
            (layer_id,),
        ).fetchone()
        if row:
            return int(row["ThumbnailCanvasWidth"]), int(row["ThumbnailCanvasHeight"])
        return self.width, self.height

    def _get_tile_blob(
        self,
        ext_id: str,
        empty_fill: int = 0,
        expected_len: Optional[int] = None,
    ) -> Optional[bytes]:
        cache_key = (ext_id, empty_fill, expected_len)
        cached = self._tile_blob_cache.get(cache_key)
        if cached is not None:
            return cached
        body = self._exta_bodies.get(ext_id)
        if body is None:
            return None
        _, blob = _parse_exta(body, empty_fill=empty_fill, expected_len=expected_len)
        self._tile_blob_cache[cache_key] = blob
        return blob

    def decode_layer(self, layer_id: int) -> Optional[np.ndarray]:
        row = self._layer_row(layer_id)
        if row is None:
            return None
        render_mipmap = row["LayerRenderMipmap"]
        if not render_mipmap:
            return None
        offscreen_id = self._mipmap_offscreen_id(render_mipmap)
        if offscreen_id is None:
            return None
        ext_id = self._resolve_mipmap_external_id(render_mipmap)
        if ext_id is None:
            return None
        off_w, off_h = self._offscreen_pixel_size(offscreen_id) or self._layer_pixel_size(layer_id)
        expected_len = ((off_w + TILE - 1) // TILE) * ((off_h + TILE - 1) // TILE) * PER_TILE_BYTES
        blob = self._get_tile_blob(ext_id, expected_len=expected_len)
        if blob is None:
            return None
        color_type = row["LayerColorTypeIndex"] if "LayerColorTypeIndex" in row.keys() else None
        if len(blob) % PER_TILE_BYTES == 0:
            rgba = _tiles_to_rgba(blob, off_w, off_h)
        elif color_type == 1:
            rgba = _tiles_to_gray_rgba(blob, off_w, off_h)
        elif color_type == 2:
            rgba = _tiles_to_mono_rgba(blob, off_w, off_h)
        else:
            rgba = _tiles_to_rgba(blob, off_w, off_h)
        dx = int(row["LayerRenderOffscrOffsetX"] or 0)
        dy = int(row["LayerRenderOffscrOffsetY"] or 0)
        if (off_w, off_h) == (self.width, self.height) and dx == 0 and dy == 0:
            return rgba
        if dx == 0 and dy == 0 and off_w >= self.width and off_h >= self.height:
            return rgba[:self.height, :self.width].copy()
        canvas = np.zeros((self.height, self.width, 4), dtype=np.uint8)
        src_x0 = max(0, -dx)
        src_y0 = max(0, -dy)
        dst_x0 = max(0, dx)
        dst_y0 = max(0, dy)
        paste_w = min(off_w - src_x0, self.width - dst_x0)
        paste_h = min(off_h - src_y0, self.height - dst_y0)
        if paste_w > 0 and paste_h > 0:
            canvas[dst_y0:dst_y0 + paste_h, dst_x0:dst_x0 + paste_w] = rgba[
                src_y0:src_y0 + paste_h,
                src_x0:src_x0 + paste_w,
            ]
        return canvas

    def _resolve_mask_external_id(self, layer_id: int) -> Optional[str]:
        """Resolve the highest-res mask blob's external_id for a layer, or None."""
        row = self._layer_row(layer_id)
        if row is None:
            return None
        mask_mipmap = row["LayerLayerMaskMipmap"]
        if not mask_mipmap:
            return None
        mip = self._db.execute(
            "SELECT BaseMipmapInfo FROM Mipmap WHERE MainId=?", (mask_mipmap,)
        ).fetchone()
        if mip is None:
            return None
        info = self._db.execute(
            "SELECT Offscreen FROM MipmapInfo WHERE MainId=?",
            (mip["BaseMipmapInfo"],),
        ).fetchone()
        if info is None:
            return None
        off = self._db.execute(
            "SELECT BlockData FROM Offscreen WHERE MainId=?", (info["Offscreen"],)
        ).fetchone()
        if off is None:
            return None
        return off["BlockData"].decode("ascii")

    def _mask_empty_fill(self, layer_id: int) -> int:
        row = self._layer_row(layer_id)
        if row is None:
            return 0
        mask_mipmap = row["LayerLayerMaskMipmap"]
        if not mask_mipmap:
            return 0
        mip = self._db.execute(
            "SELECT BaseMipmapInfo FROM Mipmap WHERE MainId=?", (mask_mipmap,)
        ).fetchone()
        if mip is None:
            return 0
        info = self._db.execute(
            "SELECT Offscreen FROM MipmapInfo WHERE MainId=?",
            (mip["BaseMipmapInfo"],),
        ).fetchone()
        if info is None:
            return 0
        off = self._db.execute(
            "SELECT Attribute FROM Offscreen WHERE MainId=?", (info["Offscreen"],)
        ).fetchone()
        if off is None:
            return 0
        return _offscreen_init_fill(off["Attribute"])

    def decode_layer_mask(self, layer_id: int) -> Optional[np.ndarray]:
        """Returns (H, W) uint8 mask at canvas size, or None if no mask / failure."""
        row = self._layer_row(layer_id)
        if row is None:
            return None
        mask_mipmap = row["LayerLayerMaskMipmap"]
        if not mask_mipmap:
            return None
        offscreen_id = self._mipmap_offscreen_id(mask_mipmap)
        if offscreen_id is None:
            return None
        ext_id = self._resolve_mipmap_external_id(mask_mipmap)
        if ext_id is None:
            return None
        offscreen_size = self._offscreen_pixel_size(offscreen_id) or (self.width, self.height)
        off_w, off_h = offscreen_size
        expected_len = (
            ((off_w + TILE - 1) // TILE)
            * ((off_h + TILE - 1) // TILE)
            * MASK_TILE_BYTES
        )
        empty_fill = self._mask_empty_fill(layer_id)
        blob = self._get_tile_blob(
            ext_id,
            empty_fill=empty_fill,
            expected_len=expected_len,
        )
        if blob is None:
            return None
        try:
            alpha = _tiles_to_alpha(blob, off_w, off_h)
        except ValueError as exc:
            log.warning("Mask decode failed for layer %d: %s", layer_id, exc)
            return None
        dx = int(row["LayerMaskOffscrOffsetX"] or 0)
        dy = int(row["LayerMaskOffscrOffsetY"] or 0)
        if (off_w, off_h) == (self.width, self.height) and dx == 0 and dy == 0:
            return alpha
        if dx == 0 and dy == 0 and off_w >= self.width and off_h >= self.height:
            return alpha[:self.height, :self.width].copy()
        canvas = np.full((self.height, self.width), empty_fill, dtype=np.uint8)
        src_x0 = max(0, -dx)
        src_y0 = max(0, -dy)
        dst_x0 = max(0, dx)
        dst_y0 = max(0, dy)
        paste_w = min(off_w - src_x0, self.width - dst_x0)
        paste_h = min(off_h - src_y0, self.height - dst_y0)
        if paste_w > 0 and paste_h > 0:
            canvas[dst_y0:dst_y0 + paste_h, dst_x0:dst_x0 + paste_w] = alpha[
                src_y0:src_y0 + paste_h,
                src_x0:src_x0 + paste_w,
            ]
        return canvas

    # ----- simple object-layer fallbacks ----- #

    def _vector_object_body(self, layer_id: int) -> Optional[bytes]:
        if not self._has_vector_object_list:
            return None
        row = self._db.execute(
            "SELECT VectorData FROM VectorObjectList WHERE LayerId=?",
            (layer_id,),
        ).fetchone()
        if row is None:
            return None
        ext_id = row["VectorData"]
        if isinstance(ext_id, bytes):
            ext_id = ext_id.decode("ascii")
        return self._exta_bodies.get(ext_id)

    def _vector_header_bbox(self, body: bytes) -> Optional[tuple[int, int, int, int]]:
        if len(body) < 96:
            return None
        x0, y0, x1, y1 = struct.unpack_from(">IIII", body, 80)
        if not (0 <= x0 < x1 <= self.width * 2 and 0 <= y0 < y1 <= self.height * 2):
            return None
        return (
            max(0, min(int(x0), self.width)),
            max(0, min(int(y0), self.height)),
            max(0, min(int(x1), self.width)),
            max(0, min(int(y1), self.height)),
        )

    def _vector_header_color(self, body: bytes) -> tuple[int, int, int]:
        if len(body) < 106:
            return (0, 0, 0)
        comps = []
        for off in (96, 100, 104):
            comps.append(struct.unpack_from(">H", body, off)[0] // 257)
        return tuple(max(0, min(int(c), 255)) for c in comps)

    def _vector_header_fill_color(self, body: bytes) -> tuple[int, int, int]:
        if len(body) < 118:
            return (255, 255, 255)
        comps = []
        for off in (108, 112, 116):
            comps.append(struct.unpack_from(">H", body, off)[0] // 257)
        rgb = tuple(max(0, min(int(c), 255)) for c in comps)
        if rgb == (0, 0, 0):
            return (255, 255, 255)
        return rgb

    def _vector_object_headers(
        self,
        body: bytes,
    ) -> list[_VectorObjectHeader]:
        headers = []
        for off in range(0, len(body) - 100, 4):
            try:
                header_len, point_header_len, stride_a, stride_b = struct.unpack_from(">IIII", body, off)
                point_count = struct.unpack_from(">I", body, off + 16)[0]
                x0, y0, x1, y1 = struct.unpack_from(">IIII", body, off + 24)
                opacity = struct.unpack_from(">d", body, off + 64)[0]
                family_id = struct.unpack_from(">I", body, off + 76)[0]
                line_style_id, fill_style_id, width = struct.unpack_from(">IId", body, off + 80)
                extra_id = struct.unpack_from(">I", body, off + 96)[0]
            except struct.error:
                continue
            if header_len != 100 or point_header_len != 76 or stride_a not in (88, 104) or stride_b != 88:
                continue
            if not (0 <= x0 < x1 <= self.width * 2 and 0 <= y0 < y1 <= self.height * 2):
                continue
            if not (0.0 <= opacity <= 1.1 and np.isfinite(width) and 0.0 < width < 100.0):
                continue
            if line_style_id > 10000 or fill_style_id > 10000:
                continue
            line = []
            fill = []
            for color_off in (off + 40, off + 44, off + 48):
                line.append(struct.unpack_from(">H", body, color_off)[0] // 257)
            for color_off in (off + 52, off + 56, off + 60):
                fill.append(struct.unpack_from(">H", body, color_off)[0] // 257)
            bbox = (
                max(0, min(int(x0), self.width)),
                max(0, min(int(y0), self.height)),
                max(0, min(int(x1), self.width)),
                max(0, min(int(y1), self.height)),
            )
            point_bbox = None
            if 1 <= point_count <= 1000:
                points_start = off + header_len
                xs = []
                ys = []
                for idx in range(point_count):
                    point_off = points_start + idx * stride_a
                    if point_off + 16 > len(body):
                        xs = []
                        ys = []
                        break
                    px = struct.unpack_from(">d", body, point_off)[0]
                    py = struct.unpack_from(">d", body, point_off + 8)[0]
                    if not (np.isfinite(px) and np.isfinite(py)):
                        xs = []
                        ys = []
                        break
                    xs.append(float(px))
                    ys.append(float(py))
                if xs and ys:
                    point_bbox = (
                        max(0, min(int(np.floor(min(xs))), self.width)),
                        max(0, min(int(np.floor(min(ys))), self.height)),
                        max(0, min(int(np.ceil(max(xs))), self.width)),
                        max(0, min(int(np.ceil(max(ys))), self.height)),
                    )
            headers.append(_VectorObjectHeader(
                bbox=bbox,
                line_rgb=tuple(max(0, min(int(c), 255)) for c in line),
                fill_rgb=tuple(max(0, min(int(c), 255)) for c in fill),
                opacity=max(0.0, min(float(opacity), 1.0)),
                width=float(width),
                line_style_id=int(line_style_id),
                fill_style_id=int(fill_style_id),
                family_id=int(family_id),
                extra_id=int(extra_id),
                point_bbox=point_bbox,
            ))
        return headers

    def _vector_object_records_100(
        self,
        body: bytes,
    ) -> list[_VectorObjectRecord]:
        records: list[_VectorObjectRecord] = []
        for off in range(0, len(body) - 100, 4):
            try:
                header_len, subclass_tail_off, point_stride, point_tail_offset = struct.unpack_from(">IIII", body, off)
                point_count = struct.unpack_from(">I", body, off + 16)[0]
                object_flags = struct.unpack_from(">I", body, off + 20)[0]
                x0, y0, x1, y1 = struct.unpack_from(">IIII", body, off + 24)
                opacity = struct.unpack_from(">d", body, off + 64)[0]
                family_id = struct.unpack_from(">I", body, off + 76)[0]
                line_style_id, fill_style_id, width = struct.unpack_from(">IId", body, off + 80)
                extra_id = struct.unpack_from(">I", body, off + 96)[0]
            except struct.error:
                continue
            if header_len != 100 or subclass_tail_off != 76:
                continue
            if point_stride not in (88, 104) or point_tail_offset != 88:
                continue
            if not (1 <= point_count <= 1000):
                continue
            if off + header_len + point_count * point_stride > len(body):
                continue
            if not (0 <= x0 < x1 <= self.width * 2 and 0 <= y0 < y1 <= self.height * 2):
                continue
            if not (0.0 <= opacity <= 1.1 and np.isfinite(width) and 0.0 < width < 100.0):
                continue
            if line_style_id > 10000 or fill_style_id > 10000:
                continue
            line = []
            fill = []
            for color_off in (off + 40, off + 44, off + 48):
                line.append(struct.unpack_from(">H", body, color_off)[0] // 257)
            for color_off in (off + 52, off + 56, off + 60):
                fill.append(struct.unpack_from(">H", body, color_off)[0] // 257)
            records.append(_VectorObjectRecord(
                off=off,
                header_len=int(header_len),
                point_stride=int(point_stride),
                point_tail_offset=int(point_tail_offset),
                point_count=int(point_count),
                object_flags=int(object_flags),
                line_rgb=tuple(max(0, min(int(c), 255)) for c in line),
                fill_rgb=tuple(max(0, min(int(c), 255)) for c in fill),
                opacity=max(0.0, min(float(opacity), 1.0)),
                width=float(width),
                line_style_id=int(line_style_id),
                fill_style_id=int(fill_style_id),
                family_id=int(family_id),
                extra_id=int(extra_id),
            ))
        return records

    def _gradation_solid_fill_color(self, layer_id: int) -> Optional[tuple[int, int, int]]:
        row = self._layer_row(layer_id)
        if row is None:
            return None
        if "GradationFillInfo" not in row.keys():
            return None
        blob = row["GradationFillInfo"]
        if not isinstance(blob, bytes) or len(blob) < 70:
            return None
        try:
            pos = 8
            name_len = struct.unpack_from(">I", blob, pos)[0]
            pos += 4
            if name_len <= 0 or pos + name_len * 2 + 4 > len(blob):
                return None
            name = blob[pos:pos + name_len * 2].decode("utf-16-be")
            pos += name_len * 2
            if name != "GradationData":
                return None
            payload_len = struct.unpack_from(">I", blob, pos)[0]
            pos += 4
            if payload_len < 24 or pos + payload_len > len(blob):
                return None
            header_len, node_stride, node_count = struct.unpack_from(">III", blob, pos)
            if header_len < 24 or node_stride < 28 or node_count <= 0:
                return None
            nodes_start = pos + header_len
            if nodes_start + node_count * node_stride > pos + payload_len:
                return None
            colors: list[tuple[int, int, int]] = []
            for idx in range(node_count):
                node_off = nodes_start + idx * node_stride
                raw = struct.unpack_from(">3I", blob, node_off)
                colors.append(tuple((value >> 24) & 0xFF for value in raw))
        except (struct.error, UnicodeDecodeError):
            return None
        if not colors or any(color != colors[0] for color in colors):
            return None
        return colors[0]

    def _frame_background_fill_color(self, layer: sqlite3.Row) -> Optional[tuple[int, int, int]]:
        for child_id in self._walk_chain(layer["LayerFirstChildIndex"]):
            color = self._gradation_solid_fill_color(child_id)
            if color is not None:
                return color
        return None

    def _frame_fill_color(
        self,
        layer: sqlite3.Row,
        object_fill: tuple[int, int, int],
    ) -> Optional[tuple[int, int, int]]:
        child_fill = self._frame_background_fill_color(layer)
        if child_fill is not None:
            return child_fill
        white_checked = True
        if "ComicFrameColorTypeWhiteChecked" in layer.keys():
            white_checked = bool(layer["ComicFrameColorTypeWhiteChecked"])
        if white_checked:
            return (255, 255, 255)
        if object_fill != (0, 0, 0):
            return object_fill
        return None

    def _frame_line_style(
        self,
        layer: sqlite3.Row,
        line_rgb: tuple[int, int, int],
        object_opacity: float,
        vector_width: float,
    ) -> Optional[tuple[tuple[int, int, int], int, int]]:
        black_checked = True
        if "ComicFrameColorTypeBlackChecked" in layer.keys():
            black_checked = bool(layer["ComicFrameColorTypeBlackChecked"])
        alpha_scale = max(0.0, min(float(object_opacity), 1.0))
        if line_rgb == (0, 0, 0):
            if not black_checked:
                return None
            return line_rgb, int(round(FRAME_LINE_FALLBACK_BLACK_ALPHA * alpha_scale)), self._fallback_outline_width(vector_width, extra=-1)
        return line_rgb, int(round(255 * alpha_scale)), 0

    def _fallback_outline_width(self, vector_width: float, extra: int = 1) -> int:
        if not np.isfinite(vector_width) or vector_width <= 0.0:
            vector_width = VECTOR_OBJECT_FALLBACK_DEFAULT_WIDTH
        return max(1, int(np.ceil(float(vector_width))) + int(extra))

    def _vector_object_family_ids(self, layer: sqlite3.Row) -> set[int]:
        body = self._vector_object_body(layer["MainId"])
        if body is None:
            return set()
        return {header.family_id for header in self._vector_object_headers(body)}

    def _is_frame_vector_layer(self, layer: sqlite3.Row) -> bool:
        if "VectorNormalType" in layer.keys() and layer["VectorNormalType"] == VECTOR_NORMAL_TYPE_FRAME:
            return True
        if "ComicFrameLineMipmap" in layer.keys() and layer["ComicFrameLineMipmap"]:
            return True
        return VECTOR_FAMILY_FRAME in self._vector_object_family_ids(layer)

    def _is_balloon_vector_layer(self, layer: sqlite3.Row) -> bool:
        if self._is_frame_vector_layer(layer):
            return False
        if "VectorNormalType" in layer.keys() and layer["VectorNormalType"] == VECTOR_NORMAL_TYPE_BALLOON:
            return True
        if "VectorNormalBalloonIndex" in layer.keys() and layer["VectorNormalBalloonIndex"]:
            return True
        return VECTOR_FAMILY_BALLOON in self._vector_object_family_ids(layer)

    def _draw_rect_rgba(
        self,
        rgba: np.ndarray,
        bbox: tuple[int, int, int, int],
        fill: Optional[tuple[int, int, int, int]] = None,
        outline: Optional[tuple[int, int, int, int]] = None,
        width: int = 1,
    ) -> None:
        x0, y0, x1, y1 = bbox
        if x1 <= x0 or y1 <= y0:
            return
        if fill is not None:
            rgba[y0:y1, x0:x1] = fill
        if outline is None:
            return
        width = max(1, min(int(width), x1 - x0, y1 - y0))
        rgba[y0:y0 + width, x0:x1] = outline
        rgba[y1 - width:y1, x0:x1] = outline
        rgba[y0:y1, x0:x0 + width] = outline
        rgba[y0:y1, x1 - width:x1] = outline

    def _draw_ellipse_rgba(
        self,
        rgba: np.ndarray,
        bbox: tuple[int, int, int, int],
        fill: tuple[int, int, int, int],
        outline: Optional[tuple[int, int, int, int]] = None,
        width: int = 1,
        power: float = 2.0,
    ) -> None:
        x0, y0, x1, y1 = bbox
        if x1 <= x0 or y1 <= y0:
            return
        yy, xx = np.ogrid[y0:y1, x0:x1]
        cx = (x0 + x1 - 1) / 2.0
        cy = (y0 + y1 - 1) / 2.0
        rx = max((x1 - x0) / 2.0, 1.0)
        ry = max((y1 - y0) / 2.0, 1.0)
        inside = np.abs((xx - cx) / rx) ** power + np.abs((yy - cy) / ry) ** power <= 1.0
        region = rgba[y0:y1, x0:x1]
        region[inside] = fill
        if outline is None:
            return
        width = max(1, min(int(width), x1 - x0, y1 - y0))
        inner_rx = max(rx - width, 1.0)
        inner_ry = max(ry - width, 1.0)
        inner = np.abs((xx - cx) / inner_rx) ** power + np.abs((yy - cy) / inner_ry) ** power <= 1.0
        region[inside & ~inner] = outline

    def _draw_ellipse_rgba_supersampled(
        self,
        rgba: np.ndarray,
        bbox: tuple[int, int, int, int],
        fill: tuple[int, int, int, int],
        outline: Optional[tuple[int, int, int, int]] = None,
        width: int = 1,
        power: float = 2.0,
        scale: int = 2,
    ) -> None:
        x0, y0, x1, y1 = bbox
        if x1 <= x0 or y1 <= y0:
            return
        scale = max(2, int(scale))
        height = y1 - y0
        width_px = x1 - x0
        yy, xx = np.ogrid[0:height * scale, 0:width_px * scale]
        sample_x = (xx + 0.5) / scale + x0
        sample_y = (yy + 0.5) / scale + y0
        cx = (x0 + x1 - 1) / 2.0
        cy = (y0 + y1 - 1) / 2.0
        rx = max((x1 - x0) / 2.0, 1.0)
        ry = max((y1 - y0) / 2.0, 1.0)
        inside = np.abs((sample_x - cx) / rx) ** power + np.abs((sample_y - cy) / ry) ** power <= 1.0
        high = np.zeros((height * scale, width_px * scale, 4), dtype=np.uint8)
        high[inside] = fill
        if outline is not None:
            outline_width = max(1, min(int(width), x1 - x0, y1 - y0))
            inner_rx = max(rx - outline_width, 1.0)
            inner_ry = max(ry - outline_width, 1.0)
            inner = (
                np.abs((sample_x - cx) / inner_rx) ** power
                + np.abs((sample_y - cy) / inner_ry) ** power
                <= 1.0
            )
            high[inside & ~inner] = outline

        alpha = high[..., 3].reshape(height, scale, width_px, scale).mean(axis=(1, 3))
        if not np.any(alpha):
            return
        src = np.zeros((height, width_px, 4), dtype=np.uint8)
        alpha_unit = alpha / 255.0
        for channel in range(3):
            premul = (
                high[..., channel].astype(np.float32)
                * (high[..., 3].astype(np.float32) / 255.0)
            ).reshape(height, scale, width_px, scale).mean(axis=(1, 3))
            with np.errstate(invalid="ignore", divide="ignore"):
                src[..., channel] = np.clip(
                    np.floor(np.where(alpha_unit > 1e-6, premul / alpha_unit, 0.0) + 0.5),
                    0,
                    255,
                ).astype(np.uint8)
        src[..., 3] = np.clip(np.floor(alpha + 0.5), 0, 255).astype(np.uint8)
        self._alpha_over_rgba(rgba[y0:y1, x0:x1], src)

    def _draw_polygon_rgba(
        self,
        rgba: np.ndarray,
        points: list[tuple[float, float]],
        color: tuple[int, int, int, int],
        scale: int = 1,
    ) -> None:
        if len(points) < 3:
            return
        scale = max(1, min(int(scale), 8))
        min_x = max(int(math.floor(min(x for x, _ in points) - 1.0)), 0)
        max_x = min(int(math.ceil(max(x for x, _ in points) + 1.0)), self.width)
        min_y = max(int(math.floor(min(y for _, y in points) - 1.0)), 0)
        max_y = min(int(math.ceil(max(y for _, y in points) + 1.0)), self.height)
        if min_x >= max_x or min_y >= max_y:
            return

        width_px = max_x - min_x
        height_px = max_y - min_y
        sample_x = (np.arange(width_px * scale, dtype=np.float64) + 0.5) / scale + min_x
        sample_y = (np.arange(height_px * scale, dtype=np.float64) + 0.5) / scale + min_y
        yy, xx = np.meshgrid(sample_y, sample_x, indexing="ij")
        inside = np.zeros((height_px * scale, width_px * scale), dtype=bool)
        prev_x, prev_y = points[-1]
        for next_x, next_y in points:
            if abs(next_y - prev_y) > 1e-12:
                crosses = (prev_y > yy) != (next_y > yy)
                x_at_y = (next_x - prev_x) * (yy - prev_y) / (next_y - prev_y) + prev_x
                inside ^= crosses & (xx < x_at_y)
            prev_x, prev_y = next_x, next_y
        if not np.any(inside):
            return

        if scale == 1:
            alpha = inside.astype(np.uint8) * int(max(0, min(color[3], 255)))
        else:
            alpha = (
                inside.reshape(height_px, scale, width_px, scale).mean(axis=(1, 3))
                * float(max(0, min(color[3], 255)))
            )
            alpha = np.clip(np.floor(alpha + 0.5), 0, 255).astype(np.uint8)
        src = np.zeros((height_px, width_px, 4), dtype=np.uint8)
        src[..., :3] = color[:3]
        src[..., 3] = alpha
        self._alpha_over_rgba(rgba[min_y:max_y, min_x:max_x], src)

    def _draw_polyline_rgba(
        self,
        rgba: np.ndarray,
        points: list[tuple[float, float]],
        color: tuple[int, int, int, int],
        radius: float,
        feather: float = 0.0,
        hard_alpha_mode: str = "overwrite",
    ) -> None:
        if len(points) < 2:
            return
        radius = max(float(radius), 1.0)
        feather = max(float(feather), 0.0)
        radius2 = radius * radius
        outer_radius = radius + feather
        for (x0, y0), (x1, y1) in zip(points, points[1:]):
            min_x = max(int(np.floor(min(x0, x1) - outer_radius - 1.0)), 0)
            max_x = min(int(np.ceil(max(x0, x1) + outer_radius + 1.0)), self.width - 1)
            min_y = max(int(np.floor(min(y0, y1) - outer_radius - 1.0)), 0)
            max_y = min(int(np.ceil(max(y0, y1) + outer_radius + 1.0)), self.height - 1)
            if max_x < min_x or max_y < min_y:
                continue
            yy, xx = np.ogrid[min_y:max_y + 1, min_x:max_x + 1]
            vx = x1 - x0
            vy = y1 - y0
            denom = vx * vx + vy * vy
            if denom <= 1e-6:
                dist2 = (xx - x0) ** 2 + (yy - y0) ** 2
            else:
                t = np.clip(((xx - x0) * vx + (yy - y0) * vy) / denom, 0.0, 1.0)
                px = x0 + t * vx
                py = y0 + t * vy
                dist2 = (xx - px) ** 2 + (yy - py) ** 2
            region = rgba[min_y:max_y + 1, min_x:max_x + 1]
            if feather <= 0.0:
                mask = dist2 <= radius2
                if hard_alpha_mode == "max" and color[3] < 255:
                    region[mask & (color[3] > region[..., 3])] = color
                else:
                    region[mask] = color
                continue

            dist = np.sqrt(dist2)
            coverage = np.clip(
                radius + feather - dist,
                0.0,
                feather * 2.0,
            ) / (feather * 2.0)
            if not np.any(coverage > 0.0):
                continue
            if hard_alpha_mode == "max" and color[3] < 255:
                src_alpha = np.clip(
                    np.floor(coverage * float(color[3]) + 0.5),
                    0,
                    255,
                ).astype(np.uint8)
                mask = src_alpha > region[..., 3]
                region[..., :3][mask] = color[:3]
                region[..., 3][mask] = src_alpha[mask]
                continue
            src_a = (coverage * (color[3] / 255.0))[..., None]
            dst_a = region[..., 3:4].astype(np.float32) / 255.0
            src_rgb = np.array(color[:3], dtype=np.float32).reshape(1, 1, 3) / 255.0
            dst_rgb = region[..., :3].astype(np.float32) / 255.0
            out_a = src_a + dst_a * (1.0 - src_a)
            with np.errstate(invalid="ignore", divide="ignore"):
                out_rgb = np.where(
                    out_a > 1e-6,
                    (src_rgb * src_a + dst_rgb * dst_a * (1.0 - src_a)) / out_a,
                    0.0,
                )
            region[..., :3] = np.clip(
                np.floor(out_rgb * 255.0 + 0.5), 0, 255
            ).astype(np.uint8)
            region[..., 3] = np.clip(
                np.floor(out_a[..., 0] * 255.0 + 0.5), 0, 255
            ).astype(np.uint8)

    def _draw_polyline_ellipse_rgba(
        self,
        rgba: np.ndarray,
        points: list[tuple[float, float]],
        color: tuple[int, int, int, int],
        radius: float,
        thickness: float,
        rotation_degrees: float,
        feather: float = 0.0,
        hard_alpha_mode: str = "overwrite",
    ) -> None:
        if len(points) < 2:
            return
        radius_major = max(float(radius), 1.0)
        radius_minor = max(radius_major * max(min(float(thickness), 1.0), 0.05), 1.0)
        feather = max(float(feather), 0.0)
        angle = np.deg2rad(float(rotation_degrees))
        cos_a = float(np.cos(angle))
        sin_a = float(np.sin(angle))
        outer_radius = radius_major + feather

        def _transform(x: np.ndarray | float, y: np.ndarray | float) -> tuple[np.ndarray | float, np.ndarray | float]:
            u = (x * cos_a + y * sin_a) / radius_major
            v = (-x * sin_a + y * cos_a) / radius_minor
            return u, v

        for (x0, y0), (x1, y1) in zip(points, points[1:]):
            min_x = max(int(np.floor(min(x0, x1) - outer_radius - 1.0)), 0)
            max_x = min(int(np.ceil(max(x0, x1) + outer_radius + 1.0)), self.width - 1)
            min_y = max(int(np.floor(min(y0, y1) - outer_radius - 1.0)), 0)
            max_y = min(int(np.ceil(max(y0, y1) + outer_radius + 1.0)), self.height - 1)
            if max_x < min_x or max_y < min_y:
                continue
            yy, xx = np.ogrid[min_y:max_y + 1, min_x:max_x + 1]
            tx0, ty0 = _transform(x0, y0)
            tx1, ty1 = _transform(x1, y1)
            txx, tyy = _transform(xx, yy)
            vx = tx1 - tx0
            vy = ty1 - ty0
            denom = vx * vx + vy * vy
            if denom <= 1e-6:
                dist2 = (txx - tx0) ** 2 + (tyy - ty0) ** 2
            else:
                t = np.clip(((txx - tx0) * vx + (tyy - ty0) * vy) / denom, 0.0, 1.0)
                px = tx0 + t * vx
                py = ty0 + t * vy
                dist2 = (txx - px) ** 2 + (tyy - py) ** 2
            region = rgba[min_y:max_y + 1, min_x:max_x + 1]
            if feather <= 0.0:
                mask = dist2 <= 1.0
                if hard_alpha_mode == "max" and color[3] < 255:
                    region[mask & (color[3] > region[..., 3])] = color
                else:
                    region[mask] = color
                continue

            dist = np.sqrt(dist2)
            feather_unit = max(feather / radius_minor, 1e-6)
            coverage = np.clip(
                1.0 + feather_unit - dist,
                0.0,
                feather_unit * 2.0,
            ) / (feather_unit * 2.0)
            if not np.any(coverage > 0.0):
                continue
            if hard_alpha_mode == "max" and color[3] < 255:
                src_alpha = np.clip(
                    np.floor(coverage * float(color[3]) + 0.5),
                    0,
                    255,
                ).astype(np.uint8)
                mask = src_alpha > region[..., 3]
                region[..., :3][mask] = color[:3]
                region[..., 3][mask] = src_alpha[mask]
                continue
            src_a = (coverage * (color[3] / 255.0))[..., None]
            dst_a = region[..., 3:4].astype(np.float32) / 255.0
            src_rgb = np.array(color[:3], dtype=np.float32).reshape(1, 1, 3) / 255.0
            dst_rgb = region[..., :3].astype(np.float32) / 255.0
            out_a = src_a + dst_a * (1.0 - src_a)
            with np.errstate(invalid="ignore", divide="ignore"):
                out_rgb = np.where(
                    out_a > 1e-6,
                    (src_rgb * src_a + dst_rgb * dst_a * (1.0 - src_a)) / out_a,
                    0.0,
                )
            region[..., :3] = np.clip(
                np.floor(out_rgb * 255.0 + 0.5), 0, 255
            ).astype(np.uint8)
            region[..., 3] = np.clip(
                np.floor(out_a[..., 0] * 255.0 + 0.5), 0, 255
            ).astype(np.uint8)

    def _native_stretched_ellipse_aa_coverage(
        self,
        center: tuple[float, float],
        radius_major: float,
        radius_minor: float,
        aa_width: float,
        rotation_degrees: float,
        bounds: tuple[int, int, int, int],
    ) -> np.ndarray:
        """Port of CSP's 0x142640420 stretched AA row helper."""
        min_x, max_x, min_y, max_y = bounds
        coverage_i = np.zeros((max_y - min_y + 1, max_x - min_x + 1), dtype=np.int32)
        major_sq = radius_major * radius_major
        minor_sq = radius_minor * radius_minor
        if major_sq <= 0.0 or minor_sq <= 0.0:
            return coverage_i

        # 0x14263F410 applies the style 0x40 axis flag before the table lookup,
        # so saved thickness strokes enter 0x142640420 as user angle + 90deg.
        angle = math.radians(float(rotation_degrees) + 90.0)
        sin_a = math.sin(angle)
        cos_a = math.cos(angle)
        sin_sq = sin_a * sin_a
        cos_sq = cos_a * cos_a
        x_coeff = minor_sq * sin_sq + major_sq * cos_sq
        row_coeff = minor_sq * cos_sq + major_sq * sin_sq
        if x_coeff <= 1e-12 or row_coeff <= 1e-12:
            return coverage_i
        cross = (major_sq - minor_sq) * sin_a * cos_a
        det = major_sq * minor_sq
        cx, cy = center
        cy0 = cy - 0.5
        y_extent = math.sqrt(x_coeff)

        inner_major = max(radius_major - aa_width, 0.0)
        inner_minor = max(radius_minor - aa_width, 0.0)
        have_inner = inner_major > 0.0 and inner_minor > 0.0
        if have_inner:
            inner_major_sq = inner_major * inner_major
            inner_minor_sq = inner_minor * inner_minor
            inner_x_coeff = inner_minor_sq * sin_sq + inner_major_sq * cos_sq
            inner_row_coeff = inner_minor_sq * cos_sq + inner_major_sq * sin_sq
            inner_cross_base = (inner_major_sq - inner_minor_sq) * sin_a * cos_a
            inner_det = inner_major_sq * inner_minor_sq
            have_inner = inner_x_coeff > 1e-12 and inner_row_coeff > 1e-12

        for row_index, y in enumerate(range(min_y, max_y + 1)):
            yr = float(y) - cy0
            row_cross = yr * cross
            disc = row_cross * row_cross - (yr * yr * row_coeff - det) * x_coeff
            if disc <= 0.0:
                continue
            root = math.sqrt(disc)
            left = cx - (root + row_cross) / x_coeff + 0.5
            right = cx - (row_cross - root) / x_coeff + 0.4999
            xs = max(min_x, int(left))
            xe = min(max_x + 1, int(right))
            if xe <= xs:
                continue

            cap = 1.0
            inner_left = inner_right = (left + right) * 0.5
            if have_inner:
                inner_cross = yr * inner_cross_base
                inner_disc = (
                    inner_cross * inner_cross
                    - (yr * yr * inner_row_coeff - inner_det) * inner_x_coeff
                )
                if inner_disc > 0.0:
                    inner_root = math.sqrt(inner_disc)
                    inner_left = cx - (inner_root + inner_cross) / inner_x_coeff + 0.5
                    inner_right = cx - (inner_cross - inner_root) / inner_x_coeff + 0.4999
                    q1 = left + (right - left) * 0.25
                    q3 = left + (right - left) * 0.75
                    if q1 > inner_right:
                        width = inner_right - inner_left
                        inner_right = q1
                        inner_left = q1 - width
                    elif inner_left > q3:
                        width = inner_right - inner_left
                        inner_left = q3
                        inner_right = q3 + width
                else:
                    raw = (
                        (float(y) - (cy0 - y_extent) + 0.5) / aa_width
                        if cy0 > float(y)
                        else ((cy0 + y_extent) - float(y) - 0.5) / aa_width
                    )
                    cap = max(0.0, min(raw, 1.0))
            else:
                raw = (
                    (float(y) - (cy0 - y_extent) + 0.5) / aa_width
                    if cy0 > float(y)
                    else ((cy0 + y_extent) - float(y) - 0.5) / aa_width
                )
                cap = max(0.0, min(raw, 1.0))

            left_end = max(xs, min(xe, int(inner_left)))
            right_start = max(xs, min(xe, int(inner_right)))
            row = coverage_i[row_index]
            denom_left = inner_left - left + 1.0
            if denom_left > 1e-12:
                for x in range(xs, left_end):
                    value = (float(x) + 1.0 - left) * cap / denom_left
                    row[x - min_x] = max(0, min(int(min(value, 1.0) * 32768.0), 32768))
            if right_start > left_end:
                row[left_end - min_x:right_start - min_x] = 32768
            denom_right = right - inner_right + 1.0
            if denom_right > 1e-12:
                for x in range(right_start, xe):
                    value = (right - float(x) - 1.0) * cap / denom_right
                    row[x - min_x] = max(0, min(int(min(value, 1.0) * 32768.0), 32768))

        return coverage_i

    def _draw_native_dab_rgba(
        self,
        rgba: np.ndarray,
        center: tuple[float, float],
        color: tuple[int, int, int],
        radius: float,
        opacity_cap: float,
        flow: float,
        aa_width: float,
        direct_max_accum: bool = False,
        alpha_i: Optional[np.ndarray] = None,
        thickness: float = 1.0,
        rotation_degrees: float = 0.0,
        hardness: float = 1.0,
    ) -> None:
        radius = max(float(radius), 0.0)
        opacity_cap = max(0.0, min(float(opacity_cap), 1.0))
        flow = max(0.0, min(float(flow), 1.0))
        aa_width = max(float(aa_width), 0.0)
        hardness = float(hardness) if np.isfinite(hardness) else 1.0
        hardness = max(0.0, min(hardness, 1.0))
        use_hardness_profile = hardness < 0.999999
        if (aa_width > 0.0 or use_hardness_profile) and 0.0 < radius < 1.0:
            # Native 0x1422D8550 promotes sub-1px AA/soft dabs to a 1px
            # plot radius, preserving strength by scaling flow by the
            # original small radius.
            flow *= radius
            radius = 1.0
            if aa_width > 0.0:
                aa_width = max(aa_width, 1.0)
        if use_hardness_profile:
            # Native 0x1422D8550 expands soft brushes before the coverage
            # profile table is applied by 0x14263AC30.
            radius *= 1.5 - 0.5 * hardness
        if opacity_cap <= 0.0 or flow <= 0.0:
            return
        opacity_cap_i = max(0, min(int(opacity_cap * 32768.0 + 0.5000000100000001), 32768))
        flow_i = max(0, min(int(flow * 32768.0 + 0.5000000100000001), 32768))
        if opacity_cap_i <= 0 or flow_i <= 0:
            return
        cx, cy = center
        thickness = float(thickness) if np.isfinite(thickness) else 1.0
        thickness = max(0.05, min(thickness, 4.0))
        if thickness > 1.0:
            radius *= thickness
            thickness = 1.0 / thickness
            rotation_degrees += 90.0
        use_ellipse = thickness < 0.999999
        radius_major = radius
        radius_minor = max(radius_major * thickness, 0.0)
        outer_radius = radius_major if aa_width <= 0.0 else radius_major + 1.0
        min_x = max(int(np.floor(cx - outer_radius - 1.0)), 0)
        max_x = min(int(np.ceil(cx + outer_radius + 1.0)), self.width - 1)
        min_y = max(int(np.floor(cy - outer_radius - 1.0)), 0)
        max_y = min(int(np.ceil(cy + outer_radius + 1.0)), self.height - 1)
        if max_x < min_x or max_y < min_y:
            return

        yy, xx = np.ogrid[min_y:max_y + 1, min_x:max_x + 1]
        if use_ellipse and radius_major > 0.0 and radius_minor > 0.0:
            # Native 0x14263F410 switches from circular helpers to rotated
            # stretched helpers when the plot thickness ratio differs from 1.
            angle = np.deg2rad(float(rotation_degrees))
            cos_a = float(np.cos(angle))
            sin_a = float(np.sin(angle))
            dx = xx - (cx - 0.5)
            dy = yy - (cy - 0.5)
            local_x = dx * cos_a + dy * sin_a
            local_y = -dx * sin_a + dy * cos_a
            dist = np.sqrt((local_x / radius_major) ** 2 + (local_y / radius_minor) ** 2)
            if aa_width <= 0.0:
                coverage_i = np.where(dist < 1.0, 32768, 0).astype(np.int32)
            elif not use_hardness_profile:
                coverage_i = self._native_stretched_ellipse_aa_coverage(
                    center,
                    radius_major,
                    radius_minor,
                    aa_width,
                    rotation_degrees,
                    (min_x, max_x, min_y, max_y),
                )
            else:
                # Native 0x142640420 builds the AA band from an outer ellipse
                # and an inner ellipse whose major/minor axes are each reduced
                # by the AA width.
                inner_major = max(radius_major - aa_width, 1e-6)
                inner_minor = max(radius_minor - aa_width, 1e-6)
                inner_dist = np.sqrt((local_x / inner_major) ** 2 + (local_y / inner_minor) ** 2)
                band = np.maximum(inner_dist - dist, 1e-9)
                coverage = np.clip((1.0 - dist) / band, 0.0, 1.0)
                coverage = np.where(inner_dist <= 1.0, 1.0, coverage)
                coverage = np.where(dist >= 1.0, 0.0, coverage)
                coverage_i = np.floor(coverage * 32768.0).astype(np.int32)
            profile_dist = dist
        elif aa_width <= 0.0:
            coverage_i = np.zeros((max_y - min_y + 1, max_x - min_x + 1), dtype=np.int32)
            radius_sq = radius * radius
            y_center = cy - 0.5
            for row_index, y in enumerate(range(min_y, max_y + 1)):
                span_sq = radius_sq - (float(y) - y_center) ** 2
                if span_sq <= 0.0:
                    continue
                span = max(0.0, math.sqrt(span_sq) - 0.4)
                x0 = max(min_x, int(cx - span))
                x1 = min(max_x, int(cx + span))
                if x0 <= x1:
                    coverage_i[row_index, x0 - min_x:x1 - min_x + 1] = 32768
            profile_dist = np.sqrt(
                (xx - (cx - 0.5)) ** 2 + (yy - (cy - 0.5)) ** 2
            ) / max(radius, 1e-6)
        else:
            # Native circular AA measures distance from ctx_center - 0.5.
            dist = np.sqrt((xx - (cx - 0.5)) ** 2 + (yy - (cy - 0.5)) ** 2)
            coverage = np.clip((radius - dist) / aa_width, 0.0, 1.0)
            coverage_i = np.floor(coverage * 32768.0).astype(np.int32)
            profile_dist = dist / max(radius, 1e-6)
        if use_hardness_profile:
            threshold = max(0.0, min(hardness * 1.3 - 0.3, 1.0))
            if not use_ellipse:
                profile_n = 1024
                scale_i = int((profile_n * 256.0) / max(radius, 1e-9) + 0.5000000100000001)
                prof_cx = cx - (0.5 if aa_width > 0.0 else 0.0)
                prof_cy = cy - (0.5 if aa_width > 0.0 else 0.0)
                off_x_f = scale_i * prof_cx
                off_y_f = scale_i * prof_cy
                off_x = int(off_x_f + (0.5000000100000001 if off_x_f >= 0.0 else -0.5000000100000001))
                off_y = int(off_y_f + (0.5000000100000001 if off_y_f >= 0.0 else -0.5000000100000001))
                ix = np.abs(((xx.astype(np.int64) * scale_i - off_x) >> 8)).astype(np.int32)
                iy = np.abs(((yy.astype(np.int64) * scale_i - off_y) >> 8)).astype(np.int32)
                profile_dist = np.sqrt(
                    ix.astype(np.float64) * ix.astype(np.float64)
                    + iy.astype(np.float64) * iy.astype(np.float64)
                ) / float(profile_n)
            profile = np.zeros_like(profile_dist, dtype=np.float64)
            inside = profile_dist < threshold
            profile[inside] = 1.0
            falloff = (profile_dist >= threshold) & (profile_dist < 1.0)
            if np.any(falloff):
                q = (profile_dist[falloff] - threshold) / max(1.0 - threshold, 1e-9)
                profile[falloff] = np.where(
                    q < 0.5,
                    1.0 - 2.0 * q * q,
                    2.0 * (1.0 - q) * (1.0 - q),
                )
            profile_i = np.floor(np.clip(profile, 0.0, 1.0) * 32768.0).astype(np.int32)
            coverage_i = (coverage_i * profile_i) >> 15
        if not np.any(coverage_i > 0):
            return

        region = rgba[min_y:max_y + 1, min_x:max_x + 1]
        if alpha_i is None:
            alpha_region_i = ((region[..., 3].astype(np.int32) * 32768) + 127) // 255
        else:
            alpha_region_i = alpha_i[min_y:max_y + 1, min_x:max_x + 1]
        old_i = alpha_region_i.astype(np.int32, copy=False)
        flow_coverage_i = (flow_i * coverage_i) >> 15
        if direct_max_accum:
            candidate_i = (opacity_cap_i * flow_coverage_i) >> 15
            new_i = np.maximum(old_i, candidate_i)
        else:
            new_i = old_i + ((flow_coverage_i * np.maximum(opacity_cap_i - old_i, 0)) >> 15)
        changed = new_i > old_i
        if not np.any(changed):
            return
        region[..., :3][changed] = np.array(color, dtype=np.uint8)
        alpha_region_i[...] = np.maximum(alpha_region_i, new_i)
        if alpha_i is None:
            region[..., 3] = np.clip(((alpha_region_i * 255) + 16384) // 32768, 0, 255).astype(np.uint8)

    def _brush_anti_alias(self, brush_style_id: int) -> int:
        if brush_style_id <= 0:
            return 0
        try:
            row = self._db.execute(
                "SELECT AntiAlias FROM BrushStyle WHERE MainId=?",
                (int(brush_style_id),),
            ).fetchone()
        except sqlite3.Error:
            return 0
        if row is None or row["AntiAlias"] is None:
            return 0
        return max(0, min(int(row["AntiAlias"]), 3))

    def _brush_style_preview(self, brush_style_id: int) -> Optional[_BrushStylePreview]:
        if brush_style_id <= 0:
            return None
        brush_style_id = int(brush_style_id)
        if brush_style_id in self._brush_style_preview_cache:
            return self._brush_style_preview_cache[brush_style_id]
        try:
            row = self._db.execute(
                """
                SELECT StyleFlag, PatternStyle, TexturePattern, TextureFlag,
                       FlowBase, Hardness, IntervalBase, AutoIntervalType,
                       ThicknessBase, RotationBase, SprayFlag,
                       TextureComposite, TextureDensityBase, TextureScale,
                       TextureRotate, TextureOffsetX, TextureOffsetY,
                       TextureBrightness, TextureContrast
                FROM BrushStyle WHERE MainId=?
                """,
                (brush_style_id,),
            ).fetchone()
        except sqlite3.Error:
            self._brush_style_preview_cache[brush_style_id] = None
            return None
        if row is None:
            self._brush_style_preview_cache[brush_style_id] = None
            return None
        preview = _BrushStylePreview(
            style_flag=int(row["StyleFlag"] or 0),
            pattern_style=int(row["PatternStyle"] or 0),
            texture_pattern=int(row["TexturePattern"] or 0),
            texture_flag=int(row["TextureFlag"] or 0),
            flow_base=float(row["FlowBase"] if row["FlowBase"] is not None else 1.0),
            hardness=float(row["Hardness"] if row["Hardness"] is not None else 1.0),
            interval_base=float(row["IntervalBase"] if row["IntervalBase"] is not None else 1.0),
            auto_interval_type=int(row["AutoIntervalType"] or 0),
            thickness_base=float(row["ThicknessBase"] if row["ThicknessBase"] is not None else 1.0),
            rotation_base=float(row["RotationBase"] if row["RotationBase"] is not None else 0.0),
            spray_flag=int(row["SprayFlag"] or 0),
            texture_composite=int(row["TextureComposite"] or 0),
            texture_density_base=float(row["TextureDensityBase"] if row["TextureDensityBase"] is not None else 1.0),
            texture_scale=float(row["TextureScale"] if row["TextureScale"] is not None else 1.0),
            texture_rotate=float(row["TextureRotate"] if row["TextureRotate"] is not None else 0.0),
            texture_offset_x=float(row["TextureOffsetX"] if row["TextureOffsetX"] is not None else 0.0),
            texture_offset_y=float(row["TextureOffsetY"] if row["TextureOffsetY"] is not None else 0.0),
            texture_brightness=float(row["TextureBrightness"] if row["TextureBrightness"] is not None else 0.0),
            texture_contrast=float(row["TextureContrast"] if row["TextureContrast"] is not None else 0.0),
        )
        self._brush_style_preview_cache[brush_style_id] = preview
        return preview

    def _brush_is_no_pattern_dab_candidate(self, brush_style_id: int) -> bool:
        style = self._brush_style_preview(brush_style_id)
        if style is None:
            return False
        return not (
            style.retained_state
            or style.spray_flag
            or style.pattern_style
            or style.texture_pattern
            or style.texture_flag
        )

    @staticmethod
    def _native_aa_width(aa_level: int, radius: float) -> float:
        if aa_level <= 0:
            return 0.0
        cap = {1: 1.5, 2: 2.5, 3: 3.5}.get(max(0, min(int(aa_level), 3)), 0.0)
        return max(0.0, min(float(radius), cap))

    def _brush_effector_blob(self, brush_style_id: int, column: str) -> Optional[bytes]:
        if brush_style_id <= 0:
            return None
        key = (int(brush_style_id), column)
        if key in self._brush_effector_blob_cache:
            return self._brush_effector_blob_cache[key]
        try:
            row = self._db.execute(
                f"SELECT {column} FROM BrushStyle WHERE MainId=?",
                (int(brush_style_id),),
            ).fetchone()
        except sqlite3.Error:
            self._brush_effector_blob_cache[key] = None
            return None
        blob = None if row is None else row[column]
        if not isinstance(blob, bytes):
            blob = None
        self._brush_effector_blob_cache[key] = blob
        return blob

    def _brush_effector_graph_points(self, graph_id: int) -> Optional[list[tuple[float, float]]]:
        graph_id = int(graph_id)
        if graph_id <= 0:
            return None
        if graph_id in self._brush_effector_graph_cache:
            return self._brush_effector_graph_cache[graph_id]
        try:
            row = self._db.execute(
                """
                SELECT ControlNumber, ControlDataSize, ControlPoints
                FROM BrushEffectorGraphData WHERE MainId=?
                """,
                (graph_id,),
            ).fetchone()
        except sqlite3.Error:
            self._brush_effector_graph_cache[graph_id] = None
            return None
        points: list[tuple[float, float]] = []
        if row is not None and isinstance(row["ControlPoints"], bytes):
            blob = row["ControlPoints"]
            stride = int(row["ControlDataSize"] or 0)
            count = int(row["ControlNumber"] or 0)
            if stride >= 16:
                for idx in range(min(count, len(blob) // stride)):
                    point_off = idx * stride
                    x, y = struct.unpack_from(">dd", blob, point_off)
                    if np.isfinite(x) and np.isfinite(y):
                        points.append((float(x), float(y)))
        points.sort(key=lambda item: item[0])
        result = points or None
        self._brush_effector_graph_cache[graph_id] = result
        return result

    def _eval_brush_effector_graph(self, graph_id: int, value: float) -> Optional[float]:
        points = self._brush_effector_graph_points(graph_id)
        if not points:
            return None
        x = max(0.0, min(float(value), 1.0))
        return self._eval_brush_effector_graph_points(points, x)

    @staticmethod
    def _eval_brush_effector_graph_points(points: list[tuple[float, float]], x: float) -> float:
        if len(points) <= 1:
            return points[0][1] if points else 0.0
        if len(points) == 2:
            return ClipFile._eval_brush_effector_graph_line(points[0], points[1], x)
        if len(points) == 3:
            return ClipFile._eval_brush_effector_graph_quad(points[0], points[1], points[2], x)

        for idx in range(1, len(points) - 1):
            start = points[0] if idx == 1 else ClipFile._midpoint(points[idx - 1], points[idx])
            end = points[-1] if idx == len(points) - 2 else ClipFile._midpoint(points[idx], points[idx + 1])
            if x <= end[0] or idx == len(points) - 2:
                return ClipFile._eval_brush_effector_graph_quad(start, points[idx], end, x)
        return points[-1][1]

    @staticmethod
    def _midpoint(a: tuple[float, float], b: tuple[float, float]) -> tuple[float, float]:
        return ((a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5)

    @staticmethod
    def _eval_brush_effector_graph_line(
        a: tuple[float, float],
        b: tuple[float, float],
        x: float,
    ) -> float:
        span = b[0] - a[0]
        if abs(span) <= 1e-9:
            return a[1]
        t = (x - a[0]) / span
        return a[1] + (b[1] - a[1]) * t

    @staticmethod
    def _eval_brush_effector_graph_quad(
        a: tuple[float, float],
        b: tuple[float, float],
        c: tuple[float, float],
        x: float,
    ) -> float:
        qa = a[0] - 2.0 * b[0] + c[0]
        qb = 2.0 * (b[0] - a[0])
        qc = a[0] - x
        roots: list[float] = []
        if abs(qa) <= 1e-9:
            if abs(qb) > 1e-9:
                roots.append(-qc / qb)
        else:
            disc = qb * qb - 4.0 * qa * qc
            if disc >= -1e-9:
                if disc <= 1e-9:
                    roots.append(-qb / (2.0 * qa))
                else:
                    sqrt_disc = math.sqrt(max(0.0, disc))
                    roots.append((-qb - sqrt_disc) / (2.0 * qa))
                    roots.append((-qb + sqrt_disc) / (2.0 * qa))
        for t in roots:
            if 0.0 < t < 1.0:
                omt = 1.0 - t
                return omt * omt * a[1] + 2.0 * omt * t * b[1] + t * t * c[1]
        return ClipFile._eval_brush_effector_graph_line(a, c, x)

    def _brush_pressure_effector_value(
        self,
        brush_style_id: int,
        column: str,
        primary_scalar: float,
        secondary_scalar: float,
    ) -> Optional[float]:
        blob = self._brush_effector_blob(brush_style_id, column)
        if not isinstance(blob, bytes) or len(blob) != 24:
            return None
        flag, _mode, graph0, amount0, graph1, amount1 = struct.unpack(">III f I f", blob)
        if flag != 0x31:
            return None
        primary = self._eval_brush_effector_graph(graph0, primary_scalar)
        if primary is None:
            return None
        primary_factor = (1.0 - float(amount0)) + float(amount0) * primary
        secondary = self._eval_brush_effector_graph(graph1, secondary_scalar)
        if secondary is not None:
            # Native compact 0x20 enables a range lane with default low 100%
            # and high amount1*100%, then multiplies the primary branch.
            primary_factor *= 1.0 + (float(amount1) - 1.0) * secondary
        return max(0.0, min(float(primary_factor), 1.0))

    def _brush_single_graph_effector_value(
        self,
        brush_style_id: int,
        column: str,
        primary_scalar: float,
    ) -> Optional[float]:
        blob = self._brush_effector_blob(brush_style_id, column)
        if not isinstance(blob, bytes) or len(blob) != 12:
            return None
        flag, floor_value, graph_id = struct.unpack(">I f I", blob)
        if flag != 0x11:
            return None
        graph_value = self._eval_brush_effector_graph(graph_id, primary_scalar)
        if graph_value is None:
            return None
        factor = float(floor_value) + (1.0 - float(floor_value)) * float(graph_value)
        return max(0.0, min(factor, 1.0))

    def _brush_flow_effector_value(
        self,
        brush_style_id: int,
        primary_scalar: float,
        auxiliary_scalar: float,
    ) -> Optional[float]:
        blob = self._brush_effector_blob(brush_style_id, "FlowEffector")
        if not isinstance(blob, bytes) or len(blob) != 12:
            return None
        flag, floor_value, graph_id = struct.unpack(">I f I", blob)
        if graph_id != 4:
            return None
        if flag == 0x11:
            source = primary_scalar
        elif flag == 0x41:
            source = auxiliary_scalar
        else:
            return None
        graph_value = self._eval_brush_effector_graph(graph_id, source)
        if graph_value is None:
            return None
        factor = float(floor_value) + (1.0 - float(floor_value)) * float(graph_value)
        return max(0.0, min(factor, 1.0))

    def _brush_secondary_or_aux_effector_value(
        self,
        brush_style_id: int,
        column: str,
        secondary_scalar: float,
        auxiliary_scalar: float,
    ) -> Optional[float]:
        blob = self._brush_effector_blob(brush_style_id, column)
        if not isinstance(blob, bytes):
            return None
        if len(blob) == 16:
            flag, low, graph_id, high = struct.unpack(">I f I f", blob)
            if flag != 0x21:
                return None
            source = secondary_scalar
            graph_value = self._eval_brush_effector_graph(graph_id, source)
            if graph_value is None:
                return None
            factor = float(low) + (float(high) - float(low)) * float(graph_value)
            return max(0.0, min(factor, 4.0))
        if len(blob) == 12:
            flag, floor_value, graph_id = struct.unpack(">I f I", blob)
            if flag != 0x41:
                return None
            graph_value = self._eval_brush_effector_graph(graph_id, auxiliary_scalar)
            if graph_value is None:
                return None
            factor = float(floor_value) + (1.0 - float(floor_value)) * float(graph_value)
            return max(0.0, min(factor, 4.0))
        return None

    def _brush_uses_secondary_or_aux_dynamics(self, brush_style_id: int, column: str) -> bool:
        if brush_style_id <= 0:
            return False
        effector = self._brush_effector_blob(brush_style_id, column)
        if not isinstance(effector, bytes):
            return False
        if len(effector) == 16:
            return struct.unpack_from(">I", effector, 0)[0] == 0x21
        if len(effector) == 12:
            return struct.unpack_from(">I", effector, 0)[0] == 0x41
        return False

    @staticmethod
    def _stable_unit_random(*values: int) -> float:
        state = 0x811C9DC5
        for value in values:
            state ^= int(value) & 0xFFFFFFFF
            state = (state * 0x01000193) & 0xFFFFFFFF
            state ^= state >> 13
        return ((state >> 8) & 0xFFFFFF) / float(0xFFFFFF)

    @staticmethod
    def _native_lcg_unit_random(seed: int) -> float:
        state = (1103515245 * (int(seed) & 0xFFFFFFFF) + 1234567) & 0xFFFFFFFF
        return ((state >> 16) & 0x7FFF) / 32768.0

    def _brush_random_effector_value(
        self,
        brush_style_id: int,
        column: str,
        salt: int,
        point_seed: Optional[int] = None,
    ) -> Optional[float]:
        blob = self._brush_effector_blob(brush_style_id, column)
        if not isinstance(blob, bytes) or len(blob) != 8:
            return None
        flag, floor_value = struct.unpack(">I f", blob)
        if flag != 0x81:
            return None
        floor_value = max(0.0, min(float(floor_value), 1.0))
        if column == "SizeEffector" and point_seed is not None:
            random_value = self._native_lcg_unit_random(point_seed)
        else:
            random_value = self._stable_unit_random(brush_style_id, salt, len(column))
        return floor_value + (1.0 - floor_value) * random_value

    def _brush_uses_random_dynamics(self, brush_style_id: int, column: str) -> bool:
        if brush_style_id <= 0:
            return False
        if not self._brush_is_no_pattern_dab_candidate(brush_style_id):
            return False
        effector = self._brush_effector_blob(brush_style_id, column)
        return (
            isinstance(effector, bytes)
            and len(effector) == 8
            and struct.unpack_from(">I", effector, 0)[0] == 0x81
        )

    def _brush_uses_flow_dynamics(self, brush_style_id: int) -> bool:
        if brush_style_id <= 0:
            return False
        if not self._brush_is_no_pattern_dab_candidate(brush_style_id):
            return False
        flow_effector = self._brush_effector_blob(brush_style_id, "FlowEffector")
        if not isinstance(flow_effector, bytes) or len(flow_effector) != 12:
            return False
        flag, floor_value, graph_id = struct.unpack(">I f I", flow_effector)
        return flag in (0x11, 0x41) and graph_id == 4 and abs(float(floor_value) - 0.5) <= 1e-6

    def _brush_uses_simple_size_dynamics(self, brush_style_id: int) -> bool:
        if brush_style_id <= 0:
            return False
        size_effector = self._brush_effector_blob(brush_style_id, "SizeEffector")
        return (
            isinstance(size_effector, bytes)
            and len(size_effector) == 12
            and struct.unpack_from(">I", size_effector, 0)[0] == 0x11
        )

    def _brush_uses_pressure_size_dynamics(self, brush_style_id: int) -> bool:
        if brush_style_id <= 0:
            return False
        size_effector = self._brush_effector_blob(brush_style_id, "SizeEffector")
        return (
            isinstance(size_effector, bytes)
            and len(size_effector) == 24
            and struct.unpack_from(">I", size_effector, 0)[0] == 0x31
        )

    def _brush_uses_pressure_opacity_dynamics(self, brush_style_id: int) -> bool:
        if brush_style_id <= 0:
            return False
        opacity_effector = self._brush_effector_blob(brush_style_id, "OpacityEffector")
        return (
            isinstance(opacity_effector, bytes)
            and len(opacity_effector) == 24
            and struct.unpack_from(">I", opacity_effector, 0)[0] == 0x31
        )

    def _brush_constant_effector_multiplier(self, brush_style_id: int, column: str) -> Optional[float]:
        blob = self._brush_effector_blob(brush_style_id, column)
        if not isinstance(blob, bytes):
            return 1.0
        if len(blob) >= 4:
            flag = struct.unpack_from(">I", blob, 0)[0]
            if flag in (0, 1):
                return 1.0
        if len(blob) == 12:
            flag, scalar, graph_id = struct.unpack(">I f I", blob)
            if flag == 0x11 and graph_id == 0:
                return float(scalar)
        return None

    @staticmethod
    def _auto_interval_123_scalar(
        auto_interval_type: int,
        hardness: float,
        thickness: float,
    ) -> Optional[float]:
        if auto_interval_type not in (1, 2, 3):
            return None
        base = 0.08
        if auto_interval_type == 1:
            base = 0.12
        elif auto_interval_type == 3:
            base = 0.04
        scalar = base * (3.5 - max(float(hardness), 0.2) * 2.5)
        if float(thickness) < 1.0:
            scalar *= max(float(thickness), 0.01)
        return scalar

    def _brush_interval_scalar(self, brush_style_id: int) -> Optional[float]:
        style = self._brush_style_preview(brush_style_id)
        if style is None:
            return None
        if style.auto_interval_type in (1, 2, 3):
            thickness_multiplier = self._brush_constant_effector_multiplier(
                brush_style_id,
                "ThicknessEffector",
            )
            thickness = style.thickness_base
            if thickness_multiplier is not None:
                thickness *= thickness_multiplier
            return self._auto_interval_123_scalar(
                style.auto_interval_type,
                style.hardness,
                thickness,
            )
        if style.auto_interval_type in (4, 5):
            return None
        interval_multiplier = self._brush_constant_effector_multiplier(
            brush_style_id,
            "IntervalEffector",
        )
        if interval_multiplier is None:
            return None
        return max(0.0, float(style.interval_base) * float(interval_multiplier))

    def _adaptive_vector_sample_step(
        self,
        brush_style_id: int,
        width: float,
        fallback_step: float,
        endpoint_tapers: tuple[float, float],
        endpoint_size_factors: tuple[float, float],
        use_size_dynamics: bool,
        adaptive_enabled: bool,
    ) -> float:
        if not adaptive_enabled or os.environ.get(VECTOR_EXPERIMENTAL_ADAPTIVE_SPACING_ENV) != "1":
            return fallback_step
        interval_scalar = self._brush_interval_scalar(brush_style_id)
        if interval_scalar is None or interval_scalar <= 0.0:
            return fallback_step
        estimates: list[float] = []
        for taper, size_factor in zip(endpoint_tapers, endpoint_size_factors):
            size_multiplier = taper if use_size_dynamics else 1.0
            effective_size = float(width) * max(0.0, min(float(size_factor), 4.0))
            effective_size *= max(0.0, min(float(size_multiplier), 4.0))
            estimates.append(2.0 * max(0.1, effective_size) * float(interval_scalar))
        native_step = sum(estimates) / len(estimates)
        if not np.isfinite(native_step) or native_step <= 0.0:
            return fallback_step
        return max(
            VECTOR_ADAPTIVE_SPACING_MIN_STEP,
            min(float(fallback_step), float(native_step)),
        )

    def _brush_pattern_mipmap(self, brush_style_id: int) -> Optional[int]:
        if brush_style_id <= 0:
            return None
        try:
            style = self._db.execute(
                "SELECT PatternStyle FROM BrushStyle WHERE MainId=?",
                (int(brush_style_id),),
            ).fetchone()
        except sqlite3.Error:
            return None
        if style is None or not style["PatternStyle"]:
            return None
        pattern = self._db.execute(
            "SELECT ImageIndex FROM BrushPatternStyle WHERE MainId=?",
            (int(style["PatternStyle"]),),
        ).fetchone()
        if pattern is None or not isinstance(pattern["ImageIndex"], bytes):
            return None
        image_ids = pattern["ImageIndex"]
        if len(image_ids) < 4:
            return None
        image_id = struct.unpack_from(">I", image_ids, 0)[0]
        image = self._db.execute(
            "SELECT Mipmap FROM BrushPatternImage WHERE MainId=?",
            (int(image_id),),
        ).fetchone()
        if image is None or not image["Mipmap"]:
            return None
        return int(image["Mipmap"])

    def _brush_texture_preview_alpha(self, brush_style_id: int) -> Optional[np.ndarray]:
        style = self._brush_style_preview(brush_style_id)
        if style is None:
            return None
        if not (
            style.pattern_style == 0
            and style.texture_pattern > 0
            and style.texture_composite == 0
            and abs(style.texture_scale - 1.0) <= 1e-6
            and abs(style.texture_rotate) <= 1e-6
            and abs(style.texture_offset_x) <= 1e-6
            and abs(style.texture_offset_y) <= 1e-6
            and abs(style.texture_brightness) <= 1e-6
            and abs(style.texture_contrast) <= 1e-6
        ):
            return None
        try:
            row = self._db.execute(
                "SELECT Mipmap FROM BrushPatternImage WHERE MainId=?",
                (int(style.texture_pattern),),
            ).fetchone()
        except sqlite3.Error:
            return None
        if row is None or not row["Mipmap"]:
            return None
        return self._decode_mipmap_alpha(int(row["Mipmap"]))

    def _apply_brush_texture_preview(self, rgba: np.ndarray, brush_style_id: int) -> None:
        texture = self._brush_texture_preview_alpha(brush_style_id)
        if texture is None or texture.size == 0:
            return
        bbox = _alpha_bbox(rgba[..., 3])
        if bbox is None:
            return
        style = self._brush_style_preview(brush_style_id)
        density = 1.0 if style is None else max(0.0, min(float(style.texture_density_base), 1.0))
        if density <= 0.0:
            return
        y0, y1, x0, x1 = bbox
        yy, xx = np.indices((y1 - y0, x1 - x0))
        tex = texture[(yy + y0) % texture.shape[0], (xx + x0) % texture.shape[1]].astype(np.float32) / 255.0
        if style is not None and (style.texture_flag & 0x200):
            avg = float(int(texture.sum(dtype=np.uint64)) // int(texture.size)) / 255.0
            remapped = np.clip(
                avg + VECTOR_TEXTURE_FLAG_0X200_DENSITY_GAIN * density * (tex - avg),
                0.0,
                1.0,
            )
            factor = 1.0 - avg + remapped
        else:
            factor = (
                1.0
                - density * VECTOR_TEXTURE_DENSITY_PREVIEW_SCALE
                + density * VECTOR_TEXTURE_DENSITY_PREVIEW_SCALE * np.power(tex, VECTOR_TEXTURE_PREVIEW_GAMMA)
            )
        alpha = rgba[y0:y1, x0:x1, 3].astype(np.float32) * factor
        rgba[y0:y1, x0:x1, 3] = np.clip(alpha + 0.5, 0, 255).astype(np.uint8)

    def _brush_material_full_lane_alpha(self, brush_style_id: int) -> Optional[np.ndarray]:
        if brush_style_id in self._brush_material_full_lane_cache:
            return self._brush_material_full_lane_cache[brush_style_id]
        style = self._brush_style_preview(brush_style_id)
        if style is None or style.pattern_style <= 0 or style.texture_pattern:
            self._brush_material_full_lane_cache[brush_style_id] = None
            return None
        try:
            pattern = self._db.execute(
                "SELECT ImageIndex FROM BrushPatternStyle WHERE MainId=?",
                (int(style.pattern_style),),
            ).fetchone()
        except sqlite3.Error:
            self._brush_material_full_lane_cache[brush_style_id] = None
            return None
        if pattern is None or not isinstance(pattern["ImageIndex"], bytes) or len(pattern["ImageIndex"]) < 4:
            self._brush_material_full_lane_cache[brush_style_id] = None
            return None
        image_id = struct.unpack_from(">I", pattern["ImageIndex"], 0)[0]
        image = self._db.execute(
            "SELECT Mipmap FROM BrushPatternImage WHERE MainId=?",
            (int(image_id),),
        ).fetchone()
        if image is None or not image["Mipmap"]:
            self._brush_material_full_lane_cache[brush_style_id] = None
            return None
        mipmap_id = int(image["Mipmap"])
        offscreen_id = self._mipmap_offscreen_id(mipmap_id)
        size = None if offscreen_id is None else self._offscreen_pixel_size(offscreen_id)
        ext_id = self._resolve_mipmap_external_id(mipmap_id)
        if offscreen_id is None or size is None or ext_id is None:
            self._brush_material_full_lane_cache[brush_style_id] = None
            return None
        blob = self._get_tile_blob(ext_id)
        if blob is None or len(blob) % MASK_TILE_BYTES != 0:
            self._brush_material_full_lane_cache[brush_style_id] = None
            return None
        total_tiles = len(blob) // MASK_TILE_BYTES
        cols = int(round(math.sqrt(total_tiles)))
        if cols <= 0 or cols * cols != total_tiles:
            cols = max(1, total_tiles)
        rows = max(1, total_tiles // cols)
        try:
            grouped = np.frombuffer(blob, dtype=np.uint8).reshape(rows, cols, TILE, TILE)
        except ValueError:
            self._brush_material_full_lane_cache[brush_style_id] = None
            return None
        full = grouped.transpose(0, 2, 1, 3).reshape(rows * TILE, cols * TILE)
        attr_w, attr_h = size
        lane_w = min(max(int(attr_w), 1), max(1, full.shape[1] // 2 if cols > 1 else full.shape[1]))
        lane_h = min(max(int(attr_h), 1), full.shape[0])
        alpha = full[:lane_h, :lane_w].copy()
        if not np.any(alpha > 4):
            self._brush_material_full_lane_cache[brush_style_id] = None
            return None
        self._brush_material_full_lane_cache[brush_style_id] = alpha
        return alpha

    def _brush_material_resource_alpha(self, brush_style_id: int) -> Optional[np.ndarray]:
        if brush_style_id in self._brush_material_resource_alpha_cache:
            return self._brush_material_resource_alpha_cache[brush_style_id]
        style = self._brush_style_preview(brush_style_id)
        if style is None or style.pattern_style <= 0 or style.texture_pattern:
            self._brush_material_resource_alpha_cache[brush_style_id] = None
            return None
        try:
            pattern = self._db.execute(
                "SELECT ImageIndex FROM BrushPatternStyle WHERE MainId=?",
                (int(style.pattern_style),),
            ).fetchone()
        except sqlite3.Error:
            self._brush_material_resource_alpha_cache[brush_style_id] = None
            return None
        if pattern is None or not isinstance(pattern["ImageIndex"], bytes) or len(pattern["ImageIndex"]) < 4:
            self._brush_material_resource_alpha_cache[brush_style_id] = None
            return None
        image_id = struct.unpack_from(">I", pattern["ImageIndex"], 0)[0]
        image = self._db.execute(
            "SELECT Mipmap FROM BrushPatternImage WHERE MainId=?",
            (int(image_id),),
        ).fetchone()
        if image is None or not image["Mipmap"]:
            self._brush_material_resource_alpha_cache[brush_style_id] = None
            return None
        mipmap_id = int(image["Mipmap"])
        offscreen_id = self._mipmap_offscreen_id(mipmap_id)
        size = None if offscreen_id is None else self._offscreen_pixel_size(offscreen_id)
        ext_id = self._resolve_mipmap_external_id(mipmap_id)
        if offscreen_id is None or size is None or ext_id is None:
            self._brush_material_resource_alpha_cache[brush_style_id] = None
            return None
        blob = self._get_tile_blob(ext_id)
        if blob is None or len(blob) % MASK_TILE_BYTES != 0:
            self._brush_material_resource_alpha_cache[brush_style_id] = None
            return None
        try:
            attr_w, attr_h = size
            cols = (int(attr_w) + TILE - 1) // TILE
            rows = (int(attr_h) + TILE - 1) // TILE
            if len(blob) != cols * rows * MASK_TILE_BYTES:
                self._brush_material_resource_alpha_cache[brush_style_id] = None
                return None
            alpha = _tiles_to_alpha(blob, int(attr_w), int(attr_h))
        except ValueError:
            self._brush_material_resource_alpha_cache[brush_style_id] = None
            return None
        if not np.any(alpha > 4):
            self._brush_material_resource_alpha_cache[brush_style_id] = None
            return None
        self._brush_material_resource_alpha_cache[brush_style_id] = alpha
        return alpha

    def _brush_material_stamp_alpha(self, brush_style_id: int) -> Optional[np.ndarray]:
        if brush_style_id in self._brush_material_stamp_cache:
            return self._brush_material_stamp_cache[brush_style_id]
        alpha = self._brush_material_full_lane_alpha(brush_style_id)
        if alpha is None:
            self._brush_material_stamp_cache[brush_style_id] = None
            return None
        ys, xs = np.where(alpha > 4)
        if len(xs) == 0:
            self._brush_material_stamp_cache[brush_style_id] = None
            return None
        stamp = alpha[ys.min():ys.max() + 1, xs.min():xs.max() + 1].copy()
        self._brush_material_stamp_cache[brush_style_id] = stamp
        return stamp

    @staticmethod
    def _resize_alpha_bilinear(alpha: np.ndarray, width: int, height: int) -> np.ndarray:
        rgba = np.zeros((alpha.shape[0], alpha.shape[1], 4), dtype=np.uint8)
        rgba[..., 3] = alpha
        return _resize_rgba_bilinear(rgba, width, height)[..., 3]

    @staticmethod
    def _resize_alpha_area(alpha: np.ndarray, width: int, height: int) -> np.ndarray:
        if width <= 0 or height <= 0 or alpha.size == 0:
            return np.zeros((0, 0), dtype=np.uint8)
        src_h, src_w = alpha.shape
        out = np.zeros((height, width), dtype=np.uint8)
        for y in range(height):
            y0 = int(np.floor(y * src_h / height))
            y1 = max(y0 + 1, int(np.ceil((y + 1) * src_h / height)))
            for x in range(width):
                x0 = int(np.floor(x * src_w / width))
                x1 = max(x0 + 1, int(np.ceil((x + 1) * src_w / width)))
                out[y, x] = np.clip(
                    np.floor(float(alpha[y0:y1, x0:x1].mean()) + 0.5),
                    0,
                    255,
                )
        return out

    def _resize_material_wide_stamp_alpha(
        self,
        brush_style_id: int,
        fallback_alpha: np.ndarray,
        width: int,
        height: int,
        render_width: Optional[float] = None,
        render_height: Optional[float] = None,
    ) -> np.ndarray:
        full_lane = self._brush_material_full_lane_alpha(brush_style_id)
        if full_lane is None:
            return self._resize_alpha_bilinear(fallback_alpha, width, height)
        native_w = max(1, int(full_lane.shape[1] / VECTOR_MATERIAL_NATIVE_MIP_SCALE))
        native_h = max(1, int(full_lane.shape[0] / VECTOR_MATERIAL_NATIVE_MIP_SCALE))
        native_alpha = self._resize_alpha_area(full_lane, native_w, native_h)
        render_width = float(width if render_width is None else render_width)
        render_height = float(height if render_height is None else render_height)
        if render_width <= 0.0 or render_height <= 0.0:
            return np.zeros((height, width), dtype=np.uint8)
        scale_x = render_width / float(native_alpha.shape[1])
        scale_y = render_height / float(native_alpha.shape[0])
        if scale_x <= 0.0 or scale_y <= 0.0:
            return np.zeros((height, width), dtype=np.uint8)
        x_step = 1.0 / scale_x
        y_step = 1.0 / scale_y
        out = np.zeros((height, width), dtype=np.uint8)

        def sample(u: float, v: float) -> int:
            sx = int(np.floor(u))
            sy = int(np.floor(v))
            if sx < 0 or sy < 0 or sx >= native_alpha.shape[1] or sy >= native_alpha.shape[0]:
                return 0
            return int(native_alpha[sy, sx])

        for y in range(height):
            v = float(y) / scale_y
            for x in range(width):
                u = float(x) / scale_x
                coverage = (
                    sample(u, v)
                    + sample(u + x_step, v)
                    + sample(u, v + 0.5 * y_step)
                    + sample(u + 0.5 * x_step, v + 0.5 * y_step)
                )
                out[y, x] = np.clip((coverage + 2) // 4, 0, 255)
        return out

    @staticmethod
    def _material_mix_preview_color(
        main_color: tuple[int, int, int],
        sub_color: tuple[int, int, int],
        mix: float,
    ) -> tuple[int, int, int]:
        mix = max(0.0, min(float(mix), 1.0))
        return tuple(
            int(np.clip(round(main_color[idx] + (sub_color[idx] - main_color[idx]) * mix), 0, 255))
            for idx in range(3)
        )

    def _draw_material_stamp_rgba(
        self,
        rgba: np.ndarray,
        center: tuple[float, float],
        color: tuple[int, int, int],
        stamp_alpha: np.ndarray,
        opacity: float,
        anchor_x: float,
        anchor_y: float,
    ) -> None:
        if stamp_alpha.size == 0 or opacity <= 0.0:
            return
        h, w = stamp_alpha.shape
        x0 = int(round(center[0] - w * anchor_x))
        y0 = int(round(center[1] - h * anchor_y))
        x1 = max(0, min(self.width, x0 + w))
        y1 = max(0, min(self.height, y0 + h))
        x0c = max(0, x0)
        y0c = max(0, y0)
        if x1 <= x0c or y1 <= y0c:
            return
        sx0 = x0c - x0
        sy0 = y0c - y0
        src_alpha = stamp_alpha[sy0:sy0 + y1 - y0c, sx0:sx0 + x1 - x0c].astype(np.float32)
        src_a = (src_alpha * max(0.0, min(float(opacity), 1.0)) / 255.0)[..., None]
        if not np.any(src_a > 0.0):
            return
        region = rgba[y0c:y1, x0c:x1]
        dst_a = region[..., 3:4].astype(np.float32) / 255.0
        src_rgb = np.array(color, dtype=np.float32).reshape(1, 1, 3) / 255.0
        dst_rgb = region[..., :3].astype(np.float32) / 255.0
        out_a = src_a + dst_a * (1.0 - src_a)
        with np.errstate(invalid="ignore", divide="ignore"):
            out_rgb = np.where(
                out_a > 1e-6,
                (src_rgb * src_a + dst_rgb * dst_a * (1.0 - src_a)) / out_a,
                0.0,
            )
        region[..., :3] = np.clip(np.floor(out_rgb * 255.0 + 0.5), 0, 255).astype(np.uint8)
        region[..., 3] = np.clip(np.floor(out_a[..., 0] * 255.0 + 0.5), 0, 255).astype(np.uint8)

    def _vector_resample_points_by_distance(
        self,
        points: list[tuple[float, float]],
        step: float,
    ) -> list[tuple[float, float]]:
        if len(points) < 2 or not np.isfinite(step) or step <= 0.0:
            return points
        sampled: list[tuple[float, float]] = [points[0]]
        carry = 0.0
        for p0, p1 in zip(points, points[1:]):
            dx = p1[0] - p0[0]
            dy = p1[1] - p0[1]
            distance = float(np.hypot(dx, dy))
            if distance <= 1e-6:
                continue
            t = ((step - carry) / distance) if carry > 1e-6 else (step / distance)
            while t < 1.0:
                sampled.append((p0[0] + dx * t, p0[1] + dy * t))
                t += step / distance
            carry = (carry + distance) % step
        if sampled[-1] != points[-1]:
            sampled.append(points[-1])
        return sampled

    @staticmethod
    def _closed_path_resample_points_by_distance(
        points: list[tuple[float, float]],
        step: float,
        phase: float = 0.0,
    ) -> list[tuple[float, float]]:
        if len(points) < 2:
            return points
        step = max(float(step), 1e-6)
        phase = max(float(phase), 0.0)
        closed = points + [points[0]]
        segments: list[tuple[tuple[float, float], tuple[float, float], float, float]] = []
        total = 0.0
        for p0, p1 in zip(closed, closed[1:]):
            length = float(math.hypot(p1[0] - p0[0], p1[1] - p0[1]))
            if length <= 1e-9:
                continue
            segments.append((p0, p1, length, total))
            total += length
        if total <= 1e-9:
            return []
        sampled: list[tuple[float, float]] = []
        distance = phase % step if phase > 0.0 else 0.0
        seg_index = 0
        while distance < total + 1e-9:
            while (
                seg_index + 1 < len(segments)
                and distance > segments[seg_index][3] + segments[seg_index][2] + 1e-9
            ):
                seg_index += 1
            p0, p1, length, base = segments[seg_index]
            t = (distance - base) / length
            sampled.append((
                p0[0] + (p1[0] - p0[0]) * t,
                p0[1] + (p1[1] - p0[1]) * t,
            ))
            distance += step
        return sampled

    def _native_spline_resample_points_by_distance(
        self,
        points: list[tuple[float, float]],
        step: float,
        point_flags: Optional[list[int]] = None,
    ) -> list[tuple[float, float]]:
        if len(points) < 2 or not np.isfinite(step) or step <= 0.0:
            return points
        sampled: list[tuple[float, float]] = []
        carry = 0.0
        for idx in range(len(points) - 1):
            controls = self._native_spline_segment_controls(points, idx, point_flags)
            curve_length = self._native_spline_length_from_controls(controls)
            if idx == 0 and not sampled:
                sampled.append(self._native_spline_point_from_controls(controls, 0.0))
            target = step - carry if carry > 1e-6 else step
            while target < curve_length:
                t = self._native_spline_t_at_distance(controls, target, curve_length)
                sampled.append(self._native_spline_point_from_controls(controls, t))
                target += step
            carry = (carry + curve_length) % step
        return sampled if sampled else points

    def _catmull_rom_point(
        self,
        p0: tuple[float, float],
        p1: tuple[float, float],
        p2: tuple[float, float],
        p3: tuple[float, float],
        t: float,
    ) -> tuple[float, float]:
        t2 = t * t
        t3 = t2 * t
        x = 0.5 * (
            (2.0 * p1[0])
            + (-p0[0] + p2[0]) * t
            + (2.0 * p0[0] - 5.0 * p1[0] + 4.0 * p2[0] - p3[0]) * t2
            + (-p0[0] + 3.0 * p1[0] - 3.0 * p2[0] + p3[0]) * t3
        )
        y = 0.5 * (
            (2.0 * p1[1])
            + (-p0[1] + p2[1]) * t
            + (2.0 * p0[1] - 5.0 * p1[1] + 4.0 * p2[1] - p3[1]) * t2
            + (-p0[1] + 3.0 * p1[1] - 3.0 * p2[1] + p3[1]) * t3
        )
        return x, y

    @staticmethod
    def _cubic_bezier_point(
        p0: tuple[float, float],
        p1: tuple[float, float],
        p2: tuple[float, float],
        p3: tuple[float, float],
        t: float,
    ) -> tuple[float, float]:
        u = 1.0 - t
        return (
            u * u * u * p0[0]
            + 3.0 * u * u * t * p1[0]
            + 3.0 * u * t * t * p2[0]
            + t * t * t * p3[0],
            u * u * u * p0[1]
            + 3.0 * u * u * t * p1[1]
            + 3.0 * u * t * t * p2[1]
            + t * t * t * p3[1],
        )

    @staticmethod
    def _native_spline_limit_control(
        center: tuple[float, float],
        neighbor: tuple[float, float],
        control: tuple[float, float],
    ) -> tuple[float, float]:
        dx = control[0] - center[0]
        dy = control[1] - center[1]
        far2 = dx * dx + dy * dy
        if far2 < 100.0:
            return control
        near_dx = center[0] - neighbor[0]
        near_dy = center[1] - neighbor[1]
        near2 = (near_dx * near_dx + near_dy * near_dy) * 6.25
        if near2 > far2:
            return control
        limit = near2 ** 0.5 if near2 > 100.0 else 10.0
        far = far2 ** 0.5
        if far <= 1e-12:
            return control
        scale = limit / far
        return center[0] + dx * scale, center[1] + dy * scale

    @classmethod
    def _native_spline_segment_controls(
        cls,
        points: list[tuple[float, float]],
        idx: int,
        point_flags: Optional[list[int]] = None,
    ) -> tuple[tuple[float, float], ...]:
        cur = points[idx]
        nxt = points[idx + 1]
        cur_flag = int(point_flags[idx]) if point_flags is not None and idx < len(point_flags) else 0
        nxt_flag = (
            int(point_flags[idx + 1])
            if point_flags is not None and idx + 1 < len(point_flags)
            else 0
        )
        if idx - 1 >= 0 and (cur_flag & 1) == 0:
            prev = cls._native_spline_limit_control(cur, nxt, points[idx - 1])
            prev_flag = (
                int(point_flags[idx - 1])
                if point_flags is not None and idx - 1 < len(point_flags)
                else 0
            )
            prevprev = (
                cls._native_spline_limit_control(prev, cur, points[idx - 2])
                if idx - 2 >= 0 and (prev_flag & 1) == 0
                else prev
            )
        else:
            prev = cur
            prevprev = cur
        if idx + 2 < len(points) and (nxt_flag & 1) == 0:
            next1 = cls._native_spline_limit_control(nxt, cur, points[idx + 2])
            next1_flag = (
                int(point_flags[idx + 2])
                if point_flags is not None and idx + 2 < len(point_flags)
                else 0
            )
            next2 = (
                cls._native_spline_limit_control(next1, nxt, points[idx + 3])
                if idx + 3 < len(points) and (next1_flag & 1) == 0
                else next1
            )
        else:
            next1 = nxt
            next2 = nxt
        return prevprev, prev, cur, nxt, next1, next2

    @classmethod
    def _native_spline_closed_segment_controls(
        cls,
        points: list[tuple[float, float]],
        idx: int,
        point_flags: Optional[list[int]] = None,
    ) -> tuple[tuple[float, float], ...]:
        count = len(points)
        cur = points[idx % count]
        nxt = points[(idx + 1) % count]
        cur_flag = int(point_flags[idx % count]) if point_flags is not None and count else 0
        nxt_flag = int(point_flags[(idx + 1) % count]) if point_flags is not None and count else 0
        if count >= 3 and (cur_flag & 1) == 0:
            prev_idx = (idx - 1) % count
            prev = cls._native_spline_limit_control(cur, nxt, points[prev_idx])
            prev_flag = int(point_flags[prev_idx]) if point_flags is not None else 0
            prevprev = (
                cls._native_spline_limit_control(prev, cur, points[(idx - 2) % count])
                if count >= 4 and (prev_flag & 1) == 0
                else prev
            )
        else:
            prev = cur
            prevprev = cur
        if count >= 3 and (nxt_flag & 1) == 0:
            next1_idx = (idx + 2) % count
            next1 = cls._native_spline_limit_control(nxt, cur, points[next1_idx])
            next1_flag = int(point_flags[next1_idx]) if point_flags is not None else 0
            next2 = (
                cls._native_spline_limit_control(next1, nxt, points[(idx + 3) % count])
                if count >= 4 and (next1_flag & 1) == 0
                else next1
            )
        else:
            next1 = nxt
            next2 = nxt
        return prevprev, prev, cur, nxt, next1, next2

    @staticmethod
    def _native_spline_point_from_controls(
        controls: tuple[tuple[float, float], ...],
        t: float,
    ) -> tuple[float, float]:
        p0, p1, p2, p3, p4, p5 = controls
        t = max(0.0, min(float(t), 1.0))
        tt = t * t
        if t >= 0.5:
            c0 = 2.0 * tt - 6.0 * t + 4.0
            c1 = 44.0 * t - 16.0 * tt - 28.0
            c2 = 96.0 * tt - 260.0 * t + 164.0
            c3 = 328.0 * t - 164.0 * tt - 65.0
            c4 = 96.0 * tt - 124.0 * t + 28.0
            c5 = 18.0 * t - 14.0 * tt - 4.0
        else:
            c0 = 10.0 * t - 14.0 * tt
            c1 = 96.0 * tt - 68.0 * t
            c2 = 99.0 - 164.0 * tt
            c3 = 96.0 * tt + 68.0 * t
            c4 = -16.0 * tt - 12.0 * t
            c5 = 2.0 * tt + 2.0 * t
        return (
            (c0 * p0[0] + c1 * p1[0] + c2 * p2[0] + c3 * p3[0] + c4 * p4[0] + c5 * p5[0])
            / 99.0,
            (c0 * p0[1] + c1 * p1[1] + c2 * p2[1] + c3 * p3[1] + c4 * p4[1] + c5 * p5[1])
            / 99.0,
        )

    @classmethod
    def _native_spline_polyline_length(
        cls,
        controls: tuple[tuple[float, float], ...],
        steps: int,
    ) -> float:
        steps = max(4, min(int(steps), 255))
        last = cls._native_spline_point_from_controls(controls, 0.0)
        total = 0.0
        for i in range(1, steps + 1):
            point = cls._native_spline_point_from_controls(controls, i / steps)
            total += float(np.hypot(point[0] - last[0], point[1] - last[1]))
            last = point
        return total

    @classmethod
    def _native_spline_length_from_controls(
        cls,
        controls: tuple[tuple[float, float], ...],
    ) -> float:
        # Native first estimates spline length from quarter points. Only
        # curves with more than four rough 4px buckets are resampled; shorter
        # spans keep the rough length directly.
        p0 = cls._native_spline_point_from_controls(controls, 0.0)
        p1 = cls._native_spline_point_from_controls(controls, 0.25)
        p2 = cls._native_spline_point_from_controls(controls, 0.5)
        p3 = cls._native_spline_point_from_controls(controls, 0.75)
        p4 = cls._native_spline_point_from_controls(controls, 1.0)
        rough = (
            float(np.hypot(p1[0] - p0[0], p1[1] - p0[1]))
            + float(np.hypot(p2[0] - p1[0], p2[1] - p1[1]))
            + float(np.hypot(p3[0] - p2[0], p3[1] - p2[1]))
            + float(np.hypot(p4[0] - p3[0], p4[1] - p3[1]))
        )
        steps = int(rough / 4.0)
        if steps <= 4:
            return rough
        return cls._native_spline_polyline_length(controls, min(steps, 255))

    @classmethod
    def _native_spline_t_at_distance(
        cls,
        controls: tuple[tuple[float, float], ...],
        target: float,
        curve_length: Optional[float] = None,
    ) -> float:
        if target <= 0.0:
            return 0.0
        if curve_length is None:
            curve_length = cls._native_spline_length_from_controls(controls)
        steps = max(4, min(int(float(curve_length) * 0.25), 255))
        last = cls._native_spline_point_from_controls(controls, 0.0)
        total = 0.0
        for i in range(1, steps + 1):
            t = i / steps
            point = cls._native_spline_point_from_controls(controls, t)
            seg = float(np.hypot(point[0] - last[0], point[1] - last[1]))
            if total + seg >= target and seg > 1e-12:
                return ((i - 1) + (target - total) / seg) / steps
            total += seg
            last = point
        return 1.0

    def _vector_sample_point(self, point: tuple[float, float]) -> tuple[float, float]:
        # CSP casts sampled vector points to LONG before pen-head rasterization.
        return float(int(point[0])), float(int(point[1]))

    def _vector_stroke_fallback_image(self, layer: sqlite3.Row) -> Optional[np.ndarray]:
        body = self._vector_object_body(layer["MainId"])
        if body is None:
            return None
        rgba = np.zeros((self.height, self.width, 4), dtype=np.uint8)
        found = False
        for off in range(56, len(body) - 92, 4):
            try:
                header_len, point_header_len, stride_a, stride_b = struct.unpack_from(">IIII", body, off)
                point_count, flags = struct.unpack_from(">II", body, off + 16)
            except struct.error:
                continue
            if (header_len, point_header_len, stride_a, stride_b) == (92, 76, 120, 88):
                if flags != VECTOR_STROKE_FLAGS_FILLED_CURVE:
                    continue
                try:
                    x0, y0, x1, y1 = struct.unpack_from(">iiii", body, off + 24)
                except struct.error:
                    continue
                if not (
                    x0 < x1
                    and y0 < y1
                    and x1 >= 0
                    and y1 >= 0
                    and x0 <= self.width * 2
                    and y0 <= self.height * 2
                ):
                    continue
                brush_style_id = struct.unpack_from(">I", body, off + 76)[0]
                width = struct.unpack_from(">d", body, off + 80)[0]
                if point_count == 1:
                    pattern_mipmap = self._brush_pattern_mipmap(brush_style_id)
                    pattern = None if pattern_mipmap is None else self._decode_mipmap_rgba(pattern_mipmap)
                    point_off = off + header_len
                    if pattern is None or point_off + 16 > len(body):
                        continue
                    px = struct.unpack_from(">d", body, point_off)[0]
                    py = struct.unpack_from(">d", body, point_off + 8)[0]
                    if not (np.isfinite(px) and np.isfinite(py)):
                        continue
                    target_w = max(1, min(int(x1) - int(x0), self.width))
                    target_h = max(
                        1,
                        min(
                            int(round(pattern.shape[0] * (target_w / max(pattern.shape[1], 1)))),
                            self.height,
                        ),
                    )
                    scaled = _resize_rgba_bilinear(pattern, target_w, target_h)
                    canvas = np.zeros_like(rgba)
                    paste_x0 = min(max(int(x0), 0), self.width)
                    paste_y0 = min(max(int(round(py)) - (target_h // 2), 0), self.height)
                    paste_x1 = min(paste_x0 + scaled.shape[1], self.width)
                    paste_y1 = min(paste_y0 + scaled.shape[0], self.height)
                    if paste_x0 >= paste_x1 or paste_y0 >= paste_y1:
                        continue
                    canvas[paste_y0:paste_y1, paste_x0:paste_x1] = scaled[
                        : paste_y1 - paste_y0,
                        : paste_x1 - paste_x0,
                    ]
                    self._alpha_over_rgba(rgba, canvas)
                    found = True
                    continue
                if point_count < 2:
                    continue
                rgb = []
                for color_off in (off + 40, off + 44, off + 48):
                    rgb.append(struct.unpack_from(">H", body, color_off)[0] // 257)
                if (
                    width <= VECTOR_FILLED_CURVE_MAX_SOLID_WIDTH
                    and max(rgb) <= VECTOR_FILLED_CURVE_DARK_RGB_THRESHOLD
                ):
                    raw_points: list[tuple[float, float]] = []
                    points: list[tuple[float, float]] = []
                    forward_controls: list[tuple[float, float]] = []
                    valid = True
                    points_start = off + header_len
                    for idx in range(point_count):
                        point_off = points_start + idx * stride_a
                        if point_off + stride_a > len(body):
                            valid = False
                            break
                        x = struct.unpack_from(">d", body, point_off)[0]
                        y = struct.unpack_from(">d", body, point_off + 8)[0]
                        ctrl_x = struct.unpack_from(">d", body, point_off + 104)[0]
                        ctrl_y = struct.unpack_from(">d", body, point_off + 112)[0]
                        if not all(np.isfinite(value) for value in (x, y, ctrl_x, ctrl_y)):
                            valid = False
                            break
                        raw_points.append((float(x), float(y)))
                        points.append(self._vector_sample_point((float(x), float(y))))
                        forward_controls.append((float(ctrl_x), float(ctrl_y)))
                    if not valid or len(points) < 2:
                        continue
                    aa_level = self._brush_anti_alias(brush_style_id)
                    draw_points: list[tuple[float, float]] = []
                    for idx in range(len(points) - 1):
                        if aa_level <= 0:
                            p0 = points[idx]
                            p3 = points[idx + 1]
                            p2 = self._vector_sample_point(forward_controls[idx])
                            sample_shift = VECTOR_FILLED_CURVE_HARD_SAMPLE_SHIFT
                            sample_scale = VECTOR_FILLED_CURVE_AA_SAMPLE_SCALE
                        else:
                            p0 = raw_points[idx]
                            p3 = raw_points[idx + 1]
                            p2 = self._vector_sample_point(forward_controls[idx])
                            sample_shift = VECTOR_FILLED_CURVE_AA_SAMPLE_SHIFT
                            sample_scale = VECTOR_FILLED_CURVE_AA_SAMPLE_SCALE
                        # Compact 120-byte curve objects store a forward tail at
                        # point +104/+112. For the AA samples this behaves as the
                        # second cubic control while the first control stays at p0.
                        p1 = p0
                        distance = max(
                            float(np.hypot(p2[0] - p1[0], p2[1] - p1[1])),
                            float(np.hypot(p3[0] - p2[0], p3[1] - p2[1])),
                            float(np.hypot(p3[0] - p0[0], p3[1] - p0[1])),
                        )
                        samples = max(1, min(192, int(np.ceil(distance / 3.0 * sample_scale))))
                        if idx == 0:
                            draw_points.append((p0[0] + sample_shift, p0[1] + sample_shift))
                        for sample in range(1, samples + 1):
                            point = self._cubic_bezier_point(
                                p0,
                                p1,
                                p2,
                                p3,
                                sample / samples,
                            )
                            if aa_level <= 0:
                                point = self._vector_sample_point(point)
                            draw_points.append((point[0] + sample_shift, point[1] + sample_shift))
                    if aa_level <= 0:
                        radius_scale = VECTOR_FILLED_CURVE_HARD_RADIUS_SCALE
                        anti_alias_feather = 0.0
                    else:
                        radius_scale = VECTOR_FILLED_CURVE_RADIUS_SCALE_BY_AA.get(
                            int(aa_level),
                            VECTOR_FILLED_CURVE_RADIUS_SCALE,
                        )
                        anti_alias_feather = VECTOR_FILLED_CURVE_FEATHER_BY_AA.get(
                            int(aa_level),
                            VECTOR_FILLED_CURVE_FEATHER_BASE
                            + VECTOR_FILLED_CURVE_FEATHER_PER_AA * aa_level,
                        )
                    self._draw_polyline_rgba(
                        rgba,
                        draw_points,
                        (
                            max(0, min(int(rgb[0]), 255)),
                            max(0, min(int(rgb[1]), 255)),
                            max(0, min(int(rgb[2]), 255)),
                            255,
                        ),
                        radius=width * radius_scale,
                        feather=anti_alias_feather,
                    )
                    found = True
                    continue
                bbox = (
                    min(max(int(x0) + VECTOR_FILLED_CURVE_ELLIPSE_INSET[0], 0), self.width),
                    min(max(int(y0) + VECTOR_FILLED_CURVE_ELLIPSE_INSET[1], 0), self.height),
                    min(max(int(x1) - VECTOR_FILLED_CURVE_ELLIPSE_INSET[2], 0), self.width),
                    min(max(int(y1) - VECTOR_FILLED_CURVE_ELLIPSE_INSET[3], 0), self.height),
                )
                self._draw_ellipse_rgba(
                    rgba,
                    bbox,
                    fill=(
                        max(0, min(int(rgb[0]), 255)),
                        max(0, min(int(rgb[1]), 255)),
                        max(0, min(int(rgb[2]), 255)),
                        255,
                    ),
                    power=VECTOR_FILLED_CURVE_ELLIPSE_POWER,
                )
                found = True
                continue
            if (header_len, point_header_len, stride_a, stride_b) != (92, 76, 88, 88):
                continue
            if flags not in (VECTOR_STROKE_FLAGS_NATIVE_AA, VECTOR_STROKE_FLAGS_LEGACY) or not (2 <= point_count <= VECTOR_STROKE_MAX_POINTS):
                continue
            try:
                x0, y0, x1, y1 = struct.unpack_from(">iiii", body, off + 24)
                stroke_opacity = struct.unpack_from(">d", body, off + 64)[0]
                width = struct.unpack_from(">d", body, off + 80)[0]
                stroke_random_seed = struct.unpack_from(">I", body, off + 88)[0]
            except struct.error:
                continue
            if not (
                x0 < x1
                and y0 < y1
                and x1 >= 0
                and y1 >= 0
                and x0 <= self.width * 2
                and y0 <= self.height * 2
            ):
                continue
            if not np.isfinite(width) or width <= 0.0 or width > VECTOR_STROKE_MAX_WIDTH:
                continue
            brush_style_id = struct.unpack_from(">I", body, off + 76)[0]
            use_simple_size_dynamics = self._brush_uses_simple_size_dynamics(brush_style_id)
            use_pressure_size_dynamics = self._brush_uses_pressure_size_dynamics(brush_style_id)
            use_pressure_opacity_dynamics = self._brush_uses_pressure_opacity_dynamics(brush_style_id)
            use_secondary_or_aux_size_dynamics = self._brush_uses_secondary_or_aux_dynamics(
                brush_style_id,
                "SizeEffector",
            )
            use_secondary_or_aux_opacity_dynamics = self._brush_uses_secondary_or_aux_dynamics(
                brush_style_id,
                "OpacityEffector",
            )
            use_random_size_dynamics = self._brush_uses_random_dynamics(
                brush_style_id,
                "SizeEffector",
            )
            use_random_opacity_dynamics = self._brush_uses_random_dynamics(
                brush_style_id,
                "OpacityEffector",
            )
            use_flow_dynamics = self._brush_uses_flow_dynamics(brush_style_id)
            points: list[tuple[float, float]] = []
            tapers: list[float] = []
            opacity_tapers: list[float] = []
            flow_tapers: list[float] = []
            native_size_factors: list[float] = []
            native_flow_factors: list[float] = []
            native_flow_effector_factors: list[float] = []
            native_primary_scalars: list[float] = []
            native_secondary_scalars: list[float] = []
            native_auxiliary_scalars: list[float] = []
            point_random_seeds: list[int] = []
            point_flags: list[int] = []
            point_primary_scalar_off = 36
            point_endpoint_taper_off = 52
            points_start = off + header_len
            valid = True
            for idx in range(point_count):
                point_off = points_start + idx * stride_a
                if point_off + 64 > len(body):
                    valid = False
                    break
                x = struct.unpack_from(">d", body, point_off)[0]
                y = struct.unpack_from(">d", body, point_off + 8)[0]
                if not (np.isfinite(x) and np.isfinite(y)):
                    valid = False
                    break
                node_flags = struct.unpack_from(">I", body, point_off + 32)[0]
                primary_scalar = struct.unpack_from(">f", body, point_off + point_primary_scalar_off)[0]
                secondary_scalar = struct.unpack_from(">f", body, point_off + 40)[0]
                if not np.isfinite(primary_scalar):
                    primary_scalar = 1.0
                if not np.isfinite(secondary_scalar):
                    secondary_scalar = 1.0
                size_factor = struct.unpack_from(">f", body, point_off + 56)[0]
                flow_factor = struct.unpack_from(">f", body, point_off + 60)[0]
                point_random_seed = (
                    struct.unpack_from(">I", body, point_off + 80)[0]
                    if point_off + 84 <= len(body)
                    else 0
                )
                if not np.isfinite(size_factor):
                    size_factor = 1.0
                if not np.isfinite(flow_factor):
                    flow_factor = 1.0
                taper_off = point_primary_scalar_off if (
                    use_simple_size_dynamics
                    or use_pressure_size_dynamics
                    or use_secondary_or_aux_size_dynamics
                    or use_random_size_dynamics
                ) else point_endpoint_taper_off
                taper = struct.unpack_from(">f", body, point_off + taper_off)[0]
                if not np.isfinite(taper):
                    taper = 1.0
                if use_pressure_size_dynamics:
                    effector_taper = self._brush_pressure_effector_value(
                        brush_style_id,
                        "SizeEffector",
                        primary_scalar,
                        secondary_scalar,
                    )
                    if effector_taper is not None:
                        taper = effector_taper
                elif use_secondary_or_aux_size_dynamics:
                    effector_taper = self._brush_secondary_or_aux_effector_value(
                        brush_style_id,
                        "SizeEffector",
                        secondary_scalar,
                        struct.unpack_from(">f", body, point_off + 44)[0],
                    )
                    if effector_taper is not None:
                        taper = effector_taper
                elif use_random_size_dynamics:
                    effector_taper = self._brush_random_effector_value(
                        brush_style_id,
                        "SizeEffector",
                        off + idx * 17,
                        point_random_seed,
                    )
                    if effector_taper is not None:
                        taper = effector_taper
                taper_min = 0.0 if (
                    use_simple_size_dynamics
                    or use_pressure_size_dynamics
                    or use_secondary_or_aux_size_dynamics
                    or use_random_size_dynamics
                ) else 0.6
                taper = max(taper_min, min(float(taper), 1.0))
                opacity_taper = 1.0
                if use_pressure_opacity_dynamics:
                    opacity_source = self._brush_pressure_effector_value(
                    brush_style_id,
                    "OpacityEffector",
                    primary_scalar,
                    secondary_scalar,
                    )
                    if opacity_source is None:
                        opacity_source = primary_scalar
                    opacity_taper = max(0.0, min(float(opacity_source), 1.0))
                elif use_secondary_or_aux_opacity_dynamics:
                    opacity_source = self._brush_secondary_or_aux_effector_value(
                        brush_style_id,
                        "OpacityEffector",
                        secondary_scalar,
                        struct.unpack_from(">f", body, point_off + 44)[0],
                    )
                    if opacity_source is not None:
                        opacity_taper = max(0.0, min(float(opacity_source), 1.0))
                elif use_random_opacity_dynamics:
                    opacity_source = self._brush_random_effector_value(
                        brush_style_id,
                        "OpacityEffector",
                        off + idx * 17,
                        point_random_seed,
                    )
                    if opacity_source is not None:
                        opacity_taper = max(0.0, min(float(opacity_source), 1.0))
                flow_taper = 1.0
                if use_flow_dynamics:
                    flow_source = self._brush_flow_effector_value(
                        brush_style_id,
                        primary_scalar,
                        struct.unpack_from(">f", body, point_off + 44)[0],
                    )
                    if flow_source is not None:
                        flow_loss = 1.0 - max(0.0, min(float(flow_source), 1.0))
                        flow_taper = 1.0 - flow_loss * VECTOR_FLOW_DYNAMIC_RADIUS_SOFTEN_MAX
                        flow_taper = max(0.5, min(flow_taper, 1.0))
                native_flow_effector = self._brush_flow_effector_value(
                    brush_style_id,
                    primary_scalar,
                    struct.unpack_from(">f", body, point_off + 44)[0],
                )
                if native_flow_effector is None:
                    native_flow_effector = 1.0
                points.append((float(x), float(y)))
                tapers.append(taper)
                opacity_tapers.append(opacity_taper)
                flow_tapers.append(flow_taper)
                native_size_factors.append(max(0.0, min(float(size_factor), 4.0)))
                native_flow_factors.append(max(0.0, min(float(flow_factor), 4.0)))
                native_flow_effector_factors.append(max(0.0, min(float(native_flow_effector), 4.0)))
                native_primary_scalars.append(float(primary_scalar))
                native_secondary_scalars.append(float(secondary_scalar))
                native_auxiliary_scalars.append(float(struct.unpack_from(">f", body, point_off + 44)[0]))
                point_random_seeds.append(point_random_seed)
                point_flags.append(int(node_flags))
            if not valid or len(points) < 2:
                continue
            xs = [p[0] for p in points]
            ys = [p[1] for p in points]
            rgb = []
            for color_off in (off + 40, off + 44, off + 48):
                rgb.append(struct.unpack_from(">H", body, color_off)[0] // 257)
            color = (
                max(0, min(int(rgb[0]), 255)),
                max(0, min(int(rgb[1]), 255)),
                max(0, min(int(rgb[2]), 255)),
                255,
            )
            sub_rgb = []
            for color_off in (off + 52, off + 56, off + 60):
                sub_rgb.append(struct.unpack_from(">H", body, color_off)[0] // 257)
            sub_color = (
                max(0, min(int(sub_rgb[0]), 255)),
                max(0, min(int(sub_rgb[1]), 255)),
                max(0, min(int(sub_rgb[2]), 255)),
            )
            material_stamp = self._brush_material_stamp_alpha(brush_style_id)
            if material_stamp is not None:
                style = self._brush_style_preview(brush_style_id) or _BrushStylePreview()
                interval_is_wide = style.interval_base > 0.1
                stamp_w = max(
                    1,
                    int(round(
                        width
                        * (
                            VECTOR_MATERIAL_STAMP_GAP_WIDTH_SCALE
                            if interval_is_wide
                            else VECTOR_MATERIAL_STAMP_WIDTH_SCALE
                        )
                    )),
                )
                render_w = width * (
                    VECTOR_MATERIAL_STAMP_GAP_WIDTH_SCALE
                    if interval_is_wide
                    else VECTOR_MATERIAL_STAMP_WIDTH_SCALE
                )
                render_h = width * (
                    VECTOR_MATERIAL_STAMP_GAP_HEIGHT_SCALE
                    if interval_is_wide
                    else VECTOR_MATERIAL_STAMP_HEIGHT_SCALE
                )
                stamp_h = max(
                    1,
                    int(round(render_h)),
                )
                stamp = (
                    self._resize_material_wide_stamp_alpha(
                        brush_style_id,
                        material_stamp,
                        stamp_w,
                        stamp_h,
                        render_w,
                        render_h,
                    )
                    if interval_is_wide
                    else self._resize_alpha_bilinear(material_stamp, stamp_w, stamp_h)
                )
                base_opacity = 1.0 if not np.isfinite(stroke_opacity) else max(0.0, min(float(stroke_opacity), 1.0))
                stamp_opacity = base_opacity * (
                    VECTOR_MATERIAL_STAMP_GAP_ALPHA
                    if interval_is_wide
                    else VECTOR_MATERIAL_STAMP_ALPHA
                )
                anchor_y = VECTOR_MATERIAL_STAMP_GAP_ANCHOR_Y if interval_is_wide else 0.50
                stamp_color = color[:3]
                if interval_is_wide:
                    stamp_color = self._material_mix_preview_color(
                        color[:3],
                        sub_color,
                        float(material_stamp.mean()) / 255.0,
                    )
                stamp_points = points
                stamp_step = max(
                    VECTOR_MATERIAL_STAMP_MIN_STEP,
                    2.0 * width * max(float(style.interval_base), 0.05),
                )
                use_float_material_center = False
                if interval_is_wide:
                    stamp_points = self._native_spline_resample_points_by_distance(
                        points,
                        stamp_step,
                        point_flags,
                    )
                    use_float_material_center = True
                else:
                    stamp_points = self._vector_resample_points_by_distance(points, stamp_step)
                for point in stamp_points:
                    self._draw_material_stamp_rgba(
                        rgba,
                        point if use_float_material_center else self._vector_sample_point(point),
                        stamp_color,
                        stamp,
                        stamp_opacity,
                        anchor_x=0.50,
                        anchor_y=anchor_y,
                    )
                found = True
                continue
            aa_level = self._brush_anti_alias(brush_style_id)
            if flags == VECTOR_STROKE_FLAGS_NATIVE_AA:
                anti_alias_feather = VECTOR_NATIVE_AA_FEATHER_BY_LEVEL.get(aa_level, 0.0)
            else:
                anti_alias_feather = VECTOR_LEGACY_AA_FEATHER if aa_level > 0 else 0.0
            radius_base = width * VECTOR_STROKE_RADIUS_SCALE
            use_thickness_ellipse = False
            thickness_base = 1.0
            rotation_base = 0.0
            if use_simple_size_dynamics:
                radius_base *= VECTOR_SIMPLE_SIZE_RADIUS_SCALE
                if flags != VECTOR_STROKE_FLAGS_NATIVE_AA and anti_alias_feather > 0.0:
                    anti_alias_feather = VECTOR_SIMPLE_SIZE_AA_FEATHER
            elif use_pressure_size_dynamics:
                radius_base = width * VECTOR_PRESSURE_SIZE_RADIUS_SCALE
            elif use_pressure_opacity_dynamics:
                radius_base = width * VECTOR_PRESSURE_OPACITY_RADIUS_SCALE
            else:
                style = self._brush_style_preview(brush_style_id)
                if (
                    style is not None
                    and style.pattern_style == 0
                    and style.texture_pattern == 0
                    and 1 <= style.auto_interval_type <= 3
                ):
                    aa_factor = min(max(float(aa_level), 0.0), 2.0) / 2.0
                    if aa_factor > 0.0:
                        radius_base *= 1.0 - (1.0 - VECTOR_AUTO_INTERVAL_RADIUS_SCALE) * aa_factor
                        anti_alias_feather *= 1.0 + (VECTOR_AUTO_INTERVAL_FEATHER_SCALE - 1.0) * aa_factor
                if (
                    style is not None
                    and style.pattern_style == 0
                    and style.texture_pattern == 0
                    and 0.0 < style.flow_base < 1.0
                ):
                    flow_factor = 1.0 - max(0.0, min(float(style.flow_base), 1.0))
                    radius_base *= 1.0 - flow_factor * VECTOR_FLOW_RADIUS_SOFTEN_MAX
                if (
                    style is not None
                    and style.pattern_style == 0
                    and style.texture_pattern == 0
                    and style.auto_interval_type == 0
                    and style.interval_base > 0.1
                ):
                    interval_factor = min(
                        max((float(style.interval_base) - 0.1) / 0.4, 0.0),
                        1.0,
                    )
                    radius_base *= 1.0 - interval_factor * VECTOR_INTERVAL_RADIUS_SOFTEN_MAX
                if (
                    style is not None
                    and style.pattern_style == 0
                    and style.texture_pattern == 0
                    and 0.0 < style.thickness_base < 1.0
                ):
                    thickness_base = max(float(style.thickness_base), 0.05)
                    rotation_base = float(style.rotation_base)
                    use_thickness_ellipse = True
                if (
                    style is not None
                    and style.pattern_style == 0
                    and style.texture_pattern == 0
                    and 0.0 < style.hardness < 1.0
                ):
                    # Native no-pattern dabs derive the softness profile from
                    # threshold = hardness * 1.3 - 0.3. Keep this as a narrow
                    # capsule-preview bridge until the real row profile lands.
                    softness = 1.0 - max(float(style.hardness) * 1.3 - 0.3, 0.0)
                    if softness > 0.0:
                        anti_alias_feather = max(
                            anti_alias_feather,
                            softness * VECTOR_HARDNESS_FEATHER_SCALE,
                        )
                        radius_base *= max(
                            0.85,
                            1.0 - softness * VECTOR_HARDNESS_RADIUS_SOFTEN_SCALE,
                        )
            curve_points: list[tuple[float, float]] = []
            curve_tapers: list[float] = []
            curve_opacities: list[float] = []
            curve_flow_tapers: list[float] = []
            curve_size_factors: list[float] = []
            curve_flow_factors: list[float] = []
            curve_flow_effector_factors: list[float] = []
            curve_random_states: list[int] = []
            native_curve_random_state = int(stroke_random_seed) & 0xFFFFFFFF
            native_no_pattern_dab = self._brush_is_no_pattern_dab_candidate(brush_style_id)
            native_random_opacity_requested = (
                use_random_opacity_dynamics
                and native_no_pattern_dab
            ) or (
                os.environ.get(VECTOR_EXPERIMENTAL_NATIVE_RANDOM_OPACITY_ENV) == "1"
            )
            for idx in range(len(points) - 1):
                p0 = points[max(idx - 1, 0)]
                p1 = points[idx]
                p2 = points[idx + 1]
                p3 = points[min(idx + 2, len(points) - 1)]
                if idx == 0:
                    curve_points.append(self._vector_sample_point(p1))
                    curve_tapers.append(tapers[idx])
                    curve_opacities.append(opacity_tapers[idx])
                    curve_flow_tapers.append(flow_tapers[idx])
                    curve_size_factors.append(native_size_factors[idx])
                    curve_flow_factors.append(native_flow_factors[idx])
                    curve_flow_effector_factors.append(native_flow_effector_factors[idx])
                    native_curve_random_state = (
                        1103515245 * native_curve_random_state + 1234567
                    ) & 0xFFFFFFFF
                    curve_random_states.append(native_curve_random_state)
                distance = float(np.hypot(p2[0] - p1[0], p2[1] - p1[1]))
                # CSP subdivides vector curves by length, clamped to 1..32
                # samples. A 5 px fallback step is the best tested compromise
                # between the old V2 96/16 step and V4's resolution-dependent
                # RenderCurve subdivision for this SQLite centerline stream.
                sample_step = (
                    VECTOR_OPACITY_PRESSURE_SAMPLE_STEP
                    if use_pressure_opacity_dynamics
                    else VECTOR_FALLBACK_SAMPLE_STEP
                )
                sample_step = self._adaptive_vector_sample_step(
                    brush_style_id,
                    width,
                    sample_step,
                    (tapers[idx], tapers[idx + 1]),
                    (native_size_factors[idx], native_size_factors[idx + 1]),
                    use_simple_size_dynamics
                    or use_pressure_size_dynamics
                    or use_secondary_or_aux_size_dynamics
                    or use_random_size_dynamics,
                    (
                    use_pressure_size_dynamics
                        or native_no_pattern_dab
                    )
                    and (not native_random_opacity_requested or use_random_opacity_dynamics),
                )
                samples_per_segment = max(1, min(32, int(np.ceil(distance / sample_step))))
                for sample in range(1, samples_per_segment + 1):
                    t = sample / samples_per_segment
                    native_curve_random_state = (
                        1103515245 * native_curve_random_state + 1234567
                    ) & 0xFFFFFFFF
                    sampled_point = (
                        (
                            p1[0] * (1.0 - t) + p2[0] * t,
                            p1[1] * (1.0 - t) + p2[1] * t,
                        )
                        if native_random_opacity_requested and use_random_opacity_dynamics
                        else self._catmull_rom_point(p0, p1, p2, p3, t)
                    )
                    curve_points.append(
                        self._vector_sample_point(
                            sampled_point
                        )
                    )
                    curve_tapers.append(tapers[idx] * (1.0 - t) + tapers[idx + 1] * t)
                    curve_opacities.append(
                        opacity_tapers[idx] * (1.0 - t) + opacity_tapers[idx + 1] * t
                    )
                    curve_flow_tapers.append(
                        flow_tapers[idx] * (1.0 - t) + flow_tapers[idx + 1] * t
                    )
                    curve_size_factors.append(
                        native_size_factors[idx] * (1.0 - t) + native_size_factors[idx + 1] * t
                    )
                    curve_flow_factors.append(
                        native_flow_factors[idx] * (1.0 - t) + native_flow_factors[idx + 1] * t
                    )
                    curve_flow_effector_factors.append(
                        native_flow_effector_factors[idx] * (1.0 - t)
                        + native_flow_effector_factors[idx + 1] * t
                    )
                    curve_random_states.append(native_curve_random_state)

            # The native packet path maps compact point +36 to the primary
            # pressure-like scalar. Point +52 remains an endpoint taper in this
            # stream; averaging adjacent values keeps the body solid while
            # trimming stroke caps.
            use_experimental_dabs = native_no_pattern_dab and (
                not native_random_opacity_requested or use_random_opacity_dynamics
            )
            if use_experimental_dabs:
                style = self._brush_style_preview(brush_style_id) or _BrushStylePreview()
                base_opacity = 1.0 if not np.isfinite(stroke_opacity) else max(0.0, min(float(stroke_opacity), 1.0))
                native_random_opacity = (
                    use_random_opacity_dynamics
                    and native_random_opacity_requested
                )
                native_feedback_samples: list[tuple[tuple[float, float], float, float, float, float, float, int]] = []
                interval_scalar = self._brush_interval_scalar(brush_style_id)
                size_dynamics_for_dab = (
                    use_simple_size_dynamics
                    or use_pressure_size_dynamics
                    or use_secondary_or_aux_size_dynamics
                    or use_random_size_dynamics
                )
                if (
                    (
                        os.environ.get(VECTOR_EXPERIMENTAL_ADAPTIVE_SPACING_ENV) == "1"
                        or native_no_pattern_dab
                        or native_random_opacity
                    )
                    and interval_scalar is not None
                    and interval_scalar > 0.0
                ):
                    residual_distance = 0.0
                    native_feedback_state = int(stroke_random_seed) & 0xFFFFFFFF
                    for idx in range(len(points) - 1):
                        p0 = points[max(idx - 1, 0)]
                        p1 = points[idx]
                        p2 = points[idx + 1]
                        p3 = points[min(idx + 2, len(points) - 1)]
                        spline_controls = self._native_spline_segment_controls(points, idx, point_flags)
                        distance = self._native_spline_length_from_controls(spline_controls)
                        if not np.isfinite(distance) or distance <= 1e-6:
                            continue
                        walk = max(0.0, float(residual_distance))
                        emitted = 0
                        while walk < distance and emitted < 512:
                            t = self._native_spline_t_at_distance(spline_controls, walk, distance)
                            native_feedback_state = (
                                1103515245 * native_feedback_state + 1234567
                            ) & 0xFFFFFFFF
                            taper = tapers[idx] * (1.0 - t) + tapers[idx + 1] * t
                            if use_pressure_size_dynamics:
                                sample_primary = (
                                    native_primary_scalars[idx] * (1.0 - t)
                                    + native_primary_scalars[idx + 1] * t
                                )
                                sample_secondary = (
                                    native_secondary_scalars[idx] * (1.0 - t)
                                    + native_secondary_scalars[idx + 1] * t
                                )
                                sample_size = self._brush_pressure_effector_value(
                                    brush_style_id,
                                    "SizeEffector",
                                    sample_primary,
                                    sample_secondary,
                                )
                                if sample_size is None:
                                    sample_size = sample_primary
                                taper = max(0.0, min(float(sample_size), 1.0))
                            elif use_random_size_dynamics:
                                effector = self._brush_effector_blob(brush_style_id, "SizeEffector")
                                if isinstance(effector, bytes) and len(effector) == 8:
                                    flag, floor_value = struct.unpack(">I f", effector)
                                    if flag == 0x81:
                                        random_value = ((int(native_feedback_state) >> 16) & 0x7FFF) / 32768.0
                                        floor_value = max(0.0, min(float(floor_value), 1.0))
                                        taper = max(
                                            0.0,
                                            min(floor_value + (1.0 - floor_value) * random_value, 1.0),
                                        )
                            elif use_secondary_or_aux_size_dynamics:
                                sample_secondary = (
                                    native_secondary_scalars[idx] * (1.0 - t)
                                    + native_secondary_scalars[idx + 1] * t
                                )
                                sample_auxiliary = (
                                    native_auxiliary_scalars[idx] * (1.0 - t)
                                    + native_auxiliary_scalars[idx + 1] * t
                                )
                                sample_size = self._brush_secondary_or_aux_effector_value(
                                    brush_style_id,
                                    "SizeEffector",
                                    sample_secondary,
                                    sample_auxiliary,
                                )
                                if sample_size is not None:
                                    taper = max(0.0, min(float(sample_size), 1.0))
                            opacity_taper = (
                                opacity_tapers[idx] * (1.0 - t) + opacity_tapers[idx + 1] * t
                            )
                            if use_pressure_opacity_dynamics:
                                sample_primary = (
                                    native_primary_scalars[idx] * (1.0 - t)
                                    + native_primary_scalars[idx + 1] * t
                                )
                                sample_secondary = (
                                    native_secondary_scalars[idx] * (1.0 - t)
                                    + native_secondary_scalars[idx + 1] * t
                                )
                                sample_opacity = self._brush_pressure_effector_value(
                                    brush_style_id,
                                    "OpacityEffector",
                                    sample_primary,
                                    sample_secondary,
                                )
                                if sample_opacity is None:
                                    sample_opacity = sample_primary
                                opacity_taper = max(0.0, min(float(sample_opacity), 1.0))
                            elif use_secondary_or_aux_opacity_dynamics:
                                sample_secondary = (
                                    native_secondary_scalars[idx] * (1.0 - t)
                                    + native_secondary_scalars[idx + 1] * t
                                )
                                sample_auxiliary = (
                                    native_auxiliary_scalars[idx] * (1.0 - t)
                                    + native_auxiliary_scalars[idx + 1] * t
                                )
                                sample_opacity = self._brush_secondary_or_aux_effector_value(
                                    brush_style_id,
                                    "OpacityEffector",
                                    sample_secondary,
                                    sample_auxiliary,
                                )
                                if sample_opacity is not None:
                                    opacity_taper = max(0.0, min(float(sample_opacity), 1.0))
                            size_factor = (
                                native_size_factors[idx] * (1.0 - t)
                                + native_size_factors[idx + 1] * t
                            )
                            flow_factor = (
                                native_flow_factors[idx] * (1.0 - t)
                                + native_flow_factors[idx + 1] * t
                            )
                            flow_effector_factor = (
                                native_flow_effector_factors[idx] * (1.0 - t)
                                + native_flow_effector_factors[idx + 1] * t
                            )
                            feedback_point = self._native_spline_point_from_controls(
                                spline_controls,
                                t,
                            )
                            native_feedback_samples.append(
                                (
                                    (float(feedback_point[0]), float(feedback_point[1])),
                                    taper,
                                    opacity_taper,
                                    size_factor,
                                    flow_factor,
                                    flow_effector_factor,
                                    native_feedback_state,
                                )
                            )
                            size_multiplier = taper if size_dynamics_for_dab else 1.0
                            effective_size = float(width) * max(0.0, min(float(size_factor), 4.0))
                            effective_size *= max(0.0, min(float(size_multiplier), 4.0))
                            next_step = 2.0 * max(0.1, effective_size) * float(interval_scalar)
                            # Native 0x1422D8550 clamps the feedback distance
                            # below 1px before it is carried into the next dab.
                            if next_step < 1.0:
                                next_step = 1.0
                            if not np.isfinite(next_step) or next_step <= 1e-6:
                                break
                            walk += next_step
                            emitted += 1
                        if (
                            emitted == 0
                            and idx == len(points) - 2
                            and walk >= distance
                            and not size_dynamics_for_dab
                            and not native_random_opacity
                            and not use_pressure_opacity_dynamics
                            and not use_secondary_or_aux_opacity_dynamics
                            and not use_flow_dynamics
                            and style.hardness >= 0.999999
                            and aa_level == 0
                            and (point_flags[idx + 1] & 0x20) == 0
                        ):
                            # Native 0x1422CC595..0x1422CC5B9 can force a
                            # segment-end sample when the next scheduled dab
                            # has already overshot a terminal short segment.
                            native_feedback_state = (
                                1103515245 * native_feedback_state + 1234567
                            ) & 0xFFFFFFFF
                            native_feedback_samples.append(
                                (
                                    self._native_spline_point_from_controls(
                                        spline_controls,
                                        1.0,
                                    ),
                                    tapers[idx + 1],
                                    opacity_tapers[idx + 1],
                                    native_size_factors[idx + 1],
                                    native_flow_factors[idx + 1],
                                    native_flow_effector_factors[idx + 1],
                                    native_feedback_state,
                                )
                            )
                        residual_distance = max(0.0, walk - distance)
                opacity_floor = None
                if native_random_opacity:
                    effector = self._brush_effector_blob(brush_style_id, "OpacityEffector")
                    if isinstance(effector, bytes) and len(effector) == 8:
                        flag, floor_value = struct.unpack(">I f", effector)
                        if flag == 0x81:
                            opacity_floor = max(0.0, min(float(floor_value), 1.0))
                sample_iter = native_feedback_samples or zip(
                    curve_points,
                    curve_tapers,
                    curve_opacities,
                    curve_size_factors,
                    curve_flow_factors,
                    curve_flow_effector_factors,
                    curve_random_states,
                )
                # Native plot +0 is passed directly to the no-pattern dab rasterizer.
                native_radius_base = float(width)
                native_alpha_i = ((rgba[..., 3].astype(np.int32) * 32768) + 127) // 255
                for point, taper, opacity_taper, size_factor, flow_factor, flow_effector_factor, random_state in sample_iter:
                    if opacity_floor is not None:
                        random_value = ((int(random_state) >> 16) & 0x7FFF) / 32768.0
                        opacity_taper = opacity_floor + (1.0 - opacity_floor) * random_value
                    size_multiplier = taper if size_dynamics_for_dab else 1.0
                    radius = native_radius_base * max(0.0, min(float(size_multiplier) * float(size_factor), 4.0))
                    self._draw_native_dab_rgba(
                        rgba,
                        point,
                        color[:3],
                        radius=radius,
                        opacity_cap=base_opacity * opacity_taper,
                        flow=style.flow_base * flow_effector_factor * flow_factor,
                        aa_width=self._native_aa_width(
                            aa_level,
                            radius * (
                                max(0.05, min(float(style.thickness_base), 1.0))
                                if 0.0 < float(style.thickness_base) < 1.0
                                else 1.0
                            ),
                        ),
                        direct_max_accum=style.direct_max_accum,
                        alpha_i=native_alpha_i,
                        thickness=(
                            max(0.05, min(float(style.thickness_base), 4.0))
                            if (
                                style.pattern_style == 0
                                and style.texture_pattern == 0
                                and style.thickness_base > 0.0
                            )
                            else 1.0
                        ),
                        rotation_degrees=(
                            float(style.rotation_base)
                            if (
                                style.pattern_style == 0
                                and style.texture_pattern == 0
                                and style.thickness_base > 0.0
                            )
                            else 0.0
                        ),
                        hardness=style.hardness,
                    )
                rgba[..., 3] = np.where(native_alpha_i > 0, (native_alpha_i - 1) >> 7, 0).astype(np.uint8)
                found = True
                continue

            base_opacity = 1.0 if not np.isfinite(stroke_opacity) else max(0.0, min(float(stroke_opacity), 1.0))
            draw_rgba = (
                np.zeros_like(rgba)
                if self._brush_texture_preview_alpha(brush_style_id) is not None
                else rgba
            )
            for p0, p1, t0, t1, a0, a1, f0, f1 in zip(
                curve_points,
                curve_points[1:],
                curve_tapers,
                curve_tapers[1:],
                curve_opacities,
                curve_opacities[1:],
                curve_flow_tapers,
                curve_flow_tapers[1:],
            ):
                opacity = base_opacity * max(0.0, min((a0 + a1) * 0.5, 1.0))
                draw_color = (color[0], color[1], color[2], int(round(color[3] * opacity)))
                hard_alpha_mode = (
                    "max"
                    if (
                        use_pressure_opacity_dynamics
                        or use_secondary_or_aux_opacity_dynamics
                        or use_random_opacity_dynamics
                        or opacity < 0.999
                    )
                    else "overwrite"
                )
                radius = radius_base * ((t0 + t1) * 0.5) * ((f0 + f1) * 0.5)
                if use_thickness_ellipse:
                    self._draw_polyline_ellipse_rgba(
                        draw_rgba,
                        [p0, p1],
                        draw_color,
                        radius=radius,
                        thickness=thickness_base,
                        rotation_degrees=rotation_base,
                        feather=anti_alias_feather,
                        hard_alpha_mode=hard_alpha_mode,
                    )
                else:
                    self._draw_polyline_rgba(
                        draw_rgba,
                        [p0, p1],
                        draw_color,
                        radius=radius,
                        feather=anti_alias_feather,
                        hard_alpha_mode=hard_alpha_mode,
                    )
            if draw_rgba is not rgba:
                self._apply_brush_texture_preview(draw_rgba, brush_style_id)
                self._alpha_over_rgba(rgba, draw_rgba)
            found = True
        if not found or not rgba[..., 3].any():
            return None
        return rgba

    def _frame_folder_fallback_image(self, layer: sqlite3.Row) -> Optional[np.ndarray]:
        body = self._vector_object_body(layer["MainId"])
        if body is None:
            return None
        headers = self._vector_object_headers(body)
        if not headers:
            bbox = self._vector_header_bbox(body)
            if bbox is None:
                return None
            headers = [_VectorObjectHeader(
                bbox=bbox,
                line_rgb=self._vector_header_color(body),
                fill_rgb=self._vector_header_fill_color(body),
                opacity=1.0,
                width=VECTOR_OBJECT_FALLBACK_DEFAULT_WIDTH,
                line_style_id=0,
                fill_style_id=0,
                family_id=0,
                extra_id=0,
                point_bbox=None,
            )]
        rgba = np.zeros((self.height, self.width, 4), dtype=np.uint8)
        has_child = bool(layer["LayerFirstChildIndex"])
        for header in headers:
            bbox = header.bbox
            line_rgb = header.line_rgb
            object_fill = header.fill_rgb
            object_opacity = header.opacity
            vector_width = header.width
            object_alpha = int(round(255 * max(0.0, min(float(object_opacity), 1.0))))
            outline_width = self._fallback_outline_width(vector_width, extra=1 if has_child else 0)
            fill_rgb = self._frame_fill_color(layer, object_fill)
            fill = (
                (fill_rgb[0], fill_rgb[1], fill_rgb[2], object_alpha)
                if has_child and fill_rgb is not None
                else None
            )
            line_style = self._frame_line_style(layer, line_rgb, object_opacity, vector_width)
            if line_style is None:
                outline = None
                draw_outline_width = outline_width
            else:
                draw_line_rgb, line_alpha, style_width = line_style
                outline = (draw_line_rgb[0], draw_line_rgb[1], draw_line_rgb[2], line_alpha)
                draw_outline_width = style_width or outline_width
            x0, y0, x1, y1 = bbox
            inset = FRAME_FALLBACK_BBOX_INSET
            inset_bbox = (
                min(max(x0 + inset, 0), self.width),
                min(max(y0 + inset, 0), self.height),
                min(max(x1 - inset, 0), self.width),
                min(max(y1 - inset, 0), self.height),
            )
            self._draw_rect_rgba(
                rgba,
                inset_bbox,
                fill=fill,
                outline=outline,
                width=draw_outline_width,
            )
        frame_line = self._frame_line_cache_image(layer)
        if frame_line is not None:
            self._alpha_over_rgba(rgba, frame_line)
        return rgba

    def _vector_object_record_point_path(
        self,
        body: bytes,
        record: _VectorObjectRecord,
        samples: int = BALLOON_NATIVE_POINT_FAMILY_SAMPLES,
    ) -> Optional[list[tuple[float, float]]]:
        points_start = record.off + record.header_len
        points: list[tuple[float, float]] = []
        point_flags: list[int] = []
        controls: list[tuple[float, float]] = []
        for idx in range(record.point_count):
            point_off = points_start + idx * record.point_stride
            if point_off + 36 > len(body):
                return None
            x = struct.unpack_from(">d", body, point_off)[0]
            y = struct.unpack_from(">d", body, point_off + 8)[0]
            if not (np.isfinite(x) and np.isfinite(y)):
                return None
            points.append((float(x), float(y)))
            point_flags.append(int(struct.unpack_from(">I", body, point_off + 32)[0]))
            if record.object_flags & VECTOR_OBJECT_FLAGS_CONTROL_POINT:
                tail_off = point_off + record.point_tail_offset
                if tail_off + 16 > len(body):
                    return None
                cx = struct.unpack_from(">d", body, tail_off)[0]
                cy = struct.unpack_from(">d", body, tail_off + 8)[0]
                if not (np.isfinite(cx) and np.isfinite(cy)):
                    return None
                controls.append((float(cx), float(cy)))

        if len(points) < 2:
            return None
        samples = max(2, int(samples))
        path: list[tuple[float, float]] = []
        if record.object_flags & VECTOR_OBJECT_FLAGS_CONTROL_POINT:
            if len(controls) != len(points):
                return None
            for idx, p0 in enumerate(points):
                ctrl = controls[idx]
                p1 = points[(idx + 1) % len(points)]
                for sample_idx in range(samples):
                    t = sample_idx / float(samples)
                    u = 1.0 - t
                    path.append((
                        u * u * p0[0] + 2.0 * u * t * ctrl[0] + t * t * p1[0],
                        u * u * p0[1] + 2.0 * u * t * ctrl[1] + t * t * p1[1],
                    ))
            return path

        if record.object_flags & VECTOR_OBJECT_FLAGS_SPLINE:
            if len(points) < 3:
                return points
            for idx in range(len(points)):
                segment = self._native_spline_closed_segment_controls(points, idx, point_flags)
                for sample_idx in range(samples):
                    path.append(self._native_spline_point_from_controls(
                        segment,
                        sample_idx / float(samples),
                    ))
            return path

        return points

    def _balloon_native_point_family_image(
        self,
        body: bytes,
        color_map=None,
    ) -> Optional[np.ndarray]:
        records = [
            record for record in self._vector_object_records_100(body)
            if record.family_id == VECTOR_FAMILY_BALLOON
        ]
        if len(records) < BALLOON_NATIVE_POINT_FAMILY_MIN_RECORDS:
            return None
        rgba = np.zeros((self.height, self.width, 4), dtype=np.uint8)
        for record in records:
            path = self._vector_object_record_point_path(body, record)
            if path is None or len(path) < 3:
                continue
            line_rgb = record.line_rgb if color_map is None else color_map(record.line_rgb)
            fill_rgb = record.fill_rgb if color_map is None else color_map(record.fill_rgb)
            alpha = int(round(255 * max(0.0, min(float(record.opacity), 1.0))))
            fill = (fill_rgb[0], fill_rgb[1], fill_rgb[2], alpha)
            outline_alpha = alpha
            outline_feather = 0.0
            line_style = self._brush_style_preview(record.line_style_id)
            if line_style is not None and line_style.pattern_style == 11:
                outline_alpha = int(round(alpha * BALLOON_NATIVE_RETAINED_PATTERN_ALPHA))
                outline_feather = BALLOON_NATIVE_RETAINED_PATTERN_FEATHER
            outline = (line_rgb[0], line_rgb[1], line_rgb[2], outline_alpha)
            self._draw_polygon_rgba(rgba, path, fill)
            if outline_alpha > 0:
                radius = max(1.0, float(record.width))
                if (
                    line_style is not None
                    and line_style.pattern_style == BALLOON_NATIVE_RETAINED_DAB_PATTERN_STYLE
                ):
                    step = max(
                        BALLOON_NATIVE_RETAINED_DAB_MIN_STEP,
                        radius * max(float(line_style.interval_base), 0.05),
                    )
                    for point in self._closed_path_resample_points_by_distance(path, step):
                        self._draw_polyline_rgba(
                            rgba,
                            [point, point],
                            outline,
                            radius=radius,
                            feather=outline_feather,
                        )
                else:
                    self._draw_polyline_rgba(
                        rgba,
                        path + [path[0]],
                        outline,
                        radius=radius,
                        feather=outline_feather,
                    )
        if not rgba[..., 3].any():
            return None
        return rgba

    def _balloon_fallback_image(self, layer: sqlite3.Row) -> Optional[np.ndarray]:
        if not self._is_balloon_vector_layer(layer):
            return None
        body = self._vector_object_body(layer["MainId"])
        if body is None:
            return None
        headers = self._vector_object_headers(body)
        native_point_image = self._balloon_native_point_family_image(body)
        if native_point_image is not None:
            return native_point_image
        if not headers:
            bbox = self._vector_header_bbox(body)
            if bbox is None:
                return None
            headers = [_VectorObjectHeader(
                bbox=bbox,
                line_rgb=self._vector_header_color(body),
                fill_rgb=self._vector_header_fill_color(body),
                opacity=1.0,
                width=VECTOR_OBJECT_FALLBACK_DEFAULT_WIDTH,
                line_style_id=0,
                fill_style_id=0,
                family_id=0,
                extra_id=0,
                point_bbox=None,
            )]
        rgba = np.zeros((self.height, self.width, 4), dtype=np.uint8)
        balloon_index = int(layer["VectorNormalBalloonIndex"] or 0)
        for header in headers:
            bbox = header.bbox
            line_rgb = header.line_rgb
            fill_rgb = header.fill_rgb
            object_opacity = header.opacity
            vector_width = header.width
            line_style_id = header.line_style_id
            point_bbox = header.point_bbox
            object_alpha = int(round(255 * max(0.0, min(float(object_opacity), 1.0))))
            tuning = BALLOON_FALLBACK_TUNING_BY_INDEX.get(
                balloon_index,
                BALLOON_FALLBACK_DEFAULT_TUNING,
            )
            bbox_expand = tuning.bbox_expand
            point_weight = tuning.point_weight
            outline_width = self._fallback_outline_width(vector_width, extra=tuning.outline_extra)
            ellipse_power = tuning.ellipse_power
            x0, y0, x1, y1 = bbox
            inset_l, inset_t, inset_r, inset_b = BALLOON_FALLBACK_BODY_BBOX_INSET
            inset_bbox = (
                min(max(x0 + inset_l - bbox_expand, 0), self.width),
                min(max(y0 + inset_t - bbox_expand, 0), self.height),
                min(max(x1 - inset_r + bbox_expand, 0), self.width),
                min(max(y1 - inset_b + bbox_expand, 0), self.height),
            )
            if point_bbox is not None:
                px0, py0, px1, py1 = point_bbox
                point_expand = BALLOON_FALLBACK_POINT_BBOX_EXPAND + bbox_expand
                point_expanded = (
                    min(max(px0 - point_expand, 0), self.width),
                    min(max(py0 - point_expand, 0), self.height),
                    min(max(px1 + point_expand, 0), self.width),
                    min(max(py1 + point_expand, 0), self.height),
                )
                inset_bbox = tuple(
                    int(round(a * (1.0 - point_weight) + b * point_weight))
                    for a, b in zip(inset_bbox, point_expanded)
                )
            draw_ellipse = (
                self._draw_ellipse_rgba_supersampled
                if self._brush_anti_alias(line_style_id) >= 3
                else self._draw_ellipse_rgba
            )
            draw_ellipse(
                rgba,
                inset_bbox,
                fill=(fill_rgb[0], fill_rgb[1], fill_rgb[2], object_alpha),
                outline=(line_rgb[0], line_rgb[1], line_rgb[2], object_alpha),
                width=outline_width,
                power=ellipse_power,
            )
        if not rgba[..., 3].any():
            return None
        return rgba

    def _tlv_records_le(self, blob: bytes, start: int = 0) -> dict[int, bytes]:
        records: dict[int, bytes] = {}
        pos = start
        while pos + 8 <= len(blob):
            rec_id, rec_len = struct.unpack_from("<II", blob, pos)
            pos += 8
            if rec_id > 10000 or rec_len > len(blob) - pos:
                break
            records[rec_id] = blob[pos:pos + rec_len]
            pos += rec_len
        return records

    def _offscreen_pixel_size(self, offscreen_id: int) -> Optional[tuple[int, int]]:
        row = self._db.execute(
            "SELECT Attribute FROM Offscreen WHERE MainId=?",
            (offscreen_id,),
        ).fetchone()
        if row is None:
            return None
        attr = row["Attribute"]
        if not isinstance(attr, bytes) or len(attr) < 20:
            return None
        try:
            _kind, _scheme, _label, payload_len, name_len = struct.unpack_from(">IIIII", attr, 0)
            name_start = 20
            payload_start = name_start + name_len * 2
            payload_end = payload_start + payload_len
            if name_len <= 0 or payload_end > len(attr):
                return None
            name = attr[name_start:payload_start].decode("utf-16-be")
            if name != "Parameter" or payload_len < 8:
                return None
            width, height = struct.unpack_from(">II", attr, payload_start)
        except (struct.error, UnicodeDecodeError):
            return None
        if width <= 0 or height <= 0 or width > self.width * 4 or height > self.height * 4:
            return None
        return int(width), int(height)

    def _decode_offscreen_rgba(self, offscreen_id: int) -> Optional[np.ndarray]:
        size = self._offscreen_pixel_size(offscreen_id)
        if size is None:
            return None
        row = self._db.execute(
            "SELECT BlockData FROM Offscreen WHERE MainId=?",
            (offscreen_id,),
        ).fetchone()
        if row is None:
            return None
        ext_id = row["BlockData"]
        if isinstance(ext_id, bytes):
            ext_id = ext_id.decode("ascii")
        cols = (size[0] + TILE - 1) // TILE
        rows = (size[1] + TILE - 1) // TILE
        expected_len = cols * rows * PER_TILE_BYTES
        blob = self._get_tile_blob(ext_id, expected_len=expected_len)
        if blob is None:
            return None
        try:
            return _tiles_to_rgba(blob, size[0], size[1])
        except ValueError as exc:
            log.warning("Offscreen decode failed for %d: %s", offscreen_id, exc)
            return None

    def _decode_mipmap_rgba(self, mipmap_id: int) -> Optional[np.ndarray]:
        offscreen_id = self._mipmap_offscreen_id(mipmap_id)
        if offscreen_id is None:
            return None
        return self._decode_offscreen_rgba(offscreen_id)

    def _decode_mipmap_alpha(self, mipmap_id: int) -> Optional[np.ndarray]:
        offscreen_id = self._mipmap_offscreen_id(mipmap_id)
        if offscreen_id is None:
            return None
        size = self._offscreen_pixel_size(offscreen_id)
        if size is None:
            return None
        ext_id = self._resolve_mipmap_external_id(mipmap_id)
        if ext_id is None:
            return None
        cols = (size[0] + TILE - 1) // TILE
        rows = (size[1] + TILE - 1) // TILE
        blob = self._get_tile_blob(ext_id, expected_len=cols * rows * MASK_TILE_BYTES)
        if blob is None:
            return None
        try:
            return _tiles_to_alpha(blob, size[0], size[1])
        except ValueError:
            return None

    def _frame_line_cache_image(self, layer: sqlite3.Row) -> Optional[np.ndarray]:
        mipmap_id = layer["ComicFrameLineMipmap"]
        if not mipmap_id:
            return None
        rgba = self._decode_mipmap_rgba(mipmap_id)
        if rgba is None or not rgba[..., 3].any():
            return None
        if rgba.shape[0] == self.height and rgba.shape[1] == self.width:
            return rgba
        canvas = np.zeros((self.height, self.width, 4), dtype=np.uint8)
        paste_h = min(self.height, rgba.shape[0])
        paste_w = min(self.width, rgba.shape[1])
        canvas[:paste_h, :paste_w] = rgba[:paste_h, :paste_w]
        return canvas

    def _alpha_over_rgba(self, dst: np.ndarray, src: np.ndarray) -> None:
        bbox = _alpha_bbox(src[..., 3])
        if bbox is None:
            return
        y0, y1, x0, x1 = bbox
        src_rgb = src[y0:y1, x0:x1, :3].astype(np.float32) / 255.0
        src_a = (src[y0:y1, x0:x1, 3:4].astype(np.float32) / 255.0)
        dst_rgb = dst[y0:y1, x0:x1, :3].astype(np.float32) / 255.0
        dst_a = (dst[y0:y1, x0:x1, 3:4].astype(np.float32) / 255.0)
        out_a = src_a + dst_a * (1.0 - src_a)
        with np.errstate(invalid="ignore", divide="ignore"):
            out_rgb = np.where(
                out_a > 1e-6,
                (src_rgb * src_a + dst_rgb * dst_a * (1.0 - src_a)) / out_a,
                0.0,
            )
        dst[y0:y1, x0:x1, :3] = np.clip(
            np.floor(out_rgb * 255.0 + 0.5), 0, 255
        ).astype(np.uint8)
        dst[y0:y1, x0:x1, 3] = np.clip(
            np.floor(out_a[..., 0] * 255.0 + 0.5), 0, 255
        ).astype(np.uint8)

    def _text_cache_fallback_image(self, layer: sqlite3.Row) -> Optional[np.ndarray]:
        if "TextLayerAttributes" not in layer.keys():
            return None
        attrs = layer["TextLayerAttributes"]
        if not isinstance(attrs, bytes):
            return None
        records = self._tlv_records_le(attrs)
        cache_id_blob = records.get(50)
        rect_blob = records.get(42)
        if cache_id_blob is None or rect_blob is None:
            return None
        if len(cache_id_blob) < 4 or len(rect_blob) < 16:
            return None
        offscreen_id = struct.unpack_from("<I", cache_id_blob, 0)[0]
        x0, y0, x1, y1 = struct.unpack_from("<IIII", rect_blob, 0)
        cache = self._decode_offscreen_rgba(offscreen_id)
        if cache is None:
            return None
        if not cache[..., 3].any():
            return None
        x0 = max(0, min(int(x0), self.width))
        y0 = max(0, min(int(y0), self.height))
        x1 = max(0, min(int(x1), self.width))
        y1 = max(0, min(int(y1), self.height))
        if x1 <= x0 or y1 <= y0:
            return None
        paste_w = min(cache.shape[1], self.width - x0)
        paste_h = min(cache.shape[0], self.height - y0)
        if paste_w <= 0 or paste_h <= 0:
            return None
        rgba = np.zeros((self.height, self.width, 4), dtype=np.uint8)
        body = self._vector_object_body(layer["MainId"])
        if body is not None:
            headers = self._vector_object_headers(body)
            if headers:
                header = headers[0]
                bbox = header.bbox
                line_rgb = header.line_rgb
                fill_rgb = header.fill_rgb
                object_opacity = header.opacity
                vector_width = header.width
            else:
                bbox = self._vector_header_bbox(body)
                line_rgb = self._vector_header_color(body)
                fill_rgb = self._vector_header_fill_color(body)
                object_opacity = 1.0
                vector_width = VECTOR_OBJECT_FALLBACK_DEFAULT_WIDTH
            if bbox is not None:
                object_alpha = int(round(255 * max(0.0, min(float(object_opacity), 1.0))))
                x0b, y0b, x1b, y1b = bbox
                inset_l, inset_t, inset_r, inset_b = TEXT_BALLOON_FALLBACK_INSET
                bbox = (
                    min(max(x0b + inset_l, 0), self.width),
                    min(max(y0b + inset_t, 0), self.height),
                    min(max(x1b - inset_r, 0), self.width),
                    min(max(y1b - inset_b, 0), self.height),
                )
                self._draw_ellipse_rgba(
                    rgba,
                    bbox,
                    fill=(fill_rgb[0], fill_rgb[1], fill_rgb[2], object_alpha),
                    outline=(line_rgb[0], line_rgb[1], line_rgb[2], object_alpha),
                    width=self._fallback_outline_width(vector_width, extra=1),
                    power=TEXT_BALLOON_FALLBACK_POWER,
                )
        rgba[y0:y0 + paste_h, x0:x0 + paste_w] = cache[:paste_h, :paste_w]
        return rgba

    # ----- composite ----- #

    def _blend_mode_for_layer(self, layer: sqlite3.Row) -> str:
        comp_int = layer["LayerComposite"]
        mode = _BLEND_MAPPING.get(comp_int)
        if mode is None:
            log.warning("Layer %d (%r): unknown LayerComposite=%d - treating as Normal. "
                        "Add this integer to clip_loader._BLEND_MAPPING when its mode is identified.",
                        layer["MainId"], layer["LayerName"], comp_int)
            mode = "NORMAL"
        return mode

    def _apply_mask_and_clip(
        self,
        layer: sqlite3.Row,
        rgba: np.ndarray,
        mask: Optional[np.ndarray],
        clip_alpha_u8: Optional[np.ndarray],
    ) -> np.ndarray:
        layer_alpha_u8 = rgba[..., 3]
        if mask is not None:
            if mask.shape == layer_alpha_u8.shape:
                layer_alpha_u8 = ((layer_alpha_u8.astype(np.uint16) * mask) // 255).astype(np.uint8)
            else:
                log.warning("Layer %d (%r): mask shape %s != layer shape %s; ignoring mask.",
                            layer["MainId"], layer["LayerName"],
                            mask.shape, layer_alpha_u8.shape)
        if layer["LayerClip"] and clip_alpha_u8 is not None:
            layer_alpha_u8 = (
                (layer_alpha_u8.astype(np.uint16) * clip_alpha_u8) // 255
            ).astype(np.uint8)
        return layer_alpha_u8

    def _apply_mask_and_clip_region(
        self,
        layer: sqlite3.Row,
        rgba: np.ndarray,
        mask: Optional[np.ndarray],
        clip_alpha_u8: Optional[np.ndarray],
        canvas_bbox: tuple[int, int, int, int],
    ) -> np.ndarray:
        layer_alpha_u8 = rgba[..., 3]
        y0, y1, x0, x1 = canvas_bbox
        if mask is not None:
            if mask.shape == (self.height, self.width):
                mask_region = mask[y0:y1, x0:x1]
            elif mask.shape == layer_alpha_u8.shape:
                mask_region = mask
            else:
                log.warning("Layer %d (%r): mask shape %s != region shape %s; ignoring mask.",
                            layer["MainId"], layer["LayerName"],
                            mask.shape, layer_alpha_u8.shape)
                mask_region = None
            if mask_region is not None:
                layer_alpha_u8 = (
                    (layer_alpha_u8.astype(np.uint16) * mask_region) // 255
                ).astype(np.uint8)
        if layer["LayerClip"] and clip_alpha_u8 is not None:
            layer_alpha_u8 = (
                (layer_alpha_u8.astype(np.uint16) * clip_alpha_u8[y0:y1, x0:x1]) // 255
            ).astype(np.uint8)
        return layer_alpha_u8

    def _layer_mask_for_composite(self, layer: sqlite3.Row) -> Optional[np.ndarray]:
        if not layer["LayerLayerMaskMipmap"]:
            return None
        return self.decode_layer_mask(layer["MainId"])

    def _chain_next_uses_clip_base(
        self,
        chain_ids: list[int],
        start_index: int,
        skip_ids: set[int],
    ) -> bool:
        for next_id in chain_ids[start_index:]:
            if next_id in skip_ids:
                continue
            row = self._layer_row(next_id)
            if row is None or not _layer_is_visible(row):
                continue
            if row["LayerType"] == LAYER_TYPE_PAPER:
                continue
            if row["LayerType"] == LAYER_TYPE_FILTER:
                return False
            if row["LayerClip"]:
                return True
            if row["LayerType"] not in RASTER_LAYER_TYPES and row["LayerType"] not in (
                LAYER_TYPE_LAYER_FOLDER,
                LAYER_TYPE_GROUP,
            ):
                return True
            return False
        return False

    def _composite_image(self, out: np.ndarray, layer: sqlite3.Row, rgba: np.ndarray,
                         layer_alpha_u8: np.ndarray, apply_opacity: bool = True,
                         dst_offset: tuple[int, int] = (0, 0)) -> bool:
        mode = self._blend_mode_for_layer(layer)

        bbox = _alpha_bbox(layer_alpha_u8)
        if bbox is None:
            return False
        y0, y1, x0, x1 = bbox
        dst_y0 = y0 + dst_offset[1]
        dst_y1 = y1 + dst_offset[1]
        dst_x0 = x0 + dst_offset[0]
        dst_x1 = x1 + dst_offset[0]

        opacity = min(layer["LayerOpacity"] / 256.0, 1.0) if apply_opacity else 1.0
        src_rgb_u8 = rgba[y0:y1, x0:x1, :3]
        src_a_u8 = layer_alpha_u8[y0:y1, x0:x1]

        src_rgb = src_rgb_u8.astype(np.float32) / 255.0
        src_a = (src_a_u8.astype(np.float32) / 255.0)[..., None] * opacity

        dst_rgb_pm = out[dst_y0:dst_y1, dst_x0:dst_x1, :3]
        dst_a = out[dst_y0:dst_y1, dst_x0:dst_x1, 3:4]

        if mode == "NORMAL":
            src_rgb_pm = src_rgb * src_a
            inv_sa = 1.0 - src_a
            out[dst_y0:dst_y1, dst_x0:dst_x1, :3] = src_rgb_pm + dst_rgb_pm * inv_sa
            out[dst_y0:dst_y1, dst_x0:dst_x1, 3:4] = src_a + dst_a * inv_sa

        elif mode == "ADD" or mode == "ADD_GLOW":
            with np.errstate(invalid="ignore", divide="ignore"):
                dst_rgb_straight = np.where(dst_a > 1e-6,
                                            dst_rgb_pm / np.maximum(dst_a, 1e-6), 0.0)
            dst_rgba = np.empty((y1 - y0, x1 - x0, 4), dtype=np.uint8)
            dst_rgba[..., :3] = np.clip(
                np.floor(dst_rgb_straight * 255.0 + 0.5), 0, 255
            ).astype(np.uint8)
            dst_rgba[..., 3] = np.clip(
                np.floor(dst_a[..., 0] * 255.0 + 0.5), 0, 255
            ).astype(np.uint8)
            src_rgba = np.empty_like(dst_rgba)
            src_rgba[..., :3] = src_rgb_u8
            src_rgba[..., 3] = np.clip(
                np.floor(src_a[..., 0] * 255.0 + 0.5), 0, 255
            ).astype(np.uint8)
            if mode == "ADD_GLOW":
                blended_u8 = _blend_add_glow_u8(dst_rgba, src_rgba)
            else:
                blended_u8 = _blend_add_u8(dst_rgba, src_rgba)
            out_a = src_a + dst_a * (1.0 - src_a)
            out[dst_y0:dst_y1, dst_x0:dst_x1, :3] = (
                blended_u8[..., :3].astype(np.float32) / 255.0
            ) * out_a
            out[dst_y0:dst_y1, dst_x0:dst_x1, 3:4] = out_a
        elif mode == "GLOW_DODGE":
            out_a = src_a + dst_a * (1.0 - src_a)
            with np.errstate(invalid="ignore", divide="ignore"):
                dst_rgb_straight = np.where(dst_a > 1e-6,
                                            dst_rgb_pm / np.maximum(dst_a, 1e-6), 0.0)
            strength_u8 = np.clip(np.floor(src_rgb * src_a * 255.0 + 0.5), 0, 255).astype(np.int32)
            dst_u8 = np.clip(np.floor(dst_rgb_straight * 255.0 + 0.5), 0, 255).astype(np.int32)
            dodge_u8 = np.where(strength_u8 >= 255, 255,
                                np.minimum(255, (dst_u8 * 255) // np.maximum(255 - strength_u8, 1)))
            dodge_rgb = dodge_u8.astype(np.float32) / 255.0
            # CSP Glow Dodge blends toward source colour on transparent/semi-
            # transparent backgrounds (documented as "stronger in semi-transparent
            # areas"). Blend in premultiplied space so the result is continuous.
            dodge_pm = dodge_rgb * out_a
            src_pm = src_rgb * src_a
            dst_blend = np.minimum(dst_a / np.maximum(out_a, 1e-6), 1.0)
            out[dst_y0:dst_y1, dst_x0:dst_x1, :3] = dodge_pm * dst_blend + src_pm * (1.0 - dst_blend)
            out[dst_y0:dst_y1, dst_x0:dst_x1, 3:4] = out_a
        elif mode == "COLOR_DODGE" or mode == "COLOR_BURN":
            eps = 1e-6
            dst_rgb_straight = dst_rgb_pm / np.maximum(dst_a, eps)
            dst_rgb_quant = np.floor(np.clip(dst_rgb_straight, 0.0, 1.0) * 255.0 + 0.5) / 255.0
            blended = np.clip(_blend_func(mode, src_rgb, dst_rgb_quant), 0.0, 1.0)

            inv_sa = 1.0 - src_a
            inv_da = 1.0 - dst_a
            src_pm = src_rgb * src_a
            dst_rgb_pm_quant = dst_rgb_quant * dst_a
            out[dst_y0:dst_y1, dst_x0:dst_x1, :3] = (
                inv_da * src_pm + inv_sa * dst_rgb_pm_quant + src_a * dst_a * blended
            )
            out[dst_y0:dst_y1, dst_x0:dst_x1, 3:4] = src_a + dst_a * inv_sa
        else:
            eps = 1e-6
            dst_rgb_straight = dst_rgb_pm / np.maximum(dst_a, eps)
            blended = np.clip(_blend_func(mode, src_rgb, dst_rgb_straight), 0.0, 1.0)

            inv_sa = 1.0 - src_a
            inv_da = 1.0 - dst_a
            src_pm = src_rgb * src_a
            out[dst_y0:dst_y1, dst_x0:dst_x1, :3] = inv_da * src_pm + inv_sa * dst_rgb_pm + src_a * dst_a * blended
            out[dst_y0:dst_y1, dst_x0:dst_x1, 3:4] = src_a + dst_a * inv_sa

        return True

    def _composite_clipped_image(
        self,
        out: np.ndarray,
        layer: sqlite3.Row,
        rgba: np.ndarray,
        layer_alpha_u8: np.ndarray,
        clip_base_alpha_u8: np.ndarray,
        always_preserve: bool = False,
    ) -> bool:
        """Composite a clipped layer inside an isolated folder/group buffer.

        CSP clipping preserves the clipping base's edge alpha when that base is
        the visible destination. If there is already more opaque artwork below
        the base at a pixel, the regular product-alpha path remains closer.
        """
        bbox = _alpha_bbox(layer_alpha_u8)
        if bbox is None:
            return False
        y0, y1, x0, x1 = bbox

        before = out[y0:y1, x0:x1].copy()
        self._composite_image(out, layer, rgba, layer_alpha_u8)
        regular = out[y0:y1, x0:x1].copy()
        out[y0:y1, x0:x1] = before

        mode = self._blend_mode_for_layer(layer)
        opacity = min(layer["LayerOpacity"] / 256.0, 1.0)
        src_rgb = rgba[y0:y1, x0:x1, :3].astype(np.float32) / 255.0
        src_strength = (rgba[y0:y1, x0:x1, 3].astype(np.float32) / 255.0)[..., None] * opacity
        effective_strength = (
            layer_alpha_u8[y0:y1, x0:x1].astype(np.float32) / 255.0
        )[..., None] * opacity
        visible = (layer_alpha_u8[y0:y1, x0:x1].astype(np.float32) > 0)[..., None]
        src_strength = np.where(visible, src_strength, 0.0)
        effective_strength = np.where(visible, effective_strength, 0.0)

        dst_a = before[..., 3:4]
        with np.errstate(invalid="ignore", divide="ignore"):
            dst_rgb = np.where(dst_a > 1e-6, before[..., :3] / np.maximum(dst_a, 1e-6), 0.0)
        if mode == "NORMAL":
            preserve_rgb = src_rgb * src_strength + dst_rgb * (1.0 - src_strength)
        elif mode == "ADD" or mode == "ADD_GLOW":
            preserve_rgb = np.minimum(dst_rgb + src_rgb * effective_strength, 1.0)
        else:
            blended = np.clip(_blend_func(mode, src_rgb, dst_rgb), 0.0, 1.0)
            preserve_rgb = blended * src_strength + dst_rgb * (1.0 - src_strength)

        preserve = before.copy()
        preserve[..., :3] = preserve_rgb * dst_a

        clip_a = clip_base_alpha_u8[y0:y1, x0:x1].astype(np.float32) / 255.0
        if always_preserve:
            out[y0:y1, x0:x1] = preserve
        else:
            use_preserve = ((clip_a > 0) & (dst_a[..., 0] <= clip_a + (2.25 / 255.0)))[..., None]
            out[y0:y1, x0:x1] = np.where(use_preserve, preserve, regular)
        return True

    def _premul_to_rgba_u8(
        self,
        premul: np.ndarray,
        transparent_rgb: int = 0,
    ) -> tuple[np.ndarray, np.ndarray]:
        alpha = premul[..., 3]
        nonzero = alpha > 0
        rgba_u8 = np.empty(premul.shape, dtype=np.uint8)
        channel = np.empty(alpha.shape, dtype=np.float32)
        with np.errstate(invalid="ignore", divide="ignore"):
            for idx in range(3):
                channel.fill(0.0)
                np.divide(premul[..., idx], alpha, out=channel, where=nonzero)
                rgba_u8[..., idx] = np.clip(channel * 255.0 + 0.5, 0, 255).astype(np.uint8)
        if transparent_rgb:
            rgba_u8[..., :3][~nonzero] = transparent_rgb
        rgba_u8[..., 3] = np.clip(alpha * 255.0 + 0.5, 0, 255).astype(np.uint8)
        return rgba_u8, rgba_u8[..., 3]

    def _premul_region_to_rgba_u8(
        self,
        premul: np.ndarray,
        bbox: tuple[int, int, int, int],
    ) -> tuple[np.ndarray, np.ndarray]:
        y0, y1, x0, x1 = bbox
        return self._premul_to_rgba_u8(premul[y0:y1, x0:x1])

    def _apply_filter_layer(self, out: np.ndarray, layer: sqlite3.Row) -> bool:
        info = _filter_info(layer)
        if info is None:
            log.warning("Layer %d (%r): filter layer missing FilterLayerInfo; skipping.",
                        layer["MainId"], layer["LayerName"])
            return False
        filter_type, payload = info
        before = out.copy()
        alpha = out[..., 3:4]
        with np.errstate(invalid="ignore", divide="ignore"):
            rgb = np.where(alpha > 0, out[..., :3] / np.maximum(alpha, 1e-6), 0.0)
        rgb_u8 = np.clip(np.floor(rgb * 255.0 + 0.5), 0, 255).astype(np.uint8)

        if filter_type == 1 and len(payload) >= 8:  # Brightness/Contrast
            brightness, contrast = struct.unpack_from(">ii", payload, 0)
            lut = _contrast_lut(contrast)[_brightness_lut(brightness)]
            rgb_u8 = lut[rgb_u8]
        elif filter_type == 2 and len(payload) >= 0x40:  # Level Correction
            adjusted = _apply_level_adjust(rgb_u8, payload)
            if adjusted is not None:
                rgb_u8 = adjusted
            else:
                log.warning("Layer %d (%r): malformed level payload; skipping.",
                            layer["MainId"], layer["LayerName"])
                return False
        elif filter_type == 3 and len(payload) >= 0x104:  # Tone Curve
            adjusted = _apply_tone_curve(rgb_u8, payload)
            if adjusted is not None:
                rgb_u8 = adjusted
            else:
                log.warning("Layer %d (%r): malformed tone curve payload; skipping.",
                            layer["MainId"], layer["LayerName"])
                return False
        elif filter_type == 4 and len(payload) >= 12:  # Hue/Saturation/Luminosity
            hue, saturation, luminosity = struct.unpack_from(">iii", payload, 0)
            rgb_u8 = _apply_hsl_adjust(rgb_u8, hue, saturation, luminosity)
        elif filter_type == 5 and len(payload) >= 40:  # Color balance
            adjusted = _apply_color_balance(rgb_u8, payload)
            if adjusted is not None:
                rgb_u8 = adjusted
            else:
                log.warning("Layer %d (%r): malformed color balance payload; skipping.",
                            layer["MainId"], layer["LayerName"])
                return False
        elif filter_type == 6:  # Reverse Gradient / Invert
            rgb_u8 = 255 - rgb_u8
        elif filter_type == 7 and len(payload) >= 4:  # Posterization
            levels = max(2, struct.unpack_from(">i", payload, 0)[0])
            x = np.arange(256, dtype=np.int32)
            bins = np.minimum((x * levels) // 256, levels - 1)
            lut = np.clip(
                np.floor(bins.astype(np.float32) * 255.0 / (levels - 1) + 0.5),
                0,
                255,
            ).astype(np.uint8)
            rgb_u8 = lut[rgb_u8]
        elif filter_type == 8 and len(payload) >= 4:  # Threshold
            threshold = struct.unpack_from(">i", payload, 0)[0]
            lum = (
                0.299 * rgb_u8[..., 0].astype(np.float32)
                + 0.587 * rgb_u8[..., 1].astype(np.float32)
                + 0.114 * rgb_u8[..., 2].astype(np.float32)
            )
            bw = np.where(lum >= threshold, 255, 0).astype(np.uint8)
            rgb_u8 = np.repeat(bw[..., None], 3, axis=-1)
        elif filter_type == 9 and len(payload) >= 28:  # Gradient Map
            adjusted = _gradient_map(rgb_u8, payload)
            if adjusted is not None:
                rgb_u8 = adjusted
            else:
                log.warning("Layer %d (%r): malformed gradient map payload; skipping.",
                            layer["MainId"], layer["LayerName"])
                return False
        else:
            log.warning("Layer %d (%r): unsupported filter type %d; skipping.",
                        layer["MainId"], layer["LayerName"], filter_type)
            return False

        out[..., :3] = (rgb_u8.astype(np.float32) / 255.0) * alpha
        out[...] = np.clip(np.floor(out * 255.0 + 0.5), 0, 255) / 255.0
        mask = self._layer_mask_for_composite(layer)
        opacity = min(layer["LayerOpacity"] / 256.0, 1.0)
        if mask is not None:
            strength = (mask.astype(np.float32) / 255.0)[..., None] * opacity
            out[...] = before * (1.0 - strength) + out * strength
            out[...] = np.clip(np.floor(out * 255.0 + 0.5), 0, 255) / 255.0
        elif opacity < 1.0:
            out[...] = before * (1.0 - opacity) + out * opacity
            out[...] = np.clip(np.floor(out * 255.0 + 0.5), 0, 255) / 255.0
        return True

    def _render_through_group(
        self,
        layer: sqlite3.Row,
        out: np.ndarray,
        preserve_clipped_alpha: bool,
    ) -> None:
        opacity = min(layer["LayerOpacity"] / 256.0, 1.0)
        if opacity >= 1.0 and not layer["LayerLayerMaskMipmap"]:
            self._render_chain(layer["LayerFirstChildIndex"], out, preserve_clipped_alpha)
            return
        before = out.copy()
        self._render_chain(layer["LayerFirstChildIndex"], out, preserve_clipped_alpha)
        mask = self._layer_mask_for_composite(layer)
        if mask is not None:
            strength = (mask.astype(np.float32) / 255.0)[..., None] * opacity
        else:
            strength = opacity
        if isinstance(strength, np.ndarray):
            out[...] = before * (1.0 - strength) + out * strength
        elif strength < 1.0:
            out[...] = before * (1.0 - strength) + out * strength

    def _render_clipping_group(
        self,
        out: np.ndarray,
        group_layers: list[int],
    ) -> None:
        """Composite a base layer + its clipped siblings as an isolated group.

        CSP treats ``[base, clipped, clipped, ...]`` as a unit: the base enters
        the group via Normal (its source content), clipped siblings composite on
        top within the group (always using edge-preserving behaviour since the
        clip base *is* the destination within the group), and the group result
        blends back to *out* through the base layer's original blend mode.
        """
        base_layer = self._layer_row(group_layers[0])
        base_rgba = self.decode_layer(base_layer["MainId"])
        if base_rgba is None:
            return
        base_alpha_u8 = base_rgba[..., 3].copy()

        # --- render the group into a fresh buffer --- #
        group_out = np.zeros_like(out)
        self._composite_image(group_out, base_layer, base_rgba, base_alpha_u8)
        clip_base = base_alpha_u8

        for lid in group_layers[1:]:
            sibling = self._layer_row(lid)
            srgb = self.decode_layer(sibling["MainId"])
            if srgb is None:
                continue
            smask = self._layer_mask_for_composite(sibling)
            s_alpha = self._apply_mask_and_clip(sibling, srgb, smask, clip_base)
            # Within a clipping group every clipped layer uses the preserve
            # path — the clip base is the visible destination.
            self._composite_clipped_image(
                group_out, sibling, srgb, s_alpha, clip_base,
                always_preserve=True,
            )

        # --- blend the group result back through the base mode --- #
        bbox = _alpha_bbox(group_out[..., 3])
        if bbox is None:
            return
        y0, _y1, x0, _x1 = bbox
        rgba, alpha_u8 = self._premul_region_to_rgba_u8(group_out, bbox)
        self._composite_image(
            out,
            base_layer,
            rgba,
            alpha_u8,
            apply_opacity=False,
            dst_offset=(x0, y0),
        )

    def _render_chain(
        self,
        first_id: int,
        out: np.ndarray,
        preserve_clipped_alpha: bool = True,
        _skip_ids: Optional[set] = None,
    ) -> Optional[np.ndarray]:
        if _skip_ids is None:
            _skip_ids = set()
        clip_base_alpha_u8 = None
        chain_ids = self._walk_chain(first_id)
        i = 0
        while i < len(chain_ids):
            lid = chain_ids[i]
            i += 1
            if lid in _skip_ids:
                continue
            layer = self._layer_row(lid)
            if not _layer_is_visible(layer):
                continue
            if layer["LayerType"] == LAYER_TYPE_PAPER:
                continue
            if layer["LayerType"] == LAYER_TYPE_FILTER:
                self._apply_filter_layer(out, layer)
                clip_base_alpha_u8 = None
                continue
            if layer["LayerType"] == LAYER_TYPE_LAYER_FOLDER:
                frame_rgba = None
                if layer["LayerFolder"] and self._is_frame_vector_layer(layer):
                    frame_rgba = self._frame_folder_fallback_image(layer)
                if frame_rgba is not None:
                    group_out = np.zeros_like(out)
                    self._composite_image(
                        group_out, layer, frame_rgba, frame_rgba[..., 3],
                        apply_opacity=False,
                    )
                    self._render_chain(layer["LayerFirstChildIndex"], group_out, True)
                    bbox = _alpha_bbox(group_out[..., 3])
                    if bbox is not None:
                        y0, _y1, x0, _x1 = bbox
                        rgba, _ = self._premul_region_to_rgba_u8(group_out, bbox)
                        mask = self._layer_mask_for_composite(layer)
                        layer_alpha_u8 = self._apply_mask_and_clip_region(
                            layer, rgba, mask, clip_base_alpha_u8, bbox
                        )
                        if self._composite_image(out, layer, rgba, layer_alpha_u8, dst_offset=(x0, y0)):
                            if not layer["LayerClip"]:
                                if self._chain_next_uses_clip_base(chain_ids, i, _skip_ids):
                                    full_alpha = np.zeros((self.height, self.width), dtype=np.uint8)
                                    full_alpha[y0:bbox[1], x0:bbox[3]] = layer_alpha_u8
                                    clip_base_alpha_u8 = full_alpha
                                else:
                                    clip_base_alpha_u8 = None
                    continue
                if not layer["LayerFirstChildIndex"]:
                    rgba = None
                    text_attrs = layer["TextLayerAttributes"] if "TextLayerAttributes" in layer.keys() else None
                    if self._is_balloon_vector_layer(layer) and not isinstance(text_attrs, bytes):
                        rgba = self._balloon_fallback_image(layer)
                    if rgba is None:
                        rgba = self.decode_layer(layer["MainId"])
                    if (
                        rgba is not None
                        and not rgba[..., 3].any()
                        and self._vector_object_body(layer["MainId"]) is not None
                    ):
                        rgba = None
                    if rgba is None:
                        rgba = self._text_cache_fallback_image(layer)
                    if rgba is None:
                        rgba = self._balloon_fallback_image(layer)
                    if rgba is None:
                        rgba = self._vector_stroke_fallback_image(layer)
                    if rgba is not None:
                        mask = self._layer_mask_for_composite(layer)
                        layer_alpha_u8 = self._apply_mask_and_clip(layer, rgba, mask, clip_base_alpha_u8)
                        if self._composite_image(out, layer, rgba, layer_alpha_u8):
                            clip_base_alpha_u8 = layer_alpha_u8
                        continue
                mode = self._blend_mode_for_layer(layer)
                if mode == "THROUGH":
                    self._render_through_group(layer, out, preserve_clipped_alpha)
                    clip_base_alpha_u8 = None
                else:
                    group_out = np.zeros_like(out)
                    self._render_chain(layer["LayerFirstChildIndex"], group_out, True)
                    bbox = _alpha_bbox(group_out[..., 3])
                    if bbox is not None:
                        y0, _y1, x0, _x1 = bbox
                        rgba, _ = self._premul_region_to_rgba_u8(group_out, bbox)
                        mask = self._layer_mask_for_composite(layer)
                        layer_alpha_u8 = self._apply_mask_and_clip_region(
                            layer, rgba, mask, clip_base_alpha_u8, bbox
                        )
                        if self._composite_image(out, layer, rgba, layer_alpha_u8, dst_offset=(x0, y0)):
                            if not layer["LayerClip"]:
                                if self._chain_next_uses_clip_base(chain_ids, i, _skip_ids):
                                    full_alpha = np.zeros((self.height, self.width), dtype=np.uint8)
                                    full_alpha[y0:bbox[1], x0:bbox[3]] = layer_alpha_u8
                                    clip_base_alpha_u8 = full_alpha
                                else:
                                    clip_base_alpha_u8 = None
                continue
            if layer["LayerType"] == LAYER_TYPE_GROUP:
                mode = self._blend_mode_for_layer(layer)
                if mode == "THROUGH":
                    self._render_through_group(layer, out, preserve_clipped_alpha)
                    clip_base_alpha_u8 = None
                    continue
                group_out = np.zeros_like(out)
                self._render_chain(layer["LayerFirstChildIndex"], group_out, True)
                bbox = _alpha_bbox(group_out[..., 3])
                if bbox is None:
                    continue
                y0, _y1, x0, _x1 = bbox
                rgba, _ = self._premul_region_to_rgba_u8(group_out, bbox)
                mask = self._layer_mask_for_composite(layer)
                layer_alpha_u8 = self._apply_mask_and_clip_region(
                    layer, rgba, mask, clip_base_alpha_u8, bbox
                )
                if self._composite_image(out, layer, rgba, layer_alpha_u8, dst_offset=(x0, y0)):
                    if not layer["LayerClip"]:
                        if self._chain_next_uses_clip_base(chain_ids, i, _skip_ids):
                            full_alpha = np.zeros((self.height, self.width), dtype=np.uint8)
                            full_alpha[y0:bbox[1], x0:bbox[3]] = layer_alpha_u8
                            clip_base_alpha_u8 = full_alpha
                        else:
                            clip_base_alpha_u8 = None
                continue
            if layer["LayerType"] not in RASTER_LAYER_TYPES:
                log.warning("Skipping layer %d (%r): unsupported type %d",
                            lid, layer["LayerName"], layer["LayerType"])
                continue
            if layer["LayerOffsetX"] or layer["LayerOffsetY"]:
                log.warning("Layer %d (%r) has offset (%d,%d); not yet handled.",
                            lid, layer["LayerName"],
                            layer["LayerOffsetX"], layer["LayerOffsetY"])

            rgba = self.decode_layer(layer["MainId"])
            if (
                rgba is not None
                and not rgba[..., 3].any()
                and self._vector_object_body(layer["MainId"]) is not None
            ):
                rgba = None
            if rgba is None:
                rgba = self._text_cache_fallback_image(layer)
            if rgba is None:
                rgba = self._balloon_fallback_image(layer)
            if rgba is None:
                rgba = self._vector_stroke_fallback_image(layer)
            if rgba is None:
                log.warning("Layer %d (%r): no raster data; skipping.",
                            layer["MainId"], layer["LayerName"])
                continue

            mask = self._layer_mask_for_composite(layer)
            layer_alpha_u8 = self._apply_mask_and_clip(layer, rgba, mask, clip_base_alpha_u8)

            if not layer["LayerClip"] and self._blend_mode_for_layer(layer) != "NORMAL":
                # A non-Normal, non-clipped layer may be the base of a clipping
                # group. CSP renders the base + clipped siblings as an isolated
                # group, then blends the group result through the base's mode.
                if i < len(chain_ids):
                    next_row = self._layer_row(chain_ids[i])
                    if next_row and next_row["LayerClip"]:
                        group_layers = [lid]
                        for j in range(i, len(chain_ids)):
                            sibling_row = self._layer_row(chain_ids[j])
                            if sibling_row and sibling_row["LayerClip"]:
                                group_layers.append(chain_ids[j])
                                _skip_ids.add(chain_ids[j])
                                i = j + 1
                            else:
                                break
                        self._render_clipping_group(out, group_layers)
                        clip_base_alpha_u8 = None
                        continue

            if (
                layer["LayerClip"]
                and preserve_clipped_alpha
                and clip_base_alpha_u8 is not None
            ):
                did_composite = self._composite_clipped_image(
                    out, layer, rgba, layer_alpha_u8, clip_base_alpha_u8
                )
            else:
                did_composite = self._composite_image(out, layer, rgba, layer_alpha_u8)
            if did_composite:
                if not layer["LayerClip"]:
                    clip_base_alpha_u8 = layer_alpha_u8

        return clip_base_alpha_u8

    def _composite_recursive(self) -> np.ndarray:
        out = np.zeros((self.height, self.width, 4), dtype=np.float32)
        paper_color = self._paper_color()
        if paper_color is not None:
            out[..., 0] = paper_color[0]
            out[..., 1] = paper_color[1]
            out[..., 2] = paper_color[2]
            out[..., 3] = 1.0

        root = self._layer_row(self.root_layer_id)
        if root is None or root["LayerType"] != LAYER_TYPE_FOLDER:
            raise ValueError("Root layer not found or not a folder.")
        self._render_chain(root["LayerFirstChildIndex"], out)
        rgba_u8, _ = self._premul_to_rgba_u8(out, transparent_rgb=255)
        return rgba_u8

    def composite(self) -> np.ndarray:
        return self._composite_recursive()



# --------------------------------------------------------------------------- #
# PNG writer (stdlib only)
# --------------------------------------------------------------------------- #

def save_png(path: str, rgba: np.ndarray, compress_level: int = 1) -> None:
    """Write a (H, W, 4) uint8 RGBA NumPy array to a PNG file.

    `compress_level` defaults to 1 (fastest) 鈥?this PNG is a sidecar cache,
    not a deliverable, so we trade ~10鈥?0% size for ~3鈥?x faster encode.
    """
    if rgba.dtype != np.uint8 or rgba.ndim != 3 or rgba.shape[2] != 4:
        raise ValueError(
            f"Expected (H, W, 4) uint8 RGBA, got shape={rgba.shape} dtype={rgba.dtype}"
        )
    h, w = rgba.shape[:2]

    filter_col = np.zeros((h, 1), dtype=np.uint8)
    filtered = np.concatenate([filter_col, rgba.reshape(h, w * 4)], axis=1).tobytes()

    def _chunk(tag, data):
        return (
            struct.pack(">I", len(data))
            + tag
            + data
            + struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF)
        )

    sig = b"\x89PNG\r\n\x1a\n"
    ihdr = struct.pack(">IIBBBBB", w, h, 8, 6, 0, 0, 0)
    idat = zlib.compress(filtered, compress_level)
    with open(path, "wb") as f:
        f.write(sig)
        f.write(_chunk(b"IHDR", ihdr))
        f.write(_chunk(b"IDAT", idat))
        f.write(_chunk(b"IEND", b""))


def main():
    import argparse
    ap = argparse.ArgumentParser()
    ap.add_argument("clip_path")
    ap.add_argument("-o", "--out", default=None,
                    help="Output PNG path (default: <clip_path>.flat.png)")
    ap.add_argument("--png-level", type=int, default=1,
                    help="PNG compression level 0-9 (default 1, fast).")
    ap.add_argument("-v", "--verbose", action="store_true")
    args = ap.parse_args()

    logging.basicConfig(level=logging.DEBUG if args.verbose else logging.INFO,
                        format="%(levelname)s %(name)s: %(message)s")

    clip = ClipFile(args.clip_path)
    print(f"Canvas: {clip.width}x{clip.height}, root folder id={clip.root_layer_id}")
    img = clip.composite()
    out_path = args.out or args.clip_path + ".flat.png"
    save_png(out_path, img, compress_level=args.png_level)
    print(f"Wrote {out_path}")
    clip.close()


if __name__ == "__main__":
    main()

