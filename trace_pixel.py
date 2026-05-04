"""
Trace pixel (2190, 1319) in Ref_Terra404_Live2D.clip layer-by-layer.
"""
from __future__ import annotations
import numpy as np
from clip_loader import ClipFile, _BLEND_MAPPING, _layer_is_visible, _alpha_bbox, RASTER_LAYER_TYPES

LAYER_TYPE_FOLDER = 256
LAYER_TYPE_GROUP = 2
LAYER_TYPE_LAYER_FOLDER = 0
LAYER_TYPE_PAPER = 1584

PX, PY = 2190, 1319

clip = ClipFile("Ref_Terra404_Live2D.clip")
print(f"Canvas: {clip.width}x{clip.height}")

# Get all layers in walk order
root = clip._layer_row(clip.root_layer_id)
print(f"Root: id={root['MainId']}, type={root['LayerType']}")

# Walk the full tree and record layers with their depth
all_layers = []

def walk(first_id, depth=0):
    for lid in clip._walk_chain(first_id):
        row = clip._layer_row(lid)
        all_layers.append((depth, lid, row))
        if row["LayerType"] in (LAYER_TYPE_FOLDER, LAYER_TYPE_LAYER_FOLDER, LAYER_TYPE_GROUP):
            walk(row["LayerFirstChildIndex"], depth + 1)

walk(root["LayerFirstChildIndex"])
print(f"Total layer rows: {len(all_layers)}")

def decode_layer_robust(layer_id):
    """Like decode_layer but falls back to blob-size-inferred dimensions."""
    try:
        return clip.decode_layer(layer_id)
    except ValueError:
        pass
    ext_id = clip._resolve_external_id(layer_id)
    if ext_id is None:
        return None
    blob = clip._get_tile_blob(ext_id)
    if blob is None:
        return None
    from clip_loader import TILE, PER_TILE_BYTES
    total_tiles = len(blob) // PER_TILE_BYTES
    side = int(np.ceil(np.sqrt(total_tiles)))
    # Try to find a reasonable tile layout
    for cols in range(side, 0, -1):
        if total_tiles % cols == 0:
            rows = total_tiles // cols
            w = min(cols * TILE, clip.width)
            h = min(rows * TILE, clip.height)
            try:
                from clip_loader import _tiles_to_rgba
                return _tiles_to_rgba(blob, w, h)
            except ValueError:
                continue
    return None

# Find layers that have non-zero alpha at (PX, PY)
print(f"\n=== Layers with non-zero alpha at ({PX}, {PY}) ===")
interesting = []
for depth, lid, row in all_layers:
    if row["LayerType"] not in RASTER_LAYER_TYPES:
        continue
    if not _layer_is_visible(row):
        continue
    rgba = decode_layer_robust(lid)
    if rgba is None:
        continue
    a = rgba[PY, PX, 3]
    if a > 0:
        r, g, b = rgba[PY, PX, 0], rgba[PY, PX, 1], rgba[PY, PX, 2]
        mask = clip._layer_mask_for_composite(row)
        mask_val = mask[PY, PX] if mask is not None else None
        mode = _BLEND_MAPPING.get(row["LayerComposite"], f"UNKNOWN({row['LayerComposite']})")

        # Apply individual mask
        masked_a = a
        if mask_val is not None:
            masked_a = (int(a) * int(mask_val)) // 255

        opacity = min(row["LayerOpacity"] / 256.0, 1.0)

        interesting.append((depth, lid, row, r, g, b, a, mask_val, masked_a, mode, opacity))

        print(f"  depth={depth} id={lid:>5} type={row['LayerType']:>4} "
              f"clip={row['LayerClip']} comp={row['LayerComposite']:>3} ({mode:<15}) "
              f"opacity={opacity:.3f} "
              f"raw=({r:>3},{g:>3},{b:>3},{a:>3}) "
              f"mask={mask_val} masked_a={masked_a} "
              f"name={row['LayerName']}")

print(f"\nFound {len(interesting)} raster layers with alpha>0 at this pixel")

# Now trace the compositor step by step
print("\n=== Step-by-step compositor trace ===")

# Initialize output
out = np.zeros((clip.height, clip.width, 4), dtype=np.float32)
paper_color = clip._paper_color()
if paper_color is not None:
    out[..., 0] = paper_color[0]
    out[..., 1] = paper_color[1]
    out[..., 2] = paper_color[2]
    out[..., 3] = 1.0
    print(f"Paper color: ({paper_color[0]:.3f}, {paper_color[1]:.3f}, {paper_color[2]:.3f})")

def pixel_state(pfx=""):
    a = out[PY, PX, 3]
    if a > 1e-6:
        r = out[PY, PX, 0] / a
        g = out[PY, PX, 1] / a
        b = out[PY, PX, 2] / a
    else:
        r = g = b = 0.0
    r_u8 = int(np.clip(r * 255 + 0.5, 0, 255))
    g_u8 = int(np.clip(g * 255 + 0.5, 0, 255))
    b_u8 = int(np.clip(b * 255 + 0.5, 0, 255))
    a_u8 = int(np.clip(a * 255 + 0.5, 0, 255))
    print(f"{pfx}output: RGBA=({r_u8:>3},{g_u8:>3},{b_u8:>3},{a_u8:>3}) "
          f"premul=({out[PY,PX,0]:.4f},{out[PY,PX,1]:.4f},{out[PY,PX,2]:.4f},{out[PY,PX,3]:.4f})")

