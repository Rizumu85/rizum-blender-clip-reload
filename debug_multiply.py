"""
debug_multiply.py — Empirically reverse the Multiply clipping formula.

Groups pixels by L5/L6/L7/L8 alpha values and compares plugin vs CSP output
to discover the exact blending formula used by CSP.
"""
import sys, numpy as np
from PIL import Image
from collections import defaultdict
sys.path.insert(0, '.')
from clip_loader import ClipFile, _BLEND_MAPPING, _blend_func

clip = ClipFile('img/Test_AddGlowMultiply.clip')
csp = np.array(Image.open('img/Test_AddGlowMultiply.png').convert('RGBA'))
result = clip.composite()

rgba5 = clip.decode_layer(5)
rgba6 = clip.decode_layer(6)
rgba7 = clip.decode_layer(7)
rgba8 = clip.decode_layer(8)

# Only pixels where ALL layers are active
mask = (rgba5[...,3] > 0) & (rgba6[...,3] > 0) & (rgba7[...,3] > 0) & (rgba8[...,3] > 0)
ys, xs = np.where(mask)
print(f'Active pixels: {len(ys)}')

# Group by L6 alpha (base) and L7 alpha (clipped source)
groups = defaultdict(list)
for i in range(min(5000, len(ys))):
    y, x = ys[i], xs[i]
    a6 = rgba6[y,x,3]
    a7_orig = rgba7[y,x,3]
    group_key = (a6 // 32, a7_orig // 32)  # bucket by base and source alpha
    groups[group_key].append((y, x))

# For each group, compute the average ratio between CSP and plugin
print('\nGroup (base_a_bucket, src_a_bucket): count | avg_CSP/Plugin_ratio')
print('-' * 70)
for (a6_b, a7_b), pixels in sorted(groups.items()):
    if len(pixels) < 5:
        continue

    ratios_r, ratios_g, ratios_b = [], [], []
    for y, x in pixels:
        plugin_px = result[y, x].astype(float)
        csp_px = csp[y, x].astype(float)
        with np.errstate(invalid='ignore', divide='ignore'):
            for ch, ratios in enumerate([ratios_r, ratios_g, ratios_b]):
                if plugin_px[ch] > 0:
                    ratios.append(csp_px[ch] / plugin_px[ch])

    if ratios_r:
        print(f'base={a6_b*32:3d} src={a7_b*32:3d}: {len(pixels):4d}px | '
              f'R={np.mean(ratios_r):.3f} G={np.mean(ratios_g):.3f} B={np.mean(ratios_b):.3f}')

# Now: for specific pixels where L8 contribution is minimal, trace the formula
print('\n--- Pixel-level trace with L8 near-zero ---')
l8_near_zero = rgba8[..., 3] < 5
pixels_3_layer = mask & l8_near_zero
ys2, xs2 = np.where(pixels_3_layer)
print(f'Pixels with L5+L6+L7 only (L8 negligible): {len(ys2)}')

for i in range(min(10, len(ys2))):
    y, x = ys2[i], xs2[i]
    r5, r6, r7 = rgba5[y,x].astype(float)/255, rgba6[y,x].astype(float)/255, rgba7[y,x].astype(float)/255

    # Manual composite: L5(Normal) + L6(ADD_GLOW) on white
    out_a = 1.0
    out_rgb = np.ones(3)
    # L5
    src_pm = r5[:3] * r5[3]; inv = 1 - r5[3]
    out_rgb = src_pm + out_rgb * inv
    out_a = r5[3] + out_a * inv
    # L6 ADD_GLOW
    src_pm = r6[:3] * r6[3]; inv = 1 - r6[3]
    out_a_new = r6[3] + out_a * inv
    out_rgb = np.minimum(src_pm + out_rgb * out_a, out_a_new)
    out_a = out_a_new

    coverage = out_a  # after L5+L6 on white
    dst_straight = out_rgb / max(out_a, 1e-6)

    # Plugin Multiply
    a7_masked = r7[3] * r6[3]
    src_a = a7_masked
    blended = r7[:3] * dst_straight
    plugin_rgb = blended * src_a + dst_straight * out_a * (1 - src_a)
    plugin_a = src_a + out_a * (1 - src_a)
    plugin_straight = plugin_rgb / max(plugin_a, 1e-6)

    csp_px = csp[y,x].astype(float)/255
    print(f'\nPixel [{y},{x}]:')
    print(f'  L5: α={r5[3]:.2f} rgb=[{r5[0]:.2f},{r5[1]:.2f},{r5[2]:.2f}]')
    print(f'  L6: α={r6[3]:.2f} rgb=[{r6[0]:.2f},{r6[1]:.2f},{r6[2]:.2f}] base')
    print(f'  L7: α={r7[3]:.2f} rgb=[{r7[0]:.2f},{r7[1]:.2f},{r7[2]:.2f}] clip')
    print(f'  After L5+L6: coverage={coverage:.3f} straight=[{dst_straight[0]:.3f},{dst_straight[1]:.3f},{dst_straight[2]:.3f}]')
    print(f'  L7 masked_α={a7_masked:.3f}')
    print(f'  Multiply: blended=[{blended[0]:.3f},{blended[1]:.3f},{blended[2]:.3f}]')
    print(f'  Plugin PD-over: straight=[{plugin_straight[0]:.3f},{plugin_straight[1]:.3f},{plugin_straight[2]:.3f}] α={plugin_a:.3f}')
    print(f'  CSP: [{csp_px[0]:.3f},{csp_px[1]:.3f},{csp_px[2]:.3f}] α={csp_px[3]:.3f}')

    # Try: blend at base alpha level (CSP coverage-based formula)
    # mask_ratio = base_α / coverage
    mask_ratio = r6[3] / coverage
    trial_a = coverage
    trial = blended * mask_ratio + dst_straight * (1 - mask_ratio)
    print(f'  Trial(mask_ratio={mask_ratio:.3f}): [{trial[0]:.3f},{trial[1]:.3f},{trial[2]:.3f}]')

    # Try: blend at masked alpha directly
    trial2 = blended * a7_masked + dst_straight * (1 - a7_masked)
    print(f'  Trial2(masked_α={a7_masked:.3f}): [{trial2[0]:.3f},{trial2[1]:.3f},{trial2[2]:.3f}]')

    # Try: just Multiply at full strength
    print(f'  Trial3(just Multiply): [{blended[0]:.3f},{blended[1]:.3f},{blended[2]:.3f}]')

clip.close()
