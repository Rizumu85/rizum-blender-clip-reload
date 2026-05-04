"""
Minimal instrumented trace of pixel (2190, 1319) in Ref_Terra404_Live2D.clip.
Monkey-patches ClipFile methods to log state changes at the target pixel only.
"""
from __future__ import annotations
import numpy as np
from clip_loader import (
    ClipFile, _BLEND_MAPPING, _layer_is_visible, _alpha_bbox, RASTER_LAYER_TYPES,
    LAYER_TYPE_FOLDER, LAYER_TYPE_GROUP, LAYER_TYPE_LAYER_FOLDER, LAYER_TYPE_PAPER,
)

PX, PY = 2190, 1319
CSP_REF = np.array([223, 164, 201, 255], dtype=np.uint8)

def fmt_state(arr):
    """Format premultiplied array at (PY, PX) as straight RGBA u8."""
    a = arr[PY, PX, 3]
    if a > 1e-6:
        r = int(np.clip(arr[PY, PX, 0] / a * 255 + 0.5, 0, 255))
        g = int(np.clip(arr[PY, PX, 1] / a * 255 + 0.5, 0, 255))
        b = int(np.clip(arr[PY, PX, 2] / a * 255 + 0.5, 0, 255))
        a_u8 = int(np.clip(a * 255 + 0.5, 0, 255))
    else:
        r = g = b = a_u8 = 0
    return f"[{r:>3},{g:>3},{b:>3},{a_u8:>3}]"

INDENT = [0]

def log(msg):
    print(f"{'  ' * INDENT[0]}{msg}")

# Instrument _render_chain
_original_render_chain = ClipFile._render_chain