# Simulate compositor
clip_base_alpha_u8 = None

def simulate_render_chain(first_id, depth=0):
    global clip_base_alpha_u8
    prefix = "  " * depth

    for lid in clip._walk_chain(first_id):
        layer = clip._layer_row(lid)
        if not _layer_is_visible(layer):
            continue
        if layer["LayerType"] == LAYER_TYPE_PAPER:
            continue
        if layer["LayerType"] == LAYER_TYPE_LAYER_FOLDER:
            mode = _BLEND_MAPPING.get(layer["LayerComposite"], f"UNKNOWN({layer['LayerComposite']})")
            visible_child = any(
                _layer_is_visible(clip._layer_row(cid))
                for cid in clip._walk_chain(layer["LayerFirstChildIndex"])
            )
            print(f"{prefix}[Folder id={lid} mode={mode} clip={layer['LayerClip']} "
                  f"name={layer['LayerName']} has_visible_children={visible_child}]")
            if mode == "THROUGH":
                simulate_render_chain(layer["LayerFirstChildIndex"], depth + 1)
                clip_base_alpha_u8 = None
            else:
                # Offscreen rendering
                group_out = np.zeros_like(out)
                save_cba = clip_base_alpha_u8
                clip_base_alpha_u8 = None
                simulate_render_chain(layer["LayerFirstChildIndex"], depth + 1)
                clip_base_alpha_u8 = save_cba

                # Would composite group_out back - but this is complex for the trace
                # Let's just note it
                ga = group_out[PY, PX, 3]
                if ga > 1e-6:
                    gr = group_out[PY, PX, 0] / ga
                    gg = group_out[PY, PX, 1] / ga
                    gb = group_out[PY, PX, 2] / ga
                else:
                    gr = gg = gb = 0.0
                print(f"{prefix}  Offscreen result at pixel: "
                      f"({int(gr*255+0.5)},{int(gg*255+0.5)},{int(gb*255+0.5)},{int(ga*255+0.5)})")
            continue
        if layer["LayerType"] == LAYER_TYPE_GROUP:
            mode = _BLEND_MAPPING.get(layer["LayerComposite"], f"UNKNOWN({layer['LayerComposite']})")
            print(f"{prefix}[Group id={lid} mode={mode} clip={layer['LayerClip']} "
                  f"opacity={layer['LayerOpacity']/256:.3f} name={layer['LayerName']}]")
            if mode == "THROUGH":
                simulate_render_chain(layer["LayerFirstChildIndex"], depth + 1)
                clip_base_alpha_u8 = None
            else:
                group_out = np.zeros_like(out)
                save_cba = clip_base_alpha_u8
                clip_base_alpha_u8 = None
                simulate_render_chain(layer["LayerFirstChildIndex"], depth + 1)
                # composite group back
                rgba_u8, _ = clip._premul_to_rgba_u8(group_out)
                mask = clip._layer_mask_for_composite(layer)
                layer_alpha_u8 = clip._apply_mask_and_clip(layer, rgba_u8, mask, clip_base_alpha_u8)
                clip._composite_image(out, layer, rgba_u8, layer_alpha_u8)
                pixel_state(f"{prefix}  after group {lid}: ")
                clip_base_alpha_u8 = save_cba
                if not layer["LayerClip"]:
                    clip_base_alpha_u8 = layer_alpha_u8
            continue
        if layer["LayerType"] not in RASTER_LAYER_TYPES:
            continue

        rgba = decode_layer_robust(lid)
        if rgba is None:
            continue

        # Check bounds
        if PY >= rgba.shape[0] or PX >= rgba.shape[1]:
            continue

        a_raw = rgba[PY, PX, 3]
        if a_raw == 0:
            continue

        r_raw, g_raw, b_raw = rgba[PY, PX, 0], rgba[PY, PX, 1], rgba[PY, PX, 2]
        mask = clip._layer_mask_for_composite(layer)
        mask_val = mask[PY, PX] if mask is not None else None

        layer_alpha_u8 = clip._apply_mask_and_clip(layer, rgba, mask, clip_base_alpha_u8)
        effective_a = layer_alpha_u8[PY, PX]

        mode = _BLEND_MAPPING.get(layer["LayerComposite"], f"UNKNOWN({layer['LayerComposite']})")

        print(f"{prefix}Layer id={lid:>5} mode={mode:<15} clip={layer['LayerClip']} "
              f"opacity={layer['LayerOpacity']/256:.3f} "
              f"raw=({r_raw:>3},{g_raw:>3},{b_raw:>3},{a_raw:>3}) "
              f"mask={mask_val} eff_a={effective_a} "
              f"clip_base_a={clip_base_alpha_u8[PY,PX] if clip_base_alpha_u8 is not None else None} "
              f"name={layer['LayerName']}")

        before = out[PY, PX].copy()

        if layer["LayerClip"] and clip_base_alpha_u8 is not None:
            clip._composite_clipped_image(out, layer, rgba, layer_alpha_u8, clip_base_alpha_u8)
        else:
            clip._composite_image(out, layer, rgba, layer_alpha_u8)

        pixel_state(f"{prefix}  after: ")

        if effective_a > 0 and not layer["LayerClip"]:
            clip_base_alpha_u8 = layer_alpha_u8

simulate_render_chain(root["LayerFirstChildIndex"])

# Final result
print("\n=== Final comparison ===")
pixel_state("Final ")
print(f"CSP ref:     [223, 164, 201, 255]")

clip.close()
