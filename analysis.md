# analysis.md — `.clip` Format Findings

## Prior Art (used)

- **Inochi2D / clip-d** ([github.com/Inochi2D/clip-d](https://github.com/Inochi2D/clip-d)) — high-level chunk format SPEC.md.
- **LavenderSnek / clipdecode** ([github.com/LavenderSnek/clipdecode](https://github.com/LavenderSnek/clipdecode)) — Rust parser; SPEC.md documents `ExternalTableAndColumnName` and `ExternalChunk` mapping.
- **Kazuhito00 / clip_studio_paint_tool** ([github.com/Kazuhito00/clip_studio_paint_tool](https://github.com/Kazuhito00/clip_studio_paint_tool), MIT) — **practical Python decoder we verified against our sample**. Returns BGRA per layer at canvas resolution.
- **ctrlcctrlv / libmarugarou** ([github.com/ctrlcctrlv/libmarugarou](https://github.com/ctrlcctrlv/libmarugarou), Apache 2.0) — Python `split_clip` reference for chunk walking; based on 2019 Rasen Suihei code.

Conclusion: chunk container, SQLite schema, and tile decode for full-color raster layers are all solved by existing OSS. We do **not** need to re-derive these.

## File Container

```
[CSFCHUNK][8B file_size][8B offset_info]
[CHNKHead ...]
[CHNKExta ...] × N    ← raster + vector blobs, one external_id each
[CHNKSQLi ...]        ← embedded SQLite database (metadata)
[CHNKFoot ...]
```

All numeric fields big-endian. Each chunk header = 8B magic + 8B size.

## SQLite Tables (verified on sample `Illustration.clip`)

| Table | Role | Sample row count |
|---|---|---|
| `Canvas` | Canvas metadata (width/height, color profile) | — |
| `CanvasPreview` | Pre-flattened canvas as embedded PNG (`ImageData` blob) | 1 |
| `Layer` | Layer hierarchy: `MainId, CanvasId, LayerName, LayerType, LayerRenderMipmap, LayerRenderThumbnail, LayerNextIndex, LayerFirstChildIndex` | 2 |
| `LayerThumbnail` | Per-layer thumbnail metadata + offscreen ref | 2 |
| `Offscreen` | Per-mipmap pixel data; `BlockData` is the external chunk id (text) | 10 |
| `Mipmap` | Mipmap chain root, refs `BaseMipmapInfo` | 2 |
| `MipmapInfo` | Per-scale entry with `Offscreen` ref | 8 |

Layer types observed: `1` (raster layer), `256` (root / folder).

## Lookup Path (Layer → Pixels)

```
Layer.LayerRenderMipmap
  → Mipmap.main_id
  → Mipmap.BaseMipmapInfo
  → MipmapInfo.main_id (highest scale entry)
  → MipmapInfo.Offscreen
  → Offscreen.main_id
  → Offscreen.BlockData (= external_id text)
  → CHNKExta chunk whose external_id matches
  → BlockDataBeginChunk records (zlib-compressed 256×256 tiles)
  → assemble tiles → BGRA at padded (multiple-of-256) size
  → crop to (image_width, image_height)
```

## Tile Format

- Canvas is divided into 256×256 tiles, padded up to next multiple of 256.
- Each tile contributes `256×320×4 = 327680` bytes of decompressed data:
  - First `256×256` bytes = alpha plane.
  - Following `256×256×4` bytes = BGRA plane (B,G,R,A).
- Compression: zlib `deflate` per tile.
- Chunk records inside `CHNKExta`: `BlockDataBeginChunk` (real data), `BlockStatus`, `BlockCheckSum`, `BlockDataEndChunk`. UTF-16-BE block names.

## Sample Verification

Test file: `Illustration.clip` (512×512, single raster layer "Layer 1").
Ground truth: `Illustration.png` (CSP-exported flat PNG).

- **Alpha channel**: 262144 / 262144 pixels identical (100%).
- **RGB channel**, raw compare: 58055 / 262144 (22.15%).
- **RGB channel, alpha-aware** (premultiplied diff): max=0.000, mean=0.0000.

The 22% raw-RGB match is not a real defect: CSP exports transparent pixels as `(255,255,255,0)`, csp_tool decodes them as `(0,0,0,0)`. Both are valid; alpha=0 makes RGB invisible in any composite. After premultiplying alpha (the only thing that affects how the texture renders), the decoded image is **bit-identical** to the ground truth.

**MVP success criterion is satisfied for single-layer files.**

## Compositor Verification (Final)

`clip_loader.py` (project root) implements the full Direction 5 compositor.

| Sample | Canvas | Layers | Max premultiplied Δ | Mean Δ | Pixels exact |
|---|---|---|---|---|---|
| `Illustration.clip`   | 512² | 1 raster | **0.000000** | 0.0 | 100.0000% |
| `Illustration4K.clip` | 4096² | 3 raster | **0.007151** | 5.8 × 10⁻⁷ | 99.9764% |

The 290 mismatched pixels in the 4K sample (0.0017%) all differ by < 2/255 in any channel — sub-perceptual float32 rounding from alpha-over compositing.

**MVP success criterion satisfied.** Decoded output `Illustration4K_decoded.png` is in the project root for visual comparison.

## 4K Sample Findings (`Illustration4K.clip`)

- **Canvas**: 4096 × 4096
- **`CanvasPreview`**: 1024 × 1024 (downsampled to 1/4 each axis). **Conclusion: CanvasPreview is NOT a viable shortcut on real-world canvases — we must build a compositor.**
- **Layers**: 3 raster layers (`Layer 1/2/3`, MainId 3/5/6) under one root folder (MainId 2, LayerType 256).
- **Per-layer pixel storage**: every layer's `LayerThumbnail.ThumbnailCanvasWidth/Height` = 4096×4096. No bbox cropping. Compositing is straight pixel-aligned alpha-over.
- **Mipmap chain**: each layer has 7 scales (100/50/25/12.5/6.25/3.125/1.5625%). Useful for future low-res preview mode.
- **Decode speed (sandbox, naive numpy)**: ~5–9 s/layer at 4K. Optimisable, but acceptable for an import-on-load workflow.

## Compositing Fields (Layer table)

| Column | Used for |
|---|---|
| `LayerType` | 1 = raster, 256 = root folder, 1584 = paper/background color layer |
| `LayerVisibility` | 0/1 — skip layer if 0 |
| `LayerOpacity` | 0–256 integer (256 = fully opaque). Divide by 256 for 0.0–1.0 |
| `LayerComposite` | Blend mode index. 0 = Normal. Observed mappings include 2 Multiply, 3 Color Burn, 5 Subtract, 8 Screen, 9 Color Dodge, 11 Add, 14 Overlay, 15 Soft Light, 16 Hard Light, 21 Difference. |
| `DrawColorMainRed/Green/Blue` | Paper layer background color source observed for `LayerType=1584`. Values may be stored as repeated-byte 32-bit components, e.g. `0xe2e2e2e2` = 226. |
| `LayerPaletteRed/Green/Blue` | Fallback color fields; observed as 0 in `IllustrationBlendModes.clip` paper layer. |
| `LayerClip` | Clipping group flag (post-MVP) |
| `LayerFirstChildIndex` | First child MainId (folder traversal) |
| `LayerNextIndex` | Next sibling MainId (z-order traversal); 0 = end of chain |
| `LayerOffsetX / OffsetY` | Layer pixel offset on canvas (observed 0 in sample but to handle generally) |
| `LayerMasking` / `LayerLayerMaskMipmap` | Layer mask (post-MVP) |

Z-order traversal: from root layer's `FirstChild`, walk `NextIndex` until 0. Order direction (bottom-up vs top-down render) to be verified against ground-truth PNG.

## Blend Mode / Paper Layer Findings

`IllustrationBlendModes.clip` identifies these `LayerComposite` values:

| Integer | Mode |
|---:|---|
| 2 | Multiply |
| 3 | Color Burn |
| 5 | Subtract |
| 8 | Screen |
| 9 | Color Dodge |
| 11 | Add |
| 14 | Overlay |
| 15 | Soft Light |
| 16 | Hard Light |
| 21 | Difference |
| 1 | Darken |
| 4 | Linear Burn |
| 6 | Darker Color |
| 7 | Lighten |
| 10 | Glow Dodge |
| 12 | Add (Glow) |
| 13 | Lighter Color |
| 17 | Vivid Light |
| 18 | Linear Light |
| 19 | Pin Light |
| 20 | Hard Mix |
| 22 | Exclusion |
| 23 | Hue |
| 24 | Saturation |
| 25 | Color |
| 26 | Brightness |
| 36 | Divide |

All blend modes shown in the supplied CSP blend-mode menu now have observed `LayerComposite` integer mappings.

Paper layers use `LayerType=1584`. They are not raster layers, but a colored paper layer still affects CSP's flattened export. In `IllustrationBlendModes.clip`, the visible paper layer stores its color in `DrawColorMainRed/Green/Blue` as `0xe2e2e2e2` per channel, so the loader initializes the compositor with opaque RGB 226/226/226 before compositing raster layers. `LayerThumbnail.ThumbnailMainColorRed/Green/Blue` carries the same color and is used as a fallback; `LayerPaletteRed/Green/Blue` was black in this sample and is only a final fallback. If a test file contains colored paper, compare against CSP output with paper left enabled; removing paper changes the expected result.

Validation against `IllustrationBlendModes.png`:

- Loader outputs `test.png` and `IllustrationBlendModes.clip.png` are byte-identical.
- Paper background matches exactly: top-left pixel is `[226, 226, 226, 255]` in both reference and loader output.
- Full-image premultiplied diff: max `0.290196`, mean `0.0002356`.
- Raw diff: max `74`, mean `0.0601`; exact RGBA pixels `231152 / 262144`.
- The largest observed pixel difference is in a multi-layer overlap at `(266, 244)`: reference `[42, 4, 214, 255]`, loader `[40, 4, 140, 255]`. Simple source/destination swaps for Color Dodge / Color Burn made the overall diff worse, so the remaining issue is formula fidelity rather than layer order, paper color, or integer mapping.

Validation against `IllustrationBlendModes2.png` after completing the blend-mode integer map:

- No unknown `LayerComposite` warnings.
- Initial result exposed a large Hard Mix formula mismatch: full-image premultiplied diff max `1.0`, mean `0.01566`, with many white-vs-magenta pixels.
- Hard Mix was corrected to threshold Vivid Light instead of Linear Light. This reduced full-image premultiplied mean diff to `0.0003222`; exact RGBA pixels `224617 / 262144`.
- Remaining max diff is a single Hard Mix threshold-edge pixel at `(320, 446)`: reference `[255, 255, 255, 255]`, loader `[255, 0, 255, 255]`. Broader formula fidelity work should use isolated single-mode samples before further tuning.

Isolated blend-mode samples (`Test_*.clip/png`) show the Glow modes need source-alpha-weighted formulas rather than generic blend-over interpolation:

- `Add (Glow)` matches CSP when composited as `dst + src * alpha` with clamping. After the fix: premultiplied mean diff `0.0`, max `0.003922`, exact pixels `262143 / 262144`.
- `Glow dodge` matches CSP when source alpha is folded into the dodge strength: `dst / (1 - src * alpha)`. After the fix: premultiplied mean diff `0.00009399`, max `0.007843`.
- Full `IllustrationBlendModes2.png` after the Glow fixes: premultiplied mean diff `0.0002774`, exact pixels `225106 / 262144`. The only `1.0` max diff remains the single Hard Mix threshold-edge pixel above.

Further isolated samples show CSP's Color Dodge and Color Burn use 8-bit integer formulas after rounding source/destination channels:

- `Color burn` is bit-perfect after integer rounding: exact pixels `262144 / 262144`.
- `Glow dodge` is bit-perfect after integer rounding in the source-alpha-weighted compositor path: exact pixels `262144 / 262144`.
- `Color dodge` is bit-perfect after using the same rounded destination color for both the dodge formula and final alpha interpolation: exact pixels `262144 / 262144`.
- Full `IllustrationBlendModes2.png` after Dodge/Burn integer rounding: premultiplied mean diff `0.0002537`, exact pixels `231306 / 262144`. The same single Hard Mix threshold-edge pixel remains the only max `1.0` outlier.

For non-separable modes, CSP appears to quantize source/destination RGB inputs for Hue, Saturation, and Brightness before applying the W3C/Photoshop formulas. Applying that only to those modes gives:

- `Hue`: exact pixels `262090 / 262144`, max diff `1`.
- `Saturation`: max diff improved from `5` to `2`; exact pixels `261941 / 262144`.
- `Brightness`: exact pixels improved from `261998` to `262070`, max diff `1`.
- `Color` was left on the unquantized W3C/Photoshop path because tested HSL variants and input/output quantization did not produce a clear improvement.
- Full `IllustrationBlendModes2.png` after this change: premultiplied mean diff `0.0002499`, exact pixels `232367 / 262144`.

## Structure Feature Findings

Isolated structure samples:

- `Test_Mask.clip/png`: masked raster content was stored as `LayerType=3` rather than `LayerType=1`. Treating type 3 as raster-like makes the layer + mask composite bit-perfect: exact pixels `262144 / 262144`.
- `Test_Opacity.clip/png`: layer opacity path is correct within rounding tolerance: max diff `1`, exact pixels `261892 / 262144`.
- `Test_FolderVisibility.clip/png`: hidden layer / empty folder visibility behavior is bit-perfect: exact pixels `262144 / 262144`. Non-root folder rows can use `LayerType=0`.
- `Test_FolderNested.clip/png`: non-empty `LayerType=0` folders are traversed recursively through `LayerFirstChildIndex`; visible child layers are composited and hidden child layers are skipped. Exact pixels `262144 / 262144`.
- `Test_Clipping.clip/png`: `LayerClip=1` is supported by multiplying the clipped layer alpha by the previous raster-like layer's effective alpha. This reduces the isolated clipping test from max diff `156` / exact `98552` to max diff `18` / exact `257987`. Remaining differences are on antialiased clip edges; binary clip variants were worse.

## Real Artwork Smoke Test

`Test_RealArt.clip/png` is a 4000x6100 real artwork with 596 layer rows. Initial decode completes, but exposes the next fidelity boundary:

- Baseline after current folder/mask/blend fixes: premultiplied mean diff `0.0032566`, exact pixels `10136539 / 24400000`.
- Median and 90th-percentile per-pixel premultiplied max diff are both `0.0`, so most pixels match; large errors are localized.
- Four `LayerType=2` rows (`袖奥`, `袖左`, `数珠ｋ`, `数珠ｋ`) have child layers and `LayerComposite=2`. Treating them as ordinary folders made the real-art diff worse (`mean 0.0101284`, exact `9995796 / 24400000`) because the group blend/mask behavior is not equivalent to flattening children directly.
- Conclusion: `LayerType=2` appears to be a group/layer folder that requires offscreen group compositing with the folder's composite mode (Multiply in this sample) before blending back into the parent stack. Keep it unsupported until group compositing is implemented, rather than flattening it incorrectly.

Additional `LayerType=2` group validation:

- `LayerType=2` offscreen group compositing is now implemented. Child layers render into a transparent buffer, the buffer is quantized back to an RGBA source, then it is blended into the parent stack through the group row's `LayerComposite` and `LayerOpacity`.
- On `Test_RealArt.clip/png`, this improves premultiplied mean diff from `0.0032566` to `0.0012807`, and exact RGBA pixels from `10136539 / 24400000` to `10445084 / 24400000`.
- The largest remaining difference is still localized: reference `[255, 246, 242, 255]` vs loader `[107, 28, 16, 255]` at `(1822, 3328)`. The next boundary is likely group-edge behavior, group masks, or still-unhandled layer data rather than the basic Multiply group model.

Follow-up on the localized real-art difference:

- The worst pixel after group compositing was under `LayerType=0` folders with `LayerComposite=30`. This value appears to be CSP's folder pass-through mode.
- At the worst point, the CSP export matched masked raster layer 204's raw pixel `[255, 246, 242, 255]`, but the decoded mask for that layer was `0`, causing the loader to hide it and reveal a darker lower layer.
- Layer 204 is a `LayerType=3` masked raster under pass-through folders. Layer 29 is also `LayerType=3`, but it lives under a `LayerType=2` Multiply group and still needs its mask applied. `Test_Mask.clip/png` also still requires normal mask application.
- The loader now maps `LayerComposite=30` to `THROUGH` and skips `LayerType=3` mask application only when the masked raster is inside a pass-through `LayerType=0` folder. `Test_Mask` remains bit-perfect.
- On `Test_RealArt.clip/png`, this lowers premultiplied mean diff from `0.0012807` to `0.0000531`, and exact RGBA pixels improve from `10445084 / 24400000` to `10518698 / 24400000`. Median, 90th, and 99th percentile per-pixel premultiplied max diff remain `0.0`.

## Known Bugs in Reference Code

- `csp_tool.py._get_layer_thumbnail` matches `MainId` against the user-supplied `layer_id` but should match `LayerId`. Coincidence in single-layer files masks this. Patched locally for verification; another reason to write our own minimal decoder rather than vendor csp_tool.

## Open Questions

1. **Z-order direction.** Whether `FirstChild → NextIndex` chain is bottom-up or top-down. Verify against PNG ground truth.
2. **Grayscale / monochrome layers.** csp_tool says unsupported. Out of scope for MVP.
3. **Vector / 3D / text layers.** Out of scope for MVP.
4. **Color management.** CSP authoring color space vs Blender scene linear. Defer until MVP add-on lands.
5. **Remaining blend modes.** Some `LayerComposite` values are still unmapped. Unknown integers warn and fall back to Normal until identified.

## Reusable Code

`csp_tool.py` is MIT and works correctly. We can either:
- (a) Vendor it inside the Blender add-on (clean attribution, single dep on numpy + cv2).
- (b) Reimplement only the minimal decode path in pure Python + Pillow (no cv2 dep — Blender ships Python without cv2 by default).

Recommendation: (b) for the add-on, since shipping cv2 inside a Blender add-on is heavy. Use csp_tool.py as the reference implementation; rewrite the ~300 lines we need against numpy + Pillow.
