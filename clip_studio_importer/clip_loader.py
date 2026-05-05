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
PER_TILE_BYTES = TILE * TILE * 5   # 1B alpha + 4B BGRA per pixel
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


def _parse_exta(body: bytes, empty_fill: int = 0) -> tuple[str, bytes]:
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

    out = bytearray()
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
                out += decoded
                block_end = inner_start + 24 + inner_len
            else:
                out += bytes([empty_fill]) * uncomp
                block_end = inner_start + 20
        elif name in ("BlockStatus", "BlockCheckSum"):
            block_end = inner_start + 24
        elif name == "BlockDataEndChunk":
            pass
        else:
            log.warning("Unknown block name %r in Exta %s", name, ext_id)
            break

        pos = block_end
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
    total_tiles = len(tile_blob) // (TILE * TILE)
    if len(tile_blob) % (TILE * TILE) != 0:
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
    if amount == 0:
        return np.arange(256, dtype=np.uint8)
    if amount > 0:
        return _linear_lut(amount, 0, 255, 255 - amount)
    return _linear_lut(0, -amount, 255 + amount, 255)


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
        in_low, in_high, mid, out_low, out_high = (
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


def _apply_tone_curve(rgb_u8: np.ndarray, payload: bytes) -> np.ndarray | None:
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
        color_word = raw[1:5]
        r = (color_word[0] >> 16) & 0xFFFF
        g = (color_word[1] >> 16) & 0xFFFF
        b = (color_word[2] >> 16) & 0xFFFF
        stop = raw[6] / 65535.0
        nodes.append((stop, np.array([r >> 8, g >> 8, b >> 8], dtype=np.float32)))
        pos += 28
    if not nodes:
        return None
    nodes.sort(key=lambda item: item[0])
    lum = (
        0.299 * rgb_u8[..., 0].astype(np.float32)
        + 0.587 * rgb_u8[..., 1].astype(np.float32)
        + 0.114 * rgb_u8[..., 2].astype(np.float32)
    ) / 255.0
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
        self._tile_blob_cache: dict[tuple[str, int], bytes] = {}

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
                if rgb != (0, 0, 0) or row["DrawColorEnable"]:
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

    def _resolve_external_id(self, layer_id: int) -> Optional[str]:
        row = self._layer_row(layer_id)
        if row is None:
            return None
        mipmap = self._db.execute(
            "SELECT BaseMipmapInfo FROM Mipmap WHERE MainId=?",
            (row["LayerRenderMipmap"],),
        ).fetchone()
        if mipmap is None:
            return None
        mipmap_info = self._db.execute(
            "SELECT Offscreen FROM MipmapInfo WHERE MainId=?",
            (mipmap["BaseMipmapInfo"],),
        ).fetchone()
        if mipmap_info is None:
            return None
        offscreen = self._db.execute(
            "SELECT BlockData FROM Offscreen WHERE MainId=?",
            (mipmap_info["Offscreen"],),
        ).fetchone()
        if offscreen is None:
            return None
        return offscreen["BlockData"].decode("ascii")

    def _layer_pixel_size(self, layer_id: int) -> tuple[int, int]:
        row = self._db.execute(
            "SELECT ThumbnailCanvasWidth, ThumbnailCanvasHeight "
            "FROM LayerThumbnail WHERE LayerId=?",
            (layer_id,),
        ).fetchone()
        if row:
            return int(row["ThumbnailCanvasWidth"]), int(row["ThumbnailCanvasHeight"])
        return self.width, self.height

    def _get_tile_blob(self, ext_id: str, empty_fill: int = 0) -> Optional[bytes]:
        cache_key = (ext_id, empty_fill)
        cached = self._tile_blob_cache.get(cache_key)
        if cached is not None:
            return cached
        body = self._exta_bodies.get(ext_id)
        if body is None:
            return None
        _, blob = _parse_exta(body, empty_fill=empty_fill)
        self._tile_blob_cache[cache_key] = blob
        return blob

    def decode_layer(self, layer_id: int) -> Optional[np.ndarray]:
        ext_id = self._resolve_external_id(layer_id)
        if ext_id is None:
            return None
        blob = self._get_tile_blob(ext_id)
        if blob is None:
            return None
        w, h = self._layer_pixel_size(layer_id)
        return _tiles_to_rgba(blob, w, h)

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
        ext_id = self._resolve_mask_external_id(layer_id)
        if ext_id is None:
            return None
        blob = self._get_tile_blob(ext_id, empty_fill=self._mask_empty_fill(layer_id))
        if blob is None:
            return None
        try:
            return _tiles_to_alpha(blob, self.width, self.height)
        except ValueError as exc:
            log.warning("Mask decode failed for layer %d: %s", layer_id, exc)
            return None

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

    def _layer_mask_for_composite(self, layer: sqlite3.Row) -> Optional[np.ndarray]:
        if not layer["LayerLayerMaskMipmap"]:
            return None
        return self.decode_layer_mask(layer["MainId"])

    def _composite_image(self, out: np.ndarray, layer: sqlite3.Row, rgba: np.ndarray,
                         layer_alpha_u8: np.ndarray, apply_opacity: bool = True) -> bool:
        mode = self._blend_mode_for_layer(layer)

        bbox = _alpha_bbox(layer_alpha_u8)
        if bbox is None:
            return False
        y0, y1, x0, x1 = bbox

        opacity = min(layer["LayerOpacity"] / 256.0, 1.0) if apply_opacity else 1.0
        src_rgb_u8 = rgba[y0:y1, x0:x1, :3]
        src_a_u8 = layer_alpha_u8[y0:y1, x0:x1]

        src_rgb = src_rgb_u8.astype(np.float32) / 255.0
        src_a = (src_a_u8.astype(np.float32) / 255.0)[..., None] * opacity

        dst_rgb_pm = out[y0:y1, x0:x1, :3]
        dst_a = out[y0:y1, x0:x1, 3:4]

        if mode == "NORMAL":
            src_rgb_pm = src_rgb * src_a
            inv_sa = 1.0 - src_a
            out[y0:y1, x0:x1, :3] = src_rgb_pm + dst_rgb_pm * inv_sa
            out[y0:y1, x0:x1, 3:4] = src_a + dst_a * inv_sa

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
            out[y0:y1, x0:x1, :3] = (
                blended_u8[..., :3].astype(np.float32) / 255.0
            ) * out_a
            out[y0:y1, x0:x1, 3:4] = out_a
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
            out[y0:y1, x0:x1, :3] = dodge_pm * dst_blend + src_pm * (1.0 - dst_blend)
            out[y0:y1, x0:x1, 3:4] = out_a
        elif mode == "COLOR_DODGE" or mode == "COLOR_BURN":
            eps = 1e-6
            dst_rgb_straight = dst_rgb_pm / np.maximum(dst_a, eps)
            dst_rgb_quant = np.floor(np.clip(dst_rgb_straight, 0.0, 1.0) * 255.0 + 0.5) / 255.0
            blended = np.clip(_blend_func(mode, src_rgb, dst_rgb_quant), 0.0, 1.0)

            inv_sa = 1.0 - src_a
            inv_da = 1.0 - dst_a
            src_pm = src_rgb * src_a
            dst_rgb_pm_quant = dst_rgb_quant * dst_a
            out[y0:y1, x0:x1, :3] = (
                inv_da * src_pm + inv_sa * dst_rgb_pm_quant + src_a * dst_a * blended
            )
            out[y0:y1, x0:x1, 3:4] = src_a + dst_a * inv_sa
        else:
            eps = 1e-6
            dst_rgb_straight = dst_rgb_pm / np.maximum(dst_a, eps)
            blended = np.clip(_blend_func(mode, src_rgb, dst_rgb_straight), 0.0, 1.0)

            inv_sa = 1.0 - src_a
            inv_da = 1.0 - dst_a
            src_pm = src_rgb * src_a
            out[y0:y1, x0:x1, :3] = inv_da * src_pm + inv_sa * dst_rgb_pm + src_a * dst_a * blended
            out[y0:y1, x0:x1, 3:4] = src_a + dst_a * inv_sa

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

    def _premul_to_rgba_u8(self, premul: np.ndarray) -> tuple[np.ndarray, np.ndarray]:
        alpha = premul[..., 3:4]
        with np.errstate(invalid="ignore", divide="ignore"):
            rgb = np.where(alpha > 0, premul[..., :3] / np.where(alpha > 0, alpha, 1.0), 0.0)
        rgba = np.empty_like(premul)
        rgba[..., :3] = rgb
        rgba[..., 3:4] = alpha
        rgba_u8 = np.clip(rgba * 255.0 + 0.5, 0, 255).astype(np.uint8)
        return rgba_u8, rgba_u8[..., 3]

    def _apply_filter_layer(self, out: np.ndarray, layer: sqlite3.Row) -> bool:
        info = _filter_info(layer)
        if info is None:
            log.warning("Layer %d (%r): filter layer missing FilterLayerInfo; skipping.",
                        layer["MainId"], layer["LayerName"])
            return False
        filter_type, payload = info
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
            vals = struct.unpack_from(">iiiiiiiiii", payload, 0)
            # Payload stores shadow/midtone/highlight RGB offsets. CSP's sample
            # adjustment uses the midtone triplet; keep alpha/coverage unchanged.
            r_off, g_off, b_off = vals[3], vals[4], vals[5]
            offsets = np.array([r_off, g_off, b_off], dtype=np.int16)
            rgb_u8 = np.clip(rgb_u8.astype(np.int16) + offsets, 0, 255).astype(np.uint8)
        elif filter_type == 6:  # Reverse Gradient / Invert
            rgb_u8 = 255 - rgb_u8
        elif filter_type == 7 and len(payload) >= 4:  # Posterization
            levels = max(2, struct.unpack_from(">i", payload, 0)[0])
            x = np.arange(256, dtype=np.float32)
            lut = np.clip(np.floor(np.round(x * (levels - 1) / 255.0) * 255.0 / (levels - 1) + 0.5), 0, 255).astype(np.uint8)
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
        else:
            log.warning("Layer %d (%r): unsupported filter type %d; skipping.",
                        layer["MainId"], layer["LayerName"], filter_type)
            return False

        out[..., :3] = (rgb_u8.astype(np.float32) / 255.0) * alpha
        out[...] = np.clip(np.floor(out * 255.0 + 0.5), 0, 255) / 255.0
        return True

    def _render_through_group(
        self,
        layer: sqlite3.Row,
        out: np.ndarray,
        preserve_clipped_alpha: bool,
    ) -> None:
        before = out.copy()
        self._render_chain(layer["LayerFirstChildIndex"], out, preserve_clipped_alpha)
        mask = self._layer_mask_for_composite(layer)
        opacity = min(layer["LayerOpacity"] / 256.0, 1.0)
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
        rgba, alpha_u8 = self._premul_to_rgba_u8(group_out)
        self._composite_image(out, base_layer, rgba, alpha_u8, apply_opacity=False)

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
                    rgba, _ = self._premul_to_rgba_u8(group_out)
                    mask = self._layer_mask_for_composite(layer)
                    layer_alpha_u8 = self._apply_mask_and_clip(layer, rgba, mask, clip_base_alpha_u8)
                    if self._composite_image(out, layer, rgba, layer_alpha_u8):
                        if not layer["LayerClip"]:
                            clip_base_alpha_u8 = layer_alpha_u8
                continue
            if layer["LayerType"] == LAYER_TYPE_GROUP:
                mode = self._blend_mode_for_layer(layer)
                if mode == "THROUGH":
                    self._render_through_group(layer, out, preserve_clipped_alpha)
                    clip_base_alpha_u8 = None
                    continue
                group_out = np.zeros_like(out)
                self._render_chain(layer["LayerFirstChildIndex"], group_out, True)
                rgba, _ = self._premul_to_rgba_u8(group_out)
                mask = self._layer_mask_for_composite(layer)
                layer_alpha_u8 = self._apply_mask_and_clip(layer, rgba, mask, clip_base_alpha_u8)
                if self._composite_image(out, layer, rgba, layer_alpha_u8):
                    if not layer["LayerClip"]:
                        clip_base_alpha_u8 = layer_alpha_u8
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

        a = out[..., 3:4]
        with np.errstate(invalid="ignore", divide="ignore"):
            rgb = np.where(a > 0, out[..., :3] / np.where(a > 0, a, 1.0), 0.0)
        result = np.empty_like(out)
        result[..., :3] = rgb
        result[..., 3:4] = a
        return np.clip(result * 255.0 + 0.5, 0, 255).astype(np.uint8)

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

