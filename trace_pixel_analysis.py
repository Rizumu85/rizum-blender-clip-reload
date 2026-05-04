"""
Targeted trace: run the actual compositor, but instrument _composite_image
and _composite_clipped_image to log only when pixel (2190, 1319) changes state.

This avoids full-array decoding by using the real compositor (which already has
cached blobs) but narrows output to just the target pixel.
"""
from __future__ import annotations
import numpy as np
from clip_loader import ClipFile, _BLEND_MAPPING

PX, PY = 2190, 1319
CSP_REF = [223, 164, 201, 255]

# Instrument _composite_image
_orig_composite_image = ClipFile._composite_image

def _instrumented_composite_image(self, out, layer, rgba, layer_alpha_u8):
    before = out[PY, PX].copy()
    result = _orig_composite_image(self, out, layer, rgba, layer_alpha_u8)
    after = out[PY, PX]

    if not np.array_equal(before, after):
        mode = _BLEND_MAPPING.get(layer["LayerComposite"], "UNK")
        def fmt(arr):
            a = arr[3]
            if a > 1e-6:
                r = int(np.clip(arr[0]/a*255+0.5,0,255))
                g = int(np.clip(arr[1]/a*255+0.5,0,255))
                b = int(np.clip(arr[2]/a*255+0.5,0,255))
                aa = int(np.clip(a*255+0.5,0,255))
            else:
                r=g=b=aa=0
            return f"[{r:>3},{g:>3},{b:>3},{aa:>3}]"
        print(f"  Layer id={layer['MainId']:>5} mode={mode:<15} clip={layer['LayerClip']} "
              f"opacity={layer['LayerOpacity']/256:.3f} "
              f"name='{layer['LayerName']}' "
              f"{fmt(before)} -> {fmt(after)}")
    return result

_orig_composite_clipped = ClipFile._composite_clipped_image

def _instrumented_composite_clipped(self, out, layer, rgba, layer_alpha_u8, clip_base):
    before = out[PY, PX].copy()
    result = _orig_composite_clipped(self, out, layer, rgba, layer_alpha_u8, clip_base)
    after = out[PY, PX]
    if not np.array_equal(before, after):
        mode = _BLEND_MAPPING.get(layer["LayerComposite"], "UNK")
        def fmt(arr):
            a = arr[3]
            if a > 1e-6:
                r = int(np.clip(arr[0]/a*255+0.5,0,255))
                g = int(np.clip(arr[1]/a*255+0.5,0,255))
                b = int(np.clip(arr[2]/a*255+0.5,0,255))
                aa = int(np.clip(a*255+0.5,0,255))
            else:
                r=g=b=aa=0
            return f"[{r:>3},{g:>3},{b:>3},{aa:>3}]"
        if clip_base is not None:
            cba = int(clip_base[PY, PX])
        else:
            cba = None
        print(f"  Layer id={layer['MainId']:>5} mode={mode:<15} CLIPPED "
              f"clip_base_a={cba} opacity={layer['LayerOpacity']/256:.3f} "
              f"name='{layer['LayerName']}' "
              f"{fmt(before)} -> {fmt(after)}")
    return result

ClipFile._composite_image = _instrumented_composite_image
ClipFile._composite_clipped_image = _instrumented_composite_clipped

# Also instrument offscreen folder composites
_orig_render_chain = ClipFile._render_chain

def _instrumented_render_chain(self, first_id, out, preserve_clipped_alpha=True):
    return _orig_render_chain(self, first_id, out, preserve_clipped_alpha)

ClipFile._render_chain = _instrumented_render_chain

clip = ClipFile("Ref_Terra404_Live2D.clip")
print(f"Canvas: {clip.width}x{clip.height}")
print(f"Tracing pixel ({PX}, {PY}) — only layers that change this pixel will be logged")
print(f"CSP ref: {CSP_REF}")
print()

result = clip.composite()

r = int(result[PY, PX, 0])
g = int(result[PY, PX, 1])
b = int(result[PY, PX, 2])
a = int(result[PY, PX, 3])
print(f"\n=== Result ===")
print(f"Loader: [{r},{g},{b},{a}]")
print(f"CSP:    {CSP_REF}")
print(f"Diff:   [{abs(r-223)},{abs(g-164)},{abs(b-201)},{abs(a-255)}]")
clip.close()
