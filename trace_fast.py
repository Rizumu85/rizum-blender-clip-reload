"""
Phase 1: Fast sweep — find all layers with alpha>0 at (2190,1319)
"""
from __future__ import annotations
from clip_loader import (
    ClipFile, _BLEND_MAPPING, _layer_is_visible, RASTER_LAYER_TYPES,
    TILE, PER_TILE_BYTES,
)
LAYER_TYPE_FOLDER = 256
LAYER_TYPE_GROUP = 2
LAYER_TYPE_LAYER_FOLDER = 0
LAYER_TYPE_PAPER = 1584

PX, PY = 2190, 1319
CSP_REF = [223, 164, 201, 255]

def fast_pixel_rgba(clip, layer_id):
    ext_id = clip._resolve_external_id(layer_id)
    if ext_id is None: return None
    w, h = clip._layer_pixel_size(layer_id)
    if PX >= w or PY >= h: return None
    blob = clip._get_tile_blob(ext_id)
    if blob is None: return None
    cols = (w + TILE - 1) // TILE
    expected = cols * ((h + TILE - 1) // TILE) * PER_TILE_BYTES
    if len(blob) != expected:
        total_tiles = len(blob) // PER_TILE_BYTES
        if len(blob) % PER_TILE_BYTES != 0: return None
        found = False
        for try_cols in range(cols, min(cols + 15, 200)):
            if try_cols <= 0: continue
            if total_tiles % try_cols == 0:
                if PX < try_cols * TILE and PY < (total_tiles // try_cols) * TILE:
                    cols = try_cols; found = True; break
        if not found: return None
    tx, ty = PX // TILE, PY // TILE
    lx, ly = PX % TILE, PY % TILE
    tile_idx = ty * cols + tx
    base = tile_idx * PER_TILE_BYTES
    alpha = blob[base + ly * TILE + lx]
    if alpha == 0: return (0, 0, 0, 0)
    bgra_base = base + TILE * TILE + (ly * TILE + lx) * 4
    b, g, r = blob[bgra_base], blob[bgra_base + 1], blob[bgra_base + 2]
    return (r, g, b, alpha)

def fast_pixel_mask(clip, layer_id):
    ext_id = clip._resolve_mask_external_id(layer_id)
    if ext_id is None: return None
    blob = clip._get_tile_blob(ext_id, empty_fill=clip._mask_empty_fill(layer_id))
    if blob is None: return None
    cols = (clip.width + TILE - 1) // TILE
    expected = cols * ((clip.height + TILE - 1) // TILE) * TILE * TILE
    if len(blob) != expected:
        total_tiles = len(blob) // (TILE * TILE)
        if len(blob) % (TILE * TILE) != 0: return None
        for try_cols in range(cols, 0, -1):
            if total_tiles % try_cols == 0:
                if PX < try_cols * TILE and PY < (total_tiles // try_cols) * TILE:
                    cols = try_cols; break
        else: return None
    tx, ty = PX // TILE, PY // TILE
    lx, ly = PX % TILE, PY % TILE
    return blob[(ty * cols + tx) * TILE * TILE + ly * TILE + lx]

clip = ClipFile("Ref_Terra404_Live2D.clip")
print(f"Canvas: {clip.width}x{clip.height}, target=({PX},{PY}), CSP ref={CSP_REF}")

root = clip._layer_row(clip.root_layer_id)

# Walk full hierarchy, print all relevant nodes
def walk(first_id, depth=0):
    for lid in clip._walk_chain(first_id):
        row = clip._layer_row(lid)
        if row is None: continue
        vis = _layer_is_visible(row)
        prefix = "  " * depth

        lt = row["LayerType"]
        comp = row["LayerComposite"]
        mode = _BLEND_MAPPING.get(comp, f"UNK({comp})")
        name = row["LayerName"]
        vis_str = "VIS" if vis else "HID"

        if lt == LAYER_TYPE_PAPER:
            if vis:
                rgb = (clip._clip_color_component(row["DrawColorMainRed"]),
                       clip._clip_color_component(row["DrawColorMainGreen"]),
                       clip._clip_color_component(row["DrawColorMainBlue"]))
                print(f"{prefix}[Paper id={lid} color={rgb} {vis_str}]")
            continue

        if lt in (LAYER_TYPE_FOLDER, LAYER_TYPE_LAYER_FOLDER, LAYER_TYPE_GROUP):
            type_name = "Folder" if lt == LAYER_TYPE_FOLDER else ("LayerFolder" if lt == LAYER_TYPE_LAYER_FOLDER else "Group")
            child_count = len(clip._walk_chain(row["LayerFirstChildIndex"]))
            print(f"{prefix}[{type_name} id={lid} comp={comp} ({mode}) "
                  f"clip={row['LayerClip']} opacity={row['LayerOpacity']/256:.3f} "
                  f"children={child_count} {vis_str} name='{name}']")
            if vis:
                walk(row["LayerFirstChildIndex"], depth + 1)
            continue

        if lt not in RASTER_LAYER_TYPES:
            if vis:
                print(f"{prefix}[Unknown type={lt} id={lid} {vis_str} name='{name}']")
            continue

        # Raster layer
        if not vis:
            continue

        px = fast_pixel_rgba(clip, lid)
        a_raw = px[3] if px else 0
        mask_val = fast_pixel_mask(clip, lid) if a_raw > 0 else None

        if a_raw > 0:
            eff_a = a_raw
            if mask_val is not None:
                eff_a = (eff_a * mask_val) // 255
            print(f"{prefix}Layer id={lid:>5} comp={comp:>3} ({mode:<15}) "
                  f"clip={row['LayerClip']} opacity={row['LayerOpacity']/256:.3f} "
                  f"raw=({px[0]:>3},{px[1]:>3},{px[2]:>3},{px[3]:>3}) "
                  f"mask={mask_val} eff_a={eff_a} "
                  f"name='{row['LayerName']}'")

walk(root["LayerFirstChildIndex"])
clip.close()
