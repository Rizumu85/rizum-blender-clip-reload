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
import os
import sqlite3
import struct
import tempfile
import zlib
from dataclasses import dataclass
from typing import Iterable, Optional

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
    if mode == "COLOR_DODGE":
        s_u8 = np.clip(np.floor(s * 255.0 + 0.5), 0, 255).astype(np.int32)
        d_u8 = np.clip(np.floor(d * 255.0 + 0.5), 0, 255).astype(np.int32)
        out = np.where(s_u8 >= 255, 255,
                       np.minimum(255, (d_u8 * 255) // np.maximum(255 - s_u8, 1)))
        return out.astype(np.float32) / 255.0
    if mode == "GLOW_DODGE":
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
    if amount > 0:
        return _linear_lut(amount, 0, 255, 255 - amount)
    if amount < 0:
        return _linear_lut(0, -amount, 255, 255)
    return np.arange(256, dtype=np.uint8)


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
        return layer_alpha_u8

    def _new_transparent_canvas(self, like: Optional[np.ndarray] = None) -> np.ndarray:
        if like is not None and like.dtype != np.uint8:
            return np.zeros_like(like)
        # CSP caches are straight RGBA, so transparent pixels can still carry white RGB.
        canvas = np.empty((self.height, self.width, 4), dtype=np.uint8)
        canvas[..., :3] = 255
        canvas[..., 3] = 0
        return canvas

    def _blend_straight_toward(self, out: np.ndarray, before: np.ndarray, strength) -> None:
        if not isinstance(strength, np.ndarray) and strength >= 1.0:
            return
        if not isinstance(strength, np.ndarray) and strength <= 0.0:
            out[...] = before
            return
        s = strength if isinstance(strength, np.ndarray) else np.float32(strength)
        before_a = (before[..., 3:4].astype(np.float32) / 255.0)
        after_a = (out[..., 3:4].astype(np.float32) / 255.0)
        before_pm = (before[..., :3].astype(np.float32) / 255.0) * before_a
        after_pm = (out[..., :3].astype(np.float32) / 255.0) * after_a
        out_a = before_a * (1.0 - s) + after_a * s
        out_pm = before_pm * (1.0 - s) + after_pm * s
        with np.errstate(invalid="ignore", divide="ignore"):
            out_rgb = np.where(out_a > 1e-6, out_pm / np.maximum(out_a, 1e-6), 1.0)
        out[..., :3] = np.clip(np.floor(out_rgb * 255.0 + 0.5), 0, 255).astype(np.uint8)
        out[..., 3] = np.clip(np.floor(out_a[..., 0] * 255.0 + 0.5), 0, 255).astype(np.uint8)

    def _composite_image_straight(
        self,
        out: np.ndarray,
        layer: sqlite3.Row,
        rgba: np.ndarray,
        layer_alpha_u8: np.ndarray,
        apply_opacity: bool = True,
        dst_offset: tuple[int, int] = (0, 0),
    ) -> bool:
        mode = self._blend_mode_for_layer(layer)
        bbox = _alpha_bbox(layer_alpha_u8)
        if bbox is None:
            return False
        y0, y1, x0, x1 = bbox
        dst_y0 = y0 + dst_offset[1]
        dst_y1 = y1 + dst_offset[1]
        dst_x0 = x0 + dst_offset[0]
        dst_x1 = x1 + dst_offset[0]

        if apply_opacity:
            opacity_u8 = min(max(int(layer["LayerOpacity"] or 0), 0), 256)
            src_a_u8 = ((layer_alpha_u8[y0:y1, x0:x1].astype(np.uint16) * opacity_u8) // 256).astype(np.uint8)
        else:
            src_a_u8 = layer_alpha_u8[y0:y1, x0:x1]
        if not np.any(src_a_u8):
            return False

        src_rgb_u8 = rgba[y0:y1, x0:x1, :3]
        dst = out[dst_y0:dst_y1, dst_x0:dst_x1]
        dst_a_u8 = dst[..., 3]

        if mode == "ADD" or mode == "ADD_GLOW":
            dst_rgba = dst.copy()
            src_rgba = np.empty_like(dst_rgba)
            src_rgba[..., :3] = src_rgb_u8
            src_rgba[..., 3] = src_a_u8
            blended_u8 = _blend_add_glow_u8(dst_rgba, src_rgba) if mode == "ADD_GLOW" else _blend_add_u8(dst_rgba, src_rgba)
            dst[...] = blended_u8
            return True

        sa = (src_a_u8.astype(np.float32) / 255.0)[..., None]
        da = (dst_a_u8.astype(np.float32) / 255.0)[..., None]
        src_rgb = src_rgb_u8.astype(np.float32) / 255.0
        dst_rgb = dst[..., :3].astype(np.float32) / 255.0
        out_a = sa + da * (1.0 - sa)

        if mode == "NORMAL":
            out_pm = src_rgb * sa + dst_rgb * da * (1.0 - sa)
        elif mode == "GLOW_DODGE":
            strength_u8 = np.clip(np.floor(src_rgb * sa * 255.0 + 0.5), 0, 255).astype(np.int32)
            dst_u8 = dst[..., :3].astype(np.int32)
            dodge_u8 = np.where(
                strength_u8 >= 255,
                255,
                np.minimum(255, (dst_u8 * 255) // np.maximum(255 - strength_u8, 1)),
            )
            dodge_rgb = dodge_u8.astype(np.float32) / 255.0
            dodge_pm = dodge_rgb * out_a
            src_pm = src_rgb * sa
            dst_blend = np.minimum(da / np.maximum(out_a, 1e-6), 1.0)
            out_pm = dodge_pm * dst_blend + src_pm * (1.0 - dst_blend)
        else:
            blended = np.clip(_blend_func(mode, src_rgb, dst_rgb), 0.0, 1.0)
            out_pm = (
                src_rgb * sa * (1.0 - da)
                + dst_rgb * da * (1.0 - sa)
                + blended * sa * da
            )

        with np.errstate(invalid="ignore", divide="ignore"):
            out_rgb = np.where(out_a > 1e-6, out_pm / np.maximum(out_a, 1e-6), 1.0)
        dst[..., :3] = np.clip(np.floor(out_rgb * 255.0 + 0.5), 0, 255).astype(np.uint8)
        dst[..., 3] = np.clip(np.floor(out_a[..., 0] * 255.0 + 0.5), 0, 255).astype(np.uint8)
        return True

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
        if out.dtype == np.uint8:
            return self._composite_image_straight(
                out,
                layer,
                rgba,
                layer_alpha_u8,
                apply_opacity=apply_opacity,
                dst_offset=dst_offset,
            )

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
        source_alpha_u8: Optional[np.ndarray] = None,
        always_preserve: bool = False,
    ) -> bool:
        """Composite a clipped layer inside an isolated folder/group buffer.

        CSP clipping preserves the clipping base's edge alpha when that base is
        the visible destination. If there is already more opaque artwork below
        the base at a pixel, the regular product-alpha path remains closer.
        """
        if source_alpha_u8 is None:
            source_mask = self._layer_mask_for_composite(layer)
            source_alpha_u8 = self._apply_mask_and_clip(layer, rgba, source_mask, None)

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
        clip_a = clip_base_alpha_u8[y0:y1, x0:x1].astype(np.float32) / 255.0
        visible = ((source_alpha_u8[y0:y1, x0:x1] > 0) & (clip_a > 0))[..., None]
        masked_strength = (
            source_alpha_u8[y0:y1, x0:x1].astype(np.float32) / 255.0
        )[..., None] * opacity
        masked_strength = np.where(visible, masked_strength, 0.0)

        if out.dtype == np.uint8:
            dst_a = before[..., 3:4].astype(np.float32) / 255.0
            dst_rgb = before[..., :3].astype(np.float32) / 255.0
        else:
            dst_a = before[..., 3:4]
            with np.errstate(invalid="ignore", divide="ignore"):
                dst_rgb = np.where(dst_a > 1e-6, before[..., :3] / np.maximum(dst_a, 1e-6), 0.0)
        if mode == "NORMAL":
            preserve_rgb = src_rgb * masked_strength + dst_rgb * (1.0 - masked_strength)
        elif mode == "ADD" or mode == "ADD_GLOW":
            # The base-owned clipping cache already carries the clip alpha.
            # Remove clip from the source strength while retaining mask/opacity.
            preserve_rgb = np.minimum(dst_rgb + src_rgb * masked_strength, 1.0)
        else:
            blended = np.clip(_blend_func(mode, src_rgb, dst_rgb), 0.0, 1.0)
            preserve_rgb = blended * masked_strength + dst_rgb * (1.0 - masked_strength)

        preserve = before.copy()
        if out.dtype == np.uint8:
            preserve[..., :3] = np.clip(np.floor(preserve_rgb * 255.0 + 0.5), 0, 255).astype(np.uint8)
        else:
            preserve[..., :3] = preserve_rgb * dst_a

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
        if out.dtype == np.uint8:
            rgb_u8 = out[..., :3].copy()
            alpha = None
        else:
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

        if out.dtype == np.uint8:
            out[..., :3] = rgb_u8
        else:
            out[..., :3] = (rgb_u8.astype(np.float32) / 255.0) * alpha
            out[...] = np.clip(np.floor(out * 255.0 + 0.5), 0, 255) / 255.0
        mask = self._layer_mask_for_composite(layer)
        opacity = min(layer["LayerOpacity"] / 256.0, 1.0)
        if mask is not None:
            strength = (mask.astype(np.float32) / 255.0)[..., None] * opacity
            if out.dtype == np.uint8:
                self._blend_straight_toward(out, before, strength)
                return True
            out[...] = before * (1.0 - strength) + out * strength
            out[...] = np.clip(np.floor(out * 255.0 + 0.5), 0, 255) / 255.0
        elif opacity < 1.0:
            if out.dtype == np.uint8:
                self._blend_straight_toward(out, before, opacity)
                return True
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
        if out.dtype == np.uint8:
            self._blend_straight_toward(out, before, strength)
            return
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
        base_mask = self._layer_mask_for_composite(base_layer)
        base_alpha_u8 = self._apply_mask_and_clip(base_layer, base_rgba, base_mask, None)

        # --- render the group into a fresh buffer --- #
        group_out = self._new_transparent_canvas(out)
        self._composite_image(group_out, base_layer, base_rgba, base_alpha_u8)
        clip_base = base_alpha_u8

        for lid in group_layers[1:]:
            sibling = self._layer_row(lid)
            if sibling is None or not _layer_is_visible(sibling):
                continue
            srgb = self.decode_layer(sibling["MainId"])
            if srgb is None and sibling["LayerType"] in (
                LAYER_TYPE_LAYER_FOLDER,
                LAYER_TYPE_GROUP,
                LAYER_TYPE_FOLDER,
            ):
                sibling_out = self._new_transparent_canvas(group_out)
                if self._blend_mode_for_layer(sibling) == "THROUGH":
                    self._render_through_group(sibling, sibling_out, True)
                else:
                    self._render_chain(sibling["LayerFirstChildIndex"], sibling_out, True)
                if _alpha_bbox(sibling_out[..., 3]) is None:
                    continue
                if sibling_out.dtype == np.uint8:
                    srgb = sibling_out
                else:
                    srgb, _ = self._premul_to_rgba_u8(sibling_out, transparent_rgb=255)
            if srgb is None:
                continue
            smask = self._layer_mask_for_composite(sibling)
            s_source_alpha = self._apply_mask_and_clip(sibling, srgb, smask, None)
            s_alpha = (
                (s_source_alpha.astype(np.uint16) * clip_base) // 255
            ).astype(np.uint8)
            # Within a clipping group every clipped layer uses the preserve
            # path - the clip base is the visible destination.
            self._composite_clipped_image(
                group_out, sibling, srgb, s_alpha, clip_base, s_source_alpha,
                always_preserve=True,
            )

        # --- blend the group result back through the base mode --- #
        bbox = _alpha_bbox(group_out[..., 3])
        if bbox is None:
            return
        y0, _y1, x0, _x1 = bbox
        if group_out.dtype == np.uint8:
            rgba = group_out[bbox[0]:bbox[1], bbox[2]:bbox[3]].copy()
            alpha_u8 = rgba[..., 3]
        else:
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
                if not layer["LayerFirstChildIndex"]:
                    rgba = self.decode_layer(layer["MainId"])
                    if rgba is not None:
                        mask = self._layer_mask_for_composite(layer)
                        layer_alpha_u8 = self._apply_mask_and_clip(layer, rgba, mask, clip_base_alpha_u8)
                        if layer["LayerClip"] and clip_base_alpha_u8 is not None:
                            layer_alpha_u8 = (
                                (layer_alpha_u8.astype(np.uint16) * clip_base_alpha_u8) // 255
                            ).astype(np.uint8)
                        if (
                            self._composite_image(out, layer, rgba, layer_alpha_u8)
                            and not layer["LayerClip"]
                        ):
                            clip_base_alpha_u8 = layer_alpha_u8
                        continue
                mode = self._blend_mode_for_layer(layer)
                if mode == "THROUGH":
                    self._render_through_group(layer, out, preserve_clipped_alpha)
                    clip_base_alpha_u8 = None
                else:
                    group_out = self._new_transparent_canvas(out)
                    self._render_chain(layer["LayerFirstChildIndex"], group_out, True)
                    bbox = _alpha_bbox(group_out[..., 3])
                    if bbox is not None:
                        y0, _y1, x0, _x1 = bbox
                        if group_out.dtype == np.uint8:
                            rgba = group_out[y0:bbox[1], x0:bbox[3]].copy()
                        else:
                            rgba, _ = self._premul_region_to_rgba_u8(group_out, bbox)
                        mask = self._layer_mask_for_composite(layer)
                        layer_alpha_u8 = self._apply_mask_and_clip_region(
                            layer, rgba, mask, clip_base_alpha_u8, bbox
                        )
                        if layer["LayerClip"] and clip_base_alpha_u8 is not None:
                            local_clip = clip_base_alpha_u8[y0:bbox[1], x0:bbox[3]]
                            if local_clip.shape == layer_alpha_u8.shape:
                                layer_alpha_u8 = (
                                    (layer_alpha_u8.astype(np.uint16) * local_clip) // 255
                                ).astype(np.uint8)
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
                group_out = self._new_transparent_canvas(out)
                self._render_chain(layer["LayerFirstChildIndex"], group_out, True)
                bbox = _alpha_bbox(group_out[..., 3])
                if bbox is None:
                    continue
                y0, _y1, x0, _x1 = bbox
                if group_out.dtype == np.uint8:
                    rgba = group_out[y0:bbox[1], x0:bbox[3]].copy()
                else:
                    rgba, _ = self._premul_region_to_rgba_u8(group_out, bbox)
                mask = self._layer_mask_for_composite(layer)
                layer_alpha_u8 = self._apply_mask_and_clip_region(
                    layer, rgba, mask, clip_base_alpha_u8, bbox
                )
                if layer["LayerClip"] and clip_base_alpha_u8 is not None:
                    local_clip = clip_base_alpha_u8[y0:bbox[1], x0:bbox[3]]
                    if local_clip.shape == layer_alpha_u8.shape:
                        layer_alpha_u8 = (
                            (layer_alpha_u8.astype(np.uint16) * local_clip) // 255
                        ).astype(np.uint8)
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
            if rgba is None:
                log.warning("Layer %d (%r): no raster data; skipping.",
                            layer["MainId"], layer["LayerName"])
                continue

            mask = self._layer_mask_for_composite(layer)
            layer_alpha_u8 = self._apply_mask_and_clip(layer, rgba, mask, clip_base_alpha_u8)
            source_alpha_u8 = None
            if layer["LayerClip"] and clip_base_alpha_u8 is not None:
                source_alpha_u8 = layer_alpha_u8
                layer_alpha_u8 = (
                    (layer_alpha_u8.astype(np.uint16) * clip_base_alpha_u8) // 255
                ).astype(np.uint8)

            if not layer["LayerClip"]:
                # CSP renders any base layer plus its clipped siblings as an
                # isolated cache, then blends the cache through the base mode.
                if i < len(chain_ids):
                    next_row = self._layer_row(chain_ids[i])
                    if next_row and next_row["LayerClip"]:
                        group_layers = [lid]
                        has_visible_clipped_sibling = False
                        for j in range(i, len(chain_ids)):
                            sibling_row = self._layer_row(chain_ids[j])
                            if sibling_row and sibling_row["LayerClip"]:
                                _skip_ids.add(chain_ids[j])
                                i = j + 1
                                if _layer_is_visible(sibling_row):
                                    group_layers.append(chain_ids[j])
                                    has_visible_clipped_sibling = True
                            else:
                                break
                        if has_visible_clipped_sibling:
                            self._render_clipping_group(out, group_layers)
                            clip_base_alpha_u8 = None
                            continue

            if (
                layer["LayerClip"]
                and preserve_clipped_alpha
                and clip_base_alpha_u8 is not None
            ):
                if source_alpha_u8 is None:
                    source_alpha_u8 = self._apply_mask_and_clip(layer, rgba, mask, None)
                did_composite = self._composite_clipped_image(
                    out, layer, rgba, layer_alpha_u8, clip_base_alpha_u8,
                    source_alpha_u8,
                )
            else:
                did_composite = self._composite_image(out, layer, rgba, layer_alpha_u8)
            if did_composite:
                if not layer["LayerClip"]:
                    clip_base_alpha_u8 = layer_alpha_u8

        return clip_base_alpha_u8

    def _composite_recursive(self) -> np.ndarray:
        out = self._new_transparent_canvas()
        paper_color = self._paper_color()
        if paper_color is not None:
            out[..., 0] = int(np.clip(paper_color[0] * 255.0 + 0.5, 0, 255))
            out[..., 1] = int(np.clip(paper_color[1] * 255.0 + 0.5, 0, 255))
            out[..., 2] = int(np.clip(paper_color[2] * 255.0 + 0.5, 0, 255))
            out[..., 3] = 255

        root = self._layer_row(self.root_layer_id)
        if root is None or root["LayerType"] != LAYER_TYPE_FOLDER:
            raise ValueError("Root layer not found or not a folder.")
        self._render_chain(root["LayerFirstChildIndex"], out)
        return out

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