def _instrumented_render_chain(self, first_id, out, preserve_clipped_alpha=True):
    global clip_base_alpha_u8_trace
    for lid in self._walk_chain(first_id):
        layer = self._layer_row(lid)
        if not _layer_is_visible(layer):
            continue
        if layer["LayerType"] == LAYER_TYPE_PAPER:
            continue

        if layer["LayerType"] == LAYER_TYPE_LAYER_FOLDER:
            mode = _BLEND_MAPPING.get(layer["LayerComposite"], f"UNK({layer['LayerComposite']})")
            if mode == "THROUGH":
                log(f"[Folder id={lid} THROUGH clip={layer['LayerClip']} name='{layer['LayerName']}']")
                INDENT[0] += 1
                _instrumented_render_chain(self, layer["LayerFirstChildIndex"], out, preserve_clipped_alpha)
                INDENT[0] -= 1
                clip_base_alpha_u8_trace = None
            else:
                log(f"[Folder id={lid} mode={mode} clip={layer['LayerClip']} "
                    f"opacity={layer['LayerOpacity']/256:.3f} name='{layer['LayerName']}']")
                group_out = np.zeros_like(out)
                save_cba = clip_base_alpha_u8_trace
                clip_base_alpha_u8_trace = None
                INDENT[0] += 1
                _instrumented_render_chain(self, layer["LayerFirstChildIndex"], group_out, True)
                INDENT[0] -= 1
                clip_base_alpha_u8_trace = save_cba
                rgba_u8, _ = self._premul_to_rgba_u8(group_out)
                mask = self._layer_mask_for_composite(layer)
                layer_alpha_u8 = self._apply_mask_and_clip(layer, rgba_u8, mask, clip_base_alpha_u8_trace)
                before = fmt_state(out)
                did = self._composite_image(out, layer, rgba_u8, layer_alpha_u8)
                after = fmt_state(out)
                ga = group_out[PY, PX, 3]
                if ga > 1e-6 or did:
                    grp = fmt_state(group_out)
                    log(f"  group_result={grp} composite: {before} -> {after}")
                if did and not layer["LayerClip"]:
                    clip_base_alpha_u8_trace = layer_alpha_u8
            continue

        if layer["LayerType"] == LAYER_TYPE_GROUP:
            mode = _BLEND_MAPPING.get(layer["LayerComposite"], f"UNK({layer['LayerComposite']})")
            if mode == "THROUGH":
                log(f"[Group id={lid} THROUGH clip={layer['LayerClip']} name='{layer['LayerName']}']")
                before = fmt_state(out)
                INDENT[0] += 1
                self._render_through_group(layer, out, preserve_clipped_alpha)
                INDENT[0] -= 1
                after = fmt_state(out)
                if before != after:
                    log(f"  THROUGH group: {before} -> {after}")
                clip_base_alpha_u8_trace = None
            else:
                log(f"[Group id={lid} mode={mode} clip={layer['LayerClip']} "
                    f"opacity={layer['LayerOpacity']/256:.3f} name='{layer['LayerName']}']")
                group_out = np.zeros_like(out)
                save_cba = clip_base_alpha_u8_trace
                clip_base_alpha_u8_trace = None
                INDENT[0] += 1
                _instrumented_render_chain(self, layer["LayerFirstChildIndex"], group_out, True)
                INDENT[0] -= 1
                rgba_u8, _ = self._premul_to_rgba_u8(group_out)
                mask = self._layer_mask_for_composite(layer)
                layer_alpha_u8 = self._apply_mask_and_clip(layer, rgba_u8, mask, clip_base_alpha_u8_trace)
                before = fmt_state(out)
                did = self._composite_image(out, layer, rgba_u8, layer_alpha_u8)
                after = fmt_state(out)
                ga = group_out[PY, PX, 3]
                if ga > 1e-6 or did:
                    grp = fmt_state(group_out)
                    log(f"  group_offscreen={grp} composite: {before} -> {after}")
                clip_base_alpha_u8_trace = save_cba
                if did and not layer["LayerClip"]:
                    clip_base_alpha_u8_trace = layer_alpha_u8
            continue

        if layer["LayerType"] not in RASTER_LAYER_TYPES:
            continue
        if layer["LayerOffsetX"] or layer["LayerOffsetY"]:
            continue

        rgba = self.decode_layer(layer["MainId"])
        if rgba is None:
            continue
        if PY >= rgba.shape[0] or PX >= rgba.shape[1]:
            continue
        a_raw = rgba[PY, PX, 3]
        if a_raw == 0:
            continue

        mask = self._layer_mask_for_composite(layer)
        mask_val = mask[PY, PX] if mask is not None else None
        layer_alpha_u8 = self._apply_mask_and_clip(layer, rgba, mask, clip_base_alpha_u8_trace)
        effective_a = layer_alpha_u8[PY, PX]
        mode = _BLEND_MAPPING.get(layer["LayerComposite"], f"UNK({layer['LayerComposite']})")
        opacity = min(layer["LayerOpacity"] / 256.0, 1.0)
        cba = clip_base_alpha_u8_trace[PY, PX] if clip_base_alpha_u8_trace is not None else None

        r, g, b = rgba[PY, PX, 0], rgba[PY, PX, 1], rgba[PY, PX, 2]
        before = fmt_state(out)

        if layer["LayerClip"] and preserve_clipped_alpha and clip_base_alpha_u8_trace is not None:
            self._composite_clipped_image(out, layer, rgba, layer_alpha_u8, clip_base_alpha_u8_trace)
            clip_type = "clipped"
        else:
            self._composite_image(out, layer, rgba, layer_alpha_u8)
            clip_type = "normal"

        after = fmt_state(out)
        log(f"Layer id={lid:>5} mode={mode:<15} {clip_type} "
            f"opacity={opacity:.3f} clip_flag={layer['LayerClip']} "
            f"raw=({r:>3},{g:>3},{b:>3},{a_raw:>3}) "
            f"mask={mask_val} eff_a={effective_a} "
            f"clip_base_a={cba} "
            f"name='{layer['LayerName']}' "
            f"{before} -> {after}")

        if effective_a > 0 and not layer["LayerClip"]:
            clip_base_alpha_u8_trace = layer_alpha_u8

ClipFile._render_chain = _instrumented_render_chain

# --- Run ---
clip_base_alpha_u8_trace = None

clip = ClipFile("Ref_Terra404_Live2D.clip")
print(f"Canvas: {clip.width}x{clip.height}")
print(f"CSP ref: {CSP_REF}")

result = clip.composite()
final = fmt_state(result.astype(np.float32) / 255.0)

# Also compute the final directly from result u8
r_u8 = int(result[PY, PX, 0])
g_u8 = int(result[PY, PX, 1])
b_u8 = int(result[PY, PX, 2])
a_u8 = int(result[PY, PX, 3])
print(f"\nFinal output: [{r_u8},{g_u8},{b_u8},{a_u8}]  (from compositor)")
print(f"CSP ref:      [223, 164, 201, 255]")

diff_r = abs(r_u8 - 223)
diff_g = abs(g_u8 - 164)
diff_b = abs(b_u8 - 201)
print(f"Diff:         [{diff_r},{diff_g},{diff_b},{abs(a_u8-255)}]")

clip.close()
