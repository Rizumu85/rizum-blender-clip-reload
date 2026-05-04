"""Check sibling layers before the first visible one in key clipping chains."""
from clip_loader import ClipFile, _BLEND_MAPPING, _layer_is_visible, RASTER_LAYER_TYPES, TILE, PER_TILE_BYTES

PX, PY = 2190, 1319

clip = ClipFile("Ref_Terra404_Live2D.clip")

def fast_alpha(layer_id):
    """Check if layer has alpha>0 at pixel without full decode."""
    ext_id = clip._resolve_external_id(layer_id)
    if ext_id is None: return 0
    w, h = clip._layer_pixel_size(layer_id)
    if PX >= w or PY >= h: return 0
    blob = clip._get_tile_blob(ext_id)
    if blob is None: return 0
    cols = (w + TILE - 1) // TILE
    expected = cols * ((h + TILE - 1) // TILE) * PER_TILE_BYTES
    if len(blob) != expected:
        total_tiles = len(blob) // PER_TILE_BYTES
        if len(blob) % PER_TILE_BYTES != 0: return 0
        for try_cols in range(cols, min(cols + 15, 200)):
            if try_cols <= 0: continue
            if total_tiles % try_cols == 0:
                if PX < try_cols * TILE and PY < (total_tiles // try_cols) * TILE:
                    cols = try_cols; break
        else: return 0
    tx, ty = PX // TILE, PY // TILE
    lx, ly = PX % TILE, PY % TILE
    return blob[(ty * cols + tx) * PER_TILE_BYTES + ly * TILE + lx]

# Check chains inside folders with key clip=1 layers
CHAINS = {
    "Folder 180 '色' (inside 影)": 180,
    "Folder 561 '色' (inside 睫毛)": 561,
    "Folder 566 '線' (inside 睫毛)": 566,
    "Folder 572 '色' (inside ハイライトH)": 572,
    "Folder 610 '線' (inside つや/通り)": 610,
    "Folder 606 '色' (inside つや)": 606,
}

for desc, parent_id in CHAINS.items():
    row = clip._layer_row(parent_id)
    if row is None:
        print(f"{desc}: NOT FOUND")
        continue
    first_id = row["LayerFirstChildIndex"]
    if not first_id:
        print(f"{desc}: no children")
        continue

    print(f"\n{desc} (parent={parent_id}, first_child={first_id}):")
    # Walk children directly — walk NextIndex from first child
    chain_ids = []
    cur = first_id
    seen = set()
    while cur and cur not in seen:
        seen.add(cur)
        chain_ids.append(cur)
        cr = clip._layer_row(cur)
        if cr is None: break
        cur = cr["LayerNextIndex"]

    for i, lid in enumerate(chain_ids):
        lr = clip._layer_row(lid)
        if lr is None: continue
        vis = _layer_is_visible(lr)
        lt = lr["LayerType"]
        comp = lr["LayerComposite"]
        mode = _BLEND_MAPPING.get(comp, f"UNK({comp})")
        clip_flag = lr["LayerClip"]
        name = lr["LayerName"]

        if lt not in RASTER_LAYER_TYPES:
            print(f"  [{i}] id={lid} type={lt} comp={comp} ({mode}) {name} vis={vis}")
            continue

        if not vis:
            print(f"  [{i}] id={lid} HIDDEN {name}")
            continue

        alpha = fast_alpha(lid)
        mask_mipmap = lr["LayerLayerMaskMipmap"]
        print(f"  [{i}] id={lid:>5} clip={clip_flag} a_at_px={alpha} "
              f"has_mask_mipmap={bool(mask_mipmap)} comp={comp} ({mode}) name='{name}'")

clip.close()
