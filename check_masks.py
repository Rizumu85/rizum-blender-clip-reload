"""Quick check: masks on key groups and layers at pixel (2190, 1319)."""
from clip_loader import ClipFile, _BLEND_MAPPING, _layer_is_visible, TILE

PX, PY = 2190, 1319

clip = ClipFile("Ref_Terra404_Live2D.clip")

# Check key groups/folders for masks, blend modes, clip status
KEY_IDS = [
    # Early shadow structure
    178, 179, 180, 189, 193,  # 影 hierarchy
    # Highlight groups
    346,  # ハイライト (NORMAL group)
    560,  # 睫毛 (THROUGH group)
    561, 562, 566, 568,  # 睫毛 children
    571,  # ハイライトH (NORMAL group)
    572, 574,  # ハイライトH children
    584, 586,  # ハイライトH かけ group
    # 通り section
    587, 588, 589, 590, 595, 596,  # 線画→通り→髪
    605, 606, 609, 610, 611, 612, 613, 614, 615, 616,  # 通り→つや
    694, 704,  # まつげ
    723, 725,  # 下まつげ
]

for lid in KEY_IDS:
    row = clip._layer_row(lid)
    if row is None:
        print(f"id={lid}: NOT FOUND")
        continue
    lt = row["LayerType"]
    comp = row["LayerComposite"]
    mode = _BLEND_MAPPING.get(comp, f"UNK({comp})")
    vis = (int(row["LayerVisibility"] or 0) & 1) != 0
    name = row["LayerName"]
    clip_flag = row["LayerClip"]
    opacity = row["LayerOpacity"]

    # Check for mask
    has_mask_mipmap = bool(row["LayerLayerMaskMipmap"])
    mask_val = None
    if has_mask_mipmap:
        mask = clip.decode_layer_mask(lid)
        if mask is not None and PY < mask.shape[0] and PX < mask.shape[1]:
            mask_val = mask[PY, PX]

    print(f"id={lid:>5} type={lt:>4} comp={comp:>3} ({mode:<15}) "
          f"clip={clip_flag} opacity={opacity/256:.3f} vis={vis} "
          f"has_mask_mipmap={has_mask_mipmap} mask_val_at_px={mask_val} "
          f"name='{name}'")

clip.close()
