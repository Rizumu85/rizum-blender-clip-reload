# `.clip` Format Findings

## Current Scope Notice

As of 2026-06-14, the active project scope is raster-only Blender import and verification. Vector strokes/fills, bubble/frame renderers, and text renderers are no longer supported targets in this repository.

Historical vector/native-material notes below are preserved as evidence and rejected-hypothesis history. Do not continue that line unless Rizum explicitly reopens it.

## 2026-06-14 Terra Masked Clipped Preserve Fix

`Ref_Terra404_Live2D` exposed a clipped masked raster layer inside `前髪/アンテナ`: layer `959` (`LayerType=3`, Normal, clipped) had raw alpha `189` at sampled blade pixels, but its layer mask reduced the effective alpha to about `18..19`. The preserve-alpha clipping path used raw source alpha as the recolor strength, so it ignored the layer mask and pushed the blade from the CSP range around `[78,95,102]` to a cyan-biased `[81,144,147]`.

The accepted fix makes clipped preserve compositing use the same source strength model for all blend modes: colour strength is `source_alpha_after_layer_mask * layer_opacity`, deliberately before clip-base attenuation. The clip base still controls the clipped layer's regular alpha/bbox and the preserve-vs-regular choice, but it is not multiplied into the colour blend strength once the layer is inside the base-owned clipping cache. This covers Normal, Add/Add Glow, and non-Normal/non-Add modes, including future masked clipped layers.

Implementation note: the caller now passes `source_alpha_u8` (mask applied, clip not applied) into `_composite_clipped_image()`. This avoids recovering source strength from already-clipped alpha via division, which can amplify uint8 edge quantization. Native evidence is kept in the reverse-engineering workspace, not this importer repo: `E:\Documents\Claude\Projects\rizum-clip-studio-paint\docs\native-evidence\clipped-preserve-mask-strength.md`. The importer-side conclusion is that downstream blend strength must not bypass layer masks or pre-multiply clip strength into the preserve colour pass. Verification on the final rule:

- `Ref_Terra404_Live2D`: `max=255`, `mean=0.034796`; premul `max=4`, `mean=0.022643`, `premul_visible_px=76978`. The remaining full-image max is transparent-RGB/export-convention noise or low-level premultiplied rounding scale, not the prior structural eye/skirt/clipped-highlight error.
- `Test_AddGlowMultiply`: raw `max=5`, `mean=0.067557`; premul `max=3`, `mean=0.062212`.
- `Test_AddGlowMultiplyClipping`: raw/premul `max=1`, `visible_px=0`.
- `Test_ClippingEdge` stayed bit-perfect.
- `Test_ToneCurve` stayed at `max=17`, `mean=0.0182`.
- `Test_Mask` stayed bit-perfect.

## Prior Art (used)

- **Inochi2D / clip-d** ([github.com/Inochi2D/clip-d](https://github.com/Inochi2D/clip-d)): high-level chunk format SPEC.md.
- **LavenderSnek / clipdecode** ([github.com/LavenderSnek/clipdecode](https://github.com/LavenderSnek/clipdecode)): Rust parser; SPEC.md documents `ExternalTableAndColumnName` and `ExternalChunk` mapping.
- **Kazuhito00 / clip_studio_paint_tool** ([github.com/Kazuhito00/clip_studio_paint_tool](https://github.com/Kazuhito00/clip_studio_paint_tool), MIT): practical Python decoder verified against this repo's sample. Returns BGRA per layer at canvas resolution.
- **ctrlcctrlv / libmarugarou** ([github.com/ctrlcctrlv/libmarugarou](https://github.com/ctrlcctrlv/libmarugarou), Apache 2.0): Python `split_clip` reference for chunk walking; based on 2019 Rasen Suihei code.

Conclusion: chunk container, SQLite schema, and tile decode for full-color raster layers are all solved by existing OSS. We do **not** need to re-derive these.

## File Container

```
[CSFCHUNK][8B file_size][8B offset_info]
[CHNKHead ...]
[CHNKExta ...] x N    鈫?raster + vector blobs, one external_id each
[CHNKSQLi ...]        鈫?embedded SQLite database (metadata)
[CHNKFoot ...]
```

All numeric fields big-endian. Each chunk header = 8B magic + 8B size.

## SQLite Tables (verified on sample `Illustration.clip`)

| Table | Role | Sample row count |
|---|---|---|
| `Canvas` | Canvas metadata (width/height, color profile) | 鈥?|
| `CanvasPreview` | Pre-flattened canvas as embedded PNG (`ImageData` blob) | 1 |
| `Layer` | Layer hierarchy: `MainId, CanvasId, LayerName, LayerType, LayerRenderMipmap, LayerRenderThumbnail, LayerNextIndex, LayerFirstChildIndex` | 2 |
| `LayerThumbnail` | Per-layer thumbnail metadata + offscreen ref | 2 |
| `Offscreen` | Per-mipmap pixel data; `BlockData` is the external chunk id (text) | 10 |
| `Mipmap` | Mipmap chain root, refs `BaseMipmapInfo` | 2 |
| `MipmapInfo` | Per-scale entry with `Offscreen` ref | 8 |

Layer types observed: `1` (raster layer), `256` (root / folder).

## Lookup Path (Layer 鈫?Pixels)

```
Layer.LayerRenderMipmap
  鈫?Mipmap.main_id
  鈫?Mipmap.BaseMipmapInfo
  鈫?MipmapInfo.main_id (highest scale entry)
  鈫?MipmapInfo.Offscreen
  鈫?Offscreen.main_id
  鈫?Offscreen.BlockData (= external_id text)
  鈫?CHNKExta chunk whose external_id matches
  鈫?BlockDataBeginChunk records (zlib-compressed 256x256 tiles)
  鈫?assemble tiles 鈫?BGRA at padded (multiple-of-256) size
  鈫?crop to (image_width, image_height)
```

## Tile Format

- Canvas is divided into 256x256 tiles, padded up to next multiple of 256.
- Each tile contributes `256x320x4 = 327680` bytes of decompressed data:
  - First `256x256` bytes = alpha plane.
  - Following `256x256x4` bytes = BGRA plane (B,G,R,A).
- Compression: zlib `deflate` per tile.
- Chunk records inside `CHNKExta`: `BlockDataBeginChunk` (real data), `BlockStatus`, `BlockCheckSum`, `BlockDataEndChunk`. UTF-16-BE block names.

## Sample Verification

Test file: `Illustration.clip` (512x512, single raster layer "Layer 1").
Ground truth: `Illustration.png` (CSP-exported flat PNG).

- **Alpha channel**: 262144 / 262144 pixels identical (100%).
- **RGB channel**, raw compare: 58055 / 262144 (22.15%).
- **RGB channel, alpha-aware** (premultiplied diff): max=0.000, mean=0.0000.

The 22% raw-RGB match is not a real defect: CSP exports transparent pixels as `(255,255,255,0)`, csp_tool decodes them as `(0,0,0,0)`. Both are valid; alpha=0 makes RGB invisible in any composite. After premultiplying alpha (the only thing that affects how the texture renders), the decoded image is **bit-identical** to the ground truth.

**MVP success criterion is satisfied for single-layer files.**

## Compositor Verification (Final)

`clip_loader.py` (project root) implements the full Direction 5 compositor.

| Sample | Canvas | Layers | Max premultiplied 螖 | Mean 螖 | Pixels exact |
|---|---|---|---|---|---|
| `Illustration.clip`   | 512虏 | 1 raster | **0.000000** | 0.0 | 100.0000% |
| `Illustration4K.clip` | 4096虏 | 3 raster | **0.007151** | 5.8 x 10鈦烩伔 | 99.9764% |

The 290 mismatched pixels in the 4K sample (0.0017%) all differ by < 2/255 in any channel 鈥?sub-perceptual float32 rounding from alpha-over compositing.

**MVP success criterion satisfied.** Decoded output `Illustration4K_decoded.png` is in the project root for visual comparison.

## 4K Sample Findings (`Illustration4K.clip`)

- **Canvas**: 4096 x 4096
- **`CanvasPreview`**: 1024 x 1024 (downsampled to 1/4 each axis). **Conclusion: CanvasPreview is NOT a viable shortcut on real-world canvases 鈥?we must build a compositor.**
- **Layers**: 3 raster layers (`Layer 1/2/3`, MainId 3/5/6) under one root folder (MainId 2, LayerType 256).
- **Per-layer pixel storage**: every layer's `LayerThumbnail.ThumbnailCanvasWidth/Height` = 4096x4096. No bbox cropping. Compositing is straight pixel-aligned alpha-over.
- **Mipmap chain**: each layer has 7 scales (100/50/25/12.5/6.25/3.125/1.5625%). Useful for future low-res preview mode.
- **Decode speed (sandbox, naive numpy)**: ~5鈥? s/layer at 4K. Optimisable, but acceptable for an import-on-load workflow.

## Compositing Fields (Layer table)

| Column | Used for |
|---|---|
| `LayerType` | 1 = raster, 256 = root folder, 1584 = paper/background color layer |
| `LayerVisibility` | 0/1 鈥?skip layer if 0 |
| `LayerOpacity` | 0鈥?56 integer (256 = fully opaque). Divide by 256 for 0.0鈥?.0 |
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
- Four `LayerType=2` rows (`琚栧ゥ`, `琚栧乏`, `鏁扮彔锝媊, `鏁扮彔锝媊) have child layers and `LayerComposite=2`. Treating them as ordinary folders made the real-art diff worse (`mean 0.0101284`, exact `9995796 / 24400000`) because the group blend/mask behavior is not equivalent to flattening children directly.
- Conclusion: `LayerType=2` appears to be a group/layer folder that requires offscreen group compositing with the folder's composite mode (Multiply in this sample) before blending back into the parent stack. Keep it unsupported until group compositing is implemented, rather than flattening it incorrectly.

Additional `LayerType=2` group validation:

- `LayerType=2` offscreen group compositing is now implemented. Child layers render into a transparent buffer, the buffer is quantized back to an RGBA source, then it is blended into the parent stack through the group row's `LayerComposite` and `LayerOpacity`.
- On `Test_RealArt.clip/png`, this improves premultiplied mean diff from `0.0032566` to `0.0012807`, and exact RGBA pixels from `10136539 / 24400000` to `10445084 / 24400000`.
- The largest remaining difference is still localized: reference `[255, 246, 242, 255]` vs loader `[107, 28, 16, 255]` at `(1822, 3328)`. The next boundary is likely group-edge behavior, group masks, or still-unhandled layer data rather than the basic Multiply group model.

Follow-up on the localized real-art difference:

- The worst pixel after group compositing was under `LayerType=0` folders with `LayerComposite=30`. This value appears to be CSP's folder pass-through mode, but it was not the root cause of the pixel error.
- At the worst point, the CSP export matched masked raster layer 204's raw pixel `[255, 246, 242, 255]`, while the loader's decoded mask value was `0`, causing the layer to be hidden and a darker lower layer to show through.
- The mask is enabled in CSP. The actual bug was that `_parse_exta()` filled omitted `exist=0` mask chunks with black (`0`). The mask offscreen attribute contains `InitColor = 0xffffffff`, so omitted mask chunks must default to white (`255`, fully visible).
- The loader now reads the mask offscreen `InitColor` and uses it as the fill byte for omitted single-channel mask chunks. `Test_Mask` remains bit-perfect.
- On `Test_RealArt.clip/png`, this lowers premultiplied mean diff from `0.0012807` to `0.0000324`, and exact RGBA pixels improve from `10445084 / 24400000` to `10520542 / 24400000`. Median, 90th, and 99th percentile per-pixel premultiplied max diff remain `0.0`.

Follow-up on the next worst real-art pixel:

- The next worst point was `(2210, 1506)`: reference `[190, 198, 217, 255]`, loader `[4, 3, 3, 255]`.
- Raw layer inspection showed the reference color exists on layer `375` (`鑰冲ゥ楂紮`) and inside group `376` (`鏁扮彔锝媊) under folder `363` (`銈ゃ兂銉娿兗`). Layer `375` has `LayerClip=1`.
- The immediate lower siblings before layer `375` are `LayerComposite=30` / `THROUGH` folders whose effective alpha at that pixel is `0`. Treating those through folders as clipping bases incorrectly hides layer `375`.
- The loader now renders `THROUGH` folders directly into the parent buffer but clears the clipping target after such a folder. This matches the observed CSP output for the pixel and improves `Test_RealArt` to premultiplied mean diff `0.00000786`, max diff `0.5333`, and exact RGBA pixels `10524499 / 24400000`.

Follow-up on consecutive clipping:

- The next worst point was `(1970, 977)`: reference `[255, 255, 255, 255]`, loader `[187, 119, 119, 255]`.
- Raw layer inspection showed the reference color exists on layer `509` (`姝痐), with `LayerClip=1`. The previous sibling `508` (`鑸宍) is also clipped and has alpha `0` at that pixel; both should clip to the same non-clipped base layer `507` (`鍔规灉`).
- The loader now keeps a separate clipping base alpha. Non-clipped layers update the base; clipped layers use that base but do not replace it. `THROUGH` folders still clear the base at their boundary.
- On `Test_RealArt.clip/png`, this lowers the premultiplied max diff from `0.5333` to `0.1490`, mean diff from `0.00000786` to `0.00000344`, and exact RGBA pixels improve from `10524499 / 24400000` to `10524997 / 24400000`.

Follow-up on `LayerType=0` folder blend modes:

- The next worst point was `(1768, 1314)`: reference `[200, 115, 110, 255]`, loader `[218, 151, 148, 255]`.
- Raw layer inspection showed folder `314` (`鍓嶉锝媊) has `LayerType=0` and `LayerComposite=2` (`MULTIPLY`). The previous implementation treated every non-group folder as direct traversal unless it was `THROUGH`, so this folder's Multiply mode was ignored.
- The loader now treats `LayerType=0` folders as pass-through only when their blend mode is `THROUGH`. Other folder modes are rendered to an offscreen buffer, then composited through the folder row's own blend mode, opacity, mask, and clipping flag.
- On `Test_RealArt.clip/png`, this aligns the point exactly and lowers the premultiplied max diff from `0.1490` to `0.1117`, mean diff from `0.00000344` to `0.00000282`, and exact RGBA pixels improve from `10524997 / 24400000` to `10525646 / 24400000`.
- The next largest remaining difference is a semi-transparent clipping edge at `(1842, 1013)`: reference `[203, 157, 149, 245]`, loader `[218, 176, 173, 253]`. The involved layers are `451` (`杓儹銉欍兗銈筦) and clipped layer `452` (`杓儹鍔规灉`). Table fields do not yet expose an obvious "base hidden" or special alpha flag, so this should be investigated separately before changing clipping edge semantics.

Follow-up on clipped edge alpha inside offscreen folders:

- Visual inspection showed the remaining `(1842, 1013)` difference was a very small color/alpha mismatch on the face outline edge rather than a structural layer-order problem.
- At that point, folder `450` (`杓儹`) is rendered to an isolated offscreen buffer. Its base layer `451` has raw `[198, 198, 198, 191]`, and clipped layer `452` has raw `[231, 177, 175, 255]`. The old generic clipping path composited both as normal layers, producing offscreen `[224, 181, 180, 239]` and final `[218, 176, 173, 253]`, while CSP's export is `[203, 157, 149, 245]`.
- CSP is better approximated there by letting the clipped layer recolor the clipping base while preserving the base edge alpha. Applying that rule globally is wrong: a lower-body point `(1966, 3577)` under `瓒宠。瑁卄 has a base alpha of only `6` over already opaque artwork, and global alpha preservation incorrectly recolors it to the clipped layer color.
- The loader now uses a hybrid clipped-layer path only inside isolated folder/group buffers: if the current destination alpha is effectively the clipping base alpha at that pixel, preserve alpha and repaint color; otherwise use the previous product-alpha clipping path. This keeps opaque-underpaint clipping cases stable.
- On `Test_RealArt.clip/png`, this changes `(1842, 1013)` to `[203, 156, 148, 246]` (within 1/255 of the CSP export), keeps the previously fixed `(1768, 1314)`, `(1970, 977)`, and `(2210, 1506)` points exact, and lowers the full-image premultiplied max diff from `0.1117` to `0.0980`. Mean diff is `0.00000585`; exact RGBA pixels are `10525908 / 24400000`.

Follow-up on layer visibility bit flags:

- `Ref_Wuwu_Live2D.clip/png` exposed large extra visible regions in the loader output: a pink necklace and dark Multiply patches on both wings.
- Layer tracing showed these came from `LayerType=2` Multiply groups: `15` (`琚栧ゥ`), `26` (`琚栧乏`), and `311` (`鏁扮彔锝媊). The problematic groups/layers had `LayerVisibility` values `2` or were downstream of layers with value `2`.
- CSP's `LayerVisibility` is a bit field, not a simple integer boolean. Bit 0 is the eye/visible state: `0` and `2` are hidden; `1` and `3` are visible. The previous loader only skipped exact `0`, so it incorrectly rendered hidden value-`2` layers.
- The loader now uses `(LayerVisibility & 1) != 0` everywhere visibility is checked, including paper layers, hierarchy flattening, and recursive compositing.
- On `Ref_Wuwu_Live2D.clip/png`, this removes the large extra regions and improves the full-image comparison to max premultiplied diff `0.007843`, mean `0.00000408`, and exact pixels `24374657 / 24400000`. The older `Test_RealArt.clip/png` result remains unchanged at max `0.0980`, mean `0.00000585`, exact `10525908 / 24400000`.

Follow-up on root-level clipping edges:

- `Test_ClippingEdge.clip/png` and `Test_ClippingEdge4K.clip/png` isolate the clipped-layer edge behavior outside an offscreen folder/group.
- CSP preserves the clipping base layer's edge alpha while repainting color from the clipped layer according to the clipped layer's own alpha. The previous root-level path only used product-alpha clipping and normal alpha-over, which made edge alpha too high.
- The isolated-folder hybrid clipping path was already a good approximation, so the loader now enables that path for root-level clipped layers as well.
- `Test_ClippingEdge.clip/png` improves to max premultiplied diff `0.003906`, mean `0.000000285`, with the previously worst edge pixels exact.
- `Test_ClippingEdge4K.clip/png` improves to max premultiplied diff `0.003568`, mean `0.000000128`, with the sampled edge pixels exact.
- The `Test_RealArt.clip/png` summary remains stable after this change: max `0.0980`, mean `0.00000585`; previously fixed points `(1970, 977)`, `(2210, 1506)`, and `(1768, 1314)` remain exact.

Follow-up on clipped Add Glow:

- `Ref_Emuri_Live2D_2024.clip/png` exposed a bright violin/hair-ornament region where the loader was too dark: max premultiplied diff `0.305882`, mean `0.0000574`.
- Layer tracing at `(2217, 268)` showed the stack under `*楂＞銈?> 銉愩偆銈儶銉砢: normal base layer `220`, clipped `ADD_GLOW` layer `222`, clipped `MULTIPLY` layer `223`, then clipped `ADD_GLOW` layer `224`.
- The hybrid clipped-layer path was still using generic blend interpolation for `ADD` / `ADD_GLOW`. CSP's isolated Add Glow samples already showed that Add Glow should add `src * alpha` into the destination with clamping.
- The loader now uses that additive color update inside the clipped hybrid path while preserving the clipping base alpha.
- On `Ref_Emuri_Live2D_2024.clip/png`, this lowers max premultiplied diff to `0.168627`, mean to `0.0000187`, and makes sampled bright points `(2217, 268)`, `(2290, 242)`, and `(2209, 90)` exact.
- `Test_ClippingEdge.clip/png` and `Test_ClippingEdge4K.clip/png` remain stable after the Add Glow change, and `Test_RealArt.clip/png` remains stable at max `0.0980`, mean `0.00000585`.

Follow-up on Add Glow + Multiply clipping stacks:

- `Test_AddGlowMultiply.clip/png` isolates a stack with a Normal base, a non-clipped `ADD_GLOW` layer, then clipped `MULTIPLY` and clipped `NORMAL` layers.
- The current loader output has max premultiplied diff `0.335840`, mean `0.028145`; the worst sampled point is reference `[187, 188, 237, 248]` vs loader `[106, 188, 146, 253]`.
- Pixel tracing shows the loader first composites the non-clipped Add Glow layer into the global output, then applies the clipped Multiply and Normal layers over that global output. This makes the clipped layers too destructive.
- A prototype that first composites the Add Glow base and its clipped siblings into an isolated clipping group, then blends that group back through the Add Glow base mode, improves the sample to max `0.207828`, mean `0.012130`, and changes the sampled point to `[187, 184, 255, 248]`.
- That prototype proves the remaining issue is at least partly structural, but it still oversaturates blue. Do not land the grouping rewrite until clipped Multiply / Normal strength inside the group is understood from another targeted sample or a stronger formula.

Follow-up on `Ref_Terra404_Live2D`:

- `Ref_Terra404_Live2D.clip/png` is a new large real-art sample that exposes a different failure shape than `Ref_Emuri_Live2D_2024`.
- A targeted comparison with the current loader showed max premultiplied diff `1.000000`, mean `0.010291082`, exact pixels `6564474 / 29280000`.
- The worst sampled point is reference `[255, 255, 255, 0]` vs loader `[177, 165, 197, 255]` at `(1845, 1990)`, which means the loader is drawing opaque content where CSP exports full transparency.
- This should be investigated as a structural visibility/mask/group/background issue before tuning color formulas; the error is not a small blend rounding mismatch.
- Pixel tracing showed the opaque pixel came from layer `489` (`钀藉奖`), a `LayerType=3` layer with `LayerLayerMaskMipmap=515` but `LayerMasking=0`.
- Decoding that mask mipmap directly showed mask value `0` at `(1845, 1990)`. The loader previously ignored the mask because it gated mask decoding on `LayerMasking`; `LayerType=3` can carry an active mask mipmap even when that flag is `0`.
- The loader now applies a layer mask whenever `LayerLayerMaskMipmap` is present. `Test_Mask.clip/png` remains bit-perfect, and the Terra sampled point now decodes as transparent `[0, 0, 0, 0]`.
- Full-image follow-up then exposed `(2162, 1449)`: reference `[41, 4, 9, 255]` vs loader `[234, 204, 218, 255]`.
- Pixel tracing showed the loader produced the correct dark value after folder `540` (`绶歚), then overwrote it through `LayerType=2` group `545` (`褰盽) with `LayerComposite=30` (`THROUGH`). Rendering `LayerType=2` THROUGH children directly into the parent buffer makes the dark-line points exact, but also bypasses the group mask and leaks opaque pixels outside the masked region.
- The loader now renders THROUGH children into the parent, then blends the before/after contribution back through the THROUGH group's own mask and opacity. Sampled Terra points now match both sides: mask-inside dark-line points `(2162, 1449)` and `(2636, 1449)` decode as `[41, 4, 9, 255]`, while mask-outside points `(2162, 1453)`, `(2165, 1458)`, and `(2587, 1597)` remain transparent.
- A later follow-up exposed clipped Add Glow over-brightening around `(2397, 559)`: reference `[71, 68, 80, 255]` vs loader `[144, 170, 177, 255]`. The involved clipped `ADD_GLOW` layer had high raw alpha but an effective layer alpha near zero after mask/clip application.
- The clipped preserve path now keeps raw source alpha for Normal/other blend recoloring, but uses effective layer alpha for `ADD` / `ADD_GLOW` strength. This keeps `Test_ClippingEdge.clip/png` at max premultiplied diff `0.003906`, `Test_ClippingEdge4K.clip/png` at max `0.003568`, preserves exact `Ref_Emuri_Live2D_2024` sampled Add Glow points, and reduces the Terra sampled point `(2397, 559)` to `[75, 114, 124, 255]`.
- Full-image Terra follow-up after the mask, THROUGH, and clipped Add Glow fixes: max premultiplied diff `0.349019617`, mean `0.000395088`, exact pixels `7054122 / 29280000`. Previously fixed samples remain exact: `(1845, 1990)`, `(2162, 1449)`, `(2636, 1449)`, and `(2162, 1453)`. The next worst point is `(2190, 1319)`, reference `[223, 164, 201, 255]` vs loader `[154, 75, 137, 255]`.
- Single-pixel tracing at `(2190, 1319)` shows `Group 584` is not leaking its Multiply layer: the group mask is `0` at this pixel. The visible darkening happens later in `Group 605` / folder `610` (`绶歚). There, two low-alpha non-clipped dark layers (`611` alpha `3`, then `612` alpha `100`) establish a clipping base, and clipped color layers `613`-`616` follow.
- The old clipped preserve threshold was `clip_base + 1.5/255`. Because layer `611` raises the destination alpha from `100` to `102`, the clipped color layers missed the preserve path by half a quantization step and used regular compositing, raising the local folder alpha to about `199` and darkening the bright parent color to `[154, 75, 137]`.
- The preserve threshold was first widened to `clip_base + 2.0/255`. A full-image Terra follow-up confirmed `(2190, 1319)` moved to near-exact: reference `[223, 164, 201, 255]` vs loader `[223, 163, 202, 255]`. Overall Terra summary became max premultiplied diff `0.345098078`, mean `0.000394923`, exact pixels `7054141 / 29280000`.
- The next Terra worst point was `(2287, 1311)`, reference `[203, 139, 186, 255]` vs loader `[137, 51, 125, 255]`. Single-pixel tracing showed the same folder `610` mechanism at a slightly higher alpha: layer `611` alpha `4`, layer `612` alpha `117`, then clipped layers `613`-`616`. The `2.0/255` threshold still missed preserve; `2.25/255` is enough for a targeted scalar replay to move the point toward `[214, 139, 200]`.
- The preserve threshold is now `clip_base + 2.25/255`. Regression samples remain stable: `Test_Mask` is bit-perfect, `Test_ClippingEdge` remains max `0.003906`, `Test_ClippingEdge4K` remains max `0.003568`, and the known `Ref_Emuri_Live2D_2024` Add Glow points remain exact. The next full-image Terra run should confirm the new worst point after the `2.25/255` adjustment.
- Mask offscreen-offset follow-up: corpus scanning found one semantic mask offset so far, `Ref_Terra404_Live2D` Layer `371` (`鐧哄厜`) with `LayerMaskOffscrOffsetY=-256`. Its mask offscreen is `4864x6400`, not the canvas `4800x6100`; the old decoder used canvas dimensions as the expected tile length and therefore cropped the wrong offscreen rows. Mask decode now uses the offscreen's own pixel size, then pastes/crops it to canvas through `LayerMaskOffscrOffsetX/Y`. Ordinary tile-padded masks with offset `0` remain equivalent; `Test_Mask` stays bit-perfect and `test_Filters_Vector_Text` remains max `225` / mean `0.577998` / visible `4.636955%`. A full Terra verification is still too slow for routine checks, but Layer 371's local mask placement now follows the stored offscreen geometry.
- Render offscreen-offset follow-up: the same corpus sweep found real render offscreen origins in `Ref_Terra404_Live2D` base layers: Layer `4` has render offscreen `5876x6100` with `LayerRenderOffscrOffsetX=-768`, while Layers `5` and `6` have `6080x6100` with `LayerRenderOffscrOffsetX=-1280`. `Test_AddGlowMultiplyClipping` also has one render offscreen with an extra tile row (`1024x1280`, offset `0`). Raster decode now mirrors the mask logic: decode the render mipmap at its offscreen pixel size, then crop/paste through `LayerRenderOffscrOffsetX/Y`, with a fast top-left crop for ordinary offset-0 tile padding. Local Terra checks show Layers `4/5/6` now return canvas-sized RGBA images from their negative-X origins.

## Known Bugs in Reference Code

- `csp_tool.py._get_layer_thumbnail` matches `MainId` against the user-supplied `layer_id` but should match `LayerId`. Coincidence in single-layer files masks this. Patched locally for verification; another reason to write our own minimal decoder rather than vendor csp_tool.

## New Sample Baselines (2026-05-04)

Measured with the current compositor (v0.8.22, clipped preserve threshold `2.25/255`):

| Sample | Canvas | Layers | Premul max 螖 | Mean 螖 | Exact pixels |
|---|---|---|---|---|---|
| `Ref_Kabi_Live2D.clip` | 2894x4093 | 839 | **233.0** | 0.0101 | 98.49% |
| `Ref_MXL_Idol1.clip` | 5877x8326 | 719 | **65.0** | 0.0556 | 95.25% |
| `Ref_绫音Aya_Live2D.clip` | 3288x6176 | 303 | **16.0** | 0.0061 | 99.17% |
| `Test_AddGlowMultiply.clip` | 4096x4096 | 5 | **85.6** | 4.47 | 74.68% |

## `LayerFolder` Field Discovery

The `Layer` SQLite table has a `LayerFolder` integer column, previously unused. It is orthogonal to `LayerType`:

- **`LayerFolder=0`**: raster-like layers (type 1, 3)
- **`LayerFolder=1`**: organizational folders
- **`LayerFolder=17`**: layer folders (special compositing behaviour)

Distribution across samples:

| Sample | LayerFolder=1 (type=0) | LayerFolder=17 (type=0) |
|---|---|---|
| Kabi | 58 | 120 |
| MXL | 2 | 121 |
| Aya | 15 | 54 |
| RealArt | 18 | 64 |

A global sort (`LayerFolder=17` first / rendered below, `LayerFolder=1` last / rendered on top) was tested on Kabi. It improved the previously-worst dark pixel `(1461,1158)` from `[22,21,21]` to `[251,245,242]` (ref `[255,212,205]`), but caused widespread new white-pixel errors (mean diff 0.010鈫?.0, exact 98.49%鈫?4.05%). The sort must be scoped more narrowly 鈥?likely only within specific parent contexts.

## Kabi Layer Ordering Root Cause

Pixel `(1461, 1158)`: reference `[255,212,205,255]`, loader `[22,21,21,255]`.

The compositor trace shows the correct layer stack rendering bottom-up:

1. L3 (搴曡壊, Normal, `[194,193,193,255]`) 鈫?canvas gray
2. Folder 47 (澶寸罕濂? 鈫?shadow layers darken canvas
3. Folder 54 (浣? 鈫?L73 (`[217,181,226,255]`) brightens
4. **Folder 107** (琛ㄦ儏, `LayerFolder=1`, inside folder 100) 鈫?L143 (`[251,245,242,255]`) near-white, composites to canvas `[0.984, 0.961, 0.949, 1.0]`
5. **Folder 232** (鏄庢殫绶? `LayerFolder=17`, inside folder 100) 鈫?L259 (`[0,0,0,101]`) dark overlay, composites to canvas `[0.086, 0.082, 0.082, 1.0]`

In CSP's bottom-up chain, folder 232 (NextIndex=265) renders AFTER folder 107 (NextIndex=149), so dark content appears on top 鈥?covering the bright layers. The CSP reference however shows the bright content `[255,212,205]`, meaning the dark overlay is either masked out at this pixel in CSP, or CSP's layer ordering differs from the chain order within certain folder contexts.

**Neither reversing the chain nor treating type=0 comp=0 folders as pass-through fixes this** 鈥?both approaches cause large regressions in Aya, RealArt, and other samples.

## MXL ADD Layer Over-Brightening

Historical pixel `(2484, 3492)`: reference `[190,73,107,255]`, older loader trace `[255,73,107,255]`.

Only the red channel differs. The compositor trace shows:

1. L3 (base) 鈫?canvas `[255,73,107]`
2. Multiply/darken layers 鈫?reduce to `[44,24,27]`
3. Several normal layers 鈫?various intermediate colors
4. **L432** (hl3, ADD) 鈫?brightens `[0.557, 0.098, 0.137]` 鈫?`[0.690, 0.219, 0.208]`
5. **L434** (hl, ADD) 鈫?`[0.690, 0.219, 0.208]` 鈫?`[1.000, 0.285, 0.420]` = `[255,73,107]` 鈥?exactly L3's base color

Follow-up with the current `RenderBlendModeCall`-derived `_blend_add_u8()` shows this is no longer an open mask/folder issue. At the point:

- L432 (`hl3`) has raw pixel `[255,232,136,34]`, decoded mask `255`, and effective alpha `34`.
- L434 (`hl`) has raw pixel `[255,49,158,87]`, decoded mask `255`, and effective alpha `87`.
- Both layers are ordinary ADD layers with `LayerClip=0` under folder `431`.

Starting from the recorded pre-highlight state `[142,25,35,255]`, current `_blend_add_u8()` gives:

1. L432 -> `[157,55,53,255]`
2. L434 -> `[190,71,106,255]`

That is within `[0,-2,-1,0]` of CSP's reference `[190,73,107,255]`. The older `[255,73,107]` trace came from the former simplified `dst + src * alpha` ADD approximation, which overfilled red. Keep the current internal `0x201` ADD branch; the remaining difference is a tiny quantization/order residual, not evidence for a hidden mask or folder limiter at this pixel.

## Type=0 Folder Semantics (Verified)

**Confirmed through regression testing:**

- Type=0 folders MUST render offscreen (not pass-through) when their composite mode is not THROUGH. Pass-through variants (all-comp-0, LayerFolder-based, NORMAL-only) all cause regressions in Aya (max diff 16鈫?7 or worse) and/or RealArt (max diff 25鈫?8).
- The offscreen buffer approach is structurally correct. The historical Kabi ordering and MXL ADD cases are now retired as later blend/offscreen fixes rather than folder-model failures. Remaining per-sample issues should be treated as localized layer interactions, not a reason to replace the folder model wholesale.
- Type=0 THROUGH folders are handled correctly by `_render_through_group` with mask/opacity blending and clip chain reset.

## AddGlowMultiply Current State

Current follow-up supersedes the earlier structural diagnosis: the current importer is close on meaningful pixels, and the scary full-RGBA metrics are mostly transparent RGB differences.

- Layer stack: L5 (Normal base), L6 (Add Glow, non-clipped), L7 (Multiply, clipped), L8 (Normal, clipped)
- Reference: `[187,188,237,248]`, Loader: `[106,188,146,253]`
- The isolated-group prototype gives `[197,204,255,253]` 鈥?close but blue-oversaturated (255 vs 237)

The historical trace above is retained as context for why the isolated clipping group path was implemented. Current checks:

- `Test_AddGlowMultiply.clip`: after matching CSP's white transparent-RGB export convention, ordinary RGBA diff is no longer dominated by fully transparent pixels (`max=117`, `mean=0.067933`, `transparent_rgb_visible_px=0`). On alpha-union pixels the max is `117` / mean `0.128411`; on high-alpha pixels max is `3` / mean `0.160755`; premultiplied RGBA max is `3` / mean `0.062168`.
- `Test_AddGlowMultiplyClipping.clip`: after the same final-output transparent RGB rule, ordinary RGBA diff is only `max=1` / `mean=0.027135`, `visible_px=0`, with `premul_max=1` and `premul_visible_px=0`.
- Keep the current clipping-group path: a non-Normal, non-clipped base plus clipped siblings is rendered as an isolated group and blended back through the base layer's mode. Future validation for these samples should use premultiplied/high-alpha stats when judging visible blend behavior; raw straight-RGBA now mostly reflects real low-alpha or high-alpha residuals rather than transparent metadata.

Verification tooling follow-up: `verify_one_clip.py` now reports alpha-aware metrics in addition to the legacy raw straight-RGBA fields. New fields include `premul_max`, `premul_mean`, `premul_visible_px`, alpha-union/high-alpha masked stats, and transparent-RGB counters. This keeps old output compatible while making transparent-RGB-only differences visible as such. The later final-output transparent RGB rule now brings `Test_AddGlowMultiplyClipping` down to raw `max=1`, `visible_px=0`, and `transparent_rgb_visible_px=0`, while the premultiplied fields remain the meaningful blend check.

Memory follow-up: `_premul_to_rgba_u8()` now converts premultiplied buffers to uint8 straight RGBA one channel at a time instead of allocating both a full 3-channel float RGB buffer and a full 4-channel float RGBA buffer. This reduces peak memory during nested offscreen-folder compositing. The previously failing full Kabi verification now completes in `167.046s` with raw/premultiplied `max=53`, `mean=0.007183`, and `visible=0.033001%`.

Older-schema fallback follow-up: `Ref_Terra404_Live2D` exposed a `LayerType=0` leaf path where `_text_cache_fallback_image()` was called on a SQLite `Layer` row without a `TextLayerAttributes` column. The fallback now checks the column set and returns `None` when the schema has no text-cache payload, letting the ordinary vector/skip paths continue. A full Terra verification now gets past that crash but still exceeds the 600s local timeout, so the remaining Terra full-image item is still a performance/long-run validation task.

THROUGH-group performance follow-up: `_render_through_group()` now skips the expensive full-canvas `before = out.copy()` path when the THROUGH folder has opacity `100%` and no layer mask. This is semantically equivalent because there is no folder-level blend-back to apply. The guarded path still handles masked/opacity THROUGH folders, including Terra's critical masked groups (`褰盽, `闋儴`, `鐬冲彸`, `鐬冲乏`). Corpus counts show `Ref_Terra404_Live2D` has `116` visible THROUGH folders with children, and `108` can use the fast path; the `8` masked groups stay on the old path. The final `_composite_recursive()` output conversion now also reuses the optimized channel-wise `_premul_to_rgba_u8()` instead of allocating a second full-canvas float RGB/RGBA pair.

Rejected performance shortcut: direct-rendering "pure" NORMAL folder subtrees into the parent buffer was tested and removed. Even when every descendant is Normal, full opacity, unmasked, and unclipped, the shortcut skips the folder boundary's straight-RGBA quantization step. On `Ref_Kabi_Live2D`, this changed metrics from `mean=0.007183` / `visible=3909` to `mean=0.007272` / `visible=3526`. Because it changes pixels and CSP appears to quantize at these group boundaries, keep NORMAL folders on the isolated offscreen path until a quantization-preserving cropped group renderer is implemented.

Group conversion performance follow-up: isolated folder/group rendering still uses full-canvas `group_out`, preserving existing group-boundary quantization. The conversion back to straight RGBA is now cropped to the group's alpha bbox before applying the folder/group mask and compositing back into the parent buffer. This is equivalent because bbox-outside alpha is zero and would not composite. `Ref_Kabi_Live2D` stays at `max=53`, `mean=0.007183`, `visible=0.033001%`, while the local run time improved from about `179s` to `135s` in the same session. `Ref_Terra404_Live2D` still exceeds the 600s timeout, so the remaining performance target is a true cropped isolated group buffer that keeps the same quantization semantics while avoiding full-canvas `group_out` allocations.

## GLOW_DODGE on Transparent Backgrounds Fix (2026-05-04)

GLOW_DODGE on a fully transparent destination produced `[0,0,0,alpha]` 鈥?invisible colour 鈥?because Color Dodge with `dst=0` always yields zero. CSP Glow Dodge is documented as "stronger in semi-transparent areas," meaning it should still show source colour on transparent backgrounds.

**Fix:** When `dst_a < 1/255` (effectively transparent), the GLOW_DODGE output adds the source's own premultiplied colour contribution. On opaque or semi-transparent backgrounds, the original Color Dodge formula is preserved unchanged.

**Validation (no regressions):**

| Sample | Before fix | After fix |
|---|---|---|
| `Test_GlowDodge.clip` | 100% exact | 100% exact |
| `Test_RealArt.clip` | max鈮?5, mean鈮?.0015 | max=25, mean=0.00085 |
| `Ref_Aya_Live2D.clip` | max=16, mean=0.0061 | identical |
| `Ref_Kabi_Live2D.clip` | max=233, mean=0.010 | **max=112, mean=0.010** |
| `Test_AddGlowMultiplyClipping.clip` | max=1 | max=1 |

The Kabi improvement is concentrated at previously-worst pixels: `(1461,1158)` went from `[22,21,21]` (near-black) to `[255,239,252]` (ref `[255,212,205]`), and the offscreen-buffer darkening at `(1444,1166)` became pixel-exact.

The fix also exposed a tile-grid auto-detection improvement: when a layer's tile blob has more tiles than expected from `ThumbnailCanvasWidth/Height`, the tile grid is now inferred from the actual blob size rather than erroring.

## Open Questions

## Filter Layer Follow-Up (2026-05-05)

`test_Filters_Vector_Text.clip` is currently the only supplied clip with non-null `Layer.FilterLayerInfo`. Type `2` Level Correction payload is a compact big-endian 16-bit table: the sample has 32 records of five `uint16` values, and only record 0 differs from identity: `(0, 32767, 65535, 0, 42130)`. Single-filter CSP exports confirm the record order is `(input low, mid, input high, output low, output high)`, not `(input low, input high, mid, ...)`; with that correction the pure Level Correction export is max=1 / mean=0.005667 / visible=0.

Tone Curve type `3` payload length is 4160 bytes, now parsed as 32 compact curves of `0x82` bytes (`uint16 count + 32 uint16 point pairs`). In the current sample, only compact curve 0 is non-identity: `(0,0) -> (16075,47014) -> (60236,35459) -> (65535,65535)`; the remaining compact curves are identity. Ghidra confirms CSP builds the table through `CreateLookUpTableToneCurveStatic -> rtGetBsplineIntTable/GetBsplineIntTable`, a quadratic B-spline sampled into a 256-entry integer LUT. The importer therefore handles the observed `master curve + identity rest` subset by generating the LUT in 16-bit coordinate space before byte quantization.

A later Ghidra pass confirms the visible Tone Curve call chain is operating on the already-expanded runtime structure, not the SQLite compact blob. `CSAdjustmentLayer::ReadSelf @ 0x122bee00` and `WriteSelf @ 0x122c4aa0` read/write an archive layout of `int32 count + tagPOINT[32]`, with each point stored as two 32-bit integers and each curve block sized `0x104`; `CreateLookUpTableToneCurveChannel @ 0x122fdbc0` consumes four such blocks at offsets `0x0 / 0x104 / 0x208 / 0x30c` for master/R/G/B and merges channel LUTs with the master. The SQLite payload is still `32 * 0x82`, so it is not just the runtime `4 * 0x104` structure copied verbatim. Unknown payloads with non-identity extra compact curves still need more samples before the channel semantics can be generalized safely.

Gradient Map type `9` payload starts with header `(220, 24, 28, 7, 16, 0, 3)` followed by seven 28-byte nodes. The first three words of each node are repeated 16-bit RGB components, and the sixth word is the stop position. Without adjustment-layer masks this worsened the mixed sample, but after mask/opacity support a first-pass Gradient Map implementation improves `test_Filters_Vector_Text` to max=237 / mean=17.654859 / visible=28.84%. Ghidra `CreateLookUpTableGRAD @ 0x122be240` confirms CSP builds per-channel LUTs from 12-byte runtime nodes `(uint32 color, float stop, uint32 extra)` and compares input positions as `i / 255.0`. The single-filter Gradient Map export shows CSP is closer to CSP-style grayscale coefficients `0.30/0.59/0.11` than channel-average input, and SQLite node colors are best converted from 16-bit to 8-bit with `/256` rounding. Scaling SQLite stops by `32768 * 256 / 255` matches the 256-entry LUT edge convention better than raw `/32768`. With those corrections the pure Gradient Map export improves from max=20 / mean=0.342189 to max=8 / mean=0.052949 / visible=0.200462%, and after the latest bubble geometry retune the current mixed sample is max=225 / mean=0.579766 / visible=4.638767%.

Brightness/Contrast type `1` payload stores brightness and contrast as signed 32-bit values. The `test_Filters_Vector_Text` layer has brightness `+91` and contrast `0`. The earlier LUT treated positive brightness as a white-point compression, turning white into `164`; CSP behavior keeps white clipped at white. Switching brightness to a simple additive clamp improves the mixed sample from max=225 / mean=59.590587 / visible=87.58% to max=255 / mean=57.201253 / visible=47.61%. Regression spot-checks remain stable: `Test_AddGlow` max=1 / visible=0, `Test_ColorBurn` exact, and `Test_Brightness` max=1 / visible=0.

Adjustment/filter layers use their own `LayerLayerMaskMipmap` as effect strength. After the brightness clamp fix, applying each supported filter into a temporary buffer and blending it back through the filter layer's mask/opacity improves `test_Filters_Vector_Text` again to max=237 / mean=21.515223 / visible=33.16%. Regression spot-checks remain stable: `Test_AddGlow` max=1 / visible=0, `Test_ColorBurn` exact, `Test_Brightness` max=1 / visible=0, `Test_ColorDodge` exact, and `Test_GlowDodge` exact. Unsupported type `3` Tone Curve is still skipped.

Early Tone Curve type `3` retests after filter-mask and Gradient Map fixes tried linear and Catmull-Rom interpretations with multiple coordinate inversion variants; all worsened the mixed sample before the compact-curve parser was corrected. The later single-filter PNG and Ghidra B-spline evidence supersede those attempts.

Text layer `16` stores both a text cache and vector balloon header fields. The vector body header bbox `(136,115)-(468,324)`, stroke color `(70,30,126)`, and fill color `(228,224,237)` match the missing top speech balloon in the reference. Rendering a superellipse fallback behind the cached text improves `test_Filters_Vector_Text` from max=237 / mean=17.654859 / visible=28.84% to max=225 / mean=9.276168 / visible=23.72%. Regression spot-checks remain stable.

The text-balloon fallback was later tuned against the pure `test_Filters_Vector_Text_bubble.png` export and the full mixed sample. CSP's bubble geometry is not a plain ellipse; the bottom-left region around `(164,283)` remains filled in CSP while a `power=2.2`, width-3 superellipse leaves it uncovered. The first useful fallback used `power=2.4` and outline width `5`, improving the mixed render from max=237 / mean=2.363531 / visible=12.13% to max=225 / mean=2.241779 / visible=11.99%. Later passes retuned this geometry again after removing direct rough-line sampling. This is still a shape approximation, not CSP's brush/material balloon renderer.

Text-balloon point-record follow-up: Layer 16's vector body has an 8-point header `(100, 76, 104, 88)`, point count `8`, BrushStyle `6`, following word `4`, and width `2.5`. The 104-byte records decode to an octagonal rounded-balloon skeleton around `(294.001,119.001)`, `(310.001,119.001)`, `(464.001,214.001)`, `(464.001,225.001)`, `(310.001,320.001)`, `(294.001,320.001)`, `(140.001,225.001)`, and `(140.001,214.001)`. A direct polygon/outline prototype using these points regressed the mixed sample badly (best mean about `4.0383`), so these records are control/skeleton data for CSP's balloon/vector renderer, not a drawable preview polygon. Keep the tuned superellipse fallback until the curve/brush renderer is decoded.

Text-frame material/ruler follow-up: SQLite `VectorNormalBalloonIndex` points directly to `VectorObjectList.MainId` (`Layer 16 -> VectorObjectList 9`, `Layer 30 -> VectorObjectList 10`). Ghidra shows the text-frame path wraps this in a dedicated material/ruler pipeline. `CSTextGroupLayer::ReadSelf @ 0x12331e70` reads/creates an internal material layer; when its material type is `0x2000001`, it iterates the material layer ruler objects, sets each ruler `+0x128 = 3`, and writes the text-frame line width from `this+0x2350` into each ruler's `RCLength +0x98`. `CSTextFrameV4Layer::DrawRulerForBitmap @ 0x12329830` then reads the ruler DB at `this+0x348`, uses `+0x98` as line width, extracts ruler points through virtual `+0x70`, transforms them to page/bitmap coordinates, and delegates to the ruler drawing virtuals. `CreateBalloonImage @ 0x12328710` confirms the final preview is generated by temporarily merging the text-frame layer and its internal text layer into an offscreen. Therefore the layer-16 point records are material/ruler control geometry, not the final raster outline.

Vector stroke layer `5` stores real stroke data in repeated 92-byte stroke headers followed by 88-byte point records. Valid headers have `(92, 76, 88, 88)`, flag `0x2081`, a point count, a bbox, a 64-bit stroke width, and 16-bit color components at header offsets `+40/+44/+48`. Rasterizing those polylines as a constrained fallback improves `test_Filters_Vector_Text` again to max=225 / mean=8.463651 / visible=23.65%. This is still a fallback, not full CSP vector rendering; the remaining gap is dominated by exact vector antialiasing/brush behavior and gradation-fill geometry.

The vector fallback was later tuned against `test_Filters_Vector_Text_Vector.png`. The stored stroke width is close to, but slightly larger than, the effective preview radius. Trimming the fallback radius to `0.95 * width` improves the pure vector export from mean=0.313279 / visible=1.12% to mean=0.258262 / visible=0.96%, and improves the mixed sample from max=225 / mean=2.241779 / visible=11.99% to max=225 / mean=2.174049 / visible=11.85%. A soft antialias fringe was tested but regressed the mixed sample because later adjustment filters amplify the edge difference. After the compact Tone Curve and rough-pattern removals, layer-5 point offset `+52` behaves like endpoint taper data, and Catmull-Rom interpolation over the full point stream better matches CSP's curved stroke model. Ghidra `DrawPenHeadCurve` uses a `96.0f` subdivision step after page coordinates are scaled by `16`, and casts sampled points to `LONG` before pen-head rasterization. A later V4 `RenderCurve` pass shows ordinary vector strokes use resolution-dependent curve subdivision, so the current fallback uses a 5px compromise step, truncates sampled coordinates, clamps taper to `0.6..1.0`, and uses `0.95 * width * averaged_taper`; this keeps the isolated vector export effectively unchanged at max=173 / mean=0.172712 / visible=0.832176% and improves the full mixed sample to max=225 / mean=0.577998 / visible=4.636955%.

Single-filter exports also corrected two more filter details. Posterization type `7` uses floor buckets (`bin = x * levels // 256`) mapped to evenly spaced output values; the pure Posterization export is now max=1 / visible=0. Color Balance type `5` stores a preserve-luminosity flag followed by shadow, midtone, and highlight RGB triplets; the sample uses the midtone triplet `(+43, -48, +48)`. The early luminosity-weighted midtone model improved the pure Color Balance export from mean=4.142662 to mean=0.370436, but it has now been replaced by the Ghidra-backed `CreateLookupTablesCB` level-LUT path.

Color Balance was retested against Ghidra `CreateLookupTablesCB @ 0x122be520`. CSP's runtime function builds per-channel level LUTs through `MakeLow`, `MakeMid`, `MakeHigh`, `MakeNormalLevel`, and `MakeLevelTable`; the nonzero first payload integer switches to a preserve-luminosity matrix path. The earlier direct attempt used the wrong level-gamma direction. Reading the constants from `iswCoreTG.dll` gives `MakeMid = 0.5 - ((((a * 2) - b) - c) * 0.3) / 400`, and `MakeLevelTable` uses that midpoint as a gamma point (`gamma = log(0.5) / log(mid_t)`, output `t ** gamma`). For the sample payload `[1, 0,0,0, 43,-48,48, 0,0,0]`, this maps paper color `226,226,226` to about `231,212,232`, matching the isolated CSP export. The pure Color Balance export now verifies at max=1 / mean=0.037455 / visible=0; after the follow-up Gradient Map and bubble retunes, the full mixed sample is max=225 / mean=0.579766 / visible=4.638767%.

Frame-layer follow-up: the missing `extrnlid21594...`, `extrnlidD516...`, and `extrnlidF7DEF...` values are not hidden `CHNKExta` chunks; they only appear inside the SQLite `Offscreen.BlockData` rows for the frame render, comic-frame line mipmap, and gradation/background render. `ExternalChunk` has no entries for them, and `RemovedExternal` is empty. The pure `test_Filters_Vector_Text_frame.png` export contains a large purple brush-like frame line, but drawing that line directly into the frame fallback improves only the isolated frame export (mean ~5.16 -> ~2.20) and regresses the full mixed sample badly (mean ~2.17 -> ~4.45), because the full image already contains the filtered vector/frame strokes in a different color/composition state. Keep the current conservative frame rectangle fallback; exact frame-line/gradation reconstruction remains open.

Frame-rectangle tuning follow-up: after separating the pure frame export's ordinary raster layer from the actual frame fallback, a narrow bbox/width sweep found a small safe improvement. Rendering the frame header bbox inset by 2 px on all sides with outline width `4` improves the pure frame-focused check from mean `0.402647` to `0.185774`, and improves the mixed `test_Filters_Vector_Text` sample from max=225 / mean=2.174049 / visible=11.849213% to max=225 / mean=2.071053 / visible=11.706924%. This remains a geometric fallback, not the real comic-frame brush/gradation renderer.

FrameFolder structure follow-up: the user's UI observation is correct. `Frame 1` is a special `LayerType=0` folder (`LayerFolder=1`) with `LayerLayerMaskMipmap=82`, `ComicFrameLineMipmap=83`, and two children: `Frame background 1` (Layer 32, `GradationFillInfo`, render mipmap `84`) and `Layer 4` (Layer 31, ordinary raster, render mipmap `85`, currently decodes fully transparent). Ghidra confirms this is a dedicated pipeline: `CSFrameFolderLayer::FrameRenderMain` loads a mask layer through `this+0x1218` / `GetMaskLayer`, a frame image layer through `this+0x1220` / `GetFrameImageLayer`, and delegates frame-line rendering to `CSFrameLineLayer::FrameLineRender` / `FrameMaskRender`. In this sample the critical cached offscreens for layer 30 render, layer 30 mask, layer 30 comic line, and layer 32 background point to `extrnlid21594...`, `extrnlid61ED...`, `extrnlidD516...`, and `extrnlidF7DEF...`, none of which are present as `CHNKExta`; only layer 31's `extrnlid15A29...` exists, and it is transparent. Therefore more rectangle tuning is unlikely to converge; exact support needs either recovered cached externals or a synthetic implementation of `FrameMaskRender` + `FrameLineRender` + gradation background.

Frame line Ghidra follow-up: `CSFrameLineLayer::FrameMaskRender` calls the same vtable draw method at `+0x5f0` that `FrameLineRenderForVector` uses for visible vector frame lines, then converts transmit/not-transmit bits into the mask plane. `FrameLineRenderForRaster` is a separate path that copies/resamples an existing frame-line offscreen into the target. For this sample, the line path is therefore the vector frame-line renderer plus brush/material settings, shared by both the visible purple line and the folder mask; the Layer 30 vector header is only one input to that renderer, not the final frame geometry.

Frame mask conversion follow-up: decompiling `FrameMaskRender @ 0x12365c50` shows the mask path first calls the shared `DrawRuler` vfunc (`+0x5f0`) on the destination offscreen, then walks the affected 1-bit blocks. If a rect is supplied, CSP expands it by 4 px on all sides before scanning. Within that scan, pixels equal to `cbNotTransmit1Bit` are flipped to `cbTransmit1Bit`; other values are preserved. So the folder mask is a post-processed line-render result, not just a filled frame rectangle.

Frame/ruler sampling follow-up: the shared frame-line draw method resolves to `CSFrameLineLayer::DrawRuler @ 0x123639e0`. It reads ruler objects from the frame-line layer DB, scales points by target resolution, then calls `CSRulerFunction::DrawSingleLine`. For vector targets this flows through `CSVectorizeV4::InitSamplingForRuler @ 0x12482090`, `DoSamplingForRuler @ 0x12481030`, and `EndSamplingForRuler @ 0x12481480`: points are multiplied by `0x10`, sampled into a `CSVStroke`, added to the vector layer search DB, and finally rendered by `CSVec4Draw::RenderNormal`. `CSVec4Draw::RenderStroke @ 0x1246a630` and `RenderCurve @ 0x12466e50` then generate pen-head hull polygons and fill them with `FillPolygon @ 0x12465250`. The CSP frame line is not a raster rectangle or a single antialiased polyline; it is a vector stroke reconstructed from ruler geometry, pen-head shape, width/pressure/rotation state, and brush/material settings.

Frame point-record follow-up: Layer 30's `VectorObjectList` body does contain the actual four frame points after the external-id wrapper. The header has `(100, 76, 88, 88)`, point count `4`, style id `7`, following word `5`, and width `2.5`; the four 88-byte records decode to centers around `(104.001,741.001)`, `(629.001,741.001)`, `(629.001,983.001)`, and `(104.001,983.001)`. A prototype that filled the point bbox and drew a closed circular polyline from those records was still worse than the current conservative rectangle fallback (best mixed mean about `2.1956` vs current `2.071053`). So the point records are useful evidence for frame geometry, but they are not enough to match CSP's final frame edge without the renderer's antialiasing/scan conversion/mask behavior.

Frame fallback retest after final filter fixes: a narrower sweep rechecked current body-bbox rectangle variants and point-record rectangle variants against both `test_Filters_Vector_Text_frame.png` and the full stacked export. The current fallback (`body bbox`, inset `2` on all sides, outline width `4`) remains the best isolated frame result (`mean=4.955398`, `visible=50484`) and keeps the full sample at `mean=0.579766`, `visible=48641`. The best point-record rectangle found (`expand=2`, outline width `5`) is worse both isolated (`mean=4.975456`, `visible=50650`) and full (`mean=0.594814`, `visible=48808`). Keep the current frame fallback; the remaining error is renderer/mask conversion, not bbox selection.

Vector object header alignment follow-up: the frame and text-balloon bodies use a 100-byte object header, while the ordinary vector stroke layer uses a 92-byte stroke header. For the 100-byte text/frame headers, offsets `+0/+4/+8/+12` are `(header_len, point_header_len, point_stride_a, point_stride_b)`, `+16` is point count, `+24..+36` repeats the object bbox, `+80` is BrushStyle id (`6` for text balloon, `7` for frame), `+84` is a subtype/variant (`4` for balloon, `5` for frame), and `+88` is a double line width (`2.5`). By contrast, the ordinary layer-5 92-byte header stores `(point_count, flags)` at `+16/+20`, bbox at `+24..+36`, and width directly at `+80` (`15.0`). The importer should keep these parser families separate.

Gradation background follow-up: Layer 32's `GradationFillInfo` is a 300-byte named-section blob, not the cached pixels themselves. It starts with a `GradationData` section (`80` bytes) and later contains `GradationSetting` / `GradationSettingAdd0001` sections. Ghidra `CSTone::Read100 @ 0x12342670` dispatches section type `2` to `ReadGradation100 @ 0x12342fc0`, which reads gradation control points and parameters into `CSTone`; `CSToneWorkData::MakeGradationTable @ 0x1235e1f0` then converts them into a draw table. So the frame background child is also a structured gradation/tone object, not a normal raster layer. In this sample its cached offscreen is missing from `CHNKExta`, so the current white-rectangle frame fallback is still only a conservative approximation of the missing gradation/mask render.

Gradation payload calibration: parsing the `GradationSetting` section at its aligned double fields exposes frame-local parameters consistent with a vertical frame background gradient: two leading `100.0` values, then points around `(367,741)` and `(367,984)`, matching the frame's top and bottom span. The rendered reference frame region is already nearly white, and the current white rectangle fallback has low local error, so adding a guessed gradient renderer is not justified without decoding the stop/color table in `GradationData`.

Gradation payload byte follow-up: `Layer 32`'s 300-byte blob starts with total length `300` and archive/type value `2`, then a `GradationData` named section. That section has a 24-byte header `(24, 28, 2, 16, 1, 3)` followed by two 28-byte nodes. Both nodes are white (`0xffffffff` channel words); their ids/stops are `(1, 0)` and `(2, 32768)`, so this sample's frame background is effectively a white two-stop gradient. The following `GradationSetting` name is followed by three big-endian ints `(0, 0, 1)` and seven big-endian doubles `(100.0, 100.0, 0.0, 367.0, 741.0, 367.0, 984.0)`, confirming the vertical control span. The cached externals remain absent: frame render `extrnlid21594...`, frame mask `extrnlid61ED...`, comic line `extrnlidD516...`, and gradation render `extrnlidF7DEF...`; only the transparent child raster `extrnlid15A29...` is present. This supports keeping the current hard-white frame background fallback for this sample while leaving a future path for true colored gradations.

Gradation fallback implementation follow-up: the importer now uses `GradationFillInfo` only for the narrow, proven case where the frame background child has a `GradationData` section whose nodes all share the same RGB color. For `Layer 32`, that resolves to `(255, 255, 255)`, so the frame fallback's fill color is now data-derived instead of hard-coded while the rendered pixels remain unchanged (`test_Filters_Vector_Text` stays max `225` / mean `0.577998` / visible `4.636955%`). The child gradation layer is not rendered as a full-canvas white image because the frame group mask external is missing and PSD shows that full-canvas background relies on the group mask. Non-solid gradients still fall back to the existing conservative frame fill until the full `CSTone` / gradation draw path is decoded.

Gradation corpus sweep: `test_Filters_Vector_Text.clip` is currently the only sample in `img/*.clip` with nonempty `Layer.GradationFillInfo`. Its sole gradation layer is `Layer 32`, and the decoded node list is exactly two white stops: `((255,255,255), id=1, stop=0)` and `((255,255,255), id=2, stop=32768)`. This confirms the new solid-fill subset is grounded in the available corpus, but there is still no sample coverage for colored gradients, multiple stops, opacity/transparency nodes, repeat/reverse settings, or the `GradationSettingAdd0001` variant.

Brush-material follow-up: the sample's SQLite brush tables confirm the rough outline clue. `BrushPatternImage` contains a pattern named `粗い線` (MainId `4`, Mipmap `80`), and `BrushPatternStyle` MainId `7` references ImageIndex `4`. `BrushStyle` MainIds `6` and `7` both use `PatternStyle=7`; the vector object headers for the text balloon and comic frame store those same style ids near header offset `+80` (`6` for balloon, `7` for frame). The decoded rough-line pattern is a narrow 23px-wide vertical strip with binary alpha and noisy RGB. A simple prototype that used this strip as a binary outline mask regressed both the pure bubble export and the full mixed sample, so the pattern cannot be applied as a naive post-mask; CSP is likely sampling it through the brush engine along the stroke path with interval/antialias/style parameters. Keep the current geometric fallback until the real vector brush renderer is decoded.

Brush/style correction follow-up: the vector headers now have a stable style-id clue: text balloon layer 16 stores BrushStyle `6` near header `0x88`, and frame layer 30 stores BrushStyle `7` at the same position; the following word is `4` for the balloon and `5` for the frame. However, the Ghidra frame-ruler path does not currently show `CSVec4Sampling` entering pattern mode. `CSVec4Sampling::SetLayer @ 0x1247b180` clears `this+0x60`, and `Sampling @ 0x1247a370` only calls `PlotPreSamplePattern` when that pattern-mode field is nonzero. The ruler/`InitSamplingForRuler` path therefore appears to create ordinary `CSVStroke` data with a default `PEN_HEAD_STRUCT`, not the `RCPatternDraw` branch. Treat the rough-line material as confirmed metadata for vector/brush objects, but do not assume the frame-line fallback can be fixed by directly stamping `BrushPatternImage` along the ruler.

BrushStyle parameter follow-up: SQLite confirms the style records are meaningful but still not directly drawable. BrushStyle `6` (balloon) and `7` (frame) both have `StyleFlag=115248`, `PatternStyle=7`, `ThicknessBase=1.5`, `AutoIntervalType=2`, `FlowBase=1.0`, `Hardness=1.0`, and the same thickness effector blob; BrushStyle `6` uses antialias `2`, while frame BrushStyle `7` uses antialias `1`. `BrushPatternStyle` `7` has `ImageNumber=1`, `ImageIndex=4`, `OrderType=3`, `Reverse2=34`, and `BrushPatternImage` `4` is named `粗い線` with mipmap `80`. Ghidra xrefs still show `CSPattern::SetPattern @ 0x12426750` and `UpdatePatternObject @ 0x12427360` only being called by `CSAdjustmentLayer::CreateTexture @ 0x122be810`; the V4 sampling pattern calls are isolated to `Sampling` / `CreateBezierCurve` after `this+0x60` is already nonzero. So the missing conversion is likely a BrushStyle-to-runtime-stroke/style setup function, not the pattern bitmap itself.

Brush bridge rejection follow-up: the tempting `SetPatternParam @ 0x12266c80` symbol belongs to `CSAdjustLayerFilterData`, where it copies `CSPattern` parameters for adjustment/texture data; it is not the vector brush style bridge. `GetPacketPenHead` / `SetPacketPenHead` and `RegPenHead` only move already-built pen-head structs in vector packet/file-conversion storage. `InitRasterOperation @ 0x12477090` remains the explicit setter that enables `CSVec4Sampling+0x60` pattern mode, but the traced frame/text ruler path still calls `SetLayer(layer, 0, 1)` immediately before `StartSampling`, clearing that mode.

Static import/export boundary for brush patterns: a lightweight PE import/export parse of the CSP install confirms the current DLL boundary. `iswCmnTG.dll` exports the full `RCPatternDrawParam` API (`SetSizeParam`, `SetIntervalParam`, `SetPatternParam`, `BeginPlotParam`, `ConvertInterval`, `DrawSinglePattern`, etc.). `iswCoreTG.dll` exports `CSVec4Sampling::InitRasterOperation`, `PlotCurvePattern`, and `PlotPreSamplePattern`, but statically imports only `RCPatternDraw::DrawSinglePattern` from `iswCmnTG.dll`. `TGXPGPlugInCore.dll` imports `CSPattern::{CreatePatternObject,SetPattern,UpdatePatternObject}` and `RCPatternDraw::CreateSumiPattern*`, but not `CSVec4Sampling::InitRasterOperation` or `RCPatternDrawParam` setters. `CLIPStudioPaint.exe`, `ClipPreview.dll`, and `LipPreview.dll` contain BrushStyle/PatternStyle schema strings but no static brush-runtime bridge imports. The missing BrushStyle-to-runtime path is therefore likely dynamic/tool-state code or a non-obvious caller, not a direct static import in the traced frame/text render path.

CSVec4 sampling serialization follow-up: `CSVec4Sampling::StartSampling @ 0x1247b400` makes the branch split explicit. When `this+0x60 == 0`, it creates ordinary `CSVStroke` records in the target `CSVec4DataBase`, stores `param_6` flags, the deduplicated pen head, `param_8` width, and `param_9` color, then later links `CSVCurve` records. `CSVStroke::Serialize @ 0x12462660`, `CSVCurve::Serialize @ 0x124620c0`, and `CSVPenHead::Serialize @ 0x12462510` do not expose BrushStyle/PatternStyle fields; pattern drawing only happens in the separate nonzero-pattern branch. This further argues that the frame-ruler path is not missing a simple direct texture stamp, but a higher-level style setup path that either changes sampling mode or materializes the effect before/after V4 stroke creation.

CSVec4 sampling field confirmation: decompiling `CSVec4Sampling::SetLayer @ 0x1247b180` directly confirms it stores the destination `CSVectorV4Layer*` at `this+0x08`, then clears `this+0x60`, `this+0x68`, and `this+0x70` before writing the two mode arguments at `+0xa0/+0xa4`. `StartSampling @ 0x1247b400` then reads those same fields: if `+0x60` remains zero, it allocates ordinary `CSVStroke` records and sets their flags, pen head, width, and color; the pattern/raster-operation state is not lazily recovered inside `StartSampling`. Therefore every traced `InitSamplingForRuler -> SetLayer -> StartSampling` path is hard evidence for ordinary V4 stroke creation unless another caller re-enables pattern mode after `SetLayer`.

Ruler sampling chain confirmation: `CSVectorizeV4::InitSamplingForRuler @ 0x12482090` computes flags at `+0x20c`, width at `+0x208 = scale * line_width * 0.5`, calls `SetLayer(layer,0,1)`, immediately calls `StartSampling` with `DAT_1268b7b0`, the computed width, and color `+0x210`, then adds the first point through `Sampling` or `AddDirect`. `DoSamplingForRuler @ 0x12481030` and `EndSamplingForRuler @ 0x12481480` only continue that same sampler, finish corners/pressure, call `EndSampling`, add the ordinary stroke to `CSVec4Search`, and finally call `Clear`. `CSVec4Sampling::Sampling @ 0x1247a370` is also explicit: the zero-pattern branch writes sample records and calls `SamplingMain`; only the nonzero branch builds pattern sample nodes and calls `PlotPreSamplePattern`. This closes the observed frame/text ruler chain as ordinary stroke reconstruction, not a delayed pattern-render path.

V4 pen-head/radius follow-up: `CSVec4Draw::Initialize(RCVOffscreen*, int) @ 0x124667c0` seeds ordinary vector rendering with `+0x15c = 3.2`, `+0x160 = 8.8`, `+0x164 = 6.4`, and `+0x158 = max(1, int(offscreen_resolution * 0.15 / 25.4 * 16))`. `RenderStroke @ 0x1246a630` uses these as internal minimum/extra radius terms before calling `StorePenHeadPoint -> FillPolygon`; `FillPolygon @ 0x12465250` converts polygon edges with a `4096.0` fixed-point scale. `StorePenHeadPoint @ 0x1246bd30` expands default circular V4 pen heads dynamically from the current radius and `+0x158`, whereas the older `_VPENHEAD` helper `CreatePenHeadPolygon @ 0x1242c320` caps circular packet pen heads at up to 64 generated points. A direct importer sweep confirms these internal radius offsets should not be copied into the current pixel fallback: current full sample remains mean `0.579766`, while `radius+1` gives `0.769308`, `radius+3.2` gives `1.351845`, `radius-1` gives `0.770794`, `0.96x` gives `0.672397`, and `1.04x` gives `0.635128`. Keep the empirically tuned `0.95 * width * taper` fallback until the actual V4 pen-head envelope and fixed-point scan conversion are implemented.

V4 curve subdivision follow-up: `CSVec4Draw::RenderCurve @ 0x12466e50` constructs the stroke body by sampling a line/quadratic curve, computing the farthest pen-head points on both sides, storing one edge forward and the other edge in reverse, then filling that envelope polygon once. For quadratic curves it uses `CSBezier::CalcLengthFast / CSVec4Draw+0x158` with a minimum of two subdivisions; for this sample `CanvasResolution=72`, so the dynamic V4 subdivision evidence no longer cleanly maps to the older V2 `96/16 = 6px` fallback step. Sweeping the current SQLite centerline fallback gives full means `3px=0.578548`, `4px=0.578540`, `5px=0.577998`, `6px=0.579766`, while the isolated vector export stays essentially flat (`5px mean=0.172712` vs `6px mean=0.172706`). The importer now uses `ceil(distance / 5px)` as the best full-stack compromise, still clamped to `1..32`.

V4 envelope rejection follow-up: `CSVPenHead::CalcFarestPoint @ 0x1245ef70` confirms the default circular head computes opposite farthest points along the tangent normal; custom heads select min/max support points from the stored pen-head polygon. `CSBezier::CalcLengthFast @ 0x1244ca10` estimates quadratic length by evaluating the curve at `t=0.25/0.5/0.75` and summing four chord lengths. A prototype that replaced the fallback's per-segment capsules with a single envelope polygon per SQLite stroke regressed badly (`full mean=0.839307`, `isolated Vector mean=0.412975`, versus current `0.577998` / `0.172712`). The SQLite layer-5 point stream should still be treated as an exported/smoothed centerline for the current fallback; direct V4 envelope construction needs the real `CSVStroke`/`CSVCurve` records and corner state, not just these raw points.

Mendel BrushStyle Ghidra follow-up: the text balloon and comic frame paths share the same shape of runtime reconstruction. `CSTextGroupLayer::ReadSelf @ 0x12331e70` sets ruler render/style flags at `CSRulerObject+0x128 = 3`, copies text-frame line width into `CSRulerObject+0x98`, and syncs colors into the material layer; `CSTextFrameV4Layer::DrawRulerForBitmap @ 0x12329830` and `CSFrameLineLayer::DrawRuler @ 0x123639e0` both ultimately feed ruler geometry to `CSVectorizeV4`. Relevant runtime offsets are `CSVectorizeV4+0x48` embedded `CSVec4Sampling`, `+0x208` computed `line_width * 16`, `+0x20c` stroke flags, `+0x210` color, and `+0x21c` coordinate scale `0x10`; `CSVStroke+0x18/+0x50/+0x58/+0x5c` hold flags, pen head, width/radius scale, and color. This makes the current best explanation: `BrushStyle=6/7` and `BrushPatternImage=4` are real file metadata, but the missing-cache frame/text export path currently observed recreates ordinary circular-pen vector strokes unless another, still-unfound style setup function populates `CSVec4Sampling+0x60/+0x68`.

Pattern-mode xref boundary: `CSVec4Sampling::SetLayer @ 0x1247b180` has only the expected xrefs from `CSVectorizeV4::InitSampling`, `InitSampling3D`, and `InitSamplingForRuler`, and it explicitly clears `this+0x60`, `this+0x68`, and `this+0x70` every time. `CSVec4Sampling::Clear @ 0x12474d60` clears `+0x68` and sampling buffers but is not the missing style setter. The only xrefs into pattern plotting are still internal (`CreateBezierCurve -> PlotCurvePattern` and `Sampling -> PlotPreSamplePattern`), so a pattern-enabled V4 stroke would need to enter through a different initialization path than the frame/text ruler path currently traced.

Default pen-head correction: `DAT_1268b7b0` looked misleading when read directly from the PE `.data` bytes, but Ghidra shows `FUN_12255070` initializes it at runtime before use: `_DAT_1268b7b0 = DAT_125536ec` (`1.0f`), `_DAT_1268b7b4 = 0`, and `_DAT_1268b7b8 = 0`, while constructing 32 `RCPoint` slots at `DAT_1268b7c0`. `CSVec4DataBase::GetSamePenHead(PEN_HEAD_STRUCT*)` then interprets that as scale `1.0`, rotation `0`, type `0` (circle). Therefore the frame-ruler path's default pen head is a circular head, not the rough brush pattern itself; any roughness still visible in CSP must come from ruler geometry, scan conversion, antialiasing/mask conversion, or another style path not yet located.

Vector-brush Ghidra follow-up: `CSVector::VectorRenderDB` drives `CSVectorDraw::DrawPacket`, which dispatches normal strokes into `DrawPacketSpline`. The spline path calls `DrawPenHeadCurve`, and `DrawPenHeadCurve` does not draw a simple wide line. It subdivides the Bezier, computes pressure/rotation, builds a pen-head polygon at each sampled point via `CreatePenHeadPolygon`, wraps adjacent pen-head polygons with `GrahamScanWrap`, and fills that polygon through `DrawPolygon -> CSVectorScanConv::ScanConvPoint`. `_VPENHEAD` fields are now identified enough for runtime packets: `+0/+4/+8` are scale/rotation/radius-like floats, `+0xc` is pen-head type, `+0xe` is custom point count, and custom pen-head points start around `+0x10` as normalized shorts. However, the `test_Filters_Vector_Text` layer-5 stream is still the file-format V2 stroke data, not the already-expanded runtime `0x74` packet layout exposed by `GetPacketPenHead`; its 92-byte headers contain color/style/width plus point records, and the current `0.95 * width` circular fallback remains the best full-sample result in a sweep. Do not replace it with bbox-ellipse pen heads unless another sample proves the runtime packet conversion.

V2 pen-head conversion follow-up: `CSVectorFileConv::LoadV2PenHead @ 0x12431610` reads count-prefixed custom pen-head points from the V2 stream, normalizes them by the file header scale, clamps to 32 points, computes a pen-head radius, then writes `0x90`-byte runtime pen-head records. Those records are what later `CSVec4DataBase::GetSamePenHead` de-duplicates and `CreatePenHeadPolygon @ 0x1242c320` expands into circle/square/custom head polygons. This further confirms the file-format vector bytes are not directly drawable; they must be converted through CSP's runtime pen-head/stroke representation before exact antialiasing can match.

V2 conversion boundary follow-up: the old `CSVectorSampling` path is separate from V4 frame/text reconstruction. `CSVectorFileConv::RenderV2 @ 0x12432b20` calls `CSVectorSampling::StartSampling @ 0x12446e50`, then renders through `CSVectorDraw::DrawPenHeadCurve`; `LoadV2CtlPos`, `LoadV2Scale`, `LoadV2Param`, and `LoadV2PenHead` split a compressed V2 stream into internal arrays. In particular `LoadV2Param @ 0x124314b0` fills internal `0x38`-byte head records, while the current `VectorObjectList` bodies for layer 5 / balloon / frame start with 92- or 100-byte object headers after the external-id wrapper. Treat the V2 decompile as renderer prior art, not as a direct parser for this SQLite vector body.

Pattern-renderer Ghidra follow-up: CSP's rough outline material flows through `CSPattern`/`RCPatternDraw`, not a direct texture overlay. `CSPattern::PushPatternElement` can store bitmap elements, `UpdatePatternObject` tiles/composes those elements into a pattern buffer, and `CSVec4Sampling::PlotCurvePattern` / `PlotPreSamplePattern` call `RCPatternDraw::DrawSinglePattern` along line or Bezier distance. This matches the user's observation that CSP speech-bubble and frame outlines look like a material/brush effect. The decoded 23px rough-line strip is only one ingredient; the missing pieces are the vector brush's sampling interval, rotation/thickness effectors, and how `BrushStyle`/`BrushPatternStyle` are converted into the `RCPatternDraw` stroke points.

Frame/text ruler recheck: `CSFrameLineLayer::FrameLineRender`, `FrameLineRenderForVector`, and `FrameMaskRender` all delegate back through the same `DrawRuler` vfunc (`+0x5f0`). For vector targets, `DrawRuler` creates a temporary `CSVectorizeV4`, calls `InitDrawRuler`, then `InitSamplingForRuler`; that initializer calls `CSVec4Sampling::SetLayer(layer, 0, 1)`, which clears `CSVec4Sampling+0x60/+0x68/+0x70` before `StartSampling`. The observed frame/text missing-cache path therefore enters ordinary V4 stroke creation and later `CSVec4Draw::RenderNormal`, not `RCPatternDraw::DrawSinglePattern`. The rough `BrushPatternImage` metadata is still real, but it is not sufficient to fix the fallback by stamping mipmap `80` onto the current rectangle/polyline output.

Single-filter verification pass: rendering only paper + raster layer + one filter confirms the supported filters are close. Hue/Saturation, Level Correction, Threshold, Posterization, Reverse Gradient, Tone Curve, and the corrected Color Balance path are all max=1 with no visible pixels; Brightness/Contrast is max=2 / mean=0.015039 / visible=28 pixels; Gradient Map is max=8 / mean=0.052949 / visible=2102 pixels. Tone Curve remains visually exact for the observed compact master-curve subset after the B-spline implementation.

Tone Curve was retested after the final frame rectangle inset, before the compact parser was corrected. The best direct 16-bit coordinate interpretations were still worse than skipping type `3`, and the isolated Tone Curve PNG gave a different local optimum than the full stacked sample. That conflict is resolved by parsing the SQLite payload as 32 compact `0x82` curves and feeding the first curve through the 16-bit-domain B-spline LUT builder.

Tone Curve empirical follow-up: using the user's single-filter export (`paper + raster + Tone Curve`) and pixels where the Tone Curve layer mask is fully white shows CSP applies one shared non-monotonic LUT to R/G/B, not separate visible channel curves in this sample. Observed channel mappings include `30 -> 73`, `70 -> 133`, `128 -> 163`, `176 -> 157`, and `226 -> 181`. The compact payload is now parsed as 32 compact curves, each `0x82` bytes (`uint16 count + 32 uint16 point pairs`); the first curve is `(0,0) -> (16075,47014) -> (60236,35459) -> (65535,65535)`, and the remaining 31 curves are identity. A first calibrated exact-LUT path verified the isolated Tone Curve export at max=1 / mean=0.000138 / visible=0 and improved the full mixed sample to max=225 / mean=0.964789 / visible=9.213734%.

Tone Curve B-spline correction: the earlier direct interpolation attempts failed because they scaled compact points to `0..255` before curve generation. CSP's `CreateLookUpTableToneCurveStatic -> GetBsplineIntTable` behavior is much closer when the compact points remain in 16-bit coordinate space, the B-spline table is sampled at 256 positions, and the resulting 16-bit values are quantized to bytes. Implementing that B-spline subset for payloads shaped as `master compact curve + identity remaining curves` improves the full mixed sample again to max=225 / mean=0.895458 / visible=8.440113%, while the isolated Tone Curve export remains visually exact at max=1 / mean=0.001581 / visible=0. The implementation still skips compact payloads with non-identity extra curves until channel semantics are confirmed.

Vector AA fallback test: a temporary soft-edge `_draw_polyline_rgba` prototype was tested for layer 5 by replacing the binary radius test with a one-pixel alpha ramp. Radius scales from `0.75` to `1.15` all regressed the complete `test_Filters_Vector_Text` render; the best soft-edge result was still mean `2.201236` at scale `0.95`, worse than the committed hard-edge fallback mean `2.071053`. Keep the current binary circular fallback until the real CSP scan conversion/material path is decoded.

Residual diff localization: connected-component analysis of the current `test_Filters_Vector_Text` diff shows the remaining error is dominated by vector/text geometry that later adjustment layers quantize into large color steps. The largest visible component spans roughly `(39,134)-(480,684)` and corresponds to the ordinary vector/bubble region; other large components around `(740,610)-(943,942)` and `(711,222)-(916,621)` are right-side vector strokes. Frame pixels still produce the absolute max (`225`) where the fallback purple frame line overlaps CSP-white pixels, but a targeted top-inset/width sweep reconfirmed the current frame fallback `(top inset=2, width=4)` is the best tested full-image score. Do not chase the max diff by moving the frame line unless the mean also improves.

Vector point-record pressure test: Layer 5 point records contain per-point bboxes and several float-looking fields after the `(x,y,bbox)` data. Directly deriving stroke radius from the point bbox is wrong: it regresses the full sample to mean `3.65+`. Using candidate per-point floats as pressure/scale (`+36`, `+40`, `+44`, `+76`) also regresses; the best tested pressure prototype was `+40 * 0.95`, mean `2.299684`, still worse than the committed constant `0.95 * width` fallback at mean `2.071053`. The bboxes/floats are useful clues for CSP's renderer, but not a safe direct-radius rule.

Post-Tone-Curve residual pass: after enabling the calibrated Tone Curve LUT, the largest visible diff component moves to the left vector/text region `(102,306)-(471,683)`, with global max=225 / mean=0.964789 / visible=9.213734%. Re-sweeping the text balloon fallback confirms the current `power=2.4`, outline width `5`, and original vector bbox remain the best tested full-image score; moving, expanding, shrinking, or changing the superellipse power regresses. Re-sweeping the frame rectangle also reconfirms current `(inset=2, width=4)` as best; no frame code change is justified.

Vector radius recheck after Tone Curve: a global radius sweep finds only tiny gains around `0.952..0.960 * width`; `0.952` slightly improves both mean and visible percentage over current, while `0.960` has the lowest mean but a slightly worse visible percentage. A coordinate descent over the four layer-5 strokes gives a best tested per-stroke scale vector `[0.94, 0.96, 0.96, 0.95]` with mean `0.960864` / visible `9.210396%`, only a `0.003925` mean improvement over the current single `0.95` scale. Because that requires stroke-index-specific tuning and does not reveal a general parser/rendering rule, leave the code unchanged.

Tone Curve formula recheck: with the corrected compact points `(0,0) -> (16075,47014) -> (60236,35459) -> (65535,65535)`, common direct formulas over points scaled to `0..255` still do not reproduce CSP's empirical LUT. The best simple linear interpolation over scaled points gives LUT samples `[87,181,166,153,140]` at inputs `[30,70,128,176,226]`, while CSP's calibrated LUT is `[73,133,163,157,181]`. Smoothstep and Hermite/Catmull variants are worse. The working rule is 16-bit-domain B-spline generation followed by byte quantization, not low-resolution interpolation.

1. **Terra localized color follow-up.** The original opaque-content worst point is fixed by honoring `LayerLayerMaskMipmap` on `LayerType=3`; the dark-line overwrite is fixed by masked THROUGH group rendering; the sampled clipped Add Glow over-brightening is reduced by using effective alpha for Add Glow strength; and the `(2190, 1319)` / `(2287, 1311)` darkening is improved by a slightly wider clipped preserve threshold. `Ref_Terra404_Live2D` still needs another full-image pass to confirm the new worst point after the `2.25/255` threshold.
2. **Clipping group structure.** The current isolated clipping-group implementation is retained. `Test_AddGlowMultiply` still looks bad under raw straight-RGBA metrics because transparent pixels keep different RGB, but premultiplied RGBA max is `3`; `Test_AddGlowMultiplyClipping` is high-alpha max `1`. Treat future work here as low-alpha/transparent-RGB validation cleanup unless a new visible mismatch appears.
3. **Kabi remaining dark pixels.** The historical new worst pixel at `(1455,1103)` is no longer a large Multiply/clipping failure in the current importer. A 1x1 replay of the current recursive compositor hits only nine nonzero layers and produces `[175,158,212,255]` versus CSP `[176,154,221,255]`. The old `[112,109,109]` state was superseded by the later GLOW_DODGE/offscreen-folder work; remaining error here is a small Glow Dodge / quantization residual, not a folder-boundary darkening bug.
4. **MXL ADD highlight layers.** The historical red-channel blowout at `(2484,3492)` was caused by the former simplified ADD formula. Current `_blend_add_u8()` with the decoded full-strength L432/L434 masks reaches `[190,71,106,255]` versus CSP `[190,73,107,255]`, so this is now a tiny quantization/order residual rather than an open mask/folder limiter.
5. **Aya systemic differences.** Small color differences (max 螖=16, mean=0.006) concentrated around a specific image region. Likely a blend formula edge case or minor mask interaction. Lowest priority.
6. **Layer offsets.** The loader warns on non-zero `LayerOffsetX / LayerOffsetY`; no supplied sample has required offset support yet.
7. **Grayscale / monochrome layers.** csp_tool says unsupported. Still out of scope until a real sample requires it.
8. **Vector / 3D / text layers.** Text cache/balloon and a constrained vector-stroke fallback now exist for the filter sample. Full CSP vector, 3D, and text layout remain out of scope; decide later whether unsupported cases should warn in Blender UI or use fallback preview data.
9. **Color management.** CSP authoring color space vs Blender scene linear may produce display differences even when raw decode is correct.

## Reusable Code

Current implementation choice:
- Keep the Blender package self-contained with stdlib + NumPy only.
- Keep `csp_tool.py` as prior-art/reference knowledge, not vendored runtime code.
- Keep the project-root `clip_loader.py` and `clip_studio_importer/clip_loader.py` in sync until the package layout duplication is resolved.

Historical note: `csp_tool.py` is MIT and was useful while deriving the minimal implementation, but it is not part of the current runtime package.

## OIIO Native Loader Spike

Goal: check whether an external OpenImageIO `ImageInput` plugin could eventually make Blender load `.clip` through the normal image-loading path, without building a custom Blender.

Findings from Blender 5.0.1 on this machine:

- Blender 5.0.1 includes dynamic OIIO runtime files:
  - `blender.shared/openimageio.dll`
  - `blender.shared/openimageio_util.dll`
  - `5.0/python/lib/site-packages/OpenImageIO/OpenImageIO.pyd`
- Blender's bundled OIIO reports version `3.0.9.1`.
- OIIO reports `psd` in `format_list` and `psd,pdd,psb` in `extension_list`, matching the existing PSD-style load path we want to emulate.
- `bpy.data.images.load()` does not hard-reject unknown extensions: a valid PNG renamed to `.fakeclip` and `.clip` loads successfully as `PNG`.
- The Python OIIO binding can set the global `plugin_searchpath` attribute, e.g. `OpenImageIO.attribute("plugin_searchpath", r"C:\tmp\oiio_plugins")`.

Current blocker:

- The installed Blender package does not include OIIO C++ headers or an import library for building an `ImageInput` plugin.
- The machine currently exposes MinGW `g++`, but not MSVC `cl`. A plugin meant to load into Blender's MSVC-built OIIO DLL should be built with a compatible MSVC toolchain and matching OIIO headers/import libs.

Conclusion:

- The Blender-side extension gate looks promising: if a `.clip` OIIO plugin can be built and discovered, Blender should at least attempt to load `.clip` through the image path.
- Do not spend more time on the native plugin until the Python decoder/compositor semantics are stable. When ready, run a focused C++/Rust spike with Blender-matched OIIO 3.0.9 headers/libs and MSVC.

## 2026-05-05 Post-Tone-Curve Fallback Retest

After the compact Tone Curve B-spline path improved `test_Filters_Vector_Text.clip` to max=225 / mean=0.895458 / visible=8.440113%, the tempting geometry fallbacks were retested rather than committed.

- Layer 16 bubble soft edge: a soft superellipse can reduce the isolated `test_Filters_Vector_Text_bubble.png` mean from `0.180917` to about `0.170764`, but visible pixels increase from `2611` to `3709`. This supports the user's observation that CSP's bubble outline is a brush/material stroke, not a simple antialias fringe.
- Layer 5 vector radius: scaling the current `0.95 * width` radius by `1.01` improves full mean only from `0.895458` to `0.893342` and pure-vector mean from `0.258262` to `0.256845`, while full visible pixels worsen from `88501` to `88563`.

No code change was made for either fallback. The next real improvement likely needs the CSP pen-head/material renderer or a recovered cached offscreen, not more scalar tuning.

## 2026-05-05 Pattern-Mode Boundary

Ghidra now shows the V4 pattern branch clearly. `CSVec4Sampling::CreateBezierCurve @ 0x12474fd0` checks `this+0x60`; when it is nonzero, it calls `PlotCurvePattern` instead of creating ordinary `CSVStroke` / `CSVCurve` records. `PlotCurvePattern @ 0x12479290` and `PlotPreSamplePattern @ 0x124797b0` use `RCPatternDraw*` at `CSVec4Sampling+0x68`, interpolate `RCStrokePoint` values, call `RCPatternDraw::DrawSinglePattern`, and keep pattern distance/interval state around `+0x74`.

The traced frame/text ruler path still calls `CSVec4Sampling::SetLayer(layer, 0, 1)`, which clears `+0x60/+0x68/+0x70` before sampling. Therefore the next useful reverse-engineering target is the setup function that populates those fields from BrushStyle/PatternStyle, not direct mipmap stamping or more fallback geometry tuning.

Follow-up: `CSVec4Sampling::InitRasterOperation @ 0x12477090` is the explicit pattern-mode setter: it stores `+0x60 = 1`, clears the transient stroke/raster rectangles, writes `RCPatternDraw*` to `+0x68`, and stores the rotate/tangent flag at `+0x70`. Ghidra only shows this function in a data/function-name table, not as a normal code xref from the traced frame/text path. The active pattern calls import `RCPatternDraw::DrawSinglePattern` through the external pointer at `0x12552260`, so the actual stamp renderer lives outside `iswCoreTG.dll`. In `DrawStraightLine`, `PlotCurvePattern`, and `PlotPreSamplePattern`, CSP builds an `RCStrokePoint` with internal integer coordinates scaled by `1/16`; after each `DrawSinglePattern` call, its return value is multiplied by `16` and accumulated into `CSVec4Sampling+0x74` as the next pattern distance. This makes the rough brush image only one input to the renderer: stamp interval/advance comes back from `RCPatternDraw`, not from a fixed pixel stride or the raw mipmap height.

`RCPatternDraw::DrawSinglePattern` lives in `iswCmnTG.dll` at `0x12125740` (`image_base=0x120d0000`, export RVA `0x55740`). Headless Ghidra on `iswCmnTG.dll` shows the external pattern pipeline: `DrawSinglePattern -> RCPatternDrawParam::GetNextInterval -> DrawStrokePattern -> BeginPlotParam / NextPlotParam -> PlotPattern`. `GetNextInterval @ 0x12128410` uses `RCPatternDrawParam+0x148` as a base size/interval and `+0x1ac` as the interval mode; mode `1` uses `ConvertInterval` directly, while mode `2` uses `ConvertInterval / 100 * ConvertPatternScale * base`, clamps the physical advance to at least `1.0`, and clamps the denominator to at least `0.001`. `NextPlotParam @ 0x12128b00` then fills a `PLOTPATTERNPARAM` with the selected `RCPattern`, converted plot point, pattern scale, opacity scale, hardness scale, rotation, and size rate before calling the pattern vfunc. This matches the SQLite brush rows for styles `6` and `7` (`AutoIntervalType=2`, `IntervalBase=1.0`, `ThicknessBase=1.5`, `PatternStyle=7`, `RotationEffector=3`), but the remaining unresolved step is mapping those SQLite rows/blobs into the runtime `RCPatternDrawParam` layout.

`RCPatternDrawParam` offset follow-up: headless decompile of init/get/set functions maps the runtime parameter block. `PDPARAM_PATTERN` is copied into `this+0x08..0x147`; the pattern array starts at `+0x40`, count/order are `+0x140/+0x144`, and `FUN_120e8420` is the copy helper. Size is `+0x148` base and `+0x150` effector flags; draw color is `+0x154/+0x15c/+0x164`; hardness is `+0x168/+0x170`; mix color is `+0x174..+0x18c`; size rate is `+0x190/+0x194/+0x198`; rotation is `+0x19c/+0x1a4`; interval is `+0x1ac/+0x1b4/+0x1bc`; airbrush is a large block at `+0x1c0`; etc/continuation flags are `+0x11e0/+0x11e8/+0x11f0`; selected/designated pattern is `+0x1228`; draw color cache is `+0x1238`; random seed/state is `+0x1280/+0x1284`. `InitIntervalParam` defaults mode `2`, base interval `0x1e`, percent `100`, and effector flags `2`. Direct rough-line use in importer fallbacks has now been removed for both frame and text balloon because it regressed the full stacked sample. Exact CSP brush reproduction still requires the missing SQLite BrushStyle-to-`RCPatternDrawParam` setup bridge.

Main-program bridge follow-up: full Ghidra analysis of `CLIPStudioPaint.exe` is too heavy for this machine (still running after 30 minutes, stopped manually), so a lightweight PE/RIP-reference scan was used. The EXE references `AutoIntervalType`, `IntervalBase`, `PatternStyle`, and `ThicknessBase` in tight static-registration functions around `0x1400bc0xx..0x1400bcaxx`; `BrushPatternStyle` / `BrushStyle` schema strings are similarly referenced around `0x140138e..0x140138f` and `0x1400c47..0x1400c51`. These are field/table registration stubs, not the runtime brush renderer. `TGXPGPlugInCore.dll` is a useful preference-loader module: `FUN_15047df0` reads `BrushStyle`, `BrushSize`, `StrokeAutoIntervalTypey`, `StrokeIntervalPercent`, `BrushTexKind`, etc., writes internal tool parameter ids such as `1000` (brush size) and `0x411` (interval minimum/control), and can create built-in `CSPattern`/sumi patterns. It imports only `RCPatternDraw::CreateSumiPattern*`, not `RCPatternDrawParam::Set*`, so it is not the missing `BrushStyle` -> `RCPatternDrawParam` bridge. `ClipPreview.dll` only registers `BrushStyleManager`, `BrushPatternStyle`, and `BrushStyle` strings.

Planeswalker brush RTTI follow-up: the live Ghidra MCP connection is still usable for the current `iswCoreTG.dll` CodeBrowser target, and a separate lightweight scan of `CLIPStudioPaint.exe` found real MSVC RTTI for `Planeswalker::PWBrushStyleManager`, `PWBrushStyle`, and `PWBrushPatternStyle`. Their vtables are at `0x1444e5ca0`, `0x1444debc0`, and `0x1444e94d8`, with constructor/destructor-looking vtable writes around `0x14249f99b`, `0x1422d70fb`, and `0x14256988b`. This strengthens the idea that brush rows are owned by a Planeswalker-side model in the main EXE. However, string/RIP-reference scanning across the CSP install still shows `RCPatternDrawParam` ownership in `iswCmnTG.dll`, the V4 pattern-mode entry points in `iswCoreTG.dll`, and no static EXE import of the `RCPatternDrawParam::Set*` bridge. So the missing connection is probably a higher-level handoff or dynamic/tool-state path, not a direct static call from the traced frame/text render chain.

Related-DLL sweep follow-up: other plugin DLLs were checked as a sanity pass. `ExportPSB.dll` is the same family as `ExportPSD.dll` and has the same Photoshop layer-set style clues. `ImportPSD.dll` / `ImportPSB.dll` expose only `TriglavPluginCall` and generic PSD layer structs (`SPSLayer`), with no CSP renderer imports. `ImportPDFX.dll` has PDF/ICC/font/curve machinery and Triglav mutable-layer/offscreen services, but no frame/vector/text CSP renderer bridge. The actually useful non-PSD module is `TGXPGPlugInCore.dll`, loaded by `TGXPGPlugIn.cfpi`, `TGMaterialPlugIn.cfpi`, `TGCxsPlugIn.cfpi`, and `TGTosPlugIn.cfpi`: it imports `CSLayerLinkDocument`, `CSLayer`/`CSBitmapLayer` render helpers, `CSTextLayer::GetTextOffscr`, `CSTextFrameV4Layer::DrawToVector/GetRasterFrame*/GetLineWidthAverage/GetRulerArrangeData`, `CSFrameFolderLayer::DrawToVector`, `CSPattern::SetPattern/CreatePatternObject/UpdatePatternObject`, and `RCPatternDraw::CreateSumiPattern*`. Headless Ghidra shows `FUN_1503a2e0` calls `CSTextFrameV4Layer::DrawToVector` for text-frame layers and `CSFrameFolderLayer::DrawToVector` for frame folders, then walks frame rulers. `FUN_1503c390` handles text raster-frame/backup-frame layers, `FUN_1503b610` reads text-frame ruler arrangement data, `FUN_1503d6e0` reads average line width, and `FUN_15040310` uses the cached text offscreen. The CSPattern callers (`FUN_150477d0`, `FUN_1503e290`) build texture/pattern objects for material/adjustment-style output, while `FUN_15047df0` creates built-in sumi patterns from tool preferences; they still do not connect BrushStyle `6/7` to `CSVec4Sampling::InitRasterOperation` for the traced frame/text render path. For exact frame/text export semantics, `TGXPGPlugInCore` is now a better target than PSD import/export plugins.

TGXPG focus follow-up: `XPGPlugInCore::ReadPageFile @ 0x15035000` initializes a page/export context (`CSMain`, antialias/simple vector settings, page dimensions), then reaches the same conversion helpers used by material/project reads. The recursive layer converter `FUN_1503ecd0` branches on `layer+0x18c`: it routes text offscreen/cache export through `FUN_15040310`, frame folders through `FUN_1503bf80` / `FUN_1503d4c0`, text frames through `FUN_1503b610`, and the shared vector-output converter through `FUN_1503a2e0`. The text-frame path checks `GetRulerArrangeData` for arrange types `5/6`, pulls raster and backup frames with `GetRasterFrameCount/GetBackupFrame/GetRasterFrame`, and obtains `GetLineWidthAverage` before calling `DrawToVector`. The frame-folder path reads the first ruler object from `param_2+0x1188`, uses the ruler line-width `RCLength` at `+0x98`, and also funnels into `DrawToVector`. This confirms CSP has an official text/frame-to-vector conversion route outside PSD export, but the observed route still packages ordinary vector/raster-frame output and pattern/material objects separately; it does not show BrushStyle `6/7` re-enabling `CSVec4Sampling` pattern mode after `SetLayer(layer,0,1)`.

TGXPG brush/material focus follow-up: a fresh install-wide PE/string scan again finds no module statically importing `RCPatternDrawParam::Set*` or `CSVec4Sampling::InitRasterOperation`; `iswCoreTG.dll` exports the V4 pattern-mode APIs and imports only `RCPatternDraw::DrawSinglePattern`, while `TGXPGPlugInCore.dll` imports `CSPattern::{Copy,SetPattern,CreatePatternObject,UpdatePatternObject}`, `RCPatternDraw::CreateSumiPattern*`, and the two text/frame `DrawToVector` exports. Headless decompile resolves the pattern calls into two buckets. `FUN_1503e290` copies an already-stored `tagPATTERNPARAMHEADER` and pattern elements from a layer/material structure around `param_2+0x580..0x5b0`, then creates/updates a `CSPattern` object. `FUN_15047df0` is the large tool-preference converter: it reads `BrushStyle` (`pen/stamp/airbrush/ribbon`), `BrushSize`, `AntiAlias`, `StrokeAutoIntervalTypey`, `StrokeIntervalPercent`, `BrushTexKind`, `BrushTexResolution`, `BrushTexDensity`, `BrushTexMethod`, and `BrushTexForStroke`; for sumi brush kinds it creates an offscreen via `RCPatternDraw::CreateSumiPattern*`, and for built-in brush textures it calls `FUN_150477d0`, which maps `BrushTexKind 1..9` to built-in texture names such as Canvas, Speckled, Drawing Paper, Vertical/Horizontal Scan Line, and Blind before building a `CSPattern`. This explains the TGXPG `BrushStyle` and `CSPattern` overlap as XPG/tool-material serialization, not as the missing runtime bridge from SQLite BrushStyle `6/7` to `RCPatternDrawParam` or V4 pattern mode in the frame/text renderer.

Core DrawToVector closure: decompiling the actual `iswCoreTG.dll` exports closes the TGXPG text/frame route. `CSTextFrameV4Layer::DrawToVector @ 0x12329c90` only calls `AddTailToDraw`, delegates to `CSFrameLineLayer::FrameLineRenderForVector`, then records each ruler object's `+0x58` id into the output array. `CSFrameFolderLayer::DrawToVector @ 0x122ea2b0` directly creates a `CSVectorizeV4`, calls `InitDrawRuler(..., 0xff000000)`, transforms ruler points by page resolution/offset, and calls `CSRulerFunction::DrawSingleLine`; no BrushStyle/PatternStyle fields are read. `CSFrameLineLayer::FrameLineRenderForVector @ 0x12365af0` is also a thin wrapper around the same vtable `DrawRuler` method (`+0x5f0`) with fixed black `0xff000000`. The shared `DrawRuler @ 0x123639e0` reads ruler point counts, point coordinates via vfunc `+0x70`, width from `RCLength +0x98`, optional flag `+0xb0`, and then either raster `DrawSingleLine` or vector `DrawSingleLine -> CSVec4Draw::RenderNormal`. Ghidra xrefs still show `CSVec4Sampling::InitRasterOperation @ 0x12477090` referenced only by data/export entries, while `InitSamplingForRuler` explicitly calls `SetLayer(...,0,1)` and `StartSampling` with default `DAT_1268b7b0`. This makes the official text/frame-to-vector export path ordinary ruler-to-V4 conversion; the rough brush metadata is real but not used by this route.

Frame double-line branch follow-up: the `DrawRuler` optional `ruler+0xb0 == 1` branch resolves to `CSFrameLineLayer::DrawDoubleLine @ 0x12362260`, not a pattern/brush-material mode. It reads a secondary `RCLength` from ruler offsets `+0xb8/+0xc0/+0xc8`, converts it with `RCLength::GetAsPixel`, and calls `CSRulerFunction::GetDoubleLinePoint` to derive the offset line. In the vector target branch it emits two ordinary `CSRulerFunction::DrawSingleLine` calls, one for the original points and one for the offset points. In the bitmap branches (`DrawDoubleLineForBitmap`, `DrawDoubleLineCurveForBitmap`, `DrawDoubleLineStraightForBitmap`, `DrawPolygonLineForBitmap`) the helpers use offscreen composition, `CSBitmap::FillPolygon`, and `CSBitmap::DrawSegment`. This identifies `+0xb0` as a double-line/polygon frame geometry switch rather than the missing BrushStyle/material-sampling bridge.

Main-EXE brush bridge follow-up: a lightweight PE scan across the CSP install checked both imports/delay imports and embedded strings. `CLIPStudioPaint.exe`, `ClipPreview.dll`, and `LipPreview.dll` import `GetProcAddress` / `LoadLibrary*`, but no module outside `iswCoreTG.dll` contains the `InitRasterOperation` string, and no module outside `iswCmnTG.dll` contains `RCPatternDrawParam` / setter-name strings. That makes a direct dynamic-by-name bridge into `CSVec4Sampling::InitRasterOperation` or `RCPatternDrawParam::Set*` unlikely. A targeted RIP-reference scan of `CLIPStudioPaint.exe` shows the `BrushStyle`, `BrushPatternStyle`, `BrushPatternImage`, `AutoIntervalType`, `IntervalBase`, `ThicknessBase`, and `PatternStyle` strings are referenced only by short table/schema registration stubs around `0x1400bc0xx`, `0x1400c47xx`, `0x14010b0xx`, and `0x140138exx`. The Planeswalker vtables remain real (`PWBrushStyleManager @ 0x1444e5ca0`, `PWBrushStyle @ 0x1444debc0`, `PWBrushPatternStyle @ 0x1444e94d8`), but each has only constructor/destructor-style code refs. Targeted no-analysis Ghidra decompilation shows `PWBrushStyle` initializing large owned parameter/state blocks and defaults such as `StyleFlag 0x1c200`, several `1.0` doubles, and mode/default integers; `PWBrushPatternStyle` destructs a `0x88` object with owned pointer cleanup. No renderer-side call to `InitRasterOperation` or `RCPatternDrawParam` appears in these focused areas. Current reading: Planeswalker owns/imports the brush database model, while the missing brush-to-renderer handoff is still a higher-level runtime/tool-state path, not a direct static or string-loaded call in the traced text/frame export route.

Planeswalker vtable follow-up: direct vtable dumping reinforces the same boundary. `PWBrushStyleManager` has a `0x198` deleting destructor, `PWBrushStyle` has a `0x720` deleting destructor, and `PWBrushPatternStyle` has a `0x88` deleting destructor. Their vtables contain only nine function entries before RTTI/string data; slots `1..8` are mostly shared Planeswalker base methods. Slot `1` returns/refcounts the owned object pointer pair at `+0x50/+0x58`; slots `2` and `3` set a dirty flag at `+0x10`, build a small stack update record, and call generic propagation helpers (`FUN_1432e5350`, `FUN_1432eba40` / `FUN_1432ece80`) on the owner at `+0x28/+0x30`; slot `4/8` is a no-op; slots `5..7` acquire the object at `+0x18/+0x20` and forward to its virtual `+0x28/+0x30/+0x38`. None of these vtable methods reference CSPattern, `RCPatternDraw`, `CSVec4Sampling`, or the V4 pattern-mode setter. So `PWBrushStyle` / `PWBrushPatternStyle` look like database/model records with generic observer/proxy plumbing, not the render-time bridge.

Brush schema registration correction: the earlier RIP-reference addresses pointed to instructions inside short registration functions; the actual function starts are four bytes earlier than the first `lea`. Re-running targeted no-analysis Ghidra from the corrected starts shows the same two-call shape for every BrushStyle-family string: `FUN_142049220(descriptor_global, string_global)`, then `FUN_1438ca71c(registration_global)`. Examples include `AutoIntervalType @ 0x1400bc030` registering string `0x1444de7d8` with descriptor `0x145475078`, `IntervalBase @ 0x1400bc330`, `PatternStyle @ 0x1400bc510`, `ThicknessBase @ 0x1400bcab0`, `BrushStyle @ 0x1400c4730`, `BrushPatternImage @ 0x14010b0d0` / `0x140138e70`, `BrushPatternStyle @ 0x140138ea0`, and `BrushStyleManager @ 0x1400c5180` / `0x140138f00`. These are static schema/table registration stubs, not runtime conversion or rendering code.

Single-filter export follow-up: the user's isolated `test_Filters_Vector_Text_*` PNGs make adjustment-layer fidelity measurable independently. Hue/Saturation, Level Correction, Tone Curve, Threshold, Posterization, Reverse Gradient, and Color Balance are now exact or within one LSB on visible pixels. Brightness/Contrast has only 28 visible pixels above 1 LSB. The earlier Color Balance midtone-offset heuristic has been replaced by the Ghidra-backed level-LUT path from `CreateLookupTablesCB`; isolated Color Balance is now max `1`, mean `0.037455`, visible `0`. Gradient Map's stop scaling and grayscale coefficients now reduce it to max `8`, mean `0.052949`, visible `0.200462%`; the remaining error is small boundary quantization around the LUT/gradient-stop transitions.

Gradient Map follow-up: unblending the isolated export on pixels where the filter mask is `255` first suggested a light-input bias, especially around luminance `226`. A constant post-bias such as `(+1,+1,+2)` dramatically improved the isolated PNG but regressed the full stacked sample, so it was rejected. The later stop-scale sweep is the safer correction: using `32768 * 256 / 255` for stop conversion and CSP-style `0.30/0.59/0.11` grayscale improves both isolated Gradient Map and the full stack. HSV/HSL interpolation tests remain worse than RGB linear (`HSV short` max `30`, `HSL short` max `32` vs RGB max `8`).

Gradient Map luma rejection follow-up: the new `Test_Gradiation.clip` sample is a pure type-9 Gradient Map over raster layers, with no `GradationFillInfo`; current rendering is already close (`max=10`, `mean=0.310779`, visible `45489`, full opaque alpha). A focused in-memory sweep tested Rec.601 float coefficients, fixed-point luma variants such as `(77R + 150G + 29B) / 256`, raw `/32768` stop scaling, PSD-style color flooring, and duplicate-stop ordering. Some luma variants reduce the pure sample mean slightly (`77/150/29` gives `mean=0.308116`), but all of them regress the full `test_Filters_Vector_Text.clip` stack from `mean=0.569085`, visible `48576` to about `mean=0.5889`, visible `63600`. Therefore the importer keeps the existing `0.30/0.59/0.11` floor-luma path; the remaining `Test_Gradiation` error is not enough evidence to retune the general Gradient Map formula.

Arkana Gradient Map LUT recheck: switching Arkana back to cached `iswCoreTG.dll` succeeded; full decompilation of `CreateLookUpTableGRAD @ 0x122be240` timed out, but lightweight `disassemble_at_address` worked. The function loops `esi` from `0` to `0xff`, divides the table index by the same `255.0`-style constant, walks 12-byte runtime gradient nodes from `[object+0x970]` with count `[object+0x98c]`, compares node stop floats at `+4`, and interpolates packed color bytes with a `0x10000` fixed-point weight before writing the four channel tables around `+0x348/+0x368/+0x388/+0x3a8`. That supports the existing stop/LUT-edge model, but it does not contain RGB-to-luminance coefficients; the luma conversion happens before this LUT builder.

Exta parsing follow-up: `decode_layer` and `decode_layer_mask` now pass an expected tile byte length to `_parse_exta`, allowing decompression to stop once the needed tile payload has been read. Raster layers use RGBA tile length (`TILE*TILE*5`), while layer masks use single-channel tile length (`TILE*TILE`). This does not change pixels (`test_Filters_Vector_Text.clip` remains max `225`, mean `0.937791`) but avoids reading past ordinary raster and adjustment-layer mask payloads into unrelated trailing Exta data. The rough-line brush-pattern mipmap exposed another Exta shape: `BlockStatus` / `BlockCheckSum` are variable-sized arrays (`header_len, count, item_size, items...`) for multi-tile payloads, not fixed 24-byte records. Handling that removes the final warning without changing verification. A test that treated `BlockDataEndChunk` as a global terminator regressed core raster samples, so that parser behavior was not changed.

Exta parser performance follow-up: profiling `Ref_Kabi_Live2D` showed `_parse_exta()` dominating the remaining local runtime even though zlib itself was tiny. `_parse_exta()` now preallocates the expected output bytearray when the caller supplies `expected_len`, writes decoded tile blocks into that buffer, and advances over omitted blocks without materializing repeated filler bytes. Pixels are unchanged (`Ref_Kabi_Live2D` remains `max=53`, `mean=0.007183`, `visible=0.033001%`), while the same verification path improved further to about `92s` locally after the earlier group conversion optimization.

Clip-base allocation follow-up: cropped folder/group blend-back now only expands the cropped alpha back to a full-canvas `clip_base_alpha_u8` when a conservative lookahead sees that a later visible sibling will actually use it for clipping. If the next effective sibling is non-clipped, or a filter layer that resets the clip base, the allocation is skipped. This is intentionally conservative around unknown layer types. Pixel checks stayed unchanged on `Test_Mask`, `Test_AddGlowMultiplyClipping`, `test_Filters_Vector_Text`, and `Ref_Kabi_Live2D`; Kabi runtime was effectively unchanged at about `92s`, so this should be treated as allocation hygiene rather than the next major Terra fix.

New isolated-sample follow-up: `Test_ Grayscale.clip` stores `LayerColorTypeIndex=1` raster data as two 8-bit planes per tile (`alpha`, then `gray`). Decoding that as straight RGBA now matches the CSP PNG exactly (`max=0`). `Test_Monochrome.clip` stores `LayerColorTypeIndex=2` as two 1-bit planes per tile (`black`, then `white`); decoding white over black over the paper color changes the sample from a crash to `max=29`, `mean=3.941142`, with the remaining error limited to paper-gray versus white decisions. The new `Test_ToneCurve.clip` finally exercises non-identity RGB compact curves: CSP applies R/G/B curves first, then the master curve, all through the same 16-bit-domain B-spline LUT builder. Enabling that path improves the sample to `max=25`, `mean=0.488793`, `visible=0.840187%`, while `test_Filters_Vector_Text` remains unchanged at `max=225`, `mean=0.577998`.

Ghidra connection note: the local Ghidra HTTP service on port `8080` remains reachable, but the active program during this pass exposed `FUN_1224...` methods and did not contain the known `iswCoreTG.dll` / `iswCmnTG.dll` / `TGXPGPlugInCore.dll` targets (`FUN_12482090`, `FUN_121243b0`, `FUN_1503a2e0`). Avoid drawing renderer conclusions until the relevant DLL tab is active in CodeBrowser.

Brush pattern alpha follow-up: the rough-line mipmap is actually single-channel data in this sample (`23x2511`, ten `256x256` alpha tiles). Feeding that full alpha image directly into the earlier balloon/frame edge perturbation worsened the full sample (`mean 0.937791 -> 0.975035`). Later testing removed the pattern perturbation from both frame and text-balloon fallbacks. This supports the earlier conclusion that the material is real but cannot be matched by direct alpha-strip sampling alone.

CSVec4 pattern-mode setter follow-up: a local PE/capstone scan of `iswCoreTG.dll` looked for other direct writes of `1` to a `+0x60` field matching `CSVec4Sampling` pattern mode. The candidates outside known `InitRasterOperation @ 0x12477090` resolve to unrelated state: `CSTone::RefreshPatternCache`, `CSPattern::UseFixedSize`, `CSRulerVanishV3::DrawThisObject`, and archive reset helpers. Export/import checks show `InitRasterOperation` is exported, but the main EXE does not import it and `TGXPGPlugInCore.dll` imports only `CSPattern` helpers. No hidden normal call path was found that pattern-enables the traced frame/text ruler renderer.

Frame/text baked-style follow-up: rewalking the ordinary ruler-to-V4 path shows no alternate place where BrushStyle gets baked into the stroke. `CSVectorizeV4::InitDrawRuler` seeds color, flags, and scale; `InitSamplingForRuler` computes width, clears pattern mode, and calls `StartSampling` with the default circular pen head, width, color, and flags. `StartSampling` serializes those into `CSVStroke+0x18/+0x50/+0x58/+0x5c`; `DrawSingleLine` only supplies width from ruler `+0x98` and point samples; `EndSamplingForRuler` finalizes corners/rect/search insertion. `FrameMaskRender` reuses the same draw vfunc and converts the result to a 1-bit mask afterward. This further supports treating BrushStyle/PatternStyle as real metadata that is not applied by the observed missing-cache frame/text render path.

V4 scan-conversion follow-up: the frame vector target calls `CSVec4Draw::Initialize(offscreen,1)` and then `RenderNormal(rect,0,bpp)`, so `CSVec4AntiAlias` is not enabled for this path. `RenderStroke` / `RenderCurve` build pen-head envelope polygons, `StorePenHeadPoint` dynamically approximates the default circular pen head with symmetric points capped at `0x20`, and `FillPolygon` converts vertices into 12-bit fixed-point scanline spans. The optional anti-alias path exists elsewhere and uses a `0x400 x 0x400` scratch bitmap plus 4x4 subpixel averaging, but it is not used by the traced frame-vector render. This matches the earlier experiment where adding a generic soft vector edge regressed the sample.

Layer-5 vector taper follow-up: the repeated 88-byte point records have a useful float at point offset `+52`. In `test_Filters_Vector_Text.clip` it behaves like an endpoint taper: endpoints are `0.0`, most interior points are `1.0`. Applying it as a conservative radius multiplier, clamped to `0.25..1.0` and averaged between adjacent points, improves the isolated Vector export from max `173` / mean `0.258262` / visible `0.955200%` to max `173` / mean `0.239301` / visible `0.915527%`. The full stacked sample improves from max `225` / mean `0.937791` / visible `8.471584%` to max `225` / mean `0.924809` / visible `8.423519%`. This is now implemented because it is backed by a per-point field and improves both isolated and full renders; the remaining vector delta still needs real CSP curve/pen-head scan conversion, not scalar radius tuning.

Layer-5 curve follow-up: Ghidra `CSVectorFileConv::LoadV2Param` / `LoadV2PenHead` / `RenderV2` still describe the older compressed V2 conversion path, not the current SQLite `92-byte header + 88-byte points` body, but `RenderV2` confirms CSP ultimately treats vector data as curved strokes before pen-head drawing. A direct experiment also showed that skipping long point-to-point gaps is wrong: thresholds around `100px` regress the isolated Vector export to mean `0.339+`, so those long segments are not disposable jumps. Rendering the point stream through a lightweight Catmull-Rom interpolation with three samples per segment is a better approximation than straight-line joins: isolated Vector improves to max `173` / mean `0.237223` / visible `0.910759%`, and the full sample improves to max `225` / mean `0.920557` / visible `8.420372%`. This is now implemented in the fallback with the existing `+52` taper; exact support still needs CSP's own curve sampling and pen-head polygon generation.

Frame pattern removal follow-up: the isolated frame export shows the interior/background is effectively white; the remaining frame error is mostly edge and corner treatment on the purple frame line. A sweep over soft edges did not beat the original hard rectangle, but it revealed that the conservative rough-line pattern perturbation on the frame was harmful. Passing `pattern=None` for the frame fallback improves the isolated frame check from max `225` / mean `4.986540` / visible `4.837704%` to max `225` / mean `4.955398` / visible `4.814529%`, and improves the full stacked sample from max `225` / mean `0.920557` / visible `8.420372%` to max `225` / mean `0.889415` / visible `8.397198%`. Keep rough-line metadata as a real CSP clue, but do not apply it directly to the frame fallback.

Text-balloon pattern removal follow-up: removing the remaining direct rough-line alpha perturbation from the text-balloon fallback also improves both isolated and full scores. The isolated bubble check moves from max `196` / mean `4.828018` / visible `4.707718%` to max `196` / mean `4.816827` / visible `4.699421%`; the full sample moves from max `225` / mean `0.889415` / visible `8.397198%` to max `225` / mean `0.878224` / visible `8.388901%`. The importer now keeps the rough-line brush data as documented metadata only; current fallbacks use geometry plus cached text/vector pixels, not direct pattern sampling.

Text-balloon geometry retune: after removing direct rough-line sampling, the previous `power=2.4`, width-5, original-bbox superellipse was no longer the best shape. After the Color Balance and Gradient Map LUT corrections, a focused sweep found a slightly tighter asymmetric fallback: inset the vector header bbox by 3 px on the left/top and 2 px on the right/bottom, use `power=2.6`, and outline width `4`. Isolated bubble is max `196` / mean `4.764089` / visible `4.653549%`; together with the current vector subdivision/taper/truncation and filter LUT corrections, the full sample is max `225` / mean `0.579766` / visible `4.638767%`.

Vector fallback rejection pass after the filter fixes: simple scalar retunes are exhausted. The current radius scale `0.95 * width * averaged_taper` remains best in a `0.88..1.12` multiplier sweep; changing it hurts both the isolated Vector export and the full stack. The earlier `ceil(distance / 6px)` subdivision had the strongest V2 basis from `DrawPenHeadCurve` (`96.0f` after `16x` coordinate scale), but a later V4 `RenderCurve` pass and full-stack sweep superseded it with the current `5px` compromise. Catmull-Rom coefficient `0.5` is the only sane value; changing the coefficient makes the stroke path diverge badly. A linear fallback is worse than the current Catmull pass.

V4 sampling follow-up: `CSVec4Sampling::CurveSampling @ 0x12475630` shows CSP can build quadratic Bezier curves from three-sample windows, with midpoint endpoints and sharp-corner branches, then later smoothing/pressure passes. Prototyping a quadratic midpoint spline and a triple-window connect-line variant on Layer 5 both regressed (`midpoint` full mean about `0.588242`, isolated Vector mean `0.179714`; `triple_line` full mean about `0.597772`, isolated mean `0.194482`). This suggests the SQLite Layer 5 point stream is already a smoothed/exported centerline sample stream, so re-running the runtime sampling algorithm on top of it over-smooths the preview. Keep the Catmull fallback until the actual serialized `CSVCurve` / smoothing output format is decoded.

PSD export reference follow-up: `PlugIn/PAINT/ExportPSD.dll` was checked as a possible official conversion guide. Static PE parsing shows it exports only `TriglavPluginCall` and does not statically import `iswCoreTG.dll`, `TGXPGPlugInCore.dll`, `RCPatternDraw`, or the CSP vector renderers. The opcode `0x402` path asks the host/plugin interfaces for canvas dimensions, resolution, color mode, layer/offscreen handles, alpha channels, and output buffers, then writes PSD records. `FUN_1800289a0` only stores an export mode field; real layer traversal happens through `FUN_180024d50` / `FUN_180027fe0` over host-provided layer handles. This makes PSD export useful as a reference for CSP's Photoshop downgrade packaging, but not for recovering frame/vector brush pixels directly.

PSD layer mapping follow-up: the layer traversal has a clear split. Type `7` is a Photoshop group/layer-set path and writes the `</Layer set>` marker while recursively walking children. Type `5` writes Photoshop text metadata (`textGridding`, `antiAliasSharp`, `warpStyle`/`warpNone`, `bounds`, `boundingBox`, `TextIndex`) plus raster/offscreen fallback. Type `4` calls `FUN_180027610` to write PSD adjustment descriptors. Ordinary drawable layers go through offscreen/mask writers such as `FUN_1800269c0`, `FUN_180028000`, and `FUN_18002b760`. `FUN_18002b760` queries offscreen width/height/pixel kind through host interfaces, converts grayscale PSD output with fixed-point luminance weights about `0.299/0.587/0.114`, and `FUN_18002e2b0` packs RGB rows by compositing partial-alpha color channels toward white while transparency is stored separately. For importer work, this supports using PSD as a filter/adjustment serialization clue, while continuing to chase CSP's own V4/brush renderer for frame and vector fidelity.

ExportPSD adjustment-dispatch follow-up: `FUN_180027610` asks the host adjustment interface for an adjustment kind, then branches over the same type range used by the `.clip` filter layers. The writer cases map exactly to Photoshop tags: `1 -> brit`, `2 -> levl`, `3 -> curv`, `4 -> hue2`, `5 -> blnc`, `6 -> nvrt`, `7 -> post`, `8 -> thrs`, and `9 -> grdm`. The conversion helpers show the plugin is consuming host/runtime adjustment data, not parsing SQLite `FilterLayerInfo` directly: Level converts runtime 16-bit level records down to Photoshop's byte/gamma records, Tone Curve right-shifts runtime point coordinates by 8 before writing Photoshop Curves, and Gradient Map builds a larger Photoshop gradient descriptor from host-provided stops. This makes PSD export a strong cross-check for tag serialization and stored-value scale, but not a substitute for the importer's CSP-side filter payload parser.

Actual PSD export follow-up: the user's `img/test_Filters_Vector_Text.psd` was parsed directly. PIL's flattened PSD composite matches `test_Filters_Vector_Text.png` exactly (`max=0`, `mean=0`, visible diff `0`), so this PSD is a reliable official export reference. The PSD contains 18 layer records. The adjustment layers are zero-sized PSD adjustment records with native tags and a `-2` mask channel: `hue2`, `levl`, `curv`, `blnc`, `thrs`, `grdm`, `brit`, `post`, and `nvrt`. The exported `娴嬭瘯娴嬭瘯` layer is raster-only in this sample (`lspf/luni/tsly`, no `TySh`). The frame exports as a layer set with `</Layer set>`, `Frame background 1`, empty `Layer 4`, a masked `Frame 1` group record, and a separate rasterized `Frame 1` line layer. No `vmsk`, `vscg`, `SoCo`, or `GdFl` vector/shape tags are present.

PSD layer-channel follow-up: decoding PSD RLE layer channels confirms the official frame downgrade split. `Frame background 1` is full-canvas white, `Layer 4` is empty, and the rasterized `Frame 1` line layer has alpha bbox `(101,738)-(633,987)`, `7444` nonzero alpha pixels, and mean RGB `(70,30,126)`. The group `Frame 1` mask is a solid 255 rectangle at `(104,741)-(629,983)` with default mask value `0`. The adjustment layer masks also decode exactly to the `.clip` masks once the PSD mask-record default value is honored outside each mask rect (`max=0` for all nine filter masks). This validates the current adjustment-mask decode and shows PSD stores frame fill/mask and frame line separately, while the importer still approximates them with one conservative generated frame image because the original frame cached externals are missing.

Frame PSD-split fallback rejection: a prototype that separated the generated white frame fill bbox from the purple line bbox using the PSD evidence regressed the full sample. Current header-inset-2 rectangle remains `max=225 / mean=0.577998 / visible=48622`; using the parsed point bbox as fill with expanded line bboxes produced means from `0.6848` to `1.1374` and higher visible pixels. Even though PSD exposes the official rasterized frame split, the importer should not retune the synthetic fallback to that single exported PSD geometry without reconstructing CSP's real frame-line renderer and group mask path.

PSD adjustment payload follow-up: the tag payloads are a useful reference for filter parameters. `blnc` directly exposes signed 16-bit Color Balance values including `43,-48,48`; `brit` stores brightness `91` and contrast `127`; `thrs` stores `179`; `post` stores `8`; `levl` stores version `2` and first channel `[0,255,0,164,100]` with following channels identity. `curv` is especially useful: CSP exports a Photoshop Curves payload rather than the raw `.clip` 32-curve compact blob. Per Adobe's Curves format, each point is stored as `(output, input)`, so the PSD master curve decodes to `(input, output)` points `(0,0) -> (62,183) -> (235,138) -> (255,255)`, followed by identity RGB curves. Those 8-bit points match the compact `.clip` master curve divided by 257 within rounding, but the PSD's low-resolution curve is still a downgrade reference; the importer should continue using CSP's 16-bit B-spline LUT for rendering.

PSD Gradient Map payload follow-up: the exported `grdm` tag also preserves a downgraded version of the `.clip` compact stops. The PSD color stops are in Photoshop's `0..4096` location domain and match `floor(stop_raw / 8)` from the `.clip` type-9 nodes (`178 -> 22`, `7726 -> 965`, `32146 -> 4018`). PSD colors are the high byte repeated as a 16-bit value (`0x448a -> 0x4444`, `0x4de2 -> 0x4d4d`, etc.), whereas the importer currently rounds the original 16-bit node colors and uses the slightly adjusted `32768 * 256 / 255` stop denominator that best matches CSP's 256-entry runtime LUT and the isolated PNG. Therefore PSD `grdm` validates the source stop/color mapping but should not replace the importer with Photoshop's lower-resolution `0..4096` / high-byte-floor representation.

Gradient Map PSD-style rejection: an in-memory variant sweep confirms the PSD descriptor should remain a reference, not the render path. Current importer (`rounded colors`, `32768 * 256 / 255` stops) is full `mean=0.577998`, visible `48622`; isolated Gradient Map is `mean=0.052949`, visible `2102`. PSD-style stops (`raw/32768` or `floor(raw/8)/4096`) expand isolated visible pixels to about `205k`, even when colors stay rounded. PSD-style colors with current stops lower the full mean to `0.538834`, but worsen full visible pixels to `52908` and isolated mean to `0.150883`. This looks like error redistribution through the full filter stack, not a correct Gradient Map formula.

PSD text-balloon layer follow-up: the exported PSD's text/balloon raster layer is a useful official downgrade target, but it does not beat the current fallback as a general rule. PSD layer #12 has bbox `(137,79)-(478,323)`, `58322` nonzero alpha pixels, and mean RGBA about `(214.4,207.3,227.5,252.3)` on nontransparent pixels. The current importer fallback for layer `16` has bbox `(139,79)-(478,322)`, `57642` nonzero alpha pixels, and mean RGBA about `(216.4,209.8,228.9,254.1)`, with raw PSD-layer diff `max=255 / mean=2.309893 / visible=13992`. A focused geometry sweep found `(left,top,right,bottom)=(2,2,2,1)`, `power=2.6`, width `5` as the best PSD-layer candidate (`mean=2.289653`), but it slightly worsens the isolated bubble PNG and increases full visible pixels (`48624` vs current `48622`). Keep the current `(3,3,2,2)`, `power=2.6`, width `4` fallback until the real CSP text-frame brush renderer is reconstructed.

Text/cache and Brightness residual follow-up: moving the text cache paste rect by `-3..+3` px in either axis regresses both the full image and the isolated bubble export, so the TLV cache rect is already aligned. The remaining Brightness/Contrast isolated residual is only 28 pixels over the 1-LSB threshold. All 28 are fully masked pixels with the same decoded pre-filter color `(111,81,152)`; CSP's export is `(+1,+2,+1)` or similar after brightness. Other filters and surrounding pixels are exact or within 1 LSB, so this is more likely a tiny source-raster decode/quantization mismatch at sparse antialias pixels than a filter formula error. Do not special-case Brightness/Contrast for that color.

Brightness/Contrast alpha-quantization rejection: a targeted in-memory compositing test tried normal-layer quantization and alternate source-alpha denominators against the pure Brightness/Contrast PNG and the full stacked sample. Using `alpha / 256` plus per-composite quantization eliminates the isolated 28 visible pixels (`max=1`, `visible=0`), but it badly regresses the full sample (`mean=0.768757`, `visible=145391` versus current `mean=0.579766`, `visible=48641`). Global 8-bit quantization alone is neutral on the full sample and slightly worse on the isolated one. Ghidra confirms the adjustment-layer code has multiple LUT entry points (`SetLookUptableBrightness`, `CreateLookUpTableCBTC`, `setLookUptable2`), so the current isolated-verified additive brightness path should stay until a broader sample proves the exact stored-value-to-runtime-LUT conversion.

Installed material database follow-up: checking the CSP install clarified where the rough outline material lives. `C:\Program Files\CELSYS\CLIP STUDIO 1.5\CLIP STUDIO` mostly contains launcher/material-manager UI resources, while `CLIP STUDIO PAINT\Settings\PAINT\BrushPreset\<lang>\BrushPreset.bfps` and `Tool\<lang>\EditImageTool.todb` are standard SQLite Planeswalker databases with `Manager`, `Node`, and `Variant` tables. The installed rough-line URI `.:Paint112:22de8cd4d9-c846-7d8f-4631-82b57d1eed:data:material_0.layer` appears as UTF-16LE in all language `BrushPreset.bfps`, `EditImageTool.todb`, and `UXEditImageTool.todb`. In Japanese `BrushPreset.bfps`, `粗いペン` is `VariantID=105` with `BrushUsePatternImage=1`, `BrushPatternOrderType=3`, and that `BrushPatternImageArray`; in Japanese `EditImageTool.todb`, the same material backs `粗いペン` variants `781/782`, `粗い角丸フキダシ` variants `783/784`, and `粗い長方形コマ` variants `785/786`. This confirms the rough balloon/frame line is a shared installed default material reference, not merely EXE-embedded data or sample-local `.clip` metadata, though the renderer bridge to V4 pattern mode remains separate from the traced `DrawToVector` path.

Installed material package follow-up: the rough-line URI resolves to `C:\ProgramData\CELSYS\CLIPStudio\Common\Material\Install\Paint112\22de8cd4d9-c846-7d8f-4631-82b57d1eed`. `CatalogMaterial.cmdb` row `_PW_ID=375` maps the package to the rough-line material, `CatalogUuid=22de8cd4d9-c846-7d8f-4631-82b57d1eed`, `MaterialUuid=916e6d59b6-3a4c-af80-6f59-bb0ba6dead`, and `Path=.:Install:Paint112:22de8cd4d9-c846-7d8f-4631-82b57d1eed`; tags include `Resizable`, `BrushPattern`, the image-material brush folder, and monochrome/art-material style tags. The package contains `catalog.xml`, `catalogMaterial.cac`, `info.xml`, `icedata/layerData.xml`, `thumbnail/thumbnail.png`, and encrypted `data/material_0.layer`. `layerData.xml` confirms `systemtag=BrushPattern`, `isTiling=1`, and `directionTiling=2`. `material_0.layer` is a `C2F` container (`89 43 32 46`) with `HEAD`/`dATA` markers, embedded SQLite-like schema strings for layer/mipmap/offscreen/material tables, and one embedded `24x2512` transparent PNG close to the sample's decoded `23x2511` rough strip dimensions; direct `sqlite3` reconstruction from likely page boundaries is malformed. Main-program strings show named `C2F` classes (`C2FCatalogElement`, `C2FCatalogGroup`, `C2FCatalogItem`, `C2FCatalog`) plus material-library paths. Useful Ghidra jump targets are `0x140024b04` for `InstallPathToCatalogId.dat`, `0x1426aed25` and `0x1426d9237` for `layerData.xml`, and `0x144007d44`, `0x144008a54`, `0x14400a3c2`, `0x14400a4b2`, `0x14400a672` for `HEAD/TAIL/dATA` C2F marker handling.

Installed material C2F decode follow-up: `catalog.zip` and `info.zip` are also C2F containers, not ordinary ZIPs. `material_0.layer` splits into two `dATA` payloads: a fixed `0x140a` first segment and a variable `0x25c02` second segment for this package, each followed by trailer/check fields before the next length or `TAIL`. Concatenating the payloads and stripping the leading 12 bytes gives exactly `39 * 4096` bytes. Pages 1-2 remain encrypted/obfuscated, but pages 3+ are standard SQLite b-tree pages. A manual b-tree decoder reads schema/table rows for `Canvas`, `CanvasPreview`, `Layer`, `Mipmap`, `MipmapInfo`, `Offscreen`, and related scheme tables; the rows identify a `24x2512` canvas, two layers with layer 2 named `粗い線`, three mipmap/offscreen rows, and a transparent preview PNG.

Installed material Offscreen tile follow-up: the actual rough-line bitmap is in `Offscreen.ImageData`, not the preview PNG. The first manual SQLite pass missed overflow pages and saw only the local `1008` byte payload; adding standard SQLite overflow-page reconstruction restores row 3 as a `9188` byte BLOB. Rows 1 and 2 decode to ten empty single-channel tiles each. Row 3 has `23x2511` attributes and ten zlib-compressed `BlockDataBeginChunk` tiles; cropping the tile stack yields a complete `23x2511` rough strip with `35603` nonzero pixels. The user's `.clip` sample embeds the exact same base image as `BrushPatternImage` MainId `4` / Mipmap `80` / Offscreen `457` (`max diff 0` against the installed package strip), while lower mipmap external ids are absent and can be regenerated. Because `layerData.xml` marks the material as tiled, CSP likely samples/repeats this strip through the brush pattern renderer. This confirms the material asset is recoverable, but importer fallbacks should still not directly stamp the alpha strip; earlier direct use regressed both isolated and full checks.

Brush preset mapping follow-up: sample BrushStyles `6` and `7` are the document/runtime forms of the installed rough balloon/frame presets. They are identical except `AntiAlias` (`2` for text balloon, `1` for frame), and share `StyleFlag=115248`, `FlowBase=1.0`, `Hardness=1.0`, `IntervalBase=1.0`, `AutoIntervalType=2`, `ThicknessBase=1.5`, `RotationEffector=3`, `RotationRandom=1.0`, `PatternStyle=7`, `CompositeMode=0`, `DualCompositeMode=1`, and `ChangeDrawColorTarget=1`. `BrushPatternStyle` `7` points at `BrushPatternImage=4`, has `OrderType=3`, and stores `Reverse2=34` (`0x22`). In the installed Japanese `EditImageTool.todb`, `粗い角丸フキダシ` variants `783/784` normalize to the sample balloon style, while `粗い長方形コマ` variants `785/786` normalize to the frame style: preset interval `10.0 -> 1.0`, thickness `150 -> 1.5`, rotation random `100 -> 1.0`, pattern order `3`, ribbon enabled, and reverse horizontal/vertical `2/2 -> 0x22`.

Text/frame vector object header follow-up: `VectorObjectList.VectorData` stores an external id, not the object body. The corresponding `CHNKExta` body contains a 100-byte object header after the external-id wrapper. In that object body, bbox is at `+24`, colors at `+40..+63`, opacity is a big-endian double `1.0` at `+64`, BrushStyle id is at `+80` (`6` for balloon, `7` for frame), a subtype-like field is at `+84` (`4` / `5`), and line width is a big-endian double `2.5` at `+88`. This connects the layer geometry, embedded brush material, installed preset, and sample BrushStyle rows. The renderer bridge remains the open piece: the traced frame/text ruler route still clears V4 pattern mode before ordinary sampling.

RCPatternDrawParam boundary follow-up: direct PE export/import inspection and Capstone disassembly show that `iswCmnTG.dll` owns the `RCPatternDrawParam` setters and runtime conversion helpers. The setters copy plain parameter structs into known offsets: size `+0x148/+0x150`, color `+0x154..+0x164`, hardness `+0x168/+0x170`, rotate `+0x19c..`, interval `+0x1ac..+0x1bc`, and pattern data into the block beginning at `+0x08`. But the checked modules do not statically import a BrushStyle-to-`RCPatternDrawParam` setup path. `iswCoreTG.dll` imports only `RCPatternDraw::DrawSinglePattern`; `TGXPGPlugInCore.dll` imports `CSPattern` helpers and `CreateSumiPattern*`; `CLIPStudioPaint.exe` imports no `RCPatternDrawParam` API by name. Internal setter xrefs in `iswCmnTG.dll` mostly belong to `CreateSumiPatternCore` and built-in pattern creation, not document BrushStyle conversion.

V4 pattern branch follow-up: `CSVec4Sampling::InitRasterOperation @ 0x12477090` only flips pattern mode (`+0x60=1`), stores the `RCPatternDraw*` at `+0x68`, and stores a rotate/tangent flag at `+0x70`. `SetLayer @ 0x1247b180` clears those fields. `InitSamplingForRuler @ 0x12482090` calls `SetLayer(...,0,1)` before `StartSampling`, so the frame/text ruler route is forced into ordinary V4 stroke creation. `Sampling @ 0x1247a370` only reaches `PlotPreSamplePattern` / `PlotCurvePattern` and the imported `DrawSinglePattern` when `+0x60` is nonzero. This leaves the confirmed rough-line BrushStyle/PatternStyle/material chain as document metadata for the observed missing-cache path; exact reproduction needs the separate high-level tool/brush renderer that constructs an `RCPatternDraw` with BrushStyle-derived params before `InitRasterOperation`.

Dynamic-loading boundary follow-up: a full scan of both installed CSP trees found no dynamic bridge by name. `CLIPStudioPaint.exe` has only seven `GetProcAddress` / `LoadLibrary*` call sites, all for Windows package/WebView functions, and the broader 76-module dynamic-loading scan resolves CUDA/runtime/system/plugin-bootstrap names rather than `InitRasterOperation`, `RCPatternDrawParam`, `RCPatternDraw::BeginDraw`, or the param setters. Focused import/export parsing gives the same boundary: `iswCoreTG.dll` imports only `RCPatternDraw::DrawSinglePattern`; `TGXPGPlugInCore.dll` imports only `RCPatternDraw::CreateSumiPattern*`; no checked module imports `RCPatternDraw::BeginDraw` or `RCPatternDrawParam::Set*`. The similarly named exports in `iswCoreTG.dll` are not the brush bridge (`CSVectorDraw::BeginDraw` and `CSAdjustLayerFilterData::SetPatternParam`). Disassembly of `RCPatternDraw::BeginDraw @ 0x121243b0` shows it consumes an already-built `RCPatternDrawParam*`: it copy-constructs a `0x1288` param object into draw object `+0xe0`, initializes random state, and converts/enables pattern data only if param `+0x11ec` is set. A raw internal `E8/E9 rel32` scan finds only one direct `BeginDraw` caller in `iswCmnTG.dll`, at `0x12124d55` inside `RCPatternDraw::CreateSumiPatternCore`; direct setter refs for size/color/rotate/hardness/etc are in that same built-in sumi-pattern constructor, while `SetPatternParam` and `SetIntervalParam` have no direct rel32 call refs. So the open problem is still constructing that param block from BrushStyle/PatternStyle/tool state, not finding a hidden import of `BeginDraw`.

RCPatternDraw call-graph follow-up: direct `E8/E9 rel32` scanning over all `RCPatternDraw`, `RCPatternDrawMethod`, and `RCPatternDrawParam` exports shows two self-contained islands inside `iswCmnTG.dll`. The first is the built-in sumi helper: `CreateSumiPattern` / `CreateSumiPatternCS -> CreateSumiPatternCore`, where CSP constructs `RCPatternDrawParam`, `RCPatternDrawMethod`, and `RCPatternDraw`, sets size/hardness/size-rate/rotate/color/airbrush/etc params, then runs `SetOffscreen -> BeginMethod -> BeginDraw -> DrawStroke -> EndDraw -> EndMethod`. The second is the external stamp consumer path used by `iswCoreTG.dll`: `DrawSinglePattern -> DrawStrokePattern / DrawStrokeSegment -> PlotPattern`, with `GetNextInterval`, `BeginPlotParam`, `NextPlotParam`, and conversion helpers as internal callees. There is no third internal call island that compiles document BrushStyle/PatternStyle records into `RCPatternDrawParam`. This makes the current boundary sharper: `iswCmnTG.dll` owns the low-level pattern engine, while the BrushStyle compiler is either outside the checked static/dynamic paths or part of a higher-level runtime/tool state not reached by the traced export/render routes.

Tool-key schema follow-up: a targeted string/RIP-reference pass checked the obvious Planeswalker/tool keys. Across the install, `BrushUsePatternImage`, `BrushPatternImageArray`, `BrushPatternOrderType`, `BrushPatternReverseHorizontal`, and `BrushPatternReverseVertical` occur in the main EXE plus the Planeswalker tool databases; `StrokeAutoIntervalTypey`, `StrokeIntervalPercent`, `BrushTexKind`, and `BrushTexForStroke` remain localized to `TGXPGPlugInCore.dll`. In `CLIPStudioPaint.exe`, each pattern-key string has exactly one RIP reference, and the disassembly is the same schema-registration shape: load string into `rdx`, load descriptor/global into `rcx`, call `FUN_142049220`, then tail-call `FUN_1438ca758`. Confirmed stubs include `BrushPatternImageArray @ 0x14010b0d0`, duplicate `BrushPatternOrderType` at `0x14010b130` / `0x14010b160`, `BrushPatternReverseHorizontal @ 0x14010b190`, `BrushPatternReverseVertical @ 0x14010b1c0`, and `BrushUsePatternImage @ 0x14010b910`. This rules out the visible tool-key strings as runtime `RCPatternDrawParam` construction code.

CSVec4Sampling field-write follow-up: a `.pdata`-driven scan plus targeted raw disassembly checked writes to the key sampling fields. `CSVec4Sampling::InitRasterOperation @ 0x12477090` and `CSVec4Sampling::SetLayer @ 0x1247b180` are tiny leaf functions without normal `.pdata` function records, which explains why some function-boundary scans miss them. Raw rel32 scanning still finds zero code refs to `InitRasterOperation`, and exactly three calls to `SetLayer` (`0x12481ce3`, `0x12481ef0`, `0x12482136`) in the `CSVectorizeV4` sampling initialization family. In the `CSVec4Sampling` region, only `InitRasterOperation` performs the enabling write pattern: `+0x60 = 1`, `+0x68 = RCPatternDraw*`, `+0x70 = flag`. `SetLayer` performs the clearing pattern: `+0x60 = 0`, `+0x68 = 0`, `+0x70 = 0`. Nearby writes are stack temporaries, connection/sample cleanup, or `+0x74` pattern-distance state inside `PlotCurvePattern` / `PlotPreSamplePattern` after pattern mode is already active. This rules out a second hidden field-level pattern setter in the V4 sampling code checked so far.

RCPatternDrawParam pattern-block follow-up: `SetPatternParam` is just a `0x140` byte struct copy into `RCPatternDrawParam+0x08..+0x147`; `GetPatternParam` copies the same block back out. `InitPatternParam` clears runtime `+0x08`, `+0x3c`, zeroes `+0x40..+0x13f`, and clears count at `+0x140`. The copied source struct's pattern pointer array is at source `+0x38..+0x137`, which lands at runtime `+0x40..+0x13f` as 32 `RCPattern*` slots. Runtime `+0x140` is the pattern count and `+0x144` is the low-level order selector. `ConvertPattern` shows the low-level order enum: count `0` returns null, count `1` returns slot `0`, order `0` advances sequentially, order `1` bounces forward/back using `+0x1214/+0x1218`, and order `2` selects randomly via random state `+0x1284`. `NextPlotParam` may use a designated pattern at `+0x1228` or override/cache pointer at `+0x1220` before falling back to `ConvertPattern`. This means `BrushPatternStyle.ImageIndex=[4]` is not directly consumable by the low-level engine; a missing compiler must resolve image ids/materials into actual `RCPattern*` objects and map document `OrderType` values into the lower-level `0/1/2` pattern selector.

RCPattern interval follow-up: Arkana rechecked `RCPatternDrawParam::GetNextInterval @ 0x12128410` in `iswCmnTG.dll`. The function reads interval mode from `+0x1ac` and base size from `+0x148`; mode `1` uses `ConvertInterval` directly, while mode `2` calls `ConvertInterval`, divides by `100.0`, calls a pattern-scale conversion helper, multiplies by the base size, and clamps through `maxsd` before returning. This matches the rough balloon/frame BrushStyle rows that store `AutoIntervalType=2` and `IntervalBase=1.0`, but it also confirms those SQLite fields do not give a standalone stamp spacing: runtime pattern scale, converted size, and the already-built `RCPatternDrawParam` are still required.

RCPattern image-pattern follow-up: the low-level subclasses in `iswCmnTG.dll` are now explicit. `RCCirclePattern`, `RCRectPattern`, `RCPolygonPattern`, and `RCImagePattern` correspond to pattern types `0`, `1`, `2`, and `3`. `RCImagePattern::ImportPatternOffscreen` allocates a `0x50` byte `RCSimpleImage` and copies an input `RCVOffscreen` into it; `ExportPatternOffscreen` copies that image back to an offscreen, `GetPatternOffscreenSize` reads the stored image width/height from `+0x34/+0x38`, and image drawing uses `RCTextureMap::Texmap`. This tells us the decoded rough-line material would become an `RCImagePattern` if we were reproducing CSP's native setup. The missing piece is still the setup code: `BeginPlotParam` only auto-constructs built-in circle/rect/polygon patterns for types `0..2`, while type `3` expects an existing `RCPattern*` pointer in the pattern array or designated slot. A full PE import scan found no checked module importing `RCImagePattern` construction/import, `SetDesignatedPattern`, or `SetPatternParam`, so importer fallbacks should continue treating `BrushPatternImage=4` as metadata until the material-offscreen-to-`RCImagePattern*` compiler path is found.

CSPattern bridge follow-up: `CSPattern` was rechecked as the possible missing compiler and does not currently bridge the gap. Its relevant exports (`CreatePatternObject`, `PushPatternElement`, `SetPattern`, `SetRaster`, `UpdatePatternObject`, `UseFixedSize`, `GetRaster`) build and update a separate procedural/raster pattern table object. `UpdatePatternObject` calls internal canvas/gauss/maculature table builders or copies raster data, but a raw rel32 scan finds no calls to `CSVec4Sampling::InitRasterOperation`. `iswCoreTG.dll` still imports only `RCPatternDraw::DrawSinglePattern` from `iswCmnTG.dll`, not `RCImagePattern` or `RCPatternDrawParam` setters. For importer planning, this reinforces the split: `CSPattern` can explain material/adjustment/XPG texture descriptors, while exact rough balloon/frame rendering still needs the missing runtime path that creates `RCImagePattern*` and feeds `RCPatternDrawParam`.

TGXPG pattern-material island follow-up: Arkana mapped the concrete `TGXPGPlugInCore.dll` callers for the pattern imports. `sub_15047df0` is the only caller of `RCPatternDraw::CreateSumiPattern` and `CreateSumiPatternCS`; it branches on material signatures such as `RBMS`, `RBSC`, and related pattern keys, creates/uses an `RCVOffscreen`, derives a small size/count parameter from file data, and calls `CreateSumiPatternCS` at `0x150483cc` or `CreateSumiPattern` at `0x150483d4`. The surrounding xrefs show this island is reached from material/tool readers (`sub_1504c780`, then `sub_150467d0` / `sub_15047120`) and local material helpers, not from the `CSFrameFolderLayer::DrawToVector` or `CSTextFrameV4Layer::DrawToVector` missing-cache render path. `sub_150477d0` and `sub_1503e290` are separate `CSPattern` construction paths: they call `CSPattern` ctor/dtor, `Copy`, `CreatePatternObject`, `SetPattern`, and `UpdatePatternObject`. This strengthens the conclusion that TGXPG can compile material packages into `CSPattern`/`RCPatternDraw` objects, but it is still a package/material compiler island rather than the document BrushStyle-to-frame/text renderer bridge.

TGXPG DrawToVector wrapper follow-up: `CSFrameFolderLayer::DrawToVector` and `CSTextFrameV4Layer::DrawToVector` each have a single TGXPG caller, `sub_1503a2e0`. That function creates a target layer with `CSLayer::CreateLayer`, reads the source layer category/kind at `+0x18c`, then branches: kind `0x800001` calls `CSTextFrameV4Layer::DrawToVector` at `0x1503a40c` with a local `RCTArray<int>`, while kind `0x40000001` calls `CSFrameFolderLayer::DrawToVector` at `0x1503a429`. Its callee set contains vector/stroke traversal helpers and `GetHeadPoint`, but no `CSPattern`, `RCPatternDraw::CreateSumiPattern*`, `RCPatternDrawParam` setters, or `CSVec4Sampling::InitRasterOperation` around the conversion calls. TGXPG's frame/text vectorization path is therefore a wrapper around the core `DrawToVector` implementations, not the missing BrushStyle/PatternStyle material bridge.

V4 pattern-mode escape-hatch follow-up: the checked static paths do not hide a second pattern-mode bridge. `RCPatternDraw::DrawSinglePattern` is called only four times in `iswCoreTG.dll`: two calls in `CSVec4Sampling::PlotCurvePattern` and two in `PlotPreSamplePattern`. `PlotCurvePattern` is reached only from `CreateBezierCurve`; `PlotPreSamplePattern` is reached only from `Sampling`; and `Sampling` is called by the `CSVectorizeV4` Init/Do/End sampling family. `CSVec4Sampling` copy/assignment preserve pattern fields `+0x60/+0x68/+0x70/+0x74`, but those copy paths are only used by `CSVectorizeV4` copy/assignment, with no relevant caller in the frame/text route. Ordinary `CSVectorizeV4` construction creates a default sampler and clears pattern mode. The apparent raster-brush helpers are also not used here: `DrawRasterStroke` and `IsRasterBrushOperation` have zero direct callers, and `DrawStraightLine`'s pattern branch is only reachable from that unreferenced raster-stroke path. A focused ordinal-import scan also found no module importing `InitRasterOperation` or the relevant `RCPattern`/`RCPatternDrawParam` APIs by ordinal.

Bitmap text/frame ruler follow-up: the non-vector bitmap branch also does not consume the rough BrushPattern material directly. `CSTextFrameV4Layer::DrawRulerForBitmap @ 0x12329830` and `CSFrameLineLayer::DrawRulerForBitmap @ 0x12363fc0` walk ruler objects, convert points through `CSRuler::RulerToPage`, then dispatch through frame-line virtuals: `+0x628` for `DrawSingleLineForBitmap`, `+0x640` for `DrawDoubleLineForBitmap`, with straight/curve/polygon helper variants underneath. Those helpers use `CSBitmap::FillPolygon`, `CSBitmap::DrawSegment`, `CSBitmap::FillPolygonConvex`, temporary offscreen composition, spline subdivision, and double-line offset geometry. `CSFrameFolderLayer::InternalRender` likewise uses offscreen ruler functions and bitmap fill helpers for mask/fill cases. No `RCPatternDraw::DrawSinglePattern`, `CSVec4Sampling::InitRasterOperation`, `RCImagePattern`, or `RCPatternDrawParam` setup appears in this branch. For importer planning, the installed rough-line material and `BrushStyle=6/7` remain important metadata for a future native brush renderer, but direct pattern perturbation stays rejected for current frame/text fallbacks.

CSBitmap primitive follow-up: the lower primitives confirm that branch is ordinary geometry rasterization. `FillPolygon` and `FillPolygonConvex` build scanline spans and call `CSBitmap::DrawSimpleSegment`; `DrawSegment`, `DrawSimpleSegment`, and `RectFill` write destination channel bytes directly according to the bitmap pixel kind, with special-case allocation/copy helpers for some fills. There is no pattern/style/brush call in this layer. Rough frame/balloon edges in native CSP therefore need either cached pixels or a different brush-material route, not a texture hook hidden inside the bitmap primitive functions.

Preview/utility module follow-up: `ClipPreview.dll`, `LipPreview.dll`, `scan.exe`, and `CLIPStudioUpdater.exe` also contain `BrushPatternImage`, but only inside the same contiguous schema string block that lists `ElemScheme`, `Canvas`, `Layer`, `Mipmap`, `Offscreen`, `VectorObjectList`, `BrushPatternStyle`, `BrushStyle`, `ToolPatternInMaterial`, and ruler table names. There are no direct code refs to the individual string and no imports of `iswCoreTG`, `iswCmnTG`, `RCPatternDraw`, `CSPattern`, or V4/vector renderer APIs in those modules. For importer planning, these preview/utility binaries are schema/database consumers, not the missing brush renderer bridge.

Frame raster/cache follow-up: `CSFrameLineLayer::FrameLineRender` and `FrameLineRenderForVector` both dispatch to the `DrawRuler` vfunc, but `FrameLineRenderForRaster` does not. Its calls stay in layer/offscreen virtuals and helper routines for extents, copy/resample/merge-style operations, with no direct `CSVec4Sampling`, `RCPatternDraw`, `DrawRuler`, or `CSBitmap` polygon primitive calls. `CSFrameFolderLayer::FrameRenderMain` / `Render` similarly delegate through child layer/mask/image virtuals. This is consistent with a cached frame-line raster path: if the document has the comic-frame-line external, rough pixels can be preserved as baked raster data; if the cache is missing, the fallback path is the ordinary geometry/V4 reconstruction already traced.

Importer cache-reuse follow-up: the decoder now has a generic high-resolution mipmap-to-offscreen helper and the frame-folder fallback checks `Layer.ComicFrameLineMipmap` before returning its conservative synthetic rectangle. The cached line is only alpha-composited when the referenced offscreen external is present and has nonzero alpha. On `test_Filters_Vector_Text.clip`, `Layer 30` still points `LayerRenderMipmap=81`, `LayerLayerMaskMipmap=82`, and `ComicFrameLineMipmap=83` to absent externals (`extrnlid21594...`, `extrnlid61ED...`, `extrnlidD516...`), while child `Layer 32`'s background render is also absent (`extrnlidF7DEF...`), so this change intentionally leaves the current verification unchanged at max `225` / mean `0.577998` / visible `4.636955%`. The purpose is future preservation of baked rough frame pixels when a `.clip` actually includes the comic-frame-line cache, not synthetic rough-brush generation.

Cache availability sweep: scanning the current `img/*.clip` corpus found no positive example where a folder-like layer's `LayerRenderMipmap` external is present in `CHNKExta`. Large Live2D samples have many `LayerType=0` / `LayerFolder=1 or 17` rows with render mipmap ids, but every checked folder render id resolves only to an absent `extrnlid...` SQLite reference. The only document with `ComicFrameLineMipmap` is still `test_Filters_Vector_Text.clip`, and its frame render, frame mask, comic line, and gradation-background externals are all absent. Therefore the importer should not currently short-circuit frame/folder rendering through `LayerRenderMipmap`: there is no sample-backed proof that the folder render cache is shipped, fresh, or child-safe, and using it as a whole-folder image could double-composite children. The narrow `ComicFrameLineMipmap` hook remains the safer cache-preservation point because it overlays only the dedicated frame-line plane when present.

Mendel cache-boundary follow-up: the function split supports the same importer rule. `FrameLineRender` / `FrameLineRenderForVector` dispatch to `DrawRuler`, `FrameMaskRender` uses the same line renderer and then mutates 1-bit mask pixels, while `FrameLineRenderForRaster` stays in layer/offscreen virtual copy/resample/read-write operations. `CSFrameFolderLayer::FrameRenderMain` / `Render` delegate across mask layer, frame image layer, frame line, and children rather than proving a parent-folder `LayerRenderMipmap` short-circuit. Keep parent folder render mipmap as diagnostic-only until a shipped external and child-safe semantics are demonstrated.

Isolated balloon/frame object follow-up: the new `Test_Ballon.clip` and `Test_Frames.clip` samples showed that `VectorNormalBalloonIndex` layers can store multiple 100-byte object headers in one `VectorObjectList` external body. The old fallback used only the first header via `_vector_header_bbox`, leaving most pure balloon objects and two of the first frame layer's rectangles invisible. A scan for `(header_len=100, point_header_len=76, stride_a in {88,104}, stride_b=88)` recovers eight headers in `Test_Ballon` layer 8, one each in layers 9/10, three in `Test_Frames` layer 21, and one in layer 25. The importer now draws every recovered object for balloon/frame fallbacks. Frame folders with no child background render outline-only, matching the sparse alpha in `Test_Frames` layer 25 better than filling the rectangle. A direct polygon fill from the following point records was tested and rejected (`Test_Ballon` premul mean regressed from `7.240398` to `9.724630`), so the current code keeps the tuned superellipse geometry. Metrics: `Test_Ballon` premul mean improves from `61.385823` before any pure-balloon fallback to `7.240398`; `Test_Frames` lands at premul mean `1.703037`; the full `test_Filters_Vector_Text` sample is unchanged at max `225` / mean `0.577998` / visible `4.636955%`. Remaining errors are edge geometry/antialiasing and CSP brush/frame renderer details, not missing object enumeration.

Vector fill-object follow-up: `Test_Vector.clip` exposed a separate vector-body record shape after the ordinary `92/76/88/88` stroke. The tail contains two `92/76/120/88` headers with `flags=0x41`; the first has `point_count=1` and a bbox `(545,31)-(873,359)` that does not correspond to visible red pixels, while the second has `point_count=2` and bbox `(210,378)-(994,715)`, matching the large filled red component in the PNG. The importer now handles only this conservative case (`stride_a=120`, `flags=0x41`, `point_count>=2`) by drawing a tuned red superellipse from the line color. The initial asymmetric inset `(4,8,12,8)`, power `3.4` improved `Test_Vector` from max `187` / mean `28.138416` / visible `25.023746%` to max `187` / mean `3.290668` / visible `4.442596%`; after the later vector-AA pass, a local bbox sweep found `(7,5,19,5)` is better, moving the current `Test_Vector` mean from `3.287201` / visible `46691` to `3.062404` / visible `44778`. A corpus sweep found this exact active fill-object pattern only in `Test_Vector.clip`, and regression checks show `test_Filters_Vector_Text`, `Test_Ballon`, `Test_Frames`, `Test_ToneCurve`, and `Test_Gradiation` unchanged.

Monochrome PSD/export follow-up: the remaining `Test_Monochrome` error was a plane interpretation bug, not a missing preview alpha block. Direct PSD layer-channel parsing shows `Layer 1` exports `13324` opaque black pixels, `409070` opaque white pixels, and `190004` transparent pixels whose RGB bytes are still white. The `.clip` base offscreen has only two 1-bit planes, and the first plane count is exactly `422394`, matching PSD alpha (`13324 + 409070`); the second plane count is `599074`, matching the PSD white RGB flag, including transparent residue. Therefore `LayerColorTypeIndex=2` is `alpha/presence` first and `white flag` second: render white where `alpha & white`, black where `alpha & !white`, and transparent where `!alpha` regardless of the stored white flag. `Test_Monochrome` is now bit-perfect (`max=0`, `mean=0`), while `Test_ Grayscale`, `Test_Vector`, `test_Filters_Vector_Text`, `Test_Ballon`, `Test_Frames`, `Test_ToneCurve`, and `Test_Gradiation` remain unchanged.

Brush anti-alias setting follow-up: CSP's tool UI exposes anti-alias choices `none / weak / medium / strong`, and the document `BrushStyle.AntiAlias` field appears to store that ordinal (`0..3`). Current samples line up with this: rough balloon styles use `AntiAlias=2` (medium), rough frame styles use `AntiAlias=1` (weak), and `Test_Vector` includes vector styles with `AntiAlias=3` (strong). This is a real renderer parameter, separate from the rough-line material/pattern. Current synthetic fallbacks should not apply a generic soft edge globally; instead, exact support needs the CSP scan-conversion / brush renderer to consume this field together with pen head, width, pressure, pattern, and frame/balloon geometry.

TGXPG Arkana anti-alias bridge follow-up: Arkana can connect to the `clip-studio-paint-vector-frame` project and load `TGXPGPlugInCore.dll` directly. `XPGPlugInCore::ReadPageFile` is exported at RVA `0x5000`, and Arkana decompilation shows the entry calls `CSMain::GetMain()`, then `CSVectorSetting::SetAntiAliasSimple(ptr->field_e0, *((int *)(index + 4)))`, followed by copies from the same `SXPGPlugInReadPageParameter`-like struct offsets `+8` and `+0xc` into `CSMain`/context fields `+0x244` and `+0x248`. Raw disassembly corroborates the parameter flow: `r14` is loaded from incoming `r8`, `edx` from `[r14+4]`, `rcx` from `[rax+0xe0]`, and the unique call bytes `FF 15 EE CE 01 00` at file offset `0x4444` / VA `0x15035044` target the `SetAntiAliasSimple` IAT entry (`0x15051f38`). This proves TGXPG has a top-level/simple vector AA parameter independent of per-brush `BrushStyle.AntiAlias`. It does not yet justify a new importer rule, because this AA value is supplied by the XPG read-page/export context rather than by the `.clip` layer's BrushStyle row; the current `0.20px` vector feather remains a narrow sample-backed fallback.

CSVectorSetting simple-AA setter follow-up: opening `iswCoreTG.dll` in Arkana and resolving the export table puts `CSVectorSetting::SetAntiAliasSimple` at RVA `0x273f0` / VA `0x122673f0` / file offset `0x267f0`. The function body is only `mov dword ptr [rcx + 8], edx; ret`, so the value passed by TGXPG's `ReadPageFile` is stored directly into `CSVectorSetting+0x08` with no translation, clamping, or mapping inside the setter. This strengthens the field-location evidence for the global/simple AA setting, but it also means the mapping from document/UI `none / weak / medium / strong` to final scan conversion still lives in callers/renderers outside this tiny setter.

CSVectorSetting simple-AA consumer follow-up: targeted Arkana byte searches for `CSMain+0xe0` loads found core-side readers of the same field. At `0x122a6744..0x122a6754`, the code calls the `CSMain` getter, loads `rcx = [rax+0xe0]`, then tests `dword ptr [rcx+8]`; in the matched layer/type branch (`[rcx+0x18c] == 0x80001` and `[rcx+0x10c] == 0x10` before the getter), a nonzero simple-AA value returns `2` early. The second reader was rechecked against the PE section map (`.text` VA `0x1000`, raw pointer `0x400`) because the first note mixed raw offsets and VAs: the bytes at raw `0xbbd00` start at VA `0x122fc900`, so the relevant path is `0x122fca31..0x122fca45`, not `0x122fbd31..0x122fbd45`. That path calls the same getter, loads `rdx = [rax+0xe0]`, and conditionally moves `[rdx+8]` into `esi` when a render-parameter field at `[rbp+0x18]` is at least `8`, before continuing through mode/scale math. Immediately before the stroke helper call, the code moves that AA value with `mov r9d, esi` and passes the mode value in `edi` as the stack argument at `[rsp+0x20]`, then calls `0x1246b590`. The callee has its own prologue, expands the bbox/rect at `rdx` by one pixel on each side, and tests `r9d`; when nonzero and `[rcx+0x1a0]` is null it allocates/initializes an auxiliary object and stores it at `[rdi+0x1a0]`. The initializer at `0x12454df0` stores the mode argument at `[rcx+8]`, then builds a `0x400` scratch/work region (using a static path for mode `8`, otherwise allocating `0x460` bytes and passing the mode to `0x12299b10`). `0x12299b10` stores width/height/mode/stride at `+0x20/+0x24/+0x28/+0x2c`; the mode-specific final helper `0x12455090` dispatches `0x10 -> 0x12455110`, `0x20 -> 0x12455560`, and `0x8 -> 0x124558d0`. The `0x12455560` path walks 4-byte pixel groups from the scratch buffer, accumulates four 4-byte samples, divides by the accumulated sample/coverage term, and writes destination pixels. This confirms `CSVectorSetting+0x08` is consumed by core stroke render/control paths, not only written by TGXPG; however, in this chain the AA value is still an on/off gate for a 4x scratch/resolve path, while weak/medium/strong coverage strength remains unrecovered.

Vector anti-alias micro-pass: ordinary 92-byte vector stroke headers carry a BrushStyle id at header offset `+76` (`5` in `test_Filters_Vector_Text` layer 5, `4` in the active ordinary stroke of `Test_Vector`). Looking up `BrushStyle.AntiAlias` and enabling a small `0.20px` feather only when the style's anti-alias flag is nonzero improves the measured vector fallback without pretending to implement the full `none/weak/medium/strong` renderer. Both observed ordinary vector styles are `AntiAlias=1` (`weak`). `Test_Vector` moves from mean `3.290668` / visible `46584` to mean `3.287201` / visible `46691`; the full `test_Filters_Vector_Text` stack moves from mean `0.577998` / visible `48622` to mean `0.569085` / visible `48576`. `Test_Monochrome`, `Test_ Grayscale`, `Test_Ballon`, `Test_Frames`, `Test_ToneCurve`, and `Test_Gradiation` are unchanged. Bigger feathers keep lowering the full-stack mean but raise vector visible pixels too much, so `0.20px` is the current compromise.

Naive 4x vector-AA rejection: after Arkana confirmed the nonzero simple-AA path wraps stroke rendering in a 4x scratch/resolve helper, a temporary monkey-patch replaced the ordinary vector fallback's `0.20px` feather with simple 4x box-downsampled hard-line rendering. This regressed `Test_Vector` to mean `3.099804` / visible `45613` and the mixed sample to mean `0.629371` / visible `50415`, versus current `Test_Vector` mean `3.062404` / visible `44778` after the fill-bbox retune and mixed mean `0.569085` / visible `48576`. CSP's 4x path is real, but a naive 4x polyline does not match its pen-head/curve renderer.

Current BrushStyle sample matrix: scanning the vector-bearing samples through the importer-backed SQLite decoder gives a narrow AA/style corpus. Ordinary vector strokes are weak AA only: `test_Filters_Vector_Text` layer 5 uses BrushStyle `5` with `AntiAlias=1`, `PatternStyle=0`, `AutoIntervalType=3`, and `Test_Vector` layer 5 uses BrushStyle `4` with `AntiAlias=1`, `PatternStyle=0`, `AutoIntervalType=1`. The active `Test_Vector` fill object uses BrushStyle `6` with `AntiAlias=3`, `PatternStyle=0`, `AutoIntervalType=2`. Text/frame rough objects are not the same as those ordinary strokes: the text balloon in `test_Filters_Vector_Text` uses BrushStyle `6` with `AntiAlias=2`, `PatternStyle=7`, `ThicknessBase=1.5`, `AutoIntervalType=2`, while the frame style uses BrushStyle `7` with `AntiAlias=1`, `PatternStyle=7`, `ThicknessBase=1.5`, `AutoIntervalType=2`. `Test_Ballon` covers extra balloon styles (`AntiAlias=2` with PatternStyles `10/11`, and `AntiAlias=3` with PatternStyle `0`). This explains why the current importer can safely use AA as metadata and a few sample-backed fallbacks, but cannot infer a general four-level `none/weak/medium/strong` renderer from the present corpus.

Vector fill-object bbox retune: re-sweeping only the conservative `92/76/120/88`, `flags=0x41`, `point_count>=2` fill-object branch after the Arkana AA investigation found that tightening the right edge and expanding vertically is a clean isolated-vector win. Changing the fill bbox inset from `(4,8,12,8)` to `(7,5,19,5)` with the same `power=3.4` improves `Test_Vector` from mean `3.287201` / visible `46691` to mean `3.062404` / visible `44778`. `test_Filters_Vector_Text`, `Test_Ballon`, `Test_Frames`, `Test_Monochrome`, `Test_ Grayscale`, `Test_ToneCurve`, and `Test_Gradiation` remain unchanged.

Vector pattern-object fallback: `Test_Vector` also contains a `92/76/120/88`, `flags=0x41`, `point_count=1` object at bbox `(545,31)-(873,359)` with BrushStyle `5`. That style has `PatternStyle=2`, whose `BrushPatternImage 2` is `Koa_Lace1` (`Mipmap 10`), a white lace strip of `732x82`. The point record stores the center at `(709,195)`, so scaling the pattern to the header width (`328px`) and centering it vertically at that point places it at the missing reference component `(545,177)-(873,213)`. The importer now handles only this narrow pattern-object shape by decoding the brush pattern mipmap, resizing it with a small NumPy bilinear helper, and alpha-compositing it into the vector fallback. `Test_Vector` improves from `mean=3.062404` / `visible=44778` to `mean=2.999559` / `visible=43411`; `test_Filters_Vector_Text`, `Test_Ballon`, `Test_Frames`, and the filter matrix are unchanged.

Simple vector size-dynamics follow-up: the ordinary vector point record stores more than centerline coordinates. Offsets `+16/+20/+24/+28` look like per-point pen-head integer bounds, and offsets `+36/+40` vary in pressure-like ways for brush-dynamic strokes. A direct `+36/+40` size/alpha interpretation was rejected because it regressed `test_Filters_Vector_Text_Vector.png` from mean `0.159297` to `0.833209` and produced segment-overlap banding. A safer branch now only handles the simple observed size-effector shape: `BrushStyle.SizeEffector` is exactly 12 bytes and starts with flag `0x11`. For those ordinary strokes, point `+36` replaces the old `+52` endpoint taper as the radius multiplier; alpha/flow remains unchanged until the real flow/opacity accumulation model is recovered. This improves `Test_Vector` from `mean=2.999559` / `visible=43411` to `mean=1.258227` / `visible=28807`, while the full `test_Filters_Vector_Text` stack stays `mean=0.569085` / `visible=48576`, its isolated vector export stays `mean=0.159297` / `visible=8857`, and balloon/frame/monochrome/grayscale/tone-curve/gradation/filter checks are unchanged.

Complex vector dynamics rejection: `test_Filters_Vector_Text` BrushStyle `5` has a 16-byte `SizeEffector` with flag `0x111`, plus a 12-byte `OpacityEffector` (`0x11`, minimum `0.3`, graph `5`), `FlowBase=0.5`, `TexturePattern=3`, and `TextureDensityBase=0.4`. Its first three ordinary vector strokes store point `+36/+40` as constant `1.0`, while the fourth stores varying `+36` (`0..0.590343`) and `+40` (`0.611111..0.744444`). Extending the simple `+36` size rule to `0x111` was rejected: the isolated vector export regressed from mean `0.159297` / visible `8857` to mean `0.669362` / visible `21170`, and the full mixed sample regressed to mean `1.157390` / visible `60097`. Treat the `0x111` path as a different brush-dynamics form that also needs opacity/flow/texture semantics, not a safe alias for the simple `0x11` size branch.

Tool-preset effector boundary: installed Planeswalker tool databases (`Settings\PAINT\Tool\japanese\EditImageTool.todb` and `BrushPreset\japanese\BrushPreset.bfps`) store verbose Variant-level effector blobs, not the compact `.clip` `BrushStyle.*Effector` form. In `EditImageTool.todb`, common `BrushSizeEffector` blobs start with their byte length/shape (`48`, `44`, `40`, `92`, `168`, etc.), while `.clip` sample BrushStyle blobs are compact forms such as `0x11`, `0x111`, `0xc1`, and `0x21`. Exact compact blobs from the samples (`000000113d8f5c2900000003`, `000001113f733333000000043f400000`, `000000113e99999900000005`, `000000c13f0000003f59999900000006`, `000000213e800000000000043f800000`) do not occur verbatim in the Japanese Variant table. So preset DBs are useful for locating UI/tool parameters, but document import/export has already compiled them into a smaller runtime/document BrushStyle representation; importer rendering should key off the `.clip` BrushStyle rows and point records, not directly off preset Variant blobs.

Vector dynamics inspection helper: `inspect_vector_dynamics.py` now dumps vector object headers, per-point field statistics (`+36/+40/+44/+52/+56/+60/+76`), per-point pen-head bbox sizes, used BrushStyle rows, compact effector words, and `BrushEffectorGraphData` control points as JSON. This replaces ad hoc probing for future samples. It supports single files, multiple files, glob patterns, `--out`, `--summary`, and `--summary-only`; for example `python inspect_vector_dynamics.py "img\*.clip" --summary-only --out tmp_vector_probe\vector_corpus_summary.json`. A corpus scan of the current 38 `img/*.clip` files finds only seven recognized 92-byte vector objects: five ordinary `92/76/88/88` strokes across two clips, plus two `92/76/120/88` `Test_Vector` pattern/fill objects. The ordinary strokes are one simple `0x11` size/flow style in `Test_Vector` and four complex `0x111` size + `0x11` opacity strokes in `test_Filters_Vector_Text`. This confirms the present corpus is too small to infer opacity/flow semantics statistically.

Pure-balloon strong-AA follow-up: the corpus scan shows pure balloon objects in `Test_Ballon` cover `AntiAlias=2` and `AntiAlias=3`, while frame objects are all `AntiAlias=1`. Selective experiments showed medium balloon AA is not safe to soften (`AntiAlias=2` lowers mean but raises visible pixels by about `1600`), but strong balloon AA is a small win. The importer now carries the BrushStyle id out of 100-byte object headers and uses 2x supersampled superellipse drawing only for pure balloon objects where `BrushStyle.AntiAlias >= 3`. `Test_Ballon` improves from premul mean `4.707546` / visible `59309` to premul mean `4.700233` / visible `59298`. `test_Filters_Vector_Text` is unchanged because its text-balloon fallback uses the cached text path and has `AntiAlias=2`; `Test_Frames` is unchanged because frame styles are weak AA and the integer-aligned rectangle fallback did not benefit from supersampling.

Pure-balloon geometry retune after strong-AA: re-sweeping only the pure balloon fallback after the strong-AA rule found a cleaner global bbox inset for the isolated balloon corpus. Keeping `power=2.0`, outline width `4`, and top/right/bottom insets `(3,2,2)`, but changing the left inset from `3` to `2`, improves `Test_Ballon` again from premul mean `4.700233` / visible `59298` to premul mean `4.625291` / visible `59290`. `test_Filters_Vector_Text` remains unchanged because the text-cache balloon path keeps its own `(3,3,2,2)`, `power=2.6` geometry. `Test_Vector`, `Test_Frames`, `Test_Monochrome`, `Test_ Grayscale`, `Test_ToneCurve`, and `Test_Gradiation` are unchanged.

Pure-balloon point-bbox follow-up: the 100-byte object point records are not a drawable polygon by themselves, but their point bbox is a strong geometry clue. For `Test_Ballon`, using a hybrid draw bbox, defined as a blend of the current header-inset bbox and the point-record bbox expanded by 2 px on each side, improves the whole isolated balloon corpus substantially. The importer now stores a `point_bbox` alongside each recovered 100-byte object header and applies that hybrid bbox only in `_balloon_fallback_image`; frame and text-cache balloon paths are unchanged. A small follow-up sweep found that weighting the point bbox at `0.55` beats the exact average without affecting other samples. `Test_Ballon` improves from premul mean `4.625291` / visible `59290` to premul mean `2.317777` / visible `47104`. `test_Filters_Vector_Text`, `Test_Frames`, `Test_Vector`, `Test_Monochrome`, `Test_ Grayscale`, `Test_ToneCurve`, and `Test_Gradiation` remain unchanged.

Empty-frame outline follow-up: after the balloon point-bbox pass, a small frame sweep found that lowering the frame outline globally from width `4` to `3` improves the isolated `Test_Frames` mean but regresses the full mixed sample (`test_Filters_Vector_Text` mean `0.577412 -> 0.703954`). The safe rule is narrower: only frame folders with no child/background layers use width `3`, while filled frame folders keep width `4`. This improves `Test_Frames` premul mean `1.703037` / visible `12807` to premul mean `1.470779` / visible `11817`; `test_Filters_Vector_Text`, `Test_Ballon`, `Test_Vector`, `Test_Monochrome`, `Test_ Grayscale`, `Test_ToneCurve`, and `Test_Gradiation` remain unchanged.

Transparent RGB export follow-up: CSP's transparent-background PNGs in the current corpus keep fully transparent pixels at white RGB (`Test_Ballon`, `Test_Frames`, `Test_AddGlowMultiply*`, and `Test_RealArt` refs all show white as the dominant transparent color). The importer previously emitted black RGB when final premultiplied alpha was zero, which is visually equivalent in premultiplied comparisons but can create raw-diff noise and worse straight-alpha texture filtering. The final `composite()` conversion now requests white RGB for `alpha == 0` pixels, while internal group-region conversions keep the old default. This does not change visible/premultiplied metrics: `Test_Ballon` stays premul mean `2.317777` / visible `47104`, `Test_Frames` stays `1.470779` / `11817`, `Test_Vector` stays `3.062404`, and `test_Filters_Vector_Text` stays mean `0.569085`. Raw means now line up with CSP's metadata convention (`Test_Ballon` `144.207671 -> 0.988340`, `Test_Frames` `152.387764 -> 1.085583`) and transparent RGB diffs go to zero on checked transparent-background samples.

CSVec4Sampling split recheck: after restarting Arkana, the cached `iswCoreTG.dll` project was usable again and the background `startup-angr` / `auto-enrichment` tasks were aborted before targeted work. Lightweight disassembly reconfirmed the V4 pattern boundary without full decompilation. `InitRasterOperation @ 0x12477090` sets `CSVec4Sampling+0x60 = 1`, stores the `RCPatternDraw*` at `+0x68`, and stores the tangent/rotate flag at `+0x70`. `SetLayer @ 0x1247b180` clears `+0x60/+0x68/+0x70`. `InitSamplingForRuler @ 0x12482090` calls `SetLayer` at `0x12482136`, then builds stack parameters from `CSVectorizeV4+0x200..+0x210` plus the default pen-head pointer and calls `StartSampling @ 0x1247b400`. Inside `Sampling @ 0x1247a370`, `ebp` is loaded from `[sampler+0x60]`; the zero path clears pattern caches at `+0xe0/+0xe8/+0xf0/+0xf8`, tests `ebp` at `0x1247a4ff`, and falls through to ordinary sample-list writes around `0x1247a5d8` / flush helper `0x1247a760`. The nonzero path preserves those pattern fields, creates/links a pattern sample node around `0x1247a548`, and calls `PlotPreSamplePattern @ 0x124797b0`. Arkana note `n_1778134687847635_10` records this conclusion. This strengthens the negative evidence: the traced frame/text ruler path cannot reach rough BrushPattern stamping unless some caller re-enables `+0x60` after `SetLayer`, and no such caller has been found in the checked route.

InitRasterOperation direct-reference follow-up: a narrow PE `.text` scan of cached `iswCoreTG.dll` found zero direct `E8/E9 rel32` call or jump references to `InitRasterOperation @ 0x12477090`. Searching the full file for the target VA/RVA also found no 64-bit VA or low 32-bit VA occurrences, and exactly one RVA occurrence (`0x00237090`) in `.rdata` at file offset `0x3a7530`, consistent with the export/function-address table rather than executable code. This matches Arkana's earlier xref shape: `InitRasterOperation` is a real exported pattern-mode setter, but no checked static code path in `iswCoreTG.dll` directly calls it.

Single-filter export recheck: the user's isolated `test_Filters_Vector_Text_*` PNGs are `Paper + Layer 1 + one adjustment layer`, and the paper color is the document `DrawColorMain*=0xe2e2e2e2` (`226`), not white. Replaying that setup keeps the existing filter matrix intact: Hue/Saturation `max=1` / `mean=0.007432`, Level Correction `max=1` / `0.005667`, Tone Curve `max=1` / `0.001581`, Color Balance `max=1` / `0.037455`, Threshold `max=1` / near zero, Posterization `max=1` / near zero, Reverse Gradient `max=1` / near zero, Brightness/Contrast only has `28` visible pixels above 1 LSB, and Gradient Map remains the main small residual at `max=8` / `mean=0.052949` / `visible=2102`.

Gradient Map variant rejection: a small sweep over stop denominators (`32768`, `32768*256/255`, nearby offsets), luma weights (`0.3/0.59/0.11`, Rec.601, integer Rec.601), luma rounding, and color rounding found no safer replacement. The current CSP-style luma floor with denominator `32768*256/255` keeps the isolated Gradient Map at `max=8` / `mean=0.052949`, the full `test_Filters_Vector_Text` at `max=225` / `mean=0.569085` / `visible=48576`, and `Test_Gradiation` at `max=10` / `mean=0.310779`. Rec.601/integer luma can shave `Test_Gradiation` mean to roughly `0.308`, but regresses the full mixed sample to about `0.589` mean and `63600` visible pixels, so no code change was kept.

Paper-aware verification follow-up: `verify_one_clip.py` now reports `paper_rgb` for every run and supports selected-stack checks through `--layers`, `--filters`, `--ref`, and `--no-paper`. This makes the single-filter setup reproducible without temporary scripts; for example `python verify_one_clip.py img\test_Filters_Vector_Text.clip --layers 3 --filters 14 --ref img\test_Filters_Vector_Text_gradientmap.png` renders document Paper (`[226,226,226]`) + Layer 3 + Gradient Map and reproduces the expected `max=8` / `mean=0.052949` / `visible=2102` metrics. Running with `--no-paper` is intentionally available as a negative control because it shows how badly white/transparent assumptions can skew filter conclusions.

Paper corpus sweep: all current Paper-bearing samples use the same visible Paper row shape: `LayerType=1584`, `DrawColorEnable=1`, `DrawColorMainRed/Green/Blue=0xe2e2e2e2`, matching thumbnail RGB `[226,226,226]`, with palette RGB `[0,0,0]`. Transparent-background samples such as `Test_Ballon`, `Test_Frames`, `Test_ToneCurve`, `Test_Gradiation`, and the Live2D refs have no Paper row and therefore keep transparent canvas initialization. `_paper_color()` was made schema-safe for older files where `DrawColorEnable` may be absent: nonzero `DrawColorMain*` still wins, but an all-zero draw color without the enable flag falls through to thumbnail/palette fallback instead of forcing black paper.

Filter-matrix verification helper: `verify_filter_exports.py` now automates the user's single-filter export matrix. It opens a `.clip`, initializes the real Paper color, renders the visible raster base layers below the first filter layer (auto-detected as Layer 3 for the current sample, or override with `--base-layers`) plus each filter layer, and compares against the `test_Filters_Vector_Text_<suffix>.png` references by filter type. On the current sample it reports the stable matrix in one run: Hue/Saturation `max=1`, Brightness/Contrast `max=2` with `28` visible pixels, Posterization `max=1`, Reverse Gradient `max=1`, Level Correction `max=1`, Tone Curve `max=1`, Color Balance `max=1`, Threshold `max=1`, and Gradient Map `max=8` / `mean=0.052949` / `visible=2102`. This is the preferred guard before changing filter math because it removes Paper/background ambiguity.

Filter-matrix threshold guard: the same helper now accepts optional `--max-max`, `--max-mean`, and `--max-visible-px` thresholds. Without thresholds it is report-only (`strict=false`) and exits successfully; with thresholds it switches to `strict=true` and returns exit code `1` if any filter exceeds a threshold or a reference is missing. The current matrix passes `python verify_filter_exports.py img\test_Filters_Vector_Text.clip --max-max 8 --max-mean 0.06 --max-visible-px 2200`; a deliberately strict `--max-visible-px 100` fails on the known Gradient Map residual. Use the threshold form before/after filter changes so regressions are caught by exit code, not eyeballing JSON.

Signed color-field guard: `_clip_color_component()` now treats negative integers as signed 32-bit color storage before extracting the high repeated byte. This keeps both observed unsigned `0xe2e2e2e2` and a possible signed representation `-0x1d1d1d1e` decoding to Paper component `226`; `0xffffffff` / `-1` decode to `255`, and ordinary 8-bit values are unchanged. The current corpus stores Paper as unsigned SQLite integers, but the signed guard prevents future host/schema variants from silently turning colored Paper black.

Reusable PSD layer inspection: `inspect_psd_layers.py` now provides a repeatable local PSD reference path instead of one-off parser snippets. It parses PSD headers, layer records, unicode names, section-divider tags (`lsct`/`lsdk`), adjustment/native tags, RLE channel data, transparency channels, and optional layer PNG exports. Example usage: `python inspect_psd_layers.py img\Test_Vector.psd --out tmp_vector_probe\Test_Vector_psd_layers.json --export-dir tmp_vector_probe\psd_layers_test_vector`. A fresh PSD corpus pass over `Test_Vector`, `Test_Ballon`, `Test_Frames`, `Test_Monochrome`, `Test_ToneCurve`, and `test_Filters_Vector_Text` writes `tmp_vector_probe\psd_layer_corpus_summary.json`.

PSD vector/balloon/frame split follow-up: the new PSD inspector confirms the official export is useful as a raster downgrade reference, but not a magic vector-parameter oracle. `Test_Vector.psd` has only `Paper` plus one rasterized `Layer 1` bbox `(210,177)-(983,714)` with `249095` nonzero alpha pixels; it does not split the ordinary pressure stroke, lace pattern object, and red fill object into separate PSD layers. Comparing the current no-paper layer-5 fallback against that PSD layer gives premultiplied `max=255`, `mean=1.467720`, `visible=31197`, with current alpha `244199` vs PSD alpha `249095`, so the remaining error is still brush/object rendering rather than layer packaging. `Test_Ballon.psd` does split the three balloon objects as separate raster layers, with alpha counts `241101`, `23410`, and `5344`; current per-layer premultiplied means are `1.666362`, `0.530791`, and `0.120625`. `Test_Frames.psd` matches the user's observation: frame exports are folder-like structures with `lsct` layer sets, full-white `Frame background 1`, empty child raster layers, and separate frame-line rasters (`Frame 1` alpha `16414`, `Frame 2` alpha `1616`). This validates the fallback targets and the frame folder model, but does not justify replacing the current constrained synthetic frame/vector code with PSD-style hardcoded layer splits.

PSD-layer balloon parameter sweep: using `Test_Ballon.psd`'s three separated raster layers as per-object references gives the same local optimum as the flattened PNG reference. Sweeping the current point-bbox blend weight, expansion, and left inset around the importer settings keeps `(expand=2, weight=0.55, top/right/bottom=(3,2,2), power=2.0, width=4)` as the best mean target; left inset `1` and `2` tie after integer rounding, and the current code uses `2`. No importer change was kept.

Simple vector pressure-radius retune: the PSD layer inspector made it possible to compare `Test_Vector` layer `5` directly against CSP's rasterized PSD vector layer. The residual split shows ordinary pressure stroke, lace object, and fill object are all in the single PSD layer; ordinary-stroke bbox `(247,193)-(529,412)` still had many over-alpha pixels after the first `+36` size-dynamics pass. A constrained sweep over only the simple `SizeEffector=0x11` branch found that reducing the ordinary-stroke radius base to `0.75 * 0.95 * width` and using `0.60px` feather for its nonzero AA improves both the flattened PNG and PSD-layer targets. `Test_Vector` improves from `max=187` / `mean=1.258227` / `visible=28807` to `max=187` / `mean=1.048985` / `visible=27837`. The complex `0x111` vector style in `test_Filters_Vector_Text` is deliberately unchanged: full sample stays `mean=0.569085` / `visible=48576`, and isolated vector export stays `mean=0.159297` / `visible=8857`. Balloon, frame, monochrome, grayscale, Tone Curve, Gradation, and filter-matrix checks are unchanged.

Vector fill-object micro-retune: after the simple pressure-radius pass, a corrected fill-only sweep rebuilt `Test_Vector` layer `5` from pre-rendered pressure stroke + lace plus candidate red fill objects. The actual fill color is `(117,39,40)`, not pure red, so an earlier pure-red skip probe was invalid. Around the previous `(7,5,19,5), power=3.4` setting, both the flattened PNG and PSD vector layer prefer `(9,5,21,5), power=3.6`. This is a tiny but clean win: `Test_Vector` improves from `mean=1.048985` / `visible=27837` to `mean=1.043658` / `visible=27806`, while `test_Filters_Vector_Text`, isolated vector export, balloon/frame, Tone Curve, Gradation, and the filter matrix stay unchanged.

IDA MCP reconnect follow-up: after adding `ida-pro-mcp` to both the global Codex config and the active `rizum-clip-studio-paint` workspace `.mcp.json`, Codex now exposes the native `mcp__ida_pro_mcp__*` tools. The active IDA instance is `iswCoreTG.dll.i64`, `auto_analysis_ready=true`, `hexrays_ready=true`, and the warmed string cache contains `9200` strings. Short IDA comments were added at `CSVectorV4Layer::RenderToOtherOffscreen @ 0x122fc900`, `CSVec4AntiAlias::Output @ 0x12455090`, `CSVec4Draw::RenderStroke @ 0x1246a630`, and the 40-byte pressure-node allocator `sub_12264A90`.

IDA simple-AA clarification: the native `CSVectorSetting+0x08` path is a global simple-vector AA on/off gate, not the per-brush `none/weak/medium/strong` ordinal. `CSVectorV4Layer::RenderToOtherOffscreen @ 0x122fc900` reads `CSMain+0xe0+0x08`; when it is nonzero, the layer's offscreen bit-depth field at `CSVectorV4Layer+0x10c` selects a `16` or `32` bit `CSVec4AntiAlias` scratch buffer before calling `CSVec4Draw::RenderTransform @ 0x1246b590`. `CSVectorV4Layer::ChangeAntiAlias @ 0x122fae30` toggles stored vector layers between offscreen formats `2` and `16` when the global switch changes. `CSVec4AntiAlias::Output @ 0x12455090` dispatches to `OutputFrom8Bit/16Bit/32Bit`, each of which downsamples a fixed 4x scratch region by summing a `4x4` subpixel block and right-shifting alpha by `4`; no weak/medium/strong kernel is selected in this layer. Therefore the importer should not map BrushStyle AA levels directly onto this global simple-AA mechanism.

IDA pressure-node clarification: `CSVStroke::Serialize @ 0x12462660` confirms runtime stroke fields `+0x18` flags, `+0x50` pen-head pointer, `+0x58` radius, and `+0x5c` ARGB color. `CSVCurve::Serialize @ 0x124620c0` links per-curve auxiliary nodes with a 40-byte stride. The node allocator `sub_12264A90 @ 0x12264a90` initializes a 40-byte node with defaults at `+0x10/+0x14/+0x18/+0x1c`; `MakeDefaultPressure` writes `+0x10=1.0` and `+0x14=t`, while `RenderStroke` reads `node+0x10` as the size multiplier and uses `node+0x18/+0x1c` as rotation/scale-like inputs depending on stroke flags. A trial importer branch that treated the `.clip` point `+40` field as opacity for compact `OpacityEffector=0x11` produced no measurable change on `test_Filters_Vector_Text` isolated vector, full mixed sample, or `Test_Vector`, and the IDA evidence does not yet justify keeping it. The candidate was reverted; future opacity/flow work needs a targeted sample where transparent pressure variation is isolated and visible.

New vector sample follow-up: the user's `Vector_AA_*`, `Vector_SizePressure`, `Vector_OpacityPressure`, `Vector_NoTexture`, and `Vector_Texture` files add clean coverage for vector flags/effectors. `inspect_vector_dynamics.py` finds 12 recognized vector objects across the new vector corpus. The AA files contain a new ordinary 92-byte/88-byte shape with `flags=0x2011` and point count `2`; the importer now accepts `0x2011` alongside the older `0x2081` ordinary-stroke shape and allows two-point strokes. This affects only the new AA samples in the current corpus: `Vector_AA_None` improves `mean 14.461335 -> 14.194455`, `Weak 14.330298 -> 14.049960`, `Medium 14.263398 -> 13.967681`, and `Strong 14.363166 -> 14.072733`, while `Vector_SizePressure`, `Vector_OpacityPressure`, `Vector_NoTexture`, `Vector_Texture`, `Test_Vector`, `test_Filters_Vector_Text`, `Test_Ballon`, and `Test_Frames` are unchanged. The AA means remain high because the four strokes were hand-drawn separately and are not strict pixel-aligned regressions.

New pressure-effector follow-up: `Vector_SizePressure` introduces a 24-byte `SizeEffector` with flag `0x31`, and `Vector_OpacityPressure` introduces a 24-byte `OpacityEffector` with flag `0x31`. Directly treating the size-pressure `0x31` style like the simple `0x11` size branch, i.e. using point `+36` as radius multiplier with the current `0.75 * 0.95 * width` base, regressed `Vector_SizePressure` from `mean=0.672778` / `visible=4162` to `mean=0.762008` / `visible=4714`, so that alias was rejected. A dedicated sweep for only 24-byte `SizeEffector=0x31` found the stable branch: use the `.clip` point `+36` value, clamp it to `0..1`, and keep a separate radius base of `1.05 * width` instead of the simple branch's `0.75 * 0.95 * width`. This improves `Vector_SizePressure` to `max=226` / `mean=0.115578` / `visible=715`. A separate sweep for only 24-byte `OpacityEffector=0x31` also points at `.clip` point `+36`, but as an opacity curve; later retunes now use `max(0.05, clamp(point36 * 1.55, 0, 1) ** 0.65)`, hard max-alpha preservation, `0.99 * width`, and a denser `3.5px` fallback sample step for that branch, improving `Vector_OpacityPressure` from `mean=1.846408` / `visible=30018` to `mean=0.262608` / `visible=26314`. `Vector_NoTexture` remains `mean=0.235198`, `Vector_Texture` remains `mean=0.900940`, and the old vector/filter guards remain unchanged (`Test_Vector mean=1.043658`, isolated `test_Filters_Vector_Text` vector mean=0.159297, strict filter matrix still passes).

Vector texture and compact 120-byte object follow-up: `Vector_Texture.clip` embeds its brush texture as a `BrushPatternImage` mipmap whose base `Exta` payload is a 512x512 one-channel alpha texture (`262144` bytes, mean `169.65`), not an RGBA mipmap. However, applying this texture as a simple post-stroke alpha modulation worsens both PSD-layer and final PNG metrics; the current final PNG remains best at `mean=0.900940`. IDA confirms why: `CSVec4Sampling::PlotCurvePattern @ 0x12479290` and `PlotPreSamplePattern @ 0x124797b0` build/interpolate `RCStrokePoint` values and call `RCPatternDraw::DrawSinglePattern` along the curve, so CSP applies pattern/texture inside the per-stamp brush rasterizer rather than multiplying a completed stroke mask. No texture code change was kept. The same PSD pass exposed a real object-classification bug in the AA samples: their `92/76/120/88`, `flags=0x41`, `point_count=2`, black `width=3` objects export as thin line layers (`alpha鈮?620`), while the importer treated every multi-point 120-byte object as a filled ellipse for the old red `Test_Vector` object. The importer now routes only small black 120-byte objects through a line fallback (`radius=0.60*width`, feather derived from BrushStyle AA), leaving large colored fill objects unchanged. This drops `Vector_AA_None` from `mean=14.194455` to `0.864298`, `Weak` from `14.049960` to `0.734641`, `Medium` from `13.967681` to `0.678055`, and `Strong` from `14.072733` to `0.649928`, with `Test_Vector`, pressure samples, texture/no-texture samples, isolated/full `test_Filters_Vector_Text`, and the strict filter matrix unchanged.

Bubble/frame new-sample follow-up: the new PSD exports make the frame/bubble fallback less guessy. `Bubble_Frame.psd` and `Bubble_Frame3_Fill.psd` export frame lines as black raster layers with mean visible alpha around `158`, not as full-alpha purple outlines. The importer now keeps black frame lines black, uses alpha `158`, and narrows only black frame outlines to width `2`; non-black legacy frame outlines keep the existing width/color path. This improves `Bubble_Frame` and `Bubble_Frame2` from `mean=0.401145` to `0.296084`, `Bubble_Frame3_Fill` from `0.463849` to `0.237134`, and leaves `Bubble_Frame4_Fill` effectively stable (`0.345811 -> 0.344158`). `Test_Frames` remains unchanged at `mean=1.085583`. The bubble shape PSDs also separate shape-specific behavior: `Bubble_Shape1` (`VectorNormalBalloonIndex=3`) wants a larger, heavier outline ellipse, while `Bubble_Shape2` (`VectorNormalBalloonIndex=2`) wants a thinner outline. The importer now applies shape-id-specific ellipse params only for vbi `2` and `3`: Shape1 improves `0.720651 -> 0.322687`, Shape2 improves `0.521636 -> 0.289637`, while old `Test_Ballon` vbi `4/5/6`, full `test_Filters_Vector_Text`, and the strict filter matrix remain unchanged.

Frame configurable color follow-up: the frame fallback now treats line/fill as document data instead of a fixed visual. `_vector_object_headers()` already extracts per-object line RGB and fill RGB from the vector object body; `_frame_folder_fallback_image()` now passes those through `_frame_line_style()` and `_frame_fill_color()`. Line rendering uses the vector object's line RGB, while `ComicFrameColorTypeBlackChecked` can suppress black frame lines and black checked lines use the PSD-observed alpha/width fallback. Fill rendering prefers a child gradation/background fill when present; otherwise `ComicFrameColorTypeWhiteChecked` keeps CSP's white frame background behavior, and only when that white fill is disabled does the fallback use the vector object's fill RGB. This preserves the current metrics (`Bubble_Frame=0.296084`, `Bubble_Frame3_Fill=0.237134`, `Test_Frames=1.085583`) while making custom frame line/fill colors reachable from file data.

Hardcoded fallback audit follow-up: a pass over the object-layer fallback paths removed two sample-specific assumptions without changing the verified corpus. First, `_text_cache_fallback_image()` now reads the first 100-byte vector object header for its balloon line RGB, fill RGB, and width; the old "black header means purple `(70,30,126)`" branch is gone, while `test_Filters_Vector_Text` stays at mean `0.569085` because the real object header already stores that purple line color. Second, frame/balloon/text fallback outline widths are derived from the vector object's width field (`ceil(width) + shape/fill extra`) instead of fixed `2/3/4/5` literals; all current headers use width `2.5`, so the existing tuned widths are preserved but wider/narrower future objects can move. Remaining empirical constants are now clearly bounded to renderer gaps rather than file metadata: PSD-observed black frame alpha `158`, frame/text/balloon bbox insets and superellipse powers, the compact 120-byte vector line/fill classification and fill inset `(9,5,21,5)`, ordinary vector subdivision `5px`, and the pressure/opacity curve approximations for compact `BrushStyle` effectors. These should be revisited only with new isolated samples or an IDA-confirmed CSP renderer rule.

Object opacity audit follow-up: the same 100-byte vector object header stores an opacity double at offset `+64`. All current frame/balloon/text object samples report `1.0`, and BrushStyle opacity/flow fields are also full for the black frame examples, so this field does not explain the PSD-observed black-frame alpha `158`. The importer now carries the object opacity through `_vector_object_headers()` and applies it to frame fill/line, balloon fill/outline, and text-cache balloon fill/outline. Current metrics are unchanged (`test_Filters_Vector_Text=0.569085`, `Bubble_Frame=0.296084`, `Bubble_Frame3_Fill=0.237134`, `Test_Ballon` premul mean `2.317777`, `Test_Frames` premul mean `1.470779`, strict filter matrix passes), but future semi-transparent object-layer samples no longer need another fallback constant.

Black frame alpha re-sweep: a direct audit of the black frame samples compared synthetic line alpha constants `0/64/128/158/192/255` against the current full compositor. `test_Filters_Vector_Text` is unchanged because its frame path uses the non-black object-header color and its comic-frame cache is absent. For `Bubble_Frame` / `Bubble_Frame2`, alpha `0` lowers mean error but removes the visible line, while alpha `255` regresses strongly (`0.404315`); the current PSD-backed alpha `158` stays the best visual compromise among line-preserving options (`0.296084`). `Bubble_Frame3_Fill` behaves similarly (`158=0.237134`, `255=0.317099`). `Bubble_Frame4_Fill` alone slightly prefers `192`, but not enough to replace the corpus rule. This confirms that object-header opacity `1.0` must not be interpreted as full-alpha black frame ink; the exact solution is still native frame-line/cache reconstruction, not a new hardcoded alpha.

Fallback constant audit follow-up: the remaining preview-only constants in the object/vector fallbacks are now named at module scope instead of being inline literals. `FRAME_LINE_FALLBACK_BLACK_ALPHA`, `FRAME_FALLBACK_BBOX_INSET`, `TEXT_BALLOON_FALLBACK_INSET`, `TEXT_BALLOON_FALLBACK_POWER`, `VECTOR_STROKE_RADIUS_SCALE`, `VECTOR_FALLBACK_SAMPLE_STEP`, and `VECTOR_OPACITY_PRESSURE_SAMPLE_STEP` are explicitly treated as renderer-gap fallback parameters. The data-bearing inputs remain parsed from `.clip` fields: object RGB, opacity, width, family id, line/fill style ids, `VectorNormalType`, `ComicFrameLineMipmap`, comic-frame color check flags, and child solid-gradation fill. Verification after the rename is unchanged: `Vector_SizePressure=0.115578`, `Vector_OpacityPressure=0.262608`, `test_Filters_Vector_Text=0.569085`, `Test_Vector=1.043658`, `Bubble_Frame=0.296084`, and `Bubble_Frame3_Fill=0.237134`.

Vector attribute inventory pass: `inspect_vector_dynamics.py` now treats vector analysis as a full layer/object/brush problem rather than only an ordinary-stroke probe. Per clip it dumps `VectorObjectList` layer metadata (`LayerOpacity`, composite/clip/mask/offscreen ids, `LayerColorType*`, `VectorNormalStrokeIndex/FillIndex/BalloonIndex/Type`, `MixSubColorForEveryPlot`, comic-frame flags), recognized 92-byte vector objects, recognized 100-byte balloon/frame/text objects, point-record stats and first-point samples, full relevant `BrushStyle` fields, `BrushEffectorGraphData`, and `BrushPatternStyle`/`BrushPatternImage` resources. A fresh 52-clip corpus summary finds 27 vector-bearing layers, 19 recognized 92-byte objects, and 22 recognized 100-byte objects. Current 92-byte families are `92/76/88/88 flags=0x2081` (9 objects), `92/76/88/88 flags=0x2011` (4 objects), and `92/76/120/88 flags=0x41` (6 objects). Current 100-byte families are `100/76/88/88` and `100/76/104/88`, with subtype and `VectorNormalBalloonIndex` separating balloon/frame/text-frame variants. This changes the next analysis priority: ordinary stroke rendering, compact 120-byte fill/pattern/line objects, and 100-byte balloon/frame/text objects should be investigated as three separate renderer families, all sharing `BrushStyle` but not the same geometry semantics.

Vector generalization policy follow-up: the project docs now state the vector/brush/frame rule explicitly: support different user settings by reading `.clip` fields and their enable flags, not by overfitting current sample values. Fields such as RGB, opacity, width, `LayerOpacity`, mask/clip ids, `BrushStyle.AntiAlias`, `PatternStyle`, `TexturePattern`, `TextureFlag`, and compact effectors should only influence rendering when the relevant feature is active. Empirical values such as black frame alpha `158`, superellipse parameters, compact 120-byte fill insets, and pressure/opacity curve approximations remain renderer-gap fallbacks bound to verified object families. Future samples should ideally vary one setting at a time, especially brush size/opacity/flow/AA/texture density and frame/bubble line/fill/opacity, so inactive defaults can be separated from active renderer parameters.

IDA ruler-object bridge follow-up: the active IDA `iswCoreTG.dll` session confirms the native frame/bubble ruler path stores important renderer parameters in `CSRulerObject`, not only in the compact `VectorObjectList` headers. Common offsets include line width `RCLength +0x98`, line ditch/double-line spacing `RCLength +0xb8`, tail width `RCLength +0xd8`, in/out lengths `RCLength +0xf8/+0x110`, closed flag `+0x58`, double-line flag `+0xb0`, draw mode `+0x128` (`bit0=line`, `bit1=fill`), draw color `+0x12c`, and have-draw-color flag `+0x12f`. `CSTextFrameV4Layer::ReadSelf` also serializes text-frame line color at `this+0x3b0` and inside/fill color at `this+0x3b4` immediately after `RCLength this+0x398`. This confirms our importer should continue treating line/fill color and width as document data, while recognizing that exact CSP frame/bubble rendering still depends on reconstructing the ruler object state and V4 sampling path.

IDA ruler fill/line render split: `CSRulerFunction::DrawSingleLine @ 0x1237a2c0` in the bitmap/offscreen route uses its two color arguments separately. If fill color `a8` has alpha, it converts ruler points to integer points and calls `RCVOffscreen::FillPolygon(..., a8, ...)`; if line color `a7` has alpha, it then calls `DrawPolygonLine(..., a7, ...)`. The vector route `CSRulerFunction::DrawSingleLine @ 0x1237a520` has no separate fill color parameter; the color is already seeded by `CSVectorizeV4::InitDrawRuler`, and `DrawPolygonLine @ 0x12379fe0` feeds points through `InitSamplingForRuler` / `DoSamplingForRuler` / `EndSamplingForRuler`. This explains why frame mask/line entry points can pass `0xff000000`: they may be generating a black alpha/shape layer first, while final frame fill/background color is composed through the frame folder's mask/image layers.

IDA frame-folder internal render follow-up: `CSFrameFolderLayer::InternalRender @ 0x122eb320` confirms frame folders maintain separate internal offscreen products. It locks both input offscreens, iterates the `CSRulerFunction` database at frame-folder offset `+0x1190/+0x1198`, clones each ruler object, converts ruler points into output resolution, then draws black line geometry into the first offscreen through `CSRulerFunction::DrawSingleLine(..., line=0xff000000, fill=0)`. For closed rulers (`object+0x58 == 1`) it also fills the second offscreen with polygon/rect geometry, using rectangle fast paths for four-point axis-aligned shapes and `RCVOffscreen::FillPolygon` otherwise. Double-line rulers first derive secondary points with `GetDoubleLinePoint` and draw the double-line black geometry. This matches the PSD model: frame folder rendering is line-shape plus filled mask/background composition, not a single colored rectangle.

IDA frame-folder offscreen/layer mapping follow-up: `CSFrameFolderLayer::RefreshRulerImage @ 0x122ebd80` is reached with `this` pointing at the embedded `CSRulerFunction` subobject (`frame + 0x1188`, qword `+561`), so its local fields must be translated back to the owning frame folder. It creates/resizes `this+9` as `RCVOffscreen::CreateOffscreen(2)`, i.e. frame qword `+570`, and `this+14` as `CreateOffscreen(1)`, i.e. frame qword `+575`; then it calls `InternalRender(frame, frame+570, frame+575)`. After rendering, it nudges qword `+580` and qword `+579`, respectively. `ReadSelf @ 0x122ebbc0` creates qword `+579` as `CSLayer::CreateLayer(0x100001, 3, pageDoc, 0)` and qword `+580` as `CreateLayer(1, 3, pageDoc, 0)`. The matching `WriteSelf @ 0x122ecdb0` serializes presence flags plus `RCVOffscreen::Write` payloads for qwords `+570` and `+575`, followed by `RCLength +576` and dwords `+571/+4572`; `GetUsedMemory @ 0x122eb250` also accounts for both offscreens. `Render @ 0x122ec010` copies offscreen `+575` into layer `+579`, then offscreen `+570` into layer `+580`; `FrameRenderMain @ 0x122ea630` uses the same pairing. `GetFrameImageExtentD @ 0x122eacf0` queries qword `+570` and transforms through layer `+580`, naming that pair as the frame-image/line-shape side. `SearchPixelForFrame @ 0x122ec350` performs hit testing directly against ruler polygons with `CSVectorMath::IntersectPoly`, not by sampling either offscreen. Combined with `InternalRender`, the practical map is: qword `+570` is the type-2 frame-image/line-shape offscreen drawn by black ruler line geometry and wrapped by bitmap layer `+580`; qword `+575` is the type-1 closed-ruler fill/mask offscreen wrapped by special layer `+579`. This is important for importer work because the two planes must not be treated as interchangeable cached images.

IDA frame extent/margin follow-up: `CSFrameFolderLayer::GetFrameMergin @ 0x122eaea0` computes frame margins from the native ruler database rather than from fixed pixel insets. It iterates the ruler objects at qword `+562`, starts from each object's rect at `+0x60/+0x70`, expands by double-line spacing `RCLength +0xb8` when `+0xb0 == 1`, always expands by line width `RCLength +0x98`, unions those rects, converts the result through `PageToLayer`, then compares each margin against the frame folder's serialized `RCLength +576` as a minimum. `GetExtentRect @ 0x122ea980` unions the ordinary bitmap-layer extent with `GetFrameImageExtent @ 0x122eac30`, and `GetFrameImageExtent` again queries offscreen `+570` through layer `+580`. This explains why importer constants such as the current 2 px frame bbox inset are only placeholders: the real rule depends on parsed ruler rects, widths, double-line spacing, and the frame-folder margin length.

IDA ruler rect serialization clarification: `CSRulerObject::Read @ 0x123a2b70` does not deserialize rect `+0x60/+0x70` as a standalone archive field. Instead, it reads the point count and each point record, appends the 32-byte point node, then calls `RCRectD::UnionRect(this+0x60, point)` while building the cache; after the point loop it calls virtual `+0x140` to recalculate/finalize object geometry. The serialized source fields used by frame margins are confirmed by `CSRulerObject::Write @ 0x123ad6e0`: line width `RCLength +0x98`, double-line flag `+0xb0`, double-line spacing/line ditch `RCLength +0xb8`, point count, point records, draw mode/color/style fields, and optional four-point data. Therefore a faithful importer should parse ruler point records and lengths, then recompute rects; it should not search for a raw stored rect field in the compact vector-object header.

IDA cubic-ruler node follow-up: `CSRulerCubicBezier::Read @ 0x1236b6b0` first reads the common `CSRulerObject`, then reads a child section with node count `this+0x18c` and a 64-byte node array at `this+0x170`; each node stores six doubles at `+0/+8/+16/+24/+32/+40` followed by u32 fields read in order `+48/+60/+56/+52`, plus a trailing mode/flag at `this+0x190`. `Write @ 0x1236c450` writes the same layout. The vector draw path `CSRulerCubicBezier::DrawSingleLine @ 0x123686e0` uses the array as cubic segments: node `+0` is the anchor, current node `+32` is the outgoing control point, next node `+16` is the incoming control point, next node `+0` is the next anchor, and node `+48` is passed to `CSVectorizeV4::DoSamplingForRuler` as a segment flag. It subdivides by `CalcBezierLengthFast * 0.0625`, clamped to `1..32`, after `InitSamplingForRuler` and before `EndSamplingForRuler`. The compact SQLite vector point records expose some matching concepts (`x/y`, per-point flags/dynamics), but they are not a raw copy of this 64-byte runtime node because current 88/104-byte records also contain pen bbox and brush-dynamics fields.

IDA cubic bitmap/offscreen branch follow-up: `CSRulerCubicBezier::DrawSingleLine @ 0x12368d60` is the bitmap/offscreen wrapper. It creates a temporary V4 layer, calls the vector cubic sampler, and only renders the outline to the destination offscreen when the line color alpha byte is nonzero. If fill is requested, it temporarily sets line width `RCLength +0x98` to zero, samples the same curve into an integer `RCTArray<RCPoint>`, then calls `RCVOffscreen::FillPolygon` with the fill color before restoring the width. The higher-level `CSRulerFunction::DrawSingleLineCurve @ 0x1237a560` handles cubic objects in the generic bitmap path by cloning a ruler object, appending the supplied points, using `GetSplinePoints` and `GetSplineDivideCount(points, 128.0, 8, 64)`, then expanding each segment with `SplineCalc`; the allocated buffer is `65 * segment_count` points. It fills only when the caller's fill flag is set, and draws outline only when line color alpha is nonzero. So the bitmap curve branch is still plain geometry plus color alpha/fill flags, not brush-pattern stamping.

IDA generic spline helper follow-up: the generic bitmap curve helper is not the same as the 64-byte cubic-control-point vector path. `CSRulerObject::AddVertex @ 0x12394df0` appends a 32-byte common point node containing only anchor `x/y` plus zeroed aux data and updates rect `+0x60`. `GetSplinePoints @ 0x123a0770` fetches three consecutive anchor points, wrapping closed rulers and mirroring the first/last point for open endpoints. `GetSplineDivideCount @ 0x123a0670` computes `(distance(p0,p1) + distance(p1,p2)) / step`, clamps to `min..max`, and the call site uses `step=128.0`, `min=8`, `max=64`. `SplineCalc @ 0x123abe50` evaluates a three-point quadratic/spline blend: `0.5*(t-1)^2*p0 + (t-t^2+0.5)*p1 + 0.5*t^2*p2`. This means importer work should keep two curve approximations separate: native V4 cubic nodes use explicit Bezier controls and `length*0.0625` subdivision, while the generic bitmap curve helper uses anchor-only three-point spline interpolation.

Generic spline fallback rejection: a monkey-patch replaced the ordinary 92-byte vector stroke fallback's Catmull-Rom interpolation with the newly recovered three-point `SplineCalc` blend. It regressed the current guards: `Test_Vector` mean `1.043658 -> 1.057602`, full `test_Filters_Vector_Text` mean `0.569085 -> 0.571996`, and the isolated AA samples worsened substantially (`Vector_AA_None 0.864298 -> 1.155426`, `Weak 0.734641 -> 1.025213`, `Medium 0.678055 -> 0.973622`, `Strong 0.649928 -> 0.945246`). Keep the current ordinary-stroke Catmull/fitted fallback until the actual compact 92-byte stroke-to-runtime conversion is decoded; the recovered three-point spline belongs to the generic bitmap ruler helper, not this fallback.

IDA polygon/double-line outline follow-up: `CSRulerFunction::DrawPolygonLine @ 0x12379dd0` is another offscreen wrapper around the V4 sampler, creating a temporary vector layer and rendering it back into the destination offscreen. Its inner vector path `DrawPolygonLine @ 0x12379fe0` reads line width `RCLength +0x98`, optional in/out lengths `+0xf8/+0x110`, and calls `InitSamplingForRuler`. For exactly two points, it does not draw a single segment directly; it interpolates 100 intermediate samples. For more points it samples each point, passing flag `1` only for repeated points; closed rulers (`+0x58 == 1`) end sampling back at the first point. `CSRulerFunction::DrawDoubleLineStraight @ 0x12379a60` creates integer point arrays for the original and offset line, fills one polygon white (`0xffffffff`), fills the other with the caller's fill color, then calls `DrawPolygonLine` along the offset line. `DrawDoubleLineCurve @ 0x123794e0` does the same composition after expanding both original and offset curves through `GetSplinePoints` / `GetSplineDivideCount(...,128,8,64)` / `SplineCalc`; it stores the offset curve as `RCPointD` and outlines that side. This reinforces that frame double-line/mask behavior is a geometry composition pass, separate from ordinary single outline width.

Vector header raw-probe follow-up: `inspect_vector_dynamics.py` now includes `raw_header` for each recognized 92-byte and 100-byte object: the header hex plus big-endian u32/f32/f64 interpretations by relative offset. This is intentionally diagnostic rather than renderer logic. A quick probe over `Bubble_Frame`, `Bubble_Frame3_Fill`, `Test_Ballon`, and `Test_Frames` shows the known fields remain stable (`+40/+44/+48` line RGB, `+52/+56/+60` fill RGB, `+64` opacity double, `+80` BrushStyle id, `+84` subtype, `+88` width double), while still-unnamed fields split object families: frame-like 100-byte objects often have `u32+76=1040` and `u32+20=280`, ordinary balloon objects often have `u32+76=304` with `u32+20=296/392`, and `u32+96` looks like an object/id counter. Future single-setting samples should be compared through this raw probe before adding renderer branches.

Vector raw-header summary follow-up: `inspect_vector_dynamics.py --summary-only` now aggregates raw header u32/f64 values by offset, shape, subtype/flags, BrushStyle id, and object colors. The current 52-clip corpus still reports 27 vector-bearing layers, 19 recognized 92-byte objects, and 22 recognized 100-byte objects. The aggregated 100-byte headers reinforce the family split: `u32+76=1040` clusters with frame-like objects including black frame samples, while `u32+76=304` clusters with ordinary balloon objects; `u32+20` varies as `280/296/392` across frame/balloon point families; `u32+16` is the point count; `u32+80/+84` are BrushStyle/subtype; `f64+64` is opacity; `f64+88` is width; and `u32+96` remains an object/id-like counter. The 92-byte raw summary similarly confirms `u32+20` is the object flags and `u32+76` is BrushStyle. The raw f64 probe is now bounded to the header size, so invalid cross-header reads such as 100-byte `f64+96` are excluded. Output snapshots are in `tmp_vector_probe/current_vector_summary_v4.json` and `tmp_vector_probe/ruler_header_summary_v3.json`.

Frame mipmap resource probe follow-up: `inspect_vector_dynamics.py` now attaches `mipmap_resources` to each vector layer for `LayerRenderMipmap`, `LayerLayerMaskMipmap`, and `ComicFrameLineMipmap`, including mipmap id, offscreen id, external id, whether the `CHNKExta` payload is present, payload length, and decoded RGBA stats when possible. `tmp_vector_probe/bubble_frame_single_v2.json` shows `Bubble_Frame` layer 8 has all three frame resources named but absent (`external_present=false`) for render, mask, and comic-frame-line mipmaps. A fresh full run wrote `tmp_vector_probe/current_vector_summary_v5.json` and kept the same corpus counts as v4. This gives future samples a repeatable way to distinguish "cached frame plane exists and can be decoded" from "only the database reference exists, so fallback must reconstruct from vector/ruler data."

Vector point cubic probe follow-up: `inspect_vector_dynamics.py` now adds `cubic64_probe` to the first few point records, interpreting each point's first 64 bytes as the native cubic-node lens from IDA (`f64 +0..+40`, `u32 +48/+52/+56/+60`). This is diagnostic only. In `tmp_vector_probe/bubble_frame_single_v3.json`, the frame points show `x/y` anchors, zero-ish control/extra doubles, a small `0.0078125` value at the sixth double slot, and `+52/+56/+60` values equal to float `1.0` bit patterns. That confirms the compact point records carry control/dynamics-like data, while also showing they are not raw `CSRulerCubicBezier` nodes.

Vector point cubic summary follow-up: `inspect_vector_dynamics.py --summary-only` now also aggregates the point-level cubic lens across the corpus as `point_cubic64_u32` and `point_cubic64_f64_stats`. A fresh run wrote `tmp_vector_probe/current_vector_summary_v6.json` with unchanged corpus counts (`52` clips, `27` vector-bearing layers, `19` recognized 92-byte objects, `22` recognized 100-byte objects). The summary makes it easy to compare compact point fields by object family, flags/subtype, and BrushStyle; for example `object_100` frame points with shape `100/76/88/88`, subtype `3`, BrushStyle `6` summarize `f64+0` as `292.001..797.001`, matching the frame anchors in the single-file probe.

Vector AA field confirmation: the new isolated `Vector_AA_*` samples confirm the document field mapping for the UI anti-alias selector. Their relevant `BrushStyle` rows differ only in `AntiAlias`: `Vector_AA_None=0`, `Vector_AA_Weak=1`, `Vector_AA_Medium=2`, and `Vector_AA_Strong=3`, while `StyleFlag`, texture fields, and density defaults stay otherwise aligned. This is stronger evidence that `.clip` stores the none/weak/medium/strong ordinal directly. It still should be treated as renderer metadata, because IDA shows the checked frame/text ruler route clears V4 pattern mode and the global simple-AA 4x path is only an on/off setting, not this four-level brush ordinal.

IDA frame-line bitmap branch follow-up: the `CSFrameLineLayer` vtable slots after `DrawSingleLineForBitmap @ 0x12364b80` resolve to `DrawSingleLineStraightForBitmap @ 0x123652d0`, `DrawSingleLineCurveForBitmap @ 0x12364800`, and `DrawPolygonLineForBitmap @ 0x123636b0`. In the bitmap path, `DrawSingleLineForBitmap` converts ruler points to integer `RCPoint`s, builds a bbox expanded by line width (`RCLength +0x98`), and either fills directly or draws into a temporary bitmap and alpha-copies nonzero pixels back to the destination. The straight branch fills the polygon first, then calls the polygon-outline slot. The curve branch clones a ruler object, appends anchor-only vertices, expands them with `GetSplinePoints` / `GetSplineDivideCount(..., 128.0, 8, 64)` / `SplineCalc`, optionally fills the sampled polygon when its fill flag is set, then draws every sampled segment with `CSBitmap::DrawSegment(width=RCLength +0x98, color=a7)`, closing back to the first point. This is a plain geometry/bitmap fallback route; it is useful for frame/ruler reconstruction, but it is still separate from ordinary vector brush pattern stamping.

IDA frame-line serialization follow-up: `CSFrameLineLayer` uses the `CSRulerLayer` Read/Write slots (`ReadSelf @ 0x12391c20`, `WriteSelf @ 0x12393490`) rather than adding a separate frame-line payload. `CSRulerLayer::ReadSelf` reads the base `CSBitmapLayer`, then `CSRulerFunction::ReadSelf(this+0x340)`, which creates/serializes a `CSRuler`, recalculates extents, and stores document resolution; `WriteSelf` writes the same `CSRulerFunction`. `CSFrameLineLayer::Create @ 0x12362190` sets layer type `4194305`, initializes `CSRulerFunction`, optionally creates a base frame, and seeds a ruler-side field from `CSMain`. `CSRulerFunction::AddRuler @ 0x1236e4d0` confirms new frame rulers are built from point vertices plus an `RCLength` copied into object offset `+0x98`. So the native archive source for frame geometry is the ruler object database, not a second color/width table on `CSFrameLineLayer`; importer color/width generalization should continue to flow from parsed vector/ruler object fields and their enable flags.

IDA ruler archive layout follow-up: `CSRuler::Serialize @ 0x12367740` wraps everything in an outer section, then stores a small settings section (`this+0x18/+0x1c/+0x24/+0x20/+0x28/+0x2c`, optional `+0x30`) followed by `CSRulerDataBase::Read/WriteRulerDB`. The DB is `section(count) + object sections`; each object begins with a nested header section containing object type (`this+0x08`) and object id/state (`this+0x20`), then dispatches the object's virtual `Read/Write`. The common `CSRulerObject` section starts with `+0x24`, closed flag `+0x58`, line width `RCLength +0x98`, double-line flag `+0xb0`, double-line spacing `RCLength +0xb8`, and point count. Each ordinary point node is 32 bytes in memory: `double x` at `+0`, `double y` at `+8`, vertex select id at `+0x10`, line select id at `+0x14`, and `IsVertexSelected` at `+0x18` (the last four bytes are not written by the common serializer). The archive point order is `x`, `y`, `IsVertexSelected(+0x18)`, `LineSelectID(+0x14)`, `VertexSelectID(+0x10)`. After the point array, remaining optional fields include tail/draw/style state (`+0x90`, `+0x88`, `RCLength +0xd8`, `+0xf0/+0xf4/+0x128/+0x94`, `+0x130/+0x138/+0x140/+0x148`, `RCLength +0xf8/+0x110`, draw color `+0x12c`, secondary style `+0x150`, have-draw-color `+0x8c`) and an optional four-point 32-byte array. This gives a concrete native target for a future `.clip` ruler parser, but it also confirms the compact SQLite 88/104-byte point records contain more than this common point node.

IDA ruler type/default follow-up: `CSRuler::CreateRulerObject @ 0x12366fb0` maps native `RulerObjectType` values as `0/1 -> CSRulerObject`, `2 -> CSRulerConcentrate`, `3 -> CSRulerParallel`, `4 -> CSRulerConcentrateV3`, `5 -> CSRulerParallelV3`, `6 -> CSRulerVanishV3`, `7 -> CSRulerFreeEmit`, `8 -> CSRulerCircles`, `9 -> CSRulerSymmetry`, and `10 -> CSRulerCubicBezier`. The base `CSRulerObject` constructor initializes useful defaults: line width `RCLength +0x98 = 1.0px`, tail width `RCLength +0xd8 = 2.0px`, double-line spacing `RCLength +0xb8 = 5.0px`, in/out lengths `RCLength +0xf8/+0x110 = 10.0px`, draw color `+0x12c = 0x00ffffff`, secondary style `+0x150 = 1`, and have-draw-color `+0x8c` defaults true for type `0/1/10` in older archives. `CSRulerCubicBezier` then adds its 64-byte cubic-node array at `+0x170` and count/mode fields at `+0x188/+0x190`. This type map is native archive metadata; current 100-byte SQLite object header `subtype` values (`4/5/...`) should not be equated with these native type ids until a conversion site is found.

Vector native-point probe follow-up: `inspect_vector_dynamics.py` now adds `native32_probe` / `native32_stats` beside the existing `cubic64_probe`, viewing compact point offsets `+16/+20/+24/+28` through the common native `CSRulerObject` point-node lens (`vertex select id`, `line select id`, `is selected`, extra). This is diagnostic only. `tmp_vector_probe/bubble_frame_single_v4.json` shows the frame compact points store pen-bbox-like coordinates there, e.g. the first point reports `(289,173,800,179)` and the next `(794,173,800,410)`, so those offsets are not native select-id fields in the SQLite compact stream. The full summary `tmp_vector_probe/current_vector_summary_v7.json` adds `point_native32_u32`; it makes future samples easier to compare while reinforcing that native archive point nodes and compact vector point records are different representations.

IDA ruler copy-field grouping follow-up: the common object copy helpers provide a cleaner semantic grouping than raw Read/Write alone. `CSRulerObject::CopyMembers @ 0x12396680` copies the point array plus all common ruler fields. `CopyParams @ 0x12396e30` copies parameters without rebuilding the point array. `CopyLineInfo @ 0x12396520` copies only line width `RCLength +0x98`, double-line flag `+0xb0`, and double-line spacing `RCLength +0xb8`. `CopyLineDrawInfo @ 0x123964e0` copies only `+0x128` and `+0x12c`, after invoking the line-info reset/copy vfunc, which strongly supports naming `+0x128` as draw mode/style and `+0x12c` as draw color. This confirms importer fallback/parser logic should treat geometry/width/double-line parameters separately from draw enable/color flags, even when the compact 100-byte header currently exposes line/fill RGB and width together.

IDA ruler draw-flag usage follow-up: `CSRulerLayer::DrawRuler @ 0x1238fc90` confirms native `CSRulerObject +0x128` is a bit-field. `bit1` (`& 2`) performs a fill pass through `CSRulerFunction::FillRulerObject @ 0x12381980`, which calls bitmap `DrawSingleLine` with `line_color=0`, the selected fill color, and `force_fill=1`. `bit0` (`& 1`) controls the ordinary outline pass; when bit0 is clear and bit1 is set, the outline is skipped. A still-unnamed `bit2` (`& 4`) can continue into a special line path with temporary small width and alternate layer color handling when bits 0/1 are clear. For 24/32-bit output, `+0x12c` is used as a per-object ARGB draw-color override only when its alpha byte (`+0x12f`) is nonzero; otherwise the layer/default draw color remains active. Importer parser work should therefore preserve `draw_flags`, `draw_color_argb`, and the alpha-gated override separately instead of collapsing current values into `line-only` / `fill-only` enums.

Vector compact/archive boundary follow-up: `inspect_vector_dynamics.py` now adds `compact_semantic_probe` and `compact_record_coverage`. Single-file dumps expose known compact fields, ARGB candidates derived from compact line/fill RGB plus object opacity, and low-bit lenses for plausible compact flag offsets. Summary output adds `compact_draw_flag_candidate_lenses`. The current 52-clip corpus reports `22` recognized 100-byte objects and `19` recognized 92-byte objects. For 100-byte balloon/frame/text records, candidate offsets `+20/+72/+76` do not look like native draw flags: `+20` commonly has low nibble `8`, `+72` is `0`, and `+76` is the known family split (`304` balloon-like, `1040` frame-like), while `+96` behaves like an object/id counter and is deliberately not treated as a flag lens. For 92-byte ordinary strokes, compact `+20` remains the real stroke flag field (`0x2081`, `0x2011`, `0x41`, etc.).

Vector body coverage follow-up: the same inspector pass shows the current `VectorObjectList.VectorData` bodies are a 56-byte prefix followed by contiguous compact records. Examples: `Bubble_Frame` is `56 + one 100/76/88/88 record`; `Test_Ballon` layer 8 is eight contiguous 100-byte records; `test_Filters_Vector_Text` vector layer 5 is four contiguous 92-byte stroke records. There is no extra trailing native `CSRulerObject`/`CSVec4DataBase` archive segment in the checked samples. IDA agrees with this boundary: `CSVectorDataBase::Load/Save` expects `CSYS`/`VECT` magic and `CSVectorPacket` records, while `CSVectorV4Layer::ReadSelf/WriteSelf` serializes `CSVec4DataBase` list families (`CSVPenHead`, `CSVLineScale`, `CSVCorner`, `CSVCurve`, `CSVStroke`). Current SQLite vector blobs are therefore a compact converted representation, not direct native archive payloads.

IDA draw-mode accessor follow-up: `CSRulerObject::GetDrawMode @ 0x1225d030` returns `+0x128`, `GetDrawColor @ 0x1225d010` returns `+0x12c`, `AddDrawMode @ 0x1225aab0` ORs bits into `+0x128`, and `DelDrawMode @ 0x1225c790` clears bits from `+0x128`. This closes the naming loop: native draw mode is intentionally a bit-field. The compact 100-byte records still need a separate converter map before these native flags can be consumed by the importer.

Vector external-header parser follow-up: `inspect_vector_dynamics.py` now parses the 56-byte wrapper at the start of `VectorObjectList.VectorData` bodies as `u64 header_len=40`, ASCII tag `extrnlid`, a 32-byte uppercase external identifier, and `u64 payload_size`. The parser reports `external_header`, compares the wrapper id against the `VectorObjectList.VectorData` id, and adds `prefix_matches_external_header` to compact-record coverage. A fresh corpus run wrote `tmp_vector_probe/current_vector_summary_v9.json`; all 27 vector layers match the same wrapper shape (`payload_offset=56`, `payload_size_matches_body=true`, `matches_vector_data=true`) and remain fully covered by contiguous compact records after the prefix. Counts stayed unchanged at 52 clips, 27 vector layers, 19 recognized 92-byte strokes, and 22 recognized 100-byte objects.

CLIPStudioPaint.exe schema/container follow-up: a targeted PE scan from the native-analysis workspace shows the current vector SQLite/Exta layer belongs to the main Planeswalker document-model side, not `iswCoreTG.dll` native vector archives. `CLIPStudioPaint.exe` contains `extrnlid`, `VectorData`, `VectorObjectList`, `CHNKExta`, RTTI for `PWVectorObjectList` / `PWVectorObject` / `PWWrapVectorManager` / `PWVectorDraw`, and `CHNKExta`/`ExternalChunk` read-write islands at `0x143a3f830` and `0x143a42360`. The direct `VectorData` / `VectorObjectList` code refs are only schema-registration stubs, so these findings explain the wrapper/storage layer but still do not expose the compact `92/100` vector-record encoder.

Planeswalker vector subclass follow-up: the main EXE RTTI names `PWVectorRuler`, `PWVectorStroke`, `PWVectorFill`, and `PWVectorBalloon` as subclasses of `PWVectorObject`. Their tail vtable methods are useful field-shape evidence: Stroke serializes a list reference plus a double plus a u32; Fill serializes one list reference; Balloon serializes a u32, two list references, a double, and a u32. This mirrors the broad compact record families, but it remains a Planeswalker object serialization layer and should not be treated as a direct byte-for-byte description of current SQLite `92/100` compact records.

Planeswalker common vector-object field follow-up: `PWVectorObject` base helpers in the main EXE suggest the common compact header is converted into a common object layout before subclass serialization. The useful object fields are low-24 flags at `+0x20`, a four-dword bbox at `+0x50..+0x5c`, two common color/style structs at `+0x60/+0x68` and `+0x6c/+0x74`, a double at `+0x78`, and a copied-but-not-base-compared dword at `+0x80`. This lines up with compact common header concepts but still needs the actual compact-to-Planewalker converter before importer code should consume it as a fixed map.

Planeswalker compact reader follow-up: a targeted `CLIPStudioPaint.exe` scan found the compact vector object reader at `0x1422cf5f0`, closing the main header map. It reads all scalar fields as big-endian stream values: `u32+0` object header length (`92/100`), `u32+4` subclass-tail offset (`76`), `u32+8` point-record stride (`88/104/120`), `u32+12` point-record tail offset (`88`), `u32+16` point count, and `u32+20` object flags/type. The common object body then reads bbox at `+24..+36`, two three-u32 color/style triplets at `+40..+48` and `+52..+60`, opacity double at `+64`, and optional `u32+72`, before seeking to `+76` and dispatching the subclass vtable slot `+0x100`. The 92-byte stroke tail reads `u32+76` as a list/style index, `f64+80` as the width/scale double, and `u32+88` as a trailing stroke field. The 100-byte balloon/frame tail reads `u32+76`, optional list/style indexes at `+80/+84`, `f64+88`, and `u32+96`. This confirms the importer should keep the current big-endian compact header parser, but rename the diagnostic view around CSP's actual reader instead of treating offsets as free-form guesses. `inspect_vector_dynamics.py` now emits `compact_native_reader_probe` for each recognized 92/100 record, and fresh probes were written to `tmp_vector_probe/bubble_frame_single_v10.json` and `tmp_vector_probe/current_vector_summary_v10.json`.

Planeswalker compact point-reader follow-up: the object reader's point loop calls `0x1422cbae0` for each compact point record. That reader confirms the fixed first 88 bytes of every current point record: `f64+0/+8` are anchor coordinates, `u32+16/+20/+24/+28` are four integer fields, `u32+32` is another point flag/state field, `f32+36..+76` are eleven float fields, and `u32+80/+84` are two trailing integer fields. For 104- and 120-byte point strides, the reader then seeks to the record's tail offset (`u32+12`, currently `88`) and dispatches the point object's vtable slot `+0xd8` for extra subclass payload. `PWVectorBezierCurve +0xd8 @ 0x142623a30` reads two tail doubles, matching `88 + 16 = 104`; `PWVectorCubicBezierCurve +0xd8 @ 0x142625de0` reads four tail doubles, matching `88 + 32 = 120`. The inspector now attaches `compact_point_reader_probe` to the first point records, including subclass tail doubles when present, with fresh outputs in `tmp_vector_probe/vector_aa_none_v12.json`, `tmp_vector_probe/test_vector_v12.json`, and `tmp_vector_probe/current_vector_summary_v12.json`. This is directly relevant to brush dynamics: the existing pressure-size/opacity fallbacks reading point `+36` are now known to be reading the first float after the fixed integer point header, not an arbitrary raw offset.

Planeswalker compact object-allocation clarification: `0x142552af0`, called by the common object reader after reading compact `u32+20`, is not a `switch` that chooses Stroke/Fill/Balloon from that value. It allocates an object from the current vector pool/list (`this+0x80`) and then calls `PWVectorObject` base init `0x1422d0510` with compact `+20` as the object flags. The concrete object class is therefore supplied by the surrounding list/pool context; compact `+20` should be treated as flags/type-state for the chosen class, not as the sole class discriminator.

Planeswalker compact point-writer follow-up: the matching compact point writer is `0x1422cd270`, and it mirrors the point reader exactly. It writes internal point fields `+0x08/+0x10` as the compact anchor doubles at record `+0/+8`, writes internal `+0x18/+0x1c/+0x20/+0x24` plus low-24 internal flags from `+0x40` as the five compact u32 fields at `+16..+32`, writes eleven internal floats `+0x44..+0x6c` as compact `f32+36..+76`, then writes internal `+0x70/+0x74` as compact `u32+80/+84`. It then dispatches the point object's vtable slot `+0xe0` for subclass-tail writing. The vtables confirm the read/write pair: `PWVectorCurve` uses no-op slots for `+0xd8/+0xe0`, `PWVectorBezierCurve` has reader `0x142623a30` and writer `0x142623ee0` for two tail doubles, and `PWVectorCubicBezierCurve` has reader `0x142625de0` and writer `0x1426262f0` for four tail doubles. This closes the 88/104/120-byte point stride map from both directions; the remaining unknowns are semantic names for the five u32 fields and eleven float fields, not their byte order.

Planeswalker point-field grouping follow-up: the nearby converter `0x1422cd3c0` copies a source point-like structure into `PWVectorCurve`. It copies source `+0/+8` into internal anchors `+0x08/+0x10`, converts seven source doubles `+0x10..+0x40` into the first seven internal floats `+0x44..+0x5c` (compact `f32+36..+60`), and toggles internal flag bits `0x1000/0x2000` from source `+0x50` bit0/bit1. It does not set the final four serialized floats `+0x60..+0x6c` (compact `f32+64..+76`) in that path. Current samples match this split: simple frame/balloon records usually show the first seven floats as `[1, 1, 0, 0, 1, 1, 1]`, pressure/dynamics samples vary the early floats, while compact `f32+76` can contain values such as `5`, `9`, or `15.079193` and should not be treated as a normalized pressure/opacity value without more evidence. `inspect_vector_dynamics.py` now emits this split as `writer_grouped_f32` inside `compact_point_reader_probe`; fresh detailed probes are in `tmp_vector_probe/bubble_frame_single_v13.json` and `tmp_vector_probe/test_vector_v13.json`.

Planeswalker compact object-writer follow-up: the outer writer is `0x1422cff30..0x1422d04db`, and it mirrors the compact object reader at `0x1422cf5f0`. It first serializes the first point with `write_point @ 0x1422cd270` into a temporary stream and records both the full point stride and the point-tail offset returned before the point subclass writer runs. It then writes the common object stream starting at compact `+8`: point stride, point-tail offset, point count from internal `+0x40`, low-24 object flags from `+0x20`, bbox `+0x50..+0x5c`, the two color/style structs `+0x60/+0x6c`, opacity double `+0x78`, and dword `+0x80`. After that it dispatches object vtable slot `+0x108` for the subclass writer, measures the stream length, and emits the final first two u32 fields as `object_header_len = common_and_subclass_len + 8` and `subclass_tail_offset = common_len_before_subclass + 8`. Finally it copies the common/subclass bytes and serializes the remaining point list with the same point writer. Vtable checks confirm the object subclass slots: `+0x100` is reader, `+0x108` is writer, and `+0x110` is compare/equality for `PWVectorRuler`, `PWVectorStroke`, `PWVectorFill`, and `PWVectorBalloon` (`Stroke +0x108 = 0x1425a4900`, `Fill +0x108 = 0x142628de0`, `Balloon +0x108 = 0x14262a570`). This confirms the header length, subclass-tail offset, point stride, and point-tail offset are produced by CSP's own writer and should be treated as authoritative self-description rather than sample constants.

Planeswalker compact list reader/writer follow-up: the direct caller of the compact object reader is `0x142554920..0x142554b0f`, and the direct caller of the compact object writer is `0x1425550b0..0x142555427`. The reader wraps the incoming payload bytes in a stream and loops `0x1422cf5f0` until the stream offset reaches the supplied payload length. It does not read a separate object count from the compact payload. The writer walks the linked object list at `+0x68`, calls `0x1422cff30` for each object, sums the produced blob sizes, allocates one output buffer, and copies the object blobs back-to-back. This confirms the inspector's coverage rule: after the 56-byte external-id wrapper, `VectorObjectList.VectorData` is a pure concatenation of self-sized compact records, with no per-payload count field or trailing native archive segment.

Planeswalker point-dynamics usage follow-up: the internal point float fields are used by runtime sampling/interpolation code, but their UI names are still not fully proven. Function `0x1422ca3fc` constructs/updates a point by writing anchors plus floats `+0x44..+0x5c` and toggling flags `0x1000/0x2000`; `0x1422ca640` converts those same fields back into a double-precision work structure and preserves those two flag bits. Interpolation paths such as `0x1422c9847` and `0x1422cabde` linearly interpolate `+0x44`, `+0x48`, `+0x4c`, `+0x54`, `+0x58`, and `+0x5c` between neighboring points; `+0x50` is blended through helper `0x14206d6e0` instead of a plain linear formula, which suggests a wrapped/angle-like quantity or another non-linear scalar. A later special branch tests point flags and uses `+0x60/+0x64/+0x68` as multipliers added to the anchor `+0x08/+0x10` before evaluating a point vtable `+0x78`, so those final floats are not the same first-seven source-double group. For importer work, keep `f32+36` pressure-size/opacity handling narrow and sample-backed; do not globally map all point floats to brush dynamics until single-setting samples and more native evidence agree.

Vector point-float corpus summary follow-up: `inspect_vector_dynamics.py` now aggregates all eleven compact point floats (`f32+36..+76`) in summary output as `point_float_stats`; previous summaries only tracked a sparse subset. A fresh run wrote `tmp_vector_probe/current_vector_summary_v14.json`. Detailed pressure probes show why the renderer should stay conservative: `Vector_SizePressure` varies `f32+36` from `0.0..0.937887`, `f32+40` from `0.588889..0.8`, and `f32+44` from `0.0..0.037606`, with `f32+76` fixed at `0`; `Vector_OpacityPressure` varies `f32+36` from `0.000366..0.527536`, `f32+40` from `0.522222..0.788889`, `f32+44` from `0.0..0.075884`, and `f32+76` from `0.0..3.11909`. But `Vector_AA_None` also has a nonzero `f32+76` endpoint value (`0.863645`) despite no pressure/opacity setting, and many balloon/frame records use `f32+76` as values like `5` or `9`. Therefore the importer should continue using `f32+36` for the current sample-backed pressure branches and treat `f32+76` as an auxiliary/runtime field until a one-setting sample proves its UI meaning.

Planeswalker point-angle interpolation follow-up: local `CLIPStudioPaint.exe` disassembly resolves helper `0x14206d6e0` as degree-domain wrap interpolation, not a generic easing function. It subtracts the start value from the end value, adjusts deltas beyond `+180` by `-360` and below `-180` by `+360`, interpolates by the supplied parameter, then wraps the result back into `[0,360]`. Direct call sites include the compact point interpolation paths at `0x1422c9b07`, `0x1422cacd2`, and `0x1422cc6a6`, plus a point-merge/update helper at `0x142624150`. Since the writer maps internal `+0x44..+0x6c` to compact `f32+36..+76`, internal `+0x50` corresponds to compact `f32+48`; treat that compact field as angle-like degree data, while still avoiding a UI label until isolated rotate/tilt/random samples prove which brush setting drives it.

Planeswalker point-tail evaluation follow-up: point vtable slot `+0x78` is the curve evaluator used by interpolation/render sampling, and it confirms the subclass tail semantics. Base `PWVectorCurve +0x78 @ 0x1422ca190` returns a straight segment/intersection-style point using only anchors. `PWVectorBezierCurve +0x78 @ 0x142623280` uses the one tail control point stored internally at `+0x78`, matching the compact 104-byte record's two tail doubles. `PWVectorCubicBezierCurve +0x78 @ 0x142625450` uses two tail control points at `+0x78` and `+0x88`, with internal flags `0x400/0x800` gating endpoint-near evaluations before falling back to a full cubic Bezier helper. This strengthens the 88/104/120 compact point stride map: the tail doubles are actual curve controls, not brush dynamics fields.

Planeswalker point auxiliary-offset follow-up: the later branch in `0x1422cabde` explains part of the final serialized-float group. When point flags allow it (`internal +0x40` low bits) and an auxiliary point/context is present, the code reads internal `+0x60`, multiplies it by `+0x64` for an x offset and by `+0x68` for a y offset, adds those offsets to the anchor at `+0x08/+0x10`, then calls the auxiliary point's vtable `+0x78`. Because internal `+0x60/+0x64/+0x68` map to compact `f32+64/+68/+72`, these fields should be treated as geometry auxiliary-offset data, not as normalized brush dynamics. `inspect_vector_dynamics.py` now also aggregates compact point integer fields `u32+16/+20/+24/+28/+32/+80/+84` as `point_u32_stats`; `tmp_vector_probe/current_vector_summary_v15.json` shows `u32+32` is often `0` for ordinary strokes, but `104-byte` object points commonly carry low flag value `1`, giving future samples a place to compare endpoint/control flags.

BrushStyle effector inventory follow-up: the vector samples' `BrushStyle` table has 71 columns, including at least 17 effector columns: size, opacity, flow, interval, thickness, rotation, rotation-in-spray, texture density, mix color, mix alpha, blur, sub color, hue/saturation/value change, spray size, and spray density. The current pressure samples show three compact effector shapes worth preserving in diagnostics: `0x01` is the 4-byte default/off form, `0x11` is a 12-byte form shaped as type + scalar + graph id, and `0x31` is a 24-byte form shaped as type + zero + graph id + scalar + graph id + scalar. `Vector_SizePressure` uses `SizeEffector=0x31` with graphs `5/6`; `Vector_OpacityPressure` uses `OpacityEffector=0x31` with graphs `7/8`; the AA sample's ordinary stroke uses `SizeEffector=0x11` and `OpacityEffector=0x11` with graphs `3/4`. This supports keeping `inspect_vector_dynamics.py` focused on full `BrushStyle` + `BrushEffectorGraphData` capture, while renderer code should only consume effector forms proven by isolated samples.

Brush effector summary follow-up: `inspect_vector_dynamics.py` summary output now aggregates every `BrushStyle` key ending in `Effector`, not only size/opacity/flow/thickness. It also emits `effector_graph_signatures` and `effector_graph_refs`, so a corpus scan can show both the compact effector blob shape and the referenced `BrushEffectorGraphData` curve. A fresh run wrote `tmp_vector_probe/current_vector_summary_v16.json` with unchanged vector counts (`52` clips, `27` vector layers, `19` stroke records, `22` 100-byte objects). The current graph refs are all present in the same `.clip` files: for example `SizeEffector=0x31` references graphs `5/6`, `OpacityEffector=0x31` references `7/8`, and the ordinary `0x11` size/opacity forms reference graphs `3/4/5` depending on style. This means future one-setting samples can be compared entirely from the `.clip` document data before any renderer branch is added.

BrushStyle schema-reference follow-up: a targeted `CLIPStudioPaint.exe` RIP-reference scan of effector and graph table strings found only schema registration references. Examples include `SizeEffector @ 0x1444de750 -> 0x1400bc6f4`, `OpacityEffector @ 0x1444de760 -> 0x1400bc4e4`, `FlowEffector @ 0x1444de780 -> 0x1400bc274`, `ThicknessEffector @ 0x1444de800 -> 0x1400bcae4`, `BrushEffectorGraphData @ 0x1445365c8 -> 0x140138e14`, and graph fields `ControlNumber/ControlDataSize/ControlPoints -> 0x1400c35b4/0x1400c3584/0x1400c35e4`. These addresses sit in the same short registration-stub family already identified for BrushStyle/PatternStyle strings, so the string refs do not expose the missing runtime effector compiler.

Brush effector decoded-form follow-up: `inspect_vector_dynamics.py` decodes compact effector blobs in each detailed `BrushStyle` row and summarizes them as `effector_decoded_forms`. The current native-branch-aligned diagnostic forms are `zero_or_disabled_0x00`, `default_or_off_0x01`, `primary_graph_0x11`, `dual_graph_0x31`, `primary_graph_velocity_0x111`, `secondary_graph_range_0x21`, and `aux_graph_random_0xc1`; unknown forms still keep their raw `payload_u32` / `payload_f32`. Earlier snapshots used looser legacy one-graph names, but `current_vector_summary_v47.json` supersedes those labels. This keeps future renderer work honest: the importer can compare semantic blob shape and graph refs directly, but should still wait for sample/native evidence before assigning a UI meaning to unknown forms.

BrushStyle-to-object diagnostics follow-up: detailed `inspect_vector_dynamics.py` dumps now attach `brush_style_effectors` directly to each recognized 92-byte stroke and 100-byte object when the style has non-default effector data. This inlines the compact effector key, decoded form, graph refs, and the referenced `BrushEffectorGraphData` curves beside the vector record that uses them. Fresh detailed outputs include `tmp_vector_probe/vector_size_pressure_v20.json`, `tmp_vector_probe/vector_opacity_pressure_v20.json`, and `tmp_vector_probe/vector_aa_none_v20.json`; the corpus summary snapshot is `tmp_vector_probe/current_vector_summary_v20.json`. This makes sample review much faster: a `Vector_SizePressure` stroke now shows `SizeEffector dual_graph_0x31` with graphs `5/6`, an opacity-pressure stroke shows `OpacityEffector dual_graph_0x31` with graphs `7/8`, and the AA ordinary stroke shows `SizeEffector`/`OpacityEffector primary_graph_0x11` with graphs `3/4`.

Effector-to-point summary follow-up: `inspect_vector_dynamics.py` summary output includes `effector_point_float_stats` and `effector_point_float_varying_stats`, grouping compact point `f32+36..+76` stats by active BrushStyle effector forms. The varying view surfaces the useful pressure cases without scanning constants: `OpacityEffector dual_graph_0x31:g=7,8:1.0,5.0` has compact `f32+36 = 0.000366..0.527536` in `Vector_OpacityPressure`, while `SizeEffector dual_graph_0x31:g=5,6:1.0,1.5` has compact `f32+36 = 0.0..0.937887` in `Vector_SizePressure`. It also keeps older complex rows visible, such as `SizeEffector primary_graph_velocity_0x111:g=4:0.95` plus `OpacityEffector primary_graph_0x11:g=5:0.3`, where several early point floats vary but renderer mapping remains unresolved. This summary is diagnostic only; it should guide targeted samples and native checks before adding broad renderer formulas.

100-byte object effector summary follow-up: the effector-form summary counts BrushStyle rows used by recognized 100-byte objects as well as 92-byte strokes. `inspect_vector_dynamics.py` now names the object-style thickness forms by their native runtime branches: `aux_graph_random_0xc1` and `secondary_graph_range_0x21`. These currently appear as `ThicknessEffector` forms, not renderer rules. The corpus reports `ThicknessEffector 0xc1` with graph/scalars such as `g=3`, `0.5/0.85` and `g=6`, `0.5/0.85`, plus `ThicknessEffector 0x21` with low/high scalars `0.25/1.0` and graph `4` or `1`. This explains the earlier hidden `0xc1/0x21` rows in `effector_point_float_varying_stats`: they are mostly balloon/frame/text 100-byte object BrushStyles, especially rough/thick outline families.

BrushStyle object-combo follow-up: `inspect_vector_dynamics.py` emits `style_combos_by_kind`, preserving the old stroke-only `style_combos` while adding a mixed stroke/object view keyed by `stroke_92` or `object_100`. The view makes the object-family BrushStyle split obvious: the most common `object_100` combo is `ThicknessEffector aux_graph_random_0xc1`, `PatternStyle=10`, `AutoIntervalType=2`, while another object family uses `FlowEffector primary_graph_0x11` plus `ThicknessEffector secondary_graph_range_0x21`, `PatternStyle=11`. This reinforces that balloon/frame/text outline styles need their own BrushStyle path; they should not be folded into ordinary stroke pressure rules just because they share the `BrushStyle` table.

Effector-to-point u32 follow-up: the v24 summary also adds `effector_point_u32_stats`, grouping compact point integer fields `u32+16/+20/+24/+28/+32/+80/+84` by active BrushStyle effector forms. The first four fields vary like point bbox/coordinate-support integers, `u32+32` is mostly a low flag/state field, and `u32+80/+84` carry trailing point state with object-family differences (`object_100` rows often expose `u32+84=1` where ordinary strokes remain `0`). This is diagnostic only, but it gives future one-setting samples a ready place to compare pressure, tilt, velocity, random, and object outline modes against per-point flags instead of guessing from floats alone.

Core pressure-node clarification: the active IDA `iswCoreTG.dll` session shows the core renderer consumes already-compiled vector pressure nodes rather than raw `BrushStyle` effectors. `CSVectorDraw::GetDrawPres @ 0x1242eac0` interpolates `_VVPRESSURE` records with a 16-byte stride: float `+0`, float `+4`, angle-like float `+8` blended through `CSVectorMath::RotOffCalc/RotNormal`, and distance/t at `+12`. `CSVectorDraw::DrawPenHeadCurve @ 0x1242db40` then applies draw flags: bit `0x2` multiplies output alpha by pressure float `+0`, bit `0x1` uses pressure float `+0` as the pen-head scale, and pressure float `+4` is passed into the pen-head transform as a second scale/thickness parameter; bit `0x4` can replace the pressure rotation with tangent-derived rotation. This is important evidence against globally naming compact point floats as UI fields: the missing piece is the upstream compiler from `.clip` `BrushStyle`/effector graphs into these `_VVPRESSURE` nodes and draw flags.

Planeswalker brush-draw bridge follow-up: local `CLIPStudioPaint.exe` PE/Capstone analysis connects the tool side to the `PWBrushDraw` setup path. `PWBrushTool::0x142e73180` calls `0x1430b8af0` and `0x1430b9be0`; both copy a source draw packet from offsets such as `+0x50..+0xa0`, `+0xc0`, `+0xc8`, `+0xd0`, `+0xd8`, `+0xdc`, `+0xe0/+0xe4`, and `+0x100/+0x104`, then call `PWBrushDraw::0x142558580`. That setup reads the copied packet through `rbp+0x90/+0x98/+0xa0/+0xa8/+0xb0/+0xb8/+0xc0/+0xc8`, prepares internal draw state, and dispatches dense helpers such as `0x14255cb20`, `0x14255d810`, `0x14255d470`, `0x14255dfe0`, and `0x14255eaa0`. This is not yet a renderer formula, but it narrows the missing BrushStyle bridge from "somewhere in the EXE" to a concrete packet pipeline: tool/options -> source draw packet -> `0x1430b8af0/0x1430b9be0` -> `PWBrushDraw::0x142558580` -> runtime draw helpers. The helper `0x1430b8280` used in the same tool flow appears to adjust the packet for point order/geometry, not parse effector strings.

Planeswalker draw-packet argument map follow-up: the two bridge functions now have a concrete source-to-destination map. Both copy packet `rbx+0x50..+0xa8` into a first draw-state block at destination `+0x68..+0xb8` and again into a second block at `+0xc0..+0x110`; they also copy `rbx+0xc0 -> dest+0x11c`, `rbx+0xc8 -> dest+0x120`, `0.05 / rbx+0xd0 -> dest+0x130`, `rbx+0xd8` or a guarded `rbx+0xd8/rbx+0x104` result into `dest+0x138`, `rbx+0x48 -> dest+0x140`, and `rbx+0xdc -> dest+0x158` in the `0x1430b8af0` path. The `PWBrushDraw::0x142558580` stack arguments then resolve cleanly: `rbp+0x90 = rbx+0x40`, `rbp+0x98 = dest+0x140/rbx+0x48`, `rbp+0xa0 = rbx+0xe0`, `rbp+0xa8 = rbx+0xe4`, `rbp+0xb0 = shared pair from rbx+0xf0/+0xf8`, `rbp+0xb8 = status flag pointer`, `rbp+0xc0 = &rbx+0x100`, and `rbp+0xc8 = rbx+0x104`. Inside `0x142558580`, helper `0x14255cb20` lazily allocates two `0x298` draw buffers at `PWBrushDraw+0x140/+0x150` and a `0x1d8` buffer at `+0xe8`; helper `0x14255d810` reads the passed object through vtable slots `+0x90/+0x80/+0x70/+0x40/+0x50` and seeds draw fields such as `+0x48`, `+0x68`, `+0x78/+0x80`, and `+0x84/+0x8c`; helper `0x14255d470` then submits the `+0xe8/+0xf0` pair through both `+0x140` and `+0x150` buffers. These are still native packet semantics, not importer rules, but they give the next analysis a much smaller target: identify who fills packet `+0x40/+0x48/+0x50..+0xa8/+0xd0/+0xe0/+0xe4/+0x100/+0x104` from `PWBrushStyle` and tool options.

Planeswalker temporary packet initializer follow-up: `PWBrushTool::0x142e73180` calls `0x142e65b20` immediately before both `0x1430b9be0` and `0x1430b8af0`, and that helper initializes the short packet later seen at bridge source `+0x50`. It writes a zeroed/default packet at `rdx`: `+0/+8` become input anchor doubles plus tool offsets `tool+0x120/+0x124`, `+0x10/+0x18/+0x30/+0x38/+0x40` default to `1.0`, `+0x20/+0x28/+0x48` default to zero, `+0x50` defaults to zero, and `+0x54` receives a stack argument from caller state `r15+0xc0`. The `+0x10` value starts from caller `xmm2` (`r15+0xa0`) and is conditionally scaled by `sin((sample_metric / tool+0x188) * 90deg)` when `tool+0x188` is larger than the supplied sample metric; if the incoming scalar is greater than `1.0`, the field is clamped back to `1.0`. Field `+0x18` is `r15+0xac / 90.0`. Field `+0x28` is enabled by `tool+0x140` or `tool+0x144`, taking caller `r15+0xa8` or `r15+0xb0`, then optionally mirroring through `360 - value` or `180 - value` based on helper predicates over the shared object pair. This strongly suggests a geometry/dynamics packet with position, scale, rotation/angle, and mode flags, but the docs should keep these as formulas until BrushStyle column names are proven.

Planeswalker packet-source boundary follow-up: the `r15` object feeding `0x142e65b20` is the call-time brush/event context passed into `PWBrushTool::0x142e73180`, not the SQLite `BrushStyle` row. The function first stores `r15+0xa0` into tool `+0x190`, later uses `r15+0xa0/+0xa8/+0xac/+0xb0/+0xc0` as temporary-packet inputs, and uses `r15+0xd0` plus `r15+0xb8` in the surrounding calls to `0x1430bb8b0` / `0x1430bbe30`. Field `r15+0xb8` behaves like a small mode enum: branches compare it to `1`, `2..3`, and `4`, and those branches influence tool `+0x188` and auxiliary output `+0x198`. Tool flags `+0x140/+0x144`, which control packet `+0x28`, are derived earlier from point/object flags at `object+0x270` bits `0x10` and `0x20`. This pushes the BrushStyle compiler boundary one step earlier: BrushStyle/tool options likely feed the call-time `r15` context before `PWBrushTool::0x142e73180`, while this function lowers that context plus point flags into per-sample packets.

Planeswalker auxiliary dynamics-state follow-up: the context pointer `r15+0xd0` is consumed by helper `0x1430bb8b0`, not passed directly into `PWBrushDraw`. That helper initializes an auxiliary state block: it clears state flags, enables a branch when the incoming draw/sample flag is nonzero and mode `r8d` is not `1`, derives a small countdown/state at `+0x50/+0x54` for mode `4`, computes a scale factor at `+0x10` from the stack doubles and sample metric, sets a minimum step/count at `+0x44`, initializes a point/history container at `+0x18`, stores the external pointer at `+0x48`, and copies the current vector block into `+0x60/+0x70`. The paired evaluator `0x1430bba20` updates that state per sample: it computes a double result into `+0x80`, shifts `+0x60` to `+0x70`, and copies the current sample vector into `+0x60`. `0x1430bbe30` computes a related scalar from mode, count, and global tool mode. This looks like stroke spacing/smoothing/dynamic scaling support around the packet initializer, but it still sits downstream of the unresolved producer of the `r15` context fields.

Planeswalker 0x108 draw-packet lifecycle follow-up: local `CLIPStudioPaint.exe` PE/Capstone analysis identifies `0x142e64d20` as the copy-constructor for the large draw packet used by the brush tool path, not as a renderer/effect formula. It copies the full packet from `rdx` to `rcx`, increments shared/resource refcounts at source offsets `+0x08`, `+0x28`, and `+0xf8`, and preserves the already-known draw fields: the short per-sample block at `+0x50..+0xa8`, scalar/status fields `+0xb0/+0xb4/+0xbc/+0xc0`, pointer/scalar fields `+0xc8/+0xd0`, mode dwords `+0xd8/+0xdc/+0xe0`, qword `+0xe4`, shared pair `+0xf0/+0xf8`, and trailing dwords `+0x100/+0x104`. The companion `0x142e656b0` releases the per-element shared fields `+0xf8`, `+0x28`, and `+0x08`; `0x142e64f10` destructs a vector of these `0x108` packets; `0x142e635f0` is the vector growth/insert helper that allocates `count * 0x108`, copy-constructs the inserted element with `0x142e64d20`, moves surrounding elements, then destroys the old elements. Direct callers show this packet vector is embedded in `PWBrushTool` state around `+0x2b8/+0x2c0`, and `PWBrushTool::0x142e73180` pushes temporary stack packet `rbp+0x970` into that vector before feeding bridge functions. This confirms the current search boundary: find who first fills the packet and call-time context; do not treat vector reallocation/copy sites as BrushStyle compiler logic.

IDA MCP multi-instance note: the active `ida-pro-mcp` server exposes `list_instances` / `select_instance`, so multiple IDA databases can be used in one Codex session when each IDA instance runs on a different RPC port. Current discovered instances were `127.0.0.1:13337` for `iswCoreTG.dll.i64` and `127.0.0.1:13338` for `CLIPStudioPaint.exe.i64`; selecting `13338` switched subsequent IDA MCP calls to the main executable. This avoids needing separate Codex sessions for core DLL and main EXE analysis.

Planeswalker snap false-lead clarification: IDA decompilation of `0x1430bafe0` shows it is a tiny context copier from `a1+0x80/+0xa0/+0xa8/+0xac/+0xb0/+0xd0` into a 0x30-byte output structure. RTTI and caller analysis place it under the `PWSnapOperation` family, not under BrushStyle compilation: surrounding vtables include `PWSnapRuler`, `PWSnapRulerParallel`, `PWSnapRulerEmit`, `PWSnapRulerGuide`, `PWSnapGrid`, `PWSnapVector`, `PWSnapInnerFrame`, and `PWSnapOperationArray`. Dispatcher `0x1430bb030` chooses snap-operation subclasses based on flags such as `0x1`, `0x10`, and `0x100`. Therefore this branch explains ruler/snap context movement around brush sampling, but should be filtered out when looking for the `.clip BrushStyle/BrushEffectorGraphData -> runtime pressure/draw packet` compiler.

IDA decompiler bridge confirmation: after switching MCP to `CLIPStudioPaint.exe`, IDA decompilation confirms the previously recovered bridge map. `0x1430b8af0` creates a draw object with mode `0x3020` or `0x2020`, copies packet `+0x50..+0xa8` into draw-state blocks at `a1+0x68..+0xb8` and `a1+0xc0..+0x110`, stores `0.05 / packet+0xd0` at `a1+0x130`, and calls `PWBrushDraw` setup `0x142558580` with packet shared pairs, flags, and trailing dwords. `0x1430b9be0` is the sibling path using mode `0x2010`; it seeds the internal object from the supplied point/vector block before calling the same `0x142558580`, then invokes additional `PWBrushDraw` helpers (`0x14255d260`, `0x14255cda0`, `0x14255a310`) to submit/update the output. `0x1430b8640`, called from adjacent `PWBrushTool` vtable methods, updates the same 88-byte sample block through compact vector-point helpers (`0x1422ce210`, `0x1422cd520`, `0x1422cfb10`) and stores the prior block at `a1+0xc0..+0x110`. This reinforces that the bridge operates on already-compiled per-sample vector/pressure-like state; the unresolved part remains upstream.

Planeswalker parameter-effector runtime evaluator: main EXE analysis now identifies `0x142568040` as the hot `PWBrushParameterEffector` scalar evaluator. A compiled effector stores flags at `+0x08`; graph/shared pointers at `+0x28`, `+0x38`, and `+0x48`; and floor/range floats at `+0x0c`, `+0x10`, `+0x14`, `+0x18`, `+0x1c`, and `+0x20`. Runtime branch `0x10` samples graph `+0x28` at context `+0x10` and blends from floor `+0x0c` to `1`; branch `0x20` samples graph `+0x38` at context `+0x18` and interpolates between floats `+0x10/+0x14`; branch `0x40` samples graph `+0x48` at context `+0x20` and blends from floor `+0x18`; branch `0x80` applies LCG random with floor `+0x1c`; branch `0x100` uses context `+0x30` with floor `+0x20`. This confirms the importer should preserve pressure/tilt/random/velocity as independent dynamic inputs and avoid baking the current pressure-only samples into a general brush formula.

BrushEffectorSettings compiler follow-up: `PWBrushEffectorSettings` constructor `0x142cb37a0` seeds enabled/mask flags and percent-style integer defaults, while compiler helpers lower those settings into `PWBrushParameterEffector`. `0x142cb44d0` copies resolved graph pointers directly, and `0x142cb45f0` does the manager-mediated version. Both require `(settings+0x08 & bit) && (settings+0x0c & bit)` for graph branches, convert integer percentages by `* 0.01`, then call runtime setters for bits `0x10`, `0x20`, and `0x40`; sign-bit `0x80` enables random through `0x1425694c0`; velocity flag `0x100` is added separately by `0x142cb5e80` through a parameter-id keyed map. The remaining native task is to map the big compiler caller `0x142b757b0`, which attaches these settings to concrete `PWBrushStyle` runtime offsets, back to SQLite columns like `SizeEffector`, `OpacityEffector`, `FlowEffector`, `ThicknessEffector`, rotation, texture-density, and color/spray effectors.

BrushStyle large compiler partial map: `0x142b757b0` is now confirmed as the big `PWBrushStyle` compiler/hydrator that assigns compiled effectors to runtime slots. The first five runtime destinations are mapped: `+0x80 = SizeEffector` with parameter id `1001`, `+0xd8 = OpacityEffector` with id `1011`, `+0x138 = FlowEffector` with id `1021`, `+0x1a8 = IntervalEffector` with id `1041`, and `+0x210 = ThicknessEffector` with id `1051`. Each slot is marked active by OR-ing bit `1` into its runtime flag field, and each may also receive velocity through `0x142cb5e80` using the same parameter id. Later slots `+0x300`, `+0x370`, `+0x3d0`, `+0x440`, `+0x4a0`, `+0x500`, `+0x560`, `+0x5c0`, `+0x628`, and `+0x688` are the remaining constructor-ordered rotation/texture-density/mix/blur/sub-color/hue-saturation-value effectors, but their exact column names still need a surrounding-scalar pass. For importer policy, the key point is that size, opacity, flow, interval, and thickness already have separate graph/random/velocity-capable runtime effectors; do not generalize one pressure mapping across parameters.

BrushStyle later effector slot map: the second pass over `0x142b757b0` identified the remaining generic `PWBrushParameterEffector` destinations, but the first name assignment overreached for the color/spray tail. The corrected names from the later schema-global pass are: `+0x498/+0x4a0 = SubColor`, `+0x4f8/+0x500 = HueChange`, `+0x558/+0x560 = SaturationChange`, `+0x5b8/+0x5c0 = ValueChange`, `+0x620/+0x628 = SpraySize`, and `+0x680/+0x688 = SprayDensity`. These all use the same native graph/random/velocity-capable effector infrastructure where applicable, but the spray pair is consumed by the spray/scatter context rather than the color-change path.

BrushStyle spray/fixed-spray path: `SpraySizeEffector` and `SprayDensityEffector` are schema columns, and the 2026-05-08 consumer pass confirms both are normal runtime `PWBrushParameterEffector` slots: `style+0x620/+0x628` for spray dab size and `style+0x680/+0x688` for spray particle count. The separate special case is only the fixed-spray point-table payload. In the compiler, id `base+0x474` enables fixed spray (`style+0x61c` bit `0x2`), then id `base+0x475` follows a separate helper path (`0x142b55450` / `0x142b55700`, then `0x14256c800` / `0x14256cfd0`, then `0x1424a4a30`) and stores a pointer pair at runtime `+0x6e8/+0x6f0`. Other spray-adjacent runtime fields include `+0x6e0`, flag field `+0x6f8`, and scalars `+0x700/+0x708/+0x710/+0x718`. Importer policy: parse/report spray size and density as ordinary dynamic effectors, while keeping the fixed-spray SPointD child payload as a separate model-data path.

PWBrushFixedSpray object detail: the special spray path resolves to `Planeswalker::PWBrushFixedSpray`, whose vtable is at `0x1444e9598`. Constructor `0x14256bc60` initializes the fixed-spray object and clears vector/shared fields; destructor `0x14256bd70` frees vector storage and releases a shared pointer. The meaningful payload is a vector at object `+0x60/+0x68/+0x70`; helper `0x14256cfd0` copies/resizes it from 16-byte records and increments dword `+0x78`, while equality helper `0x14256cac0` compares each record as two doubles with `1e-8` tolerance. `PWBrushStyle` copy/equality only clones or compares runtime `+0x6e8/+0x6f0` when `style+0x61c` bit `0x2` is enabled. This gives the importer a future target for fixed-spray fidelity: decode the double-pair records first, then decide whether any preview approximation is useful.

PWBrushFixedSpray stream-source follow-up: IDA now shows how the point vector is loaded. Builder `0x14256c200` registers count, stride, and blob-stream properties, then wraps the blob in `PWImportStream`; for each item it seeks to `stride * index`, reads two eight-byte big-endian values through `0x142057a90` / `0x1420574c0`, and stores them as doubles in the `+0x60` vector. The typed lookup helper `0x142b55450` compares against `boost::shared_ptr<std::vector<Planeswalker::SPointD>>`, confirming each 16-byte record is an `SPointD` / 2D double point. The neighboring `PWBrushStyleManager` child branch `0x14256a0b0` builds `PWBrushPatternStyle` objects differently, reading one big-endian u32 per item and resolving it through the style manager. So fixed spray is a serialized child-list point payload, not a compact effector form.

PWBrushFixedSpray clone/intern follow-up: `0x14256be20` copies fixed-spray objects by resizing and copying the complete `+0x60` vector of 16-byte `SPointD` records, then incrementing the destination revision/count at `+0x78`. The style-manager helper `0x1424a4a30` interns fixed-spray children: it scans existing manager entries, calls the point-vector equality helper `0x14256cac0`, and reuses a matching object or creates a new one. Therefore the point records are persistent brush model data that CSP deduplicates, not a draw-time cache. The importer should continue to report them as analysis data but avoid rendering assumptions until a consumer reveals coordinate units and sampling behavior.

PWBrushStyle runtime evaluator follow-up: IDA decompilation of `0x1422d8550` identifies the main per-sample evaluator for compiled runtime `PWBrushStyle`. It writes a plot/sample output record: `+0` effective size/diameter, `+8` opacity-like scalar, `+16` flow/alpha-like scalar, `+24` thickness/texture-size ratio, `+32` rotation angle, `+40/+44` style/interval flags, `+64/+68` pattern-material selection and flip flags, `+72` an auxiliary scalar, and `+88` the caller plane/pass flag propagated from `draw+616`. The function evaluates generic effectors through `0x142568040` and calls `0x14256ace0` only for pattern-material selection. For importer policy, this is a native middle layer to mirror cautiously before adding broad vector brush rendering rules.

PWBrushPatternStyle selector correction: `0x14256ace0` is a `PWBrushPatternStyle` material selector, not the fixed-spray consumer. It reads a vector at object `+0x60/+0x68`, order mode at `+0x78`, and reverse/random flags at `+0x7c`. Modes cover cycle, ping-pong, clamp-last, LCG random, stop-at-end, and caller-supplied custom order list. It returns the selected shared pointer and writes horizontal/vertical flip flags. Runtime `style+0x298` therefore belongs to pattern material selection, while fixed-spray remains the separate `style+0x6e8/+0x6f0` object pair.

PWBrushStyle inverse export map follow-up: `0x142b821a0` serializes runtime `PWBrushStyle` fields back into a BrushStyle parameter map and confirms several offset/schema links. It maps `style+0x298` PatternStyle fields to ids `base+0x42f/+0x432/+0x433`, `style+0x61c` bits to ids `base+0x46a/+0x470/+0x474`, `style+0x620/+0x628` to ids `base+0x46b/+0x46c`, `style+0x680/+0x688` to ids `base+0x46d/+0x46e`, and `style+0x6e0` to id `base+0x46f`. It records only the fixed-spray enable bit; the actual fixed-spray SPointD payload comes from id `base+0x475` through the child-object compiler path.

PWBrushDraw plot-record consumer follow-up: the main direct caller of `0x1422d8550` is `PWBrushDraw` loop helper `0x14255dfe0` at call site `0x14255e22c`. It calls the style evaluator into a stack plot record, then checks coverage through `0x14255c2a0`, optionally splits around tile boundaries through `0x14255c680`, and writes the full record into a draw queue through `0x14260f550`. The downstream code clarifies that record `+48/+56` is a pattern/material shared pointer; coverage uses record `+0` size, record `+24` scale ratio, and expands the effective radius by `1.5x` when the pattern pointer is present. `0x14260f550` copies record `+0..+88` into queue offsets `+296..+384`, including dwords `+64/+68`, doubles/pointers `+72/+80`, and the caller plane/pass flag at `+88`.

PWBrushDraw queue consumer split: `0x14260db90` consumes the queued plot record written by `0x14260f550`. It branches on queue `+344`, the pattern/material shared pointer copied from plot record `+48`: if nonzero with queue `+376`, it calls `0x142636cc0`; if nonzero without queue `+376`, it calls `0x142642010`; if zero, it calls ordinary no-pattern helper `0x14263f410`. This is the concrete Planeswalker brush-material split in the main EXE draw path and should be tracked separately from the older `CSVec4Sampling::InitRasterOperation` pattern-mode path.

Planeswalker dab helper split: `0x14263f410` handles ordinary no-pattern dabs, storing center, clipped bbox, effective size, opacity/flow fixed-point values, rotation, and stretch flags before calling circular or stretched/rotated stamping helpers. `0x142642010` is the pattern-only branch: it resolves the pattern material image from the shared pointer, reads width/height, derives pattern scale from the effective brush size, builds a rotated/flipped UV quad, and submits through `0x1426410b0`. `0x142636cc0` is the pattern+transform branch, computing the material quad from two transformed point/axis pairs before the same submit helper. This supports treating rough balloon/frame line texture as per-dab material drawing rather than a simple post mask.

Pattern quad submit detail: `0x1426410b0(ctx, quad, modeFlag)` rasterizes a four-vertex pattern/material quad. Each vertex is `double x, y, u, v` with stride `0x20`; pattern-only passes mode `0`, pattern+transform passes mode `1`, and the mode changes half-pixel bias when `ctx+0x1d0 == 0`. The function clips the quad against `ctx+0x190`, stores the clipped dab rect at `ctx+0x1a0`, builds edge records, scans horizontal spans, writes fixed-point UV state at `ctx+0x208/+0x20c/+0x210/+0x214`, and dispatches rows through `0x14263b7f0`. Material resolution remains upstream: `ctx+0x1f8/+0x1fc` hold width/height, `ctx+0x200/+0x204` hold max fixed UV, and `0x142642e90` caches pixel base/strides at `ctx+0x238/+0x240/+0x244`. UV lookup is `0x14263e170(ctx, uFixed, vFixed)`.

Pattern material alpha/color detail: pattern spans use `ctx+0x1e0/+0x1e4` as fixed-point opacity/flow, main RGB at `ctx+0x24c/+0x250/+0x254`, secondary/mix RGB at `ctx+0x25c/+0x260/+0x264`, and material format flags at `ctx+0x228`. Flag `0x20` is BGRA/BGRx-like, using byte `+3` for alpha and bytes `+2/+1/+0` for RGB; flag `0x10` is a two-channel/mix material where byte `+1` affects alpha and byte `+0` mixes main/secondary colors; the default path treats byte `+0` as a single-channel alpha mask over main RGB. Final output goes through span dispatcher `0x14263b7f0` and writer family including `0x14263ddb0`.

No-pattern dab helper detail: ordinary brush dabs under `0x14263f410` split into hard circle `0x142640150`, AA circle `0x14263fc50`, hard stretched/rotated ellipse `0x142640c90`, AA stretched/rotated ellipse `0x142640420`, and narrow-minor-axis fallback `0x1426427d0`. All eventually send spans to `0x14263ac30`, where `effective = (ctx+0x1e4 * geometricCoverage) >> 15`; `ctx+0x1e0` is the opacity cap, `ctx+0x1dc` chooses accumulation style, and brush RGB comes from `ctx+0x24c/+0x250/+0x254`. Flags `ctx+0x268/+0x26c/+0x270/+0x274` select writer families and still need UI names.

Fixed-spray consumer follow-up: `0x1422d92d0` is the spray/scatter context builder called by `PWBrushDraw` loop `0x14255dfe0` when `style+0x61c & 1` is enabled. The context fields are now useful: `+0` mode flags, `+4` particle count, `+8` scatter scale/radius, `+16` ordinary random radial-distribution parameter, `+24` fixed-spray rotation angle, and `+32/+40` fixed-spray shared pointer. If `style+0x61c & 2`, the function sets fixed mode, copies `PWBrushFixedSpray` from `style+0x6e8/+0x6f0`, and computes count from the SPointD vector length. If not fixed, it evaluates `style+0x680/+0x688` (`SprayDensityBase/Effector`), rounds/clamps it to at least one particle, and stores `style+0x6e0` (`SprayBias`) as the random radial-distribution parameter.

Fixed-spray particle offset follow-up: `0x142559e30` consumes that spray context. Fixed mode reads `SPointD[i]` from the fixed-spray object's vector at object `+0x60`, multiplies x/y by context `+0x08`, rotates by context `+0x18` using sine/cosine table helper `0x14206d330`, then adds the offset to the current dab center. Ordinary mode uses the LCG seed to generate a shaped random radius from context `+0x10`, a random angle through the same sine/cos helper, and the same center-offset output. So fixed spray is deterministic point-table expansion into additional dab centers, after which the usual brush style, pattern-material, and no-pattern dab rasterizers handle the actual pixels.

BrushStyle schema-global correction: direct xrefs from the actual BrushStyle schema globals into the runtime-layout serializer settle the color/spray tail names. `SubColorBase/Effector` map to `style+0x498/+0x4a0`; `HueChangeBase/Effector` map to `+0x4f8/+0x500`; `SaturationChangeBase/Effector` map to `+0x558/+0x560`; `ValueChangeBase/Effector` map to `+0x5b8/+0x5c0`; `ChangeDrawColorTarget` maps to `+0x618`; `SprayFlag` maps to `+0x61c` bit `1`; `SpraySizeBase/Effector` map to `+0x620/+0x628`; `SprayDensityBase/Effector` map to `+0x680/+0x688`; `SprayBias` maps to `+0x6e0`; and `FixedSpray` maps to the child payload represented at runtime by `+0x6e8/+0x6f0` when `style+0x61c & 2`.

SpraySize consumer correction: IDA decompilation of `0x1422d8550` shows `SpraySizeBase/Effector` is consumed before the spray particle loop, not inside the scatter-offset helper. When `style+0x61c & 1` is enabled, the evaluator uses `style+0x620` as the candidate dab-size scalar, evaluates `style+0x628` through the same `0x142568040` generic dynamic-effector path unless its mode is constant, multiplies by the sample size factor at input `+0x38`, and writes the final particle dab size to the plot record at output `+0`. The call order in `PWBrushDraw` loop `0x14255dfe0` is therefore: `0x1422d8550` computes per-particle dab parameters including SpraySize; `0x1422d92d0` builds count/bias/scatter context using ordinary size for scatter radius and `SprayDensity` for count; `0x142559e30` offsets each particle center; then the normal pattern/no-pattern dab rasterizers draw the actual pixels.

BrushStyle AntiAlias runtime slot: schema xrefs show document `BrushStyle.AntiAlias` maps to runtime `PWBrushStyle+0x190`, and inverse exporter `0x1422e08f0` writes that same field back through the `AntiAlias` schema key. This preserves the UI ordinal `none/weak/medium/strong = 0..3`, matching the isolated `Vector_AA_*` samples. It is separate from the global `CSVectorSetting+0x08` simple-AA on/off path and should not be treated as the same as Planeswalker dab context `ctx+0x1d0` without another consumer proof.

BrushStyle AntiAlias dab consumer: the missing consumer proof now exists for Planeswalker no-pattern dabs. `0x1422d8550` seeds plot record `+44` from runtime `PWBrushStyle+0x190` when the caller does not suppress style AA; `0x14260db90` passes queue `+340` (plot record `+44`) to `0x14263f410`; and `0x14263f410` stores it at draw context `+0x1d0`. Helper `0x1422dbf30(ctx+0x1d0, radius)` maps the ordinal to AA width caps: `0` returns hard/no AA, `1` clamps to `1.5px`, `2` clamps to `2.5px`, and `3` clamps to `3.5px`. Circular and stretched/rotated no-pattern dab helpers use that width for edge coverage, and `0x142637570` uses the same ordinal in the transform-adjacent path. Renderer implication: `.clip BrushStyle.AntiAlias` is no longer just metadata for ordinary Planeswalker dabs, but importer changes still need the native dab geometry/pen-head path to avoid softening the wrong fallback shape.

Plot-record opacity/flow chain: `0x14260f550` is the mechanical copy from the `0x1422d8550` plot record into the draw queue. For the no-pattern branch, plot `+0/+8/+16/+24/+32/+40/+44` becomes queue `+0x128/+0x130/+0x138/+0x140/+0x148/+0x150/+0x154`, then `0x14260db90` passes those to `0x14263f410` as effective size, opacity, flow, thickness/stretch ratio, rotation, rotation/perpendicular flag, and AntiAlias. `0x14263f410` stores size at context `+0x1c0`, AntiAlias at `+0x1d0`, opacity as `32768`-scaled cap at `+0x1e0`, and flow as `32768`-scaled coverage multiplier at `+0x1e4`. The row writer `0x14263ac30` starts with `effective = (ctx+0x1e4 * geometricCoverage) >> 15` and then applies `ctx+0x1e0` as the cap in its accumulation branches. This confirms the native roles of the two plot scalars previously described only as opacity-like and flow-like.

FixedSpray diagnostic boundary: `inspect_vector_dynamics.py` now only decodes byte blobs with `_effector_words()` for keys in `EFFECTOR_KEYS`. Non-effector bytes, including future nonzero `FixedSpray` payloads, stay as raw `{len, hex}` metadata. This keeps `SpraySizeEffector` and `SprayDensityEffector` in the ordinary effector diagnostics while preventing the fixed-spray SPointD child payload from being mislabeled as a compact effector form.

StyleFlag accumulation selector: the remaining `ctx+0x1dc` field is now traced back to `BrushStyle.StyleFlag`, not to `Hardness`. Helper `0x1422dc770` copies `PWBrushStyle+0x78 & 0x1000` into its small writer-state descriptor; `0x14255d810` passes that descriptor field through `0x14260f8b0` into `0x142644180`, which stores it at draw context `+0x1dc`. In the no-pattern row writer `0x14263ac30`, a nonzero value selects direct/max alpha accumulation (`candidate = opacity_cap * flow_coverage`, then max with the existing pixel), while zero selects build-up accumulation (`old + flow_coverage * (opacity_cap - old)`). Current diagnostic output decodes `BrushStyle.StyleFlag` and labels bit `0x1000` as `direct_max_accum_0x1000`; the earlier Bubble_Frame2 `0x1d240` example was superseded by the refreshed `current_vector_summary_v47.json` check, where both `style_combos` and `style_combos_by_kind` contain zero recognized vector/object styles with this bit set.

BrushStyle Hardness no-pattern path: direct schema xrefs map document `BrushStyle.Hardness` to runtime `PWBrushStyle+0x198`. Helper `0x1422dc740` returns that value unless the pattern/material style bit is active, in which case it returns `1.0`. `0x14255d810` places the result in its writer-state block at `+0x10`; `0x14260f8b0` passes it in `xmm1` to `0x142644180`; and `0x142644180` stores it at no-pattern draw context `+0x1c8`. In `0x14263f410`, values below ~`1.0` set `ctx+0x1d4` and initialize a radial hardness/softness profile through `0x142664050` before row writing. This separates the two nearby controls: `ctx+0x1c8` is the actual hardness softness scalar, while `ctx+0x1dc` is the `StyleFlag & 0x1000` accumulation selector.

Hardness profile formula: `0x142663b40` caches the radial profile used by no-pattern dabs. It computes `threshold = hardness * 1.3 - 0.3`. For the normal 16-bit profile, a positive threshold creates a full-coverage center plateau for normalized radius squared below `threshold^2`; outside that plateau the code remaps `r = (sqrt(norm_r2) - threshold) / (1 - threshold)` and uses a smooth two-piece curve: inner half `1 - 2*r^2`, outer half `2*(1-r)^2`, scaled to `0x8000`. The direct/max mode builds a sibling 32-bit table with the same two-piece curve and a scale of `32768 / (1 - threshold)` when `threshold > 0`. `0x142663960` samples the cached 16-bit table at transformed x/y coordinates; non-rotated row paths index the same table directly. `inspect_vector_dynamics.py` now includes both `Hardness` and the decoded `StyleFlag` accumulation bit in `style_combos`, so future samples can be grouped by the two controls separately.

Hardness activation detail: `0x14263f410` only enables the softness/profile path when draw context `ctx+0x1c8` (`BrushStyle.Hardness` after the pattern/material override) is below `~1.0`. When active, it sets `ctx+0x1d4`, initializes the profile with `0x142664050`, and shifts the profile center by `-0.5px` in x/y when AntiAlias is nonzero before scaling by `profile_size / dab_radius`. The row writer then samples the profile through either direct `0x142663960` calls or the precomputed row offset fields. `inspect_vector_dynamics.py` now emits `hardness_profile_active` and `hardness_threshold`; the refreshed `tmp_vector_probe/current_vector_summary_v27.json` shows only the `Hardness=0.95` style activates the profile (`threshold=0.935`), while current pressure-opacity styles remain at `Hardness=1.0`.

Hardness preview experiment rejection: a narrow in-memory sweep on the only active-hardness current style (`Test_Vector.clip` style `4`, `Hardness=0.95`, ordinary `stroke_92`) found that softening the simplified fallback can improve that one metric, but not safely enough to keep. Changing the simple-size branch from radius multiplier `0.75` / feather `0.60` to `0.70` / `0.90` improves `Test_Vector` from mean `1.043658` to `1.023975` with similar visible pixels, and `0.70` / `1.10` reaches mean `1.019231` but increases visible pixels. However, applying the radius change broadly regresses the isolated `Vector_AA_*` samples (`None 0.864298 -> 0.907943`, `Weak 0.731469 -> 0.774714`, `Medium 0.663973 -> 0.705907`, `Strong 0.621886 -> 0.664075`). A gated hardness-only formula would currently be supported by just one style, so no renderer change was kept; exact hardness should wait for the real dab/profile sampling path or more isolated hardness values.

BrushStyle color-mix/water-edge runtime offsets: the inverse exporter `0x1422e08f0` gives a clean schema-to-runtime map for the remaining color-mix block. `UseWaterColor` is `PWBrushStyle+0x358`, `WaterColorType` is `+0x35c`, `BrushColorMixingMode` is `+0x360`, `BrushLMSLinearity` is `+0x364`, `MixColorBase/Effector` are `+0x368/+0x370`, and `MixAlphaBase/Effector` are `+0x3c8/+0x3d0`. The water-edge tail maps separately at the end of the style object: `WaterEdgeFlag` is `+0x6f8`, `WaterEdgeRadius` is `+0x700`, `WaterEdgeAlphaPower` is `+0x708`, `WaterEdgeValuePower` is `+0x710`, and `WaterEdgeBlur` is `+0x718`. These are now confirmed storage/runtime offsets only; renderer consumption still needs targeted samples and consumer tracing before importer preview rules should apply them.

DualComposite/color-mix helper follow-up: `0x142663370` sets up a separate color/mix draw context reached through the writer-state path (`state+0x28` / draw context `a1+40`), so its offsets overlap numerically with the no-pattern dab context but must be named per-context. In that color/mix context, `ctx+392` is the blend/composite selector and traces back through helper `0x1422dc720` to runtime `PWBrushStyle+0x2ac`, the inverse-exported `DualCompositeMode`. `0x142645630` is the RGB+alpha helper: `ctx+396` gates RGB mixing, coverage is scaled as `(coverage * strength) >> 15`, optional `ctx+440` shaping calls `0x142664760`, and fallback RGB comes from `ctx+456/+460/+464` unless source pixel pointers override it. `0x142644cf0` is the matching alpha-only helper and switches on the same `ctx+392` mode family. Modes `0..12` implement normal alpha interpolation plus multiply/add/subtract/min/max/screen/overlay/dodge/burn/linear-burn/hard-mix/vivid-light-like formulas. This confirms `DualCompositeMode` is consumed inside brush color/alpha accumulation, but the exact UI mapping for every formula and its relationship to `MixColor/MixAlpha/UseWaterColor` still needs the upstream `0x14255d810` writer-state field map before importer rendering should depend on it.

Writer-state color/mix field map follow-up: `0x14255d810 -> 0x14260f8b0 -> 0x142663370` now explains which fields actually feed that color/mix context. Primary `sub_1422dc770` descriptor fields are `+0 = CompositeMode (style+0x2a8)`, `+4 = StyleFlag & 0x1000`, `+8 = StyleFlag & 1`, `+12 = active UseWaterColor gate`, `+16 = WaterColorType`, `+20 = BrushColorMixingMode`, `+24 = BrushLMSLinearity`, `+28 = color-change / water / sub-color active gate`, `+32 = texture/color auxiliary pointer/value from the `style+0x2f8` area`, and `+40/+48/+56/+64/+72 = WaterEdgeFlag/Radius/AlphaPower/ValuePower/Blur`. The color/mix context receives the primary `CompositeMode` at `ctx+412`, primary `StyleFlag&1` inverted into `ctx+416`, and primary strength at `ctx+420`; secondary style presence goes to `ctx+388`, secondary `DualCompositeMode` goes to `ctx+392`, secondary `StyleFlag&0x40000` goes to `ctx+396`, secondary texture/water shaping gates flow into `ctx+440/+448`, and current brush RGB is copied to `ctx+456/+460/+464`. This means `MixColorBase/Effector` and `MixAlphaBase/Effector` are not directly consumed by this helper family; they remain confirmed runtime offsets, but their pixel consumer is a separate target.

Row-writer mode-source follow-up: `0x1422dc770 -> 0x14255d810 -> 0x14260f8b0 -> 0x142644180` resolves the no-pattern row-writer family flags that were previously only listed as `ctx+0x268/+0x270/+0x274`. Descriptor `+12` is the active `UseWaterColor` gate and descriptor `+16` is `WaterColorType`; when the gate is active, `0x14255d810` converts `WaterColorType 0/1/2` into `writer_state+0x1c` modes `1/2/3`, otherwise mode `0`. `0x142644180` stores `ctx+0x268 = mode != 0`, `ctx+0x274 = mode == 2`, and `ctx+0x270 = mode == 3`. In `0x14263ac30`, mode `0` keeps the coverage/alpha-buffer writer path, mode `2` enters the branch that can dispatch material-format helpers `0x142638f10/0x142638b90/0x1426388d0`, and mode `3` enters a separate multi-buffer family that can dispatch `0x142639fb0/0x14263a4c0/0x1426432e0`. Current vector pressure samples have default `UseWaterColor=0`, so these branches are important for future watercolor/color-mix samples but should not drive the present opacity-pressure fallback.

BrushStyle color-mix compiler map: `0x142b757b0` confirms the schema ids for the water/color-mix block. Ids `base+0x43e`, `base+0x438`, `base+0x43f`, and `base+0x440` compile `UseWaterColor`, `WaterColorType`, `BrushColorMixingMode`, and `BrushLMSLinearity` into runtime `style+0x358/+0x35c/+0x360/+0x364`. Ids `base+0x439/+0x43a` compile `MixColorBase/Effector` into `style+0x368/+0x370`, and `base+0x43b/+0x43c` compile `MixAlphaBase/Effector` into `style+0x3c8/+0x3d0`. The neighboring color-change family is separate: `base+0x44e` enables the family and sets `StyleFlag` bit `0x20000`, then `base+0x44f/+0x450`, `+0x451/+0x452`, `+0x453/+0x454`, and `+0x44d/+0x44c` compile Hue/Saturation/Value/SubColor base-effector pairs. `inspect_vector_dynamics.py` now emits `color_mix_combos` / `color_mix_combos_by_kind`, fixes the `style_combos` JSON labels for `Hardness` and decoded `StyleFlag`, and adds derived `water_writer_mode = 0` or `WaterColorType + 1` for the row-writer mode. The current corpus snapshot `tmp_vector_probe/current_vector_summary_v26.json` has all 41 vector object styles at the default color-mix combination (`UseWaterColor=0`, `water_writer_mode=0`, `MixColorBase=1.0`, `MixAlphaBase=1.0`, zero effectors, no water edge), so non-default color-mix rendering still needs targeted samples.

Per-sample color descriptor consumer: the missing draw-time consumer for `MixColorBase/Effector` and `MixAlphaBase/Effector` is `0x1422d8140 -> 0x1425597a0`. `0x1422d8140` builds an 88-byte-ish descriptor before each `PWBrushDraw` sample: descriptor `+0/+8/+16/+24` receive evaluated SubColor/Hue/Saturation/Value from runtime `style+0x498/+0x4a0`, `+0x4f8/+0x500`, `+0x558/+0x560`, and `+0x5b8/+0x5c0`; descriptor `+32` receives `ChangeDrawColorTarget` from `style+0x618`; descriptor `+36/+40` receive `BrushColorMixingMode` and `BrushLMSLinearity`; descriptor `+48/+56` receive evaluated `MixColorBase/Effector` and `MixAlphaBase/Effector`; descriptor `+64` receives `ColorExtension`; and descriptor `+72/+80` receive the WaterColorType==1 blur special flag/value. `0x1425597a0` applies descriptor `+8/+16/+24` through `0x14255a020` as HSV-like deltas, uses descriptor `+56` to mix output alpha against the sampled canvas alpha/fallback, and uses descriptor `+48` to mix RGB back toward the sampled canvas color; partial `MixColor` values are squared before weighting. This explains why direct xrefs from `style+0x368/+0x3c8` looked sparse: the style slots are consumed only after being evaluated into the per-sample descriptor.

Historical pressure-sample baseline (superseded by the native `0x31` range-lane entries below): importer verification on the isolated samples then gave `Vector_SizePressure` max `226`, mean `0.115578`, visible `0.068188%`; `Vector_OpacityPressure` max `219`, mean `0.456774`, visible `2.702713%`; `Vector_AA_None` mean `0.864298`, visible `0.558376%`; and `Vector_AA_Medium` mean `0.678055`, visible `0.605106%`. At that stage the size-pressure fallback was considered a narrow heuristic and opacity-pressure was still under-modeled. The corresponding detailed diagnostic snapshot is `tmp_vector_probe/pressure_samples_v1.json`.

Opacity-pressure radius/curve retune: a targeted in-memory sweep showed that the low-risk improvement is to retune only the isolated 24-byte `OpacityEffector=0x31` pressure form. The first pass used `radius_base = width * 1.02` and improved `Vector_OpacityPressure` from mean `0.456774` to `0.386302`; after the `BrushEffectorGraphData` evaluator landed, a narrower sweep found `radius_base = width * 0.99` is better (`0.355155`). A follow-up hard-stroke alpha test found that the preview was being lightened when later low-opacity segments overwrote earlier darker pixels; using a narrow max-alpha preservation mode only for this branch improves the sample to `0.296838`. With that mode active, the best current opacity curve is `max(0.05, clamp(point36 * 1.55, 0, 1) ** 0.65)`, dropping the sample to `0.268066`; finally, changing only this pressure-opacity branch's fallback sample step from `5.0px` to `3.5px` gives `max=219` / `mean=0.262608` / `visible=26314`. `Vector_SizePressure`, the AA samples, `Test_Vector`, and the mixed `test_Filters_Vector_Text` stack remain unchanged, because all changes are gated to `use_pressure_opacity_dynamics`. This is still a preview heuristic, not a replacement for the native opacity/flow/accumulation model.

Vector bridge sample-field copy: local Capstone analysis of the decompiler-timeout bridge confirms `0x1422cd520` is a compact packet-to-internal-context copier. It copies packet `+0/+8` into context `+0x08/+0x10`, converts packet double fields `+0x10/+0x18/+0x20/+0x28/+0x30/+0x38/+0x40` into floats at context `+0x44/+0x48/+0x4c/+0x50/+0x54/+0x58/+0x5c`, and maps packet `+0x50` bit0/bit1 into context flags `0x1000/0x2000`. Caller `0x1430b8640` reaches it after building/updating internal vector point state through `0x1422ce210`. This is important for pressure/opacity work: compact `.clip` point floats should not be named directly as final pressure/velocity fields until the upstream packet fill from serialized point records is mapped.

Vector bridge packet builder: `0x142e65b20` fills the 0x58 packet that later flows through `0x1430b8640 -> 0x1422cd520`. It initializes packet `+0/+8` to zero, scalar fields `+0x10/+0x18/+0x30/+0x38/+0x40` to `1.0`, rotation/aux fields to zero, flags `+0x50` to zero, and status/id `+0x54` from caller `a8`. Then it writes packet `+0/+8 = sampleBlock[0]/[1] + tool offsets a1+0x120/+0x124`, packet `+0x10 = a3`, packet `+0x18 = a4 / 90.0`, and, if tool flags `a1+0x140` or `a1+0x144` are set, packet `+0x28 = a5` or `a6` with flip corrections from `0x1429c38f0/0x1429c3a20`. In the main call site around `0x142e76a94`, the 48-byte sample block at `rbp+0x310` comes from `0x1430bafb0`, which copies one record from a state array at `*(state+0x70+8)`, and `0x1430b8280` optionally transforms only the first two doubles. Sibling helpers confirm the array semantics: `0x1430baf50` copies the last 48-byte sample record, and `0x1430bbc40 -> 0x1430bba20` computes spacing/interval state from sample positions. The remaining upstream target is therefore the producer of that 48-byte sample array.

Vector sample record field semantics follow-up: decompiling `0x142e762f0` confirms the immediate 48-byte sample record layout. Record `+0/+8` are x/y doubles, `+0x10` is the primary scalar passed into packet `+0x10`, `+0x1c` is divided by `90.0` into packet `+0x18`, `+0x18/+0x20` are optional angle-like sources for packet `+0x28`, `+0x24` can OR packet flags with `0x10`, and `+0x28` feeds the distance/progress helper. Since `0x1422cd520` then copies packet `+0x10/+0x18/+0x28` into compact-context floats, compact point float `+36` is the safest pressure-like scalar for active pressure effectors, compact float `+40` is a normalized angle/tilt-like channel, and compact float `+48` is angle-like. History getters `0x1429a5ef0`, `0x1429a5f60`, and `0x1429a5fc0` read an input ring at object `+0x2c0` with entry `+0/+8` x/y, entry `+0x10` scalar, and entry `+0x18` progress/timestamp-like qword. This supports the current importer choice to key pressure-size/opacity samples off compact `+36`, while warning against treating compact `+40` as opacity or pressure.

Vector primary-scalar source path: the wrapper trio `0x142cc0e30`, `0x142cc1020`, and `0x142cc1160` proves compact point `+36` comes from event field `+0xa0` after optional tool-level remapping. Stroke begin initializes a scalar-transform object at `this+0x3c0`, checks tool/event properties `9501` and `9520`, and appends the first 24-byte `{event+0x60 x/y, event+0xa0 scalar}` sample through `0x14309dd50`. Updates either append another sample or evaluate the current scalar through `0x1423e8640`; stroke end calls `0x14309eb40`, which converts the buffered x/y/scalar samples into a double-pair remap table and stores it with `0x1423ea0d0` at object `+0x60`, count `+0x160`. `0x1423e8640` evaluates the table with `0x1431f7f10`, using `xmm1` as the input scalar. Importer implication: compact `+36` is the dynamic context scalar after tool correction/remap, not the final brush size or opacity. Exact pressure-size/opacity preview should eventually evaluate the active `SizeEffector` / `OpacityEffector` graph against this scalar instead of treating the scalar itself as the final output.

Historical pressure-effector graph implementation note (superseded for `0x31` by the native range-lane formula below): the importer added a small `BrushEffectorGraphData` reader/evaluator for the isolated 24-byte pressure form. It parsed the referenced graph control points, evaluated the first graph against compact point `+36`, and used that value as the pressure source for `SizeEffector` / `OpacityEffector` fallbacks; this originally landed as a linear evaluator, then the later native graph pass replaced it with CSP's midpoint-quadratic rule for curved graphs. At that stage the second graph lane was parsed but disabled for the preview because direct compact `+40` application regressed `Vector_OpacityPressure`; later IDA work showed the correct native path is per-emitted-sample range-lane multiplication, not this disabled-lane heuristic.

Native graph evaluator implementation: IDA on `0x1423E8640 -> 0x1431F7F10` shows `BrushEffectorGraphData` is not evaluated as plain piecewise-linear except for two-point graphs. Native points are double `(x,y)` pairs; graphs with three or more points use midpoint-based quadratic Bezier spans, solve `x(t)=input` through `0x1431F6480`, then evaluate `y(t) = (1-t)^2*y0 + 2(1-t)t*y1 + t^2*y2`. The importer graph evaluator now mirrors that rule while keeping the previous clamped input. Current verification is unchanged (`Vector_SizePressure=0.115578`, `Vector_OpacityPressure=0.262608`, `test_Filters_Vector_Text=0.569085`, `Test_Vector=1.043658`, `Vector_AA_None/Weak/Medium/Strong=0.864298/0.731469/0.663973/0.621886`, `Vector_Texture=0.900940`, `Vector_NoTexture=0.235198`, `Bubble_Frame=0.296084`, `Bubble_Frame3_Fill=0.237134`, `Test_Ballon` premul mean `2.317777`). The active sample graphs are effectively equivalent under both evaluators, but future curved effector graphs no longer fall back to a linear approximation.

Primary-graph `0x11` effector rendering rejection: the importer can parse the 12-byte `primary_graph_0x11` form (`flag`, floor scalar, graph id), but applying that graph directly to the ordinary size-dynamics stroke fallback is not safe yet. Native `0x1422d8550` shows the Planeswalker size formula is `base_size * SizeEffector(sample_context+0x10) * sample_context+0x38`, and compact point `+56` is the corresponding packet `+0x38` channel. Even so, substituting `floor + (1-floor) * graph(point+36)` for the existing `point+36` taper made `Test_Vector` worse (`mean 1.043658 -> 1.082250`), and the follow-up `graph(point+36) * point+56` experiment produced the same regression. The helper remains available for diagnostics and future isolated samples, but ordinary `0x11` size rendering stays on the earlier sample-backed taper heuristic until the full CSP dab/spacing/opacity path is modeled.

Plot record field map follow-up: full decompilation of `0x1422d8550` confirms the compact sample channels and output plot fields more tightly. The source context channels derived from the compact point writer are `+36 = sample_context+0x10 primary scalar`, `+40 = +0x18 angle/90`, `+44 = +0x20 aux scalar`, `+48 = +0x28 angle-like`, `+52 = +0x30 velocity factor`, `+56 = +0x38 size factor`, and `+60 = +0x40 flow factor`. The plot record written at the end stores effective size at `+0`, opacity cap at `+8`, flow/coverage at `+16`, thickness/stretch at `+24`, rotation at `+32`, material flags at `+40`, AntiAlias at `+44`, and the texture/color auxiliary scalar at `+72`. In the current pressure and `Test_Vector` samples, compact `+56` and `+60` are constant `1.0`, so they do not explain the remaining mismatch; future isolated flow or size-factor samples should change those channels cleanly.

Interval evaluator detail: `0x1422d8550` also computes brush spacing into the caller state pointer passed as `a8`, not into the plot record itself. Runtime `style+0x200` behaves as `AutoIntervalType`, while `style+0x1a0/+0x1a8` are `IntervalBase/IntervalEffector`. In the normal interval path it evaluates the interval effector when active, computes `spacing = 2 * max(0.1, effective_size) * interval`, and applies lower clamps when AA/softness would otherwise make spacing too small. `AutoIntervalType` values `4/5` instead derive spacing from `(1 - hardness) * effective_size`, with an optional cap from `IntervalBase` for type `5`. This is the native counterpart to the importer's hand-written `sample_step` compensation, but the draw-loop spacing state is not a direct replacement for the current centerline subdivision yet. `inspect_vector_dynamics.py` now includes `interval_base` and `interval_effector` in style combos; `tmp_vector_probe/current_vector_summary_v28.json` shows ordinary pressure/AA styles mostly use `IntervalBase=1.0` with an off effector, while some object/material styles expose distinct interval values such as `0.001`, `2.0`, and `0.014793`.

Rotation/texture/spray diagnostic expansion: `inspect_vector_dynamics.py` now emits separate `rotation_combos`, `texture_combos`, and `spray_combos`, each with `*_by_kind` splits for `stroke_92` and `object_100`. The refreshed `tmp_vector_probe/current_vector_summary_v30.json` shows spray is fully disabled in the current vector corpus (`SprayFlag=0`, no fixed spray). Texture is active only on a few `stroke_92` styles: `test_Filters_Vector_Text` uses `TexturePattern=3`, `TextureFlag=257`, `TextureDensityBase=0.4`, brightness `0.22`, contrast `0.5`; `Vector_Texture` uses `TexturePattern=2`, `TextureFlag=513`, `TextureDensityBase=0.35`. Rotation varies across both strokes and objects. The native `0x1422d8550` rotation block treats runtime `style+0x270` as a bitmask: `0x1` enables rotation, `0x10/0x20` add a sample angle channel, `0x40` adds an auxiliary rotation scalar, and `0x80` adds random rotation scaled by the runtime random amount. Observed SQLite `RotationEffector` values decode as `3` active/base, `19` sample-angle, `67` auxiliary-rotation, and `131` random. These are diagnostics only; renderer use still needs object-family-specific geometry/material sampling.

Color-change/blur diagnostic closure: `inspect_vector_dynamics.py` now emits `color_change_combos` and `blur_combos`, plus `*_by_kind` splits. These correspond to the native per-sample descriptor path `0x1422d8140 -> 0x1425597a0`: `SubColorBase/Effector`, `HueChangeBase/Effector`, `SaturationChangeBase/Effector`, and `ValueChangeBase/Effector` feed descriptor offsets `+0/+8/+16/+24`; `ChangeDrawColorTarget` feeds `+32`; `BlurBase/Effector` feeds the WaterColorType==1 blur special fields `+72/+80`. The refreshed `tmp_vector_probe/current_vector_summary_v31.json` shows the current recognized vector corpus is fully default for these paths: all 41 styles have zero SubColor/HSV/Value changes, off effectors, `BlurKind=0`, `BlurBase=0`, off blur effector, and `UseWaterColor=0`. Therefore current vector preview residuals should stay focused on geometry, dab spacing, opacity/flow/texture/material, and object-family rendering rather than color-change or blur.

Vector schema coverage audit: diagnostics now cover all 71 observed `BrushStyle` columns in the current corpus. The last missing columns were identity/linkage fields (`_PW_ID`, `CanvasId`, and `NextIndex`), not renderer parameters, and they are now included in per-style dumps. The layer-side vector/frame field set was also checked against the current `Layer` table: all vector/frame-relevant columns are covered by `LAYER_VECTOR_KEYS`. `VectorObjectList` itself only has `_PW_ID`, `CanvasId`, `MainId`, `LayerId`, and `VectorData`; `inspect_vector_dynamics.py` now emits the identity subset as `vector_row` on each vector layer. A refreshed full detail dump with these fields is `tmp_vector_probe/current_vector_details_v33.json`.

Object_100 tail naming correction: IDA vtable checks for `PWVectorBalloon` confirm that the 100-byte subclass tail is not `brush_id/subtype/width`. Reader `0x14262a3a0` consumes `+76` into object `+180`, `+80` as the line-style/list id, `+84` as the fill-style/list id, `+88` as the double width/scale, and `+96` as the extra id. Writer `0x14262a570` emits the same order. The importer now stores these as `_VectorObjectHeader.family_id`, `line_style_id`, `fill_style_id`, `width`, and `extra_id`; diagnostics keep legacy `brush_style_id/subtype` aliases only for compatibility. A refreshed corpus snapshot is `tmp_vector_probe/current_vector_summary_v34.json` / `current_vector_details_v34.json`: frame objects are `family_id=1040`, while balloon objects are `family_id=304`. Rendering metrics are unchanged because this was a naming/data-flow cleanup.

Object_100 style split follow-up, corrected: after the tail correction, the frame/bubble line styles reveal why the current fallback cannot be just a solid rectangle. Frame line styles vary through `PatternStyle` and interval families (`Bubble_Frame4_Fill` line style uses `PatternStyle=3`, `AutoIntervalType=2`; `Test_Frames` includes `PatternStyle=10/16`). Fill ids are separate `FillStyle` rows, not BrushStyle rows; the current corpus only exposes `StyleFlag`, `AntiAlias`, `CompositeMode`, and `TextureDensity` for those fills. Therefore line color/fill color should keep coming from object headers and background child layers, but exact frame/bubble rendering still needs separate line-style/material/ruler sampling and fill-style handling rather than hardcoded alpha/rectangle rules.

PWVectorBalloon family-flag follow-up: selected virtuals around the `PWVectorBalloon` vtable decode several `family_id` bits. Bit `0x01` means inherit/follow through the previous balloon object chain; bit `0x10` makes `+88` width active; bit `0x40` suppresses point-geometry traversal; bit `0x200` forces use of the object's own fill list; bit `0x1000` is an enable/visibility-style gate where set means disabled for one queried virtual. The current corpus only exposes two object families: bubble objects use `0x130` (`0x10 + 0x20 + 0x100`), while frame objects use `0x410` (`0x10 + 0x400`). Diagnostics now emit `family_flags` and refreshed snapshots are `tmp_vector_probe/current_vector_summary_v35.json` / `current_vector_details_v35.json`.

PWVectorBalloon family-flag refinement: a full vtable slot pass in IDA adds three more guarded names without changing renderer behavior. Bit `0x02` propagates update/invalidation-style traversals through adjacent balloon objects, bit `0x100` enables a chain hit-test mode that scans related objects, and bit `0x400` marks later siblings that can block that hit-test when their geometry contains the queried area. Bit `0x20` still has no direct consumer in the checked virtuals, so it remains `flag_0x20` in diagnostics. `inspect_vector_dynamics.py` now labels the confirmed bits and writes the refreshed summary as `tmp_vector_probe/current_vector_summary_v37.json`.

PWVectorBalloon line-list render path follow-up: the main geometry slot `0x142629750` follows inherited balloon objects when family bit `0x01` is set, requires family bit `0x10` before using width/style data, then walks child point/curve geometry through `0x1422CC1E0` with the line list at object `+136/+144`. The helper `0x1422DC6F0` returns a secondary shared resource from that list (`+96/+104`), and the balloon renderer can run a second geometry pass with that resource. The deep sampler `0x1422CC1E0` and selection builder `0x1422CA9A0` both use `0x20/0x1000/0x2000` flags on point/list records, but those are not the `PWVectorBalloon` family `+180` bits. This keeps compact family bit `0x20` intentionally unnamed and reinforces that exact bubble/frame outlines need the line-list/style sampler, not a simple object-header-only renderer.

PWVectorBalloon line/fill style resolver follow-up: `0x1424A4450` is the line-style cache resolver, scanning the manager cache at `+0xa8/+0xb0` and building a compiled `PWBrushStyle` through `0x1422D9BE0` when no match exists. That builder allocates a `0x720` byte `PWBrushStyle` (`0x1422D7240`) and copies/compiles the full brush runtime: effectors, pattern/material resources, texture, spray, color-mix, and fixed-spray state. `0x1424A4740` is the separate fill-style cache resolver at `+0x168/+0x170`, building a much smaller `0xc0` byte `PWFillStyle` through `0x14256FD30` / `0x14256FB20`. Fill equality `0x142570A90` compares fill color/state and optional resource fields, while line equality `0x1422DD5B0` compares the full brush plus secondary resource. This confirms compact `line_style_id` and `fill_style_id` resolve into different runtime classes; importer diagnostics should keep them separate, and exact frame/bubble fills should not be rendered by blindly reusing line-brush logic.

PWFillStyle field follow-up, corrected: the `PWFillStyle` constructor initializes direct fill fields at `+0x60/+0x64/+0x68` and an optional image/resource branch controlled by `+0x80 bit0`, whose pointer pair lives at `+0x70/+0x78` with transform/scale-like doubles at `+0x88/+0x90/+0x98/+0xa0`. The clone/copy helper `0x142570140` copies direct fields first, then resolves the optional resource through `0x1424A4ED0`; the equality helper `0x142570A90` uses the same split. The current SQLite `FillStyle` rows do not expose the BrushStyle texture columns (`TexturePattern`, brightness/contrast, flow, etc.); the earlier `fill_style_id=5 -> Paper` reading was caused by incorrectly looking up same-numbered `BrushStyle` row `5`. Native `PWFillStyle` still supports an optional resource path, but current samples only prove the small `FillStyle` schema and object-header/child-background fill color path.

PWFillStyle optional resource resolver follow-up, naming corrected: the optional fill branch uses the same `PWBrushPatternImage` / optional resource-image wrapper family as brush material paths; it does not point directly at a raw brush-pattern image. Manager resolver `0x1424A4ED0` uses a separate cache at manager `+0xd8/+0xe0`, compares entries with `0x14251CDA0`, and builds misses through `0x14251B960`. The builder allocates a `0xc8` byte wrapper object, resolves the source image/resource into pointer pair `+0x60/+0x68`, creates a resizable/transform object at `+0x70/+0x78`, binds the two (`0x1424AC7B0` / `0x1424AF4A0`), computes a transform id/hash at `+0x80`, then copies transform/equality state from source `+0x84` and descriptor data from source `+0x98`. Constructor `0x14251B730` initializes the wrapper vtable as `PWBrushPatternImage`, confirming this is a shared pattern/resource image wrapper rather than a fill-only class.

Resizable material-image mipmap follow-up: the wrapper field above is more specifically a resizable/mipmap image object. `0x1424AC7B0` clears its existing 16-byte `PWMipmapInfo` vector at object `+0x68/+0x70`, marks it dirty at `+0x60`, and appends the source image through `0x1425B26C0`; that `PWMipmapInfo` entry stores a scale/ratio double at `+0x60` and the referenced image pair at `+0x68/+0x70`. `0x1424AF4D0` then ensures the mip pyramid exists: if requested width/height are zero it uses the source image size, chooses levels until dimensions shrink below roughly `64px` (max `8`), trims excess levels, and appends generated downsampled mips. The downsample builders `0x1424AE0C0` and `0x1424ADED0` preserve the source material image's row-sampler selector and storage format (`0x1419BCBD0` / `0x1419BCDA0`) while creating half-size material images. This is the missing bridge between persisted `BrushPatternImage` payloads and draw-time material image objects: native CSP wraps the decoded resource into a resizable mip pyramid before the row sampler sees width/height/format/selector fields.

Material image format initializer follow-up: `0x1432BDA20` is the layout initializer used by those downsample builders. Its fifth argument is the row-sampler/material selector and is stored in the `+0x98` selector family; observed branches include `1`, `0x11`, `0x21`, `0x41`, and `0x100`. Its fourth argument is the paired storage/unit format used for channel counts and row strides around the adjacent `+0x9c` family. Region downsampler `0x1432DC0C0` refuses to copy unless source and destination storage format and selector match, then dispatches per-format resampling helpers. Therefore the importer-side `native_material_selector_guess` remains a diagnostic shortcut from decoded bytes-per-pixel; exact rendering should eventually read or reconstruct this native material-image selector/storage pair, not infer behavior directly from SQLite `TextureFlag`.

Material selector diagnostic refresh: `inspect_vector_dynamics.py` now includes the native selector family and initializer note in each raw material mipmap probe. Refreshed `tmp_vector_probe/texture_flag_probe_v5.json` confirms the custom `Vector_Texture` material still decodes as `1.0` byte/pixel with guessed selector `0x1`, family `single_lane`, coverage lane `0`, while the `Paper` material decodes as `2.0` bytes/pixel with guessed selector `0x11`, family `two_lane`, coverage lane `1`. Each probe also carries a `native_material_format_note` warning that exact rendering should use the resolved material-image selector/storage pair initialized by native `0x1432BDA20`, not `TextureFlag` alone.

Material image wrapper copy/convert follow-up: `0x14251C280` is the richer `PWBrushPatternImage` wrapper builder variant. It allocates a target material image pair at wrapper `+0x60/+0x68` through `0x1432BDE10 -> 0x1432BBA90` (`PWVOffscreen` / material image object) and a resizable mip image at `+0x70/+0x78` through `0x1424AD6B0`. The source image getters are now confirmed: width `0x140D67D30` reads object `+0x60`, height `0x140F5C600` reads `+0x64`, selector `0x1419BCBD0` reads `+0x98`, and storage/pixel format `0x1419BCDA0` reads `+0x9c`. If source storage is `1` and source state flag `+0xd0` is clear, the builder direct-copies pixels with `0x1432BE2F0`; otherwise it initializes the target with `0x1432BDA20(target, width, height, 1, selector)` and converts pixels with `0x1432CD910`. Refreshed `texture_flag_probe_v5.json` and `vector_texture_details_v5.json` now include these getter/copy-convert addresses in `native_material_format_note`.

Material region copy/converter follow-up: the `0x1432CD910` conversion call reaches region core `0x1432D4260`. The core clips source/destination rectangles, prepares material pixel accessors, compares selector (`0x1419BCBD0`, object `+0x98`) and storage (`0x1419BCDA0`, object `+0x9c`), and uses memcpy fast paths when both match. When format conversion is needed, the callback used here is `0x1432D4100`: it walks each destination pixel, resolves destination/source wrappers through `0x1432C58D0`, and calls per-pixel converter `0x1432CDA40`. That converter reads unit size from `0x140F5C610` (`+0x84`) and flags from `0x140F5C620` (`+0x80`), direct-copies identical formats through `0x1432CDEE0`, treats flag `0x10` as a single extra/material lane, and treats flag `0x20` as RGB-like lanes. Renderer implication: exact material/texture preview must preserve selector, storage, unit size, and flags together; decoded bytes-per-pixel is only a diagnostic hint.

Material channel helper follow-up: the `0x1432CDA40` converter relies on the compact packed-channel helper family. `0x14206C3A0`, `0x14206C300`, and `0x14206C350` construct three packed channels at offsets `+0/+4/+8` from 8-bit, 16-bit, and 32-bit source lanes; `0x14206BD90` expands an 8-bit value to `0xVVVVVVVV`, `0x14206BD80` duplicates a 16-bit value into a 32-bit packed channel, and `0x14206BDB0` is the generic bit-depth packer. The three 32-bit component getters are `0x14206BEF0/BEE0/BED0`. When RGB-like lanes need to collapse to a single material lane, `0x142085BA0` uses luminance-style weights `0.298912 / 0.586611 / 0.114478` and clamps the result to `0..1`; it is not merely taking lane 0.

Material stamp-buffer bridge follow-up: `0x142642E90` is the draw-time cache builder that turns a resolved pattern/material image into the temporary stamp/material buffer used by the span sampler. It reuses cache entries keyed by source image, reads source storage with `0x1419BCDA0`, initializes a temp buffer with `0x141AA4BD0(width=ctx+0x1f8, height=ctx+0x1fc, storage, flags=ctx+0x228)`, and then copies/resamples source pixels through `0x141AA5730` when `ctx+0x248` is nonzero or `0x141AA6130` otherwise. `0x141AA4BD0` stores flags at buffer `+0x18`, storage at `+0x1c`, computes lane counts (`0x10 -> 1`, `0x20 -> 3`, plus optional base lane when `flags&1`), and feeds allocation through `0x141AA4C70`. The builder finally publishes the cached pixel base/strides to draw context `+0x238/+0x240/+0x244`, which is what the fixed-UV material pixel lookup consumes.

Material stamp-buffer copy-path follow-up: `0x141AA5730` is the general multi-lane source-to-stamp-buffer copy path. It clips the requested rectangle, builds a source block accessor, divides source/destination strides by unit size from `0x140F5C610`, and branches on temp-buffer flags from `0x140894590` (`buffer+0x18`). For unit sizes `1` and `2`, flags/selector `1` copies only the source base lane (`0x1419D5CD0`); `0x11` writes one extra/material lane from `0x1432C58A0` plus the base lane; `0x21` writes three RGB-like extra/material lanes plus the base lane. Missing source blocks zero-fill the corresponding lane width. The sibling `0x141AA6130` is a base-lane fast path only: it requires destination storage to match source storage, temp flags equal `1`, and aligned buffer state. Therefore two-lane Paper-like materials and RGB-like materials are not just wider base masks; they use the general extra-lane copy route.

Material sampler/row-dispatch follow-up: the lane order copied into the stamp buffer is consumed directly by `0x142637930`. That sampler fetches the fixed-UV material pixel and chooses coverage by `ctx+0x228`: bit `0x20` reads pixel byte `3`, bit `0x10` reads byte `1`, and the default reads byte `0`; then it multiplies active span coverage by the selected byte divided by `255`. Row dispatcher `0x14263B7F0` uses the same flags to choose writer families: `0x20` routes to RGB-like material row writers (`0x142639A30` / `0x14263D9F0`), `0x10` routes to two-lane/material-mix writers (`0x142639710` / `0x14263D750`), and `1` routes to base-lane writers (`0x1426394E0` / `0x14263D580`). This is the native end-to-end proof that Paper-style two-lane materials use byte lane `1` for coverage, while RGB-like `0x21` resources use byte lane `3`.

Four-sample material helper follow-up: material sampling is not a simple nearest lookup. Cached-buffer helper `0x14263E5D0` samples four fixed-UV material pixels through `0x14263E170`, sums byte `0`, and scales active coverage by `sum/(4*255) = sum/0x3fc`. Cached two-lane helper `0x14263E970` treats byte `1` as coverage/alpha and byte `0` as material mix; only nonzero-alpha samples count, coverage uses `sum(byte1)/0x3fc`, and mix output is average byte `0`. Cached RGB-like helper `0x14263EE20` treats byte `3` as alpha and bytes `2/1/0` as RGB-like material channels, averaging RGB over nonzero-alpha samples. `0x14263E700`, `0x14263EB10`, and `0x14263F070` are the parallel source-image/block-accessor variants used when the row path is not reading the cached temp buffer. This means exact texture preview needs a four-sample material footprint or a close approximation, not point-sampling.

Final material row writer follow-up: `0x14263DDB0` is a concrete final writer for material/color rows. It computes `candidateCoverage = min(0x40000000, sampledCoverage * opacityCap(ctx+0x1e0)) >> 15`, writes into the 16-bit coverage plane, and stores BGR bytes. When `ctx+0x1dc` is nonzero, it is a direct/max accumulation path: only candidates larger than existing coverage replace the pixel and color. Otherwise, it uses build-up/capped accumulation, moving existing coverage toward the opacity cap and blending BGR with `0x8000` fixed-point weights. This connects material texture coverage to the same opacity-cap/accumulation model already observed in no-pattern dabs.

Row accumulator variant follow-up: several adjacent writers are now classified. `0x14263C060` applies brush profile/hardness, optional layer mask, and `0x142664760` texture/coverage adjustment, then delegates BGR+coverage accumulation to `0x14263DDB0`. `0x14263C3A0` is the dynamic material-color variant: each pixel calls `0x142637A70` or `0x142637C70` to resolve coverage plus BGR before the same final writer. `0x14263C5C0` is coverage-only and updates the 16-bit coverage plane toward opacity cap without writing BGR. `0x14263CB20` and `0x14263D2F0` are 16-bit/material accumulation variants using flow `ctx+0x258` and optional color-mode helper `0x14256DF40`. `0x1426388D0` and `0x142638F10` read a source span through `ctx+0x50`: `0x14266D2F0` exposes the span x range and `0x14266D2B0` clamps x/y then returns the temp-buffer pixel via `0x141AA4F90`. These paths show the same coverage ingredients recurring across material rows: brush profile, mask, texture modifier, flow, opacity cap, and optional auxiliary coverage plane `ctx+0x148`.

Texture coverage modifier follow-up: `0x142664760` is the row-writer texture/coverage adjustment helper. It samples a repeating 8-bit texture using rotation/offset fields (`+0x58/+0x60` transform, `+0x68/+0x6c` offset, `+0x70/+0x74` width/height, `+0x78` stride, `+0x40` raw pixel base or `+0x18` image wrapper). If `+0x7c` is set it uses `0x142664550`, a bilinear sampler that weights four neighboring 8-bit samples with the fixed-point fractional coordinate and returns `HIWORD(weighted_sum)`. The sampled byte can be inverted (`+0x54`), remapped through threshold/range fields (`+0x88..+0x9c`), then scaled by density/amount fields (`+0x4c/+0x50/+0x80/+0x84`). Finally mode `+0x48` selects one of ten blend formulas against the incoming coverage and clamps to `0..0x8000`. This is why a faithful texture preview must model CSP's coverage blend mode and bilinear/repeating sample coordinates; a simple post-alpha material mask is structurally wrong.

Texture descriptor source follow-up: `0x1422DC680` copies the optional 72-byte `PWBrushStyle` texture descriptor from runtime style `+0x2b0` when `style+0x2c0 bit0` is set. The descriptor layout feeding `0x142664C00` is resource pair `+0/+8`, flags `+0x10`, coverage blend mode `+0x14`, density/scale `+0x18`, angle `+0x20`, offsets `+0x28/+0x30`, and remap/brightness-contrast-like doubles `+0x38/+0x40`. `0x14255D810` stores the primary descriptor in writer-state `+0x88..+0xc8`; `0x14260F8B0` passes that stack descriptor to `0x142664C00`; and `0x142644180` stores the enable bit into draw context `ctx+0x1e8`, which is the flag tested before row writers call `0x142664760`. This gives the implementation rule: parse all texture columns for diagnostics, but apply texture rendering only when the runtime/serialized texture enable flag is active.

Texture descriptor SQLite map: the descriptor fields align with the observed `BrushStyle` columns: resource pair `+0/+8` comes from `TexturePattern` resource resolution, `+0x10` is `TextureFlag`, `+0x14` is `TextureComposite`, `+0x18` is `TextureScale`, `+0x20` is `TextureRotate`, `+0x28/+0x30` are `TextureOffsetX/Y`, and `+0x38/+0x40` are `TextureBrightness/Contrast`. `TextureFlag & 0x100` controls inversion before the sampled byte is converted to `0..0x8000`. `TextureFlag & 0x200` enables the remap/contrast path; `0x1426642D0` computes the whole-texture average byte used as the baseline for that remap. Current examples match this split: `test_Filters_Vector_Text` has `TextureFlag=0x101`, brightness `0.22`, contrast `0.5` but no `0x200` remap bit; `Vector_Texture` has `TextureFlag=0x201`, zero brightness/contrast, and therefore enables the average-baseline remap path.

TextureComposite formula boundary: `TextureComposite` is copied to texture context `ctx+0x48` and switches the final coverage formula in `0x142664760`. Let `c` be the incoming row coverage and `t` be the sampled/remapped texture coverage, both in `0..0x8000`; `b=ctx+0x84` is the average-baseline value used only when remap is enabled; `d=ctx+0x4c` and `m=ctx+0x50` are density/amplitude fields initialized by `0x1426640E0` and updated by `0x142664260`. Mode `0` scales `c` by `0x8000 + (b or 0x4000) - t`; mode `1` multiplies by inverse texture `0x8000-t`; mode `2` subtracts `t` (scaled by `m` under remap); mode `3` caps to inverse texture; mode `4` density-adjusts `c` then subtracts `t`; mode `5` is overlay-like using `c` against inverse texture; mode `6` divides `c` by `t` with saturation; mode `7` is inverse divide/dodge-like; mode `8` is `2*c - t`; mode `9` combines density-scaled `c` with inverse texture and caps to `m`. Current samples use `TextureComposite=0`, so other modes are native-mapped but still need varied samples before renderer tuning.

TextureDensity effector path: `0x1422D8550` computes plot record `+72` from the runtime fields immediately after the 72-byte texture descriptor (`style+0x2f8` base and `+0x300` effector area, matching `TextureDensityBase/TextureDensityEffector`) only when descriptor flags satisfy `(TextureFlag & 0x11) == 0x11`. That plot scalar copies through `0x14260F550` to queue `+0x170`, reaches `0x14263F410` as `a11`, and is passed into `0x142664260(textureCtx, density, flowFixed)`, where it becomes texture context `ctx+0x4c`; the third argument is the already fixed-point row flow `ctx+0x1e4`, stored at texture context `ctx+0x50`. Current texture samples `0x101` and `0x201` do not set bit `0x10`, so they do not prove the density-effector branch visually. Future samples should include `TextureFlag` with bit `0x10` set and varied `TextureDensityBase/Effector`.

Optional brush/fill resource-list follow-up: the resource wrapper machinery above is shared by `PWBrushStyle`, not only `PWFillStyle`. In `PWBrushStyle` clone/compile copier `0x1422DA100`, the material/list field at style `+0x298/+0x2a0` is resolved through `0x1424A5120`; that resolver scans manager cache `+0x108/+0x110`, compares candidates with `0x14256AFF0`, and builds misses with `0x142569BC0`. The composite wrapper is `0x88` bytes: vector range `+0x60/+0x68` stores 16-byte resolved resource pairs, `+0x78/+0x7c` stores two scalar ids, and every list item is resolved by `0x142569A70 -> 0x1424A4ED0`. `0x1422DA100` also resolves a single optional resource at `+0x2b0/+0x2b8` through `0x1424A4ED0` when `+0x2c0 bit0` is set. Exact textured strokes and textured fills should therefore share one resource-wrapper parser with two callers: single resource and composite resource list.

Resource diagnostics split, corrected: `inspect_vector_dynamics.py` decodes `BrushPatternStyle.ImageIndex` as big-endian image ids and emits separate `pattern_material_refs_by_kind` / `texture_resource_refs_by_kind` sections for the `BrushStyle` path. A later audit found that applying those helpers to `fill_style_id` was wrong: 100-byte object `fill_style_id` resolves through the separate `FillStyle` table, not `BrushStyle`, even when the ids numerically collide. The corrected full-corpus snapshot is `tmp_vector_probe/current_vector_summary_v41.json`: `object_100_line` still shows line material patterns such as `粗い線`, `Sampled Brush 1`, and `unknown material`, while `object_100_fill` no longer reports BrushStyle pattern/texture resources.

Per-object resource diagnostics correction: ordinary strokes may include `pattern_material_resource` or `texture_resource`, and 100-byte bubble/frame objects may include `line_pattern_material_resource` / `line_texture_resource` for their line `BrushStyle`. Fill details are now attached as a `fill_style` row from the real `FillStyle` table. In `test_Filters_Vector_Text.clip`, both bubble and frame line styles resolve to `PatternStyle=7 -> `粗い線` / Mipmap 80`; the frame fill id `5` resolves to `FillStyle` `{StyleFlag=0, AntiAlias=0, CompositeMode=0, TextureDensity=1.0}`. The earlier `fill_texture_resource -> Paper` conclusion was a BrushStyle-id collision and is superseded.

Object_100 fill-style summary correction: `object_100_fill` is now counted through `fill_style_combos_by_kind`, using the actual `FillStyle` schema (`StyleFlag`, `AntiAlias`, `CompositeMode`, `TextureDensity`). `tmp_vector_probe/current_vector_summary_v41.json` shows two combinations in the current corpus: `CompositeMode=31` used 13 times and `CompositeMode=0` used 9 times, both with `TextureDensity=1.0` and `AntiAlias=0`. Current samples therefore prove fill is a separate style path, but they do not prove pattern/texture fill resources in `FillStyle`.

Object_100 fill context follow-up: `inspect_vector_dynamics.py` now emits child-layer summaries on each 100-byte object plus `object_100_fill_contexts` in the corpus summary. The refreshed `tmp_vector_probe/current_vector_summary_v43.json` shows a clean current-corpus split: `FillStyle.CompositeMode=31` appears only with balloon family `304` / `VectorNormalType=1` / no child layers, while `CompositeMode=0` appears only with frame family `1040` / `VectorNormalType=3` / `ComicFrameLineMipmap`. Frame fill visibility still correlates with folder children: fill-bearing frames have `Frame background 1` child rows with render mipmaps and `GradationFillInfo`, while no-child frame samples keep the line/cache but should not synthesize a filled rectangle from the object header alone. This reinforces the importer rule: use `FillStyle` for fill-state classification, object headers for RGB, and frame child layers/background rows for actual frame fill presence.

PWFillStyle direct-field offset refinement: rechecking `0x14256FB20`, `0x142570140`, and `0x142570A90` in IDA shows the three direct fields are runtime offsets `+0x60`, `+0x64`, and `+0x68`. The constructor defaults `+0x60=0`, `+0x64=2`, and `+0x68=0`, and the copy/equality helpers compare/copy them before any optional resource branch. Given the current SQLite `FillStyle` schema order, the most likely mapping is `StyleFlag -> +0x60`, `AntiAlias -> +0x64`, and `CompositeMode -> +0x68`; however, the visible sample distinction is currently stronger evidence than a named enum: mode `31` clusters with balloons and mode `0` clusters with frames.

Installed tool-preset cross-check: opening `Settings\PAINT\Tool\english\EditImageTool.todb` as SQLite confirms the built-in rough balloon/frame presets do not directly explain `FillStyle.CompositeMode=31/0` through the preset `Variant.CompositeMode` column. `Rough rounded balloon` variants `783/784` have `Variant.CompositeMode=NULL`, `AntiAlias=2`, `BrushSize=0.5`, `BrushThickness=150`, `BrushUsePatternImage=1`, `BrushPatternOrderType/2=3`, reverse horizontal/vertical `2/2`, `ShapeStrokeAndFillType=2`, line color type `0` with RGB `0`, fill color type `1` with RGB `0xffffffff`, `BalloonInsertType=1`, and `BalloonFillOpacity=100`. `Rough rectangle` variants `785/786` share the rough line settings but have `AntiAlias=1` and frame-specific flags `ComicFrameMakeBaseLayer=1`, `ComicFrameMakeFillLayer=1`, `ComicFrameDrawLine=1`, with no `Shape*` or `Balloon*` fill fields. Therefore the `.clip` FillStyle composite split should be treated as a persisted object/fill runtime classification, while the tool preset split explains how CSP decides to create balloon fill objects versus frame folder/background layers.

Diagnostic naming cleanup: `tmp_vector_probe/current_vector_summary_v44.json` adds `style_usage` as the accurate top-level name for mixed stroke/line/fill style references. The older `brush_usage` field remains as a compatibility alias, but new consumers should prefer `style_usage` because `object_100_fill` entries are `FillStyle` ids, not brush ids.

Effector branch naming cleanup: `inspect_vector_dynamics.py` now annotates every decoded compact effector blob with `native_runtime_branches`, naming the branch bits consumed by native `PWBrushParameterEffector` evaluator `0x142568040`: `0x10` primary graph input, `0x20` secondary graph input, `0x40` auxiliary graph input, `0x80` random, and `0x100` velocity. A focused refresh wrote `tmp_vector_probe/vector_effector_branch_details_v1.json`; for example, `0x31` now clearly reports primary+secondary graph branches, while `0x111` reports primary graph plus velocity. Summary output also includes `effector_runtime_branches`, refreshed in `tmp_vector_probe/effector_branch_summary_v1.json`, so future pressure/tilt/random/velocity samples can be compared without rereading raw hex.

Graph curvature diagnostics: `inspect_vector_dynamics.py` now includes `native_eval` on each `BrushEffectorGraphData` record and in `effector_graph_signatures`. It reports whether the graph uses the native `linear` or `midpoint_quadratic` rule and samples the maximum difference versus the old piecewise-linear approximation. A focused probe (`tmp_vector_probe/graph_eval_diagnostics_v1.json`) shows current curved graphs can differ materially from linear evaluation (`max_delta_vs_linear` up to about `0.289`), even if the active renderer branches tested so far remain unchanged. The refreshed full snapshot is `tmp_vector_probe/current_vector_summary_v46.json`.

Vector AA sample pass: the isolated `Vector_AA_None/Weak/Medium/Strong` clips confirm the AA UI maps directly to `BrushStyle.AntiAlias = 0/1/2/3`; the paired styles and strokes are otherwise stable, including the `92/76/88/88 flags=0x2011` pressure-ish stroke and the `92/76/120/88 flags=0x41` thin black curve object. The current fallback already consumes the AA ordinal as feather strength. A temporary test treating the 120-byte point tail as a standard cubic Bezier, as a control-point polyline, or as a mixed control/cubic path was rejected: all three worsened the four AA metrics versus the existing straight-line fallback. Current final PNG means remain `None=0.864298`, `Weak=0.731469`, `Medium=0.663973`, `Strong=0.621886`; the remaining errors are CSP brush rasterizer/curve sampling details, not the AA enum lookup.

Leaf vector fallback hardening: non-folder leaf layers now share the same vector/text/balloon fallback sequence used by leaf folder layers when `decode_layer()` is absent or decodes to an empty vector cache. This is a generalization guard for `LayerType=0` vector-bearing samples. The current AA samples already reach the same fallback path in practice, so their metrics are unchanged; regression checks also keep `test_Filters_Vector_Text`, `Test_Ballon`, `Test_Frames`, `Vector_Texture`, `Vector_NoTexture`, and pressure samples stable.

Object_100 layer-context follow-up: diagnostics now attach layer vector/frame fields to each 100-byte object and summarize them as `object_100_layer_counts` in `tmp_vector_probe/current_vector_summary_v36.json`. The current corpus maps cleanly: `family_id=304` always appears on `VectorNormalType=1` layers with no `ComicFrameLineMipmap` (ordinary bubbles), while `family_id=1040` always appears on `VectorNormalType=3` layers with `ComicFrameLineMipmap` and comic-frame color flags (frames). Future renderer branching should use these data fields rather than layer names.

Object_100 data-driven renderer routing: the importer now classifies frame/vector object layers through `_is_frame_vector_layer()` and `_is_balloon_vector_layer()`, using `VectorNormalType`, `ComicFrameLineMipmap`, and parsed `family_id` (`1040` for frames, `304` for bubbles) instead of layer names or generic `LayerFolder` truthiness. This keeps ordinary vector strokes away from bubble/frame fallbacks while allowing renamed frame/bubble layers to render through the same preview path. Regression checks after the route change stayed unchanged for `Bubble_Frame*`, `Bubble_Shape*`, `Test_Ballon`, `Test_Frames`, `test_Filters_Vector_Text`, `Vector_AA_*`, `Test_Vector`, `Vector_SizePressure`, and `Vector_OpacityPressure`.

Simple FlowEffector rendering rejection: `Test_Vector`'s simple stroke also has `FlowEffector=0x11`, and native `0x1422d8550` writes flow to plot `+16`, later consumed by row coverage/accumulation. A direct importer experiment that multiplied the simplified stroke opacity by `FlowEffector(point+36)` regressed `Test_Vector` (`mean 1.043658 -> 1.187665`) while not helping the AA/mixed guards. Keep flow as native/diagnostic metadata for now; it needs the real row writer coverage and accumulation model, not a naive per-segment alpha taper.

Opacity-pressure accumulation experiment: native row writer `0x14263ac30` distinguishes direct/max accumulation from build-up capped accumulation. A first importer experiment using alpha max/cap before the later graph/radius retunes produced no metric change on `Vector_OpacityPressure` (`mean 0.386302` unchanged at the time), but a narrower hard-stroke max-alpha preservation mode after those retunes does help (`0.355155 -> 0.296838`) by preventing later low-opacity segments from overwriting earlier darker pixels. Plain alpha-over is rejected because it over-darkens the same sample (`1.508120`). The remaining residual is still likely in dab geometry, coverage profile, or sample placement rather than generic segment overlap.

Vector AA half-width import rule: native no-pattern dabs map `BrushStyle.AntiAlias` levels `1/2/3` to roughly `1.5/2.5/3.5px` AA width caps, but applying the corresponding half-widths (`0.75/1.25/1.75px`) to every ordinary compact vector stroke regressed the mixed `test_Filters_Vector_Text` stack (`mean 0.569085 -> 0.574793`). The importer now uses those native-inspired widths only for the newer `flags=0x2011` two-point ordinary stroke family exposed by the isolated AA samples, while the older `0x2081` path keeps the established conservative `0.20px` micro-feather and the simple-size branch's `0.60px` feather. Verification after narrowing and the opacity retunes: `Vector_AA_None` remains `0.864298`, `Weak` improves to `0.731469`, `Medium` to `0.663973`, and `Strong` to `0.621886`; `Test_Vector` stays at `1.043658`, `Vector_SizePressure` at `0.115578`, `Vector_OpacityPressure` at `0.262608`, and full `test_Filters_Vector_Text` stays at `0.569085`.

PWBrushDraw coverage-gate follow-up: rechecking `0x14255c2a0`, `0x1425590b0`, and `0x1422e04c0` narrows the opacity-pressure residual. `0x14255c2a0` is only a plot-parameter bounds test: it rounds the sample center, applies plot `+0` size and `+24` scale, and expands radius by `1.5x` only when plot `+48` has a pattern material pointer. `0x1425590b0` is a buffer/queue state gate before sample submission, and `0x1422e04c0` is mostly spray/pattern post-processing for rotation, pattern selection, and extra effector re-evaluation. These are now commented in IDA and do not justify changing ordinary no-pattern opacity rendering; remaining work should stay focused on dab coverage/profile/sample placement.

Opacity-pressure follow-up rejections: after the current `0.262608` result, several tempting geometry tweaks were tested and rejected. Enabling the second `OpacityEffector=0x31` graph lane directly still regresses badly (`mean 1.005212`), while a 20% secondary-lane blend gives only a tiny mean change (`0.268066 -> 0.267481` before the sample-step retune) and increases visible pixels, so it stays disabled. Switching the opacity-pressure centerline from Catmull-Rom to linear interpolation only changes `0.262608 -> 0.261427` but increases visible pixels; using compact point pen-bbox centers worsens mean to `0.326946`; and small anchor/bbox-center blends do not beat the current anchor path. Whole-image shift tests also keep `dx=0, dy=0` as best, so there is no obvious layer-offset correction to apply.

Native spacing-state note: IDA decompilation of `0x1430bba20` and wrapper `0x1430bbc40` confirms a real sample spacing/history state exists upstream of the draw queue. `0x1430bba20` computes Euclidean distance between current and stored double x/y points via `0x141a8e410`, multiplies by a state scale at `+0x10`, applies early/history modifiers, and clamps the stored output at `<=1.0`; `0x1430bbc40` then derives a per-substep delta from the previous spacing state over the caller-supplied count. These functions were commented in IDA. This supports keeping the importer `3.5px` opacity-pressure step as a narrow preview compensation, not as a claimed native constant.

Hard-circle span rejection: `0x142640150` is the hard no-pattern circular dab helper and its row loop computes `sqrt(radius^2 - dy^2)`, then shrinks the horizontal span with `fmax(0, sqrt(...) - 0.4)` before submitting full `0x8000` coverage to `0x14263ac30`. This is native evidence that CSP's hard dab scan conversion is slightly inset, but it does not transfer cleanly to the current importer capsule fallback. A focused opacity-pressure sweep applying constant radius deltas around the current branch gave worse means for every tested delta (`-0.8=0.371191`, `-0.6=0.331427`, `-0.4=0.316888`, `-0.2=0.293425`, `+0.2=0.271846`, `+0.4=0.275275`, `+0.6=0.297116`) compared with the current `0.262608` at delta `0.0`. Keep the `-0.4` finding documented for the eventual real dab rasterizer, but do not apply it as another preview heuristic.

Vector texture mipmap probe follow-up: `inspect_vector_dynamics.py` now attaches a `mipmap_probe` to each `BrushPatternImage` resource. The refreshed pressure/texture snapshot `tmp_vector_probe/vector_pressure_texture_details_v3.json` confirms `Vector_Texture` style `11` uses `TexturePattern=2`, `TextureFlag=513`, `TextureDensityBase=0.35`, and mipmap `29`. The external block is compressed/packed (`external_len=225997`), but after `_parse_exta` it becomes `decoded_tile_blob_len=262144` for a `512x512` offscreen, i.e. `decoded_bytes_per_pixel=1.0`; ordinary RGBA tile decode correctly fails because the expected RGBA tile length would be `1048576`. This supports the existing native conclusion that brush texture/material is a single-channel mask-like resource consumed inside the stamp/material rasterizer, not a post-stroke RGBA overlay.

Native texture sampler follow-up: IDA comments now mark `0x142637930` as the pattern/material alpha sampler and `0x14263E170` as the fixed-point material pixel lookup. The draw path advances 15-bit fixed UVs per span pixel, looks up material pixels from `ctx+568` using strides `ctx+576/+580`, selects byte `0`, `1`, or `3` depending on material format flags at `ctx+552`, and multiplies the current coverage scalar by that `0..255` material value. Dispatcher `0x14263B7F0` then routes the row through the appropriate material-format blend path. This is the native reason texture has to be integrated into stamp/row coverage, not multiplied over the completed vector fallback mask.

TextureFlag corpus split, superseded by native map: `inspect_vector_dynamics.py` emits `TextureFlagDecoded` while preserving the raw `TextureFlag` value. The initial texture probe showed two observed families: `Vector_Texture` style `11` uses `TextureFlag=0x201` and a `1.0` byte/pixel decoded mipmap; `test_Filters_Vector_Text` style `5` uses `TextureFlag=0x101`, texture name `Paper`, and a `2.0` byte/pixel decoded mipmap. Later native tracing of `0x142664C00` and `0x1426642D0` names the bits more precisely: `0x100` is sample inversion, and `0x200` enables the average-baseline remap path. Both resources still fail RGBA decode by design because they are material/texture inputs rather than RGBA tiles.

Texture raw-lane statistics: `tmp_vector_probe/texture_flag_probe_v3.json` adds byte-lane stats for non-RGBA material mipmaps. The custom `Vector_Texture` `0x201` resource has one lane with min `0`, max `255`, avg `169.652912`, and `258903/262144` nonzero bytes. The `Paper` `0x101` resource has two non-empty lanes with nearly identical distributions: lane `0` avg `188.452492`, lane `1` avg `188.459919`, both min `0` and max `255`; however, the lanes are not a simple duplicate, because only `135865/262144` pixels are equal (`0.518284`), mean absolute difference is `14.602528`, and max difference is `149`. This proves the second Paper lane is real auxiliary data, but not yet whether it is an alpha/mix/height-like channel; native material format flags still need to name the lane semantics.

Material image object layout follow-up: IDA comments now identify the draw-time material image getters used by the pattern/stamp path. `0x140D67D30` returns width from image object `+0x60`, `0x140F5C600` returns height from `+0x64`, `0x140F5C620` returns capability/format flags from `+0x80`, `0x1419BCBD0` returns the row-sampler material selector from `+0x98`, and `0x1419BCDA0` returns the pixel/storage format from `+0x9c`. Cache builder `0x142642E90` resolves the source material image, then initializes a temporary draw buffer through `0x141AA4BD0`. That initializer computes lane count and row stride from the storage format plus material flags: the `0x10` family has one base lane plus an optional extra lane, the `0x20` family has three base lanes plus an optional extra lane, and the low nibble/bit0 path handles simpler formats. This explains how single- and two-byte material mipmaps become native stamp resources while remaining separate from the persisted SQLite `TextureFlag` field.

Material lane-selection follow-up: disassembly at `0x1426379b7` shows the exact coverage lane selector. If draw context `+0x228` has bit `0x20`, the sampler reads `pixel[3]`; if it has bit `0x10`, it reads `pixel[1]`; otherwise it reads `pixel[0]`. The selected byte is multiplied into the current coverage scalar at `0x142637A33`. `inspect_vector_dynamics.py` now emits a conservative `native_material_selector_guess` from decoded material bytes-per-pixel: `1.0 bpp -> selector 0x1 / lane 0`, `2.0 bpp -> selector 0x11 / lane 1`, `4.0 bpp -> selector 0x21 / lane 3`. In `texture_flag_probe_v4.json`, this means the custom `Vector_Texture` resource is expected to use lane `0`, while `Paper` is expected to use lane `1`.

Composite resource-list source follow-up: `0x14256A7D0` is the explicit-list constructor for the same `0x88` byte `PWBrushPatternStyle` / composite resource wrapper family. It receives a source vector of 16-byte resource pairs, iterates every entry, resolves each through `0x142569A70 -> 0x1424A4ED0`, resizes the destination pair vector through `0x14256BAA0`, and only then stores scalar order/reverse ids at `+0x78/+0x7c`. The alternate manager entry `0x1424A1B40` is called by `0x142E58F00` after that caller builds a temporary vector, while the main compiled brush-style path remains `0x1422DA100 -> 0x1424A5120`. This is native evidence that `BrushPatternStyle.ImageIndex` should stay a list in diagnostics/import data, even when the current samples often contain one image: exact pattern rendering must preserve item order, reverse/random rules, and per-item wrapper transforms instead of collapsing the style to the first `BrushPatternImage`.

Writer-state bridge refinement: the IDA pass over `0x14260F8B0`, `0x142644180`, `0x1422DC770`, and `0x142663370` now names the primary/secondary draw-context bridge more explicitly. `0x1422DC770` builds a compact writer descriptor from `CompositeMode`, `StyleFlag`, `UseWaterColor`, `WaterColorType`, `BrushColorMixingMode`, `BrushLMSLinearity`, texture/color auxiliary state, and water-edge fields; it does not by itself render MixColor/MixAlpha. `0x14260F8B0` sends the primary state block through `0x142644180`, mapping writer-state fields into no-pattern/material context slots such as hardness, accumulation selector, water/material writer flags, material enable, color/water mode, and trailing row-writer flags. When the secondary descriptor/resource branch is active, the same bridge is called again from the secondary block. `0x142663370` separately seeds the color/mix context with secondary-style gates (`ctx+0x184/+0x188/+0x18c`) and fixed-point strengths, so future importer work should treat color-mix/watercolor as a draw-context family rather than a single post-blend formula.

MixColor/MixAlpha consumer refinement: `0x1425597A0` applies the per-sample descriptor built by `0x1422D8140`. Descriptor `+0` first blends the primary/secondary output colors as a sub-color amount in fixed `0..0x8000`. Descriptor `+56` (`MixAlpha`) then sets output alpha as `(1 - mixAlpha) * sampledCanvasAlpha + mixAlpha * fallbackAlpha`, where the fallback alpha is controlled by the sampled/fallback state at `a1+204`. Descriptor `+48` (`MixColor`) blends RGB after alpha is nonzero; partial values are squared (`mixColor^2`) before weighting current brush RGB against sampled canvas RGB. Therefore CSP color-mixing and watercolor preview cannot be approximated as a plain linear layer post-process; it has to run per dab/sample after canvas sampling and before the row writer's DualCompositeMode family.

Current color-mix corpus check: `current_vector_summary_v47.json` still reports a single default `color_mix_combos` row across all 41 recognized vector styles: `UseWaterColor=0`, `water_writer_mode=0`, `BrushColorMixingMode=0`, `BrushLMSLinearity=0`, `MixColorBase=1.0`, `MixAlphaBase=1.0`, both mix effectors off, and all water-edge fields at defaults. The by-kind split is only `22 object_100` plus `19 stroke_92`, with identical values. This means the native MixColor/MixAlpha formula is ready for future samples, but the current preview residuals should not change renderer behavior for color mix yet.

Opacity/flow row accumulation refinement: decompiling `0x14263AC30` and `0x14263DDB0` clarifies the native no-pattern row formulas. The row writer first computes `flowCoverage = (ctx+0x1e4 flow * geometricCoverage) >> 15`; `ctx+0x1e0` is the opacity cap. If `ctx+0x1dc` is nonzero, CSP uses direct/max accumulation: `candidate = (opacityCap * flowCoverage) >> 15`, then writes only when `candidate` exceeds the current 16-bit coverage. If `ctx+0x1dc` is zero, CSP uses build-up accumulation: `new = old + flowCoverage * (opacityCap - old) >> 15`, capped by the opacity cap. The hardness/profile branch applies the profile to `flowCoverage` first and then uses the same direct/max vs build-up split. This confirms the importer's narrow max-alpha opacity-pressure fallback has a native analogue, but the exact UI/style switch still needs a sample that varies the `StyleFlag & 0x1000` accumulation bit.

Current accumulation corpus check: `inspect_vector_dynamics.py` already decodes `StyleFlag` as `(hex, direct_max_accum_0x1000)`. In `current_vector_summary_v47.json`, both `style_combos` and `style_combos_by_kind` have zero rows with `direct_max_accum_0x1000=True`; all current recognized vector/object styles use the build-up flag state. Therefore the current `OpacityEffector=0x31` max-alpha preview branch should remain documented as a narrow visual compensation for the simplified fallback, not as a claim that the sample's stored `StyleFlag` enables native direct/max accumulation.

CSP installed tool database follow-up: the shipped `Settings\PAINT\Tool\*\EditImageTool.todb` / `UXEditImageTool.todb` files are SQLite tool-preset databases with `Manager`, `Node`, and `Variant` tables. `Variant` exposes UI/source fields such as `AntiAlias`, `BrushSize`, `BrushOpacityEffector`, `BrushFlow`, `BrushFlowEffector`, `BrushHardness`, `BrushUsePatternImage`, `BrushRibbon`, `BrushUseWaterColor`, `BrushMixColor`, `BrushMixAlpha`, and `BrushUseSpray`, but does not expose a literal `StyleFlag` column. The UX database confirms `BrushOpacityEffector` as a first-class preset-side field, matching the `.clip` opacity effector records. The Japanese rough presets line up with the sample family: `粗いペン` variant `781`, `粗い角丸フキダシ` variant `783`, and `粗い長方形コマ` variant `785` all use pattern images plus ribbon, interval `10`, auto interval `2`, thickness `150`, pattern order `3/3`, reverse horizontal/vertical `2/2`, and no water/spray; the balloon/frame pair mainly differs in `AntiAlias` (`2` for the rough rounded balloon, `1` for the rough rectangular frame). This supports using the tool DB as a preset-input reference, while the actual `StyleFlag` bit mapping still has to come from the native compiler path (`0x142B757B0` / inverse exporters), not from a direct database column.

StyleFlag compiler id follow-up: raw disassembly of `BrushStyle` compiler `0x142B757B0` shows the accumulation bit is read through parameter id `base+0x460`, not from a literal tool-DB `StyleFlag` column. At `0x142B78825..0x142B78885`, CSP reads id `base+0x460`; when the value is present/nonzero it sets `PWBrushStyle+0x78 |= 0x1000`. The inverse exporter `0x142B821A0` mirrors this at `0x142B8339B..0x142B833DE` by extracting `PWBrushStyle+0x78 & 0x1000` and writing parameter id `base+0x460`. Nearby id `base+0x456` maps cleanly to `BrushRibbon`, setting/exporting `StyleFlag bit 0x20`, matching the installed rough pen/balloon/frame presets. Current conclusion: `direct_max_accum_0x1000` is a real native parameter projection, but it is hidden/derived relative to the shipped tool DB; importer behavior still needs a `.clip` or preset sample that actually stores this bit before using direct/max broadly.

Vector diagnostics v48 and routing cleanup: refreshed focused probes are `tmp_vector_probe/current_vector_details_v48.json` and `current_vector_summary_v48.json`, covering the small vector/bubble/frame corpus without the large Live2D clips. They confirm color-mix and spray remain fully default in current vector/object samples. Non-default evidence is concentrated in texture/material strokes (`Vector_Texture` uses `TexturePattern=2`, `TextureFlag=0x201`, `TextureDensityBase=0.35`; `test_Filters_Vector_Text` style `5` uses `TexturePattern=3`, `TextureFlag=0x101`, `TextureDensityBase=0.4`, `FlowBase=0.5`), the lone `Hardness=0.95` style in `Test_Vector`, and pattern/ribbon line styles on frame/balloon objects. The importer no longer uses a `LayerName` contains `"balloon"` shortcut for childless vector-bearing folders; when `TextLayerAttributes` are present, it lets the normal raster/text-cache path run before falling back to balloon/vector rendering, and when text attrs are absent it uses the `VectorNormalType` / family-id balloon classifier directly. Verification after this cleanup is pixel-stable for the guards: `test_Filters_Vector_Text=0.569085`, `Test_Ballon` premul mean `2.317777`, `Bubble_Frame=0.296084`, `Bubble_Frame3_Fill=0.237134`, `Vector_SizePressure=0.115578`, `Vector_OpacityPressure=0.262608`, `Test_Vector=1.043658`, `Vector_Texture=0.900940`, `Vector_NoTexture=0.235198`, and AA none/weak/medium/strong `0.864298/0.731469/0.663973/0.621886`.

Vector fallback constant naming cleanup: the importer now gives names to the remaining data-driven vector routing constants and renderer-gap parameters without changing pixels. `VectorNormalType` values `1/3` are labeled as balloon/frame, family ids `0x130/0x410` are labeled as balloon/frame, observed stroke flags `0x41`, `0x2011`, and `0x2081` are separated from the fallback radius/feather constants, and the `VectorNormalBalloonIndex` shape-specific bbox/outline/power tweaks are grouped as balloon fallback tuning rather than inlined branch literals. The important boundary is unchanged: family ids, normal type, stroke flags, colors, width, opacity, AA ordinal, style ids, frame mipmap presence, balloon index, and child gradation fill are `.clip`/SQLite evidence; constants such as the simple/pressure radius scales, legacy micro-feather, filled-curve superellipse scale, text/balloon inset tuning, and native-inspired AA half-widths are importer preview approximations until the real CSP vector dab/span renderer is implemented. Repacking the add-on and rerunning guard clips kept all v48 metrics unchanged; focused checks after the balloon-tuning naming pass stayed at `test_Filters_Vector_Text=0.569085`, `Test_Ballon` premul mean `2.317777`, `Bubble_Shape2=0.289637`, and `Vector_OpacityPressure=0.262608`.

PWVectorStroke line-list pass follow-up: the current IDA session confirms ordinary strokes and balloon/frame objects share more renderer structure than the importer fallback currently models. `0x1425A4100` walks a stroke's primary line-list resource through `0x1422CC1E0`, optionally repeats with the secondary resource from `0x1422DC6F0`, and is called from draw-context bridge `0x14255A7E0`; `0x1425A3D60` and `0x1425A4320` are dirty/update and region-aware variants of the same traversal. This mirrors the `PWVectorBalloon` path at `0x142629750`, so `VectorNormalBalloonIndex` should be treated as a pointer/entry into vector object/ruler data, not as the final shape algorithm. Exact bubble/frame/vector fidelity remains a compiled line-style resource plus sampler-output problem, not a reason to add broader ellipse-shape heuristics.

Line-list secondary resource refinement: `0x1422DC6F0` is a small getter for the compiled line/list object's secondary resource pair at `+0x60/+0x68`; `0x1422DD7A0` checks mode bits at `+624/+648` and style flags at `+120`; and `0x1422DF860 -> 0x14256B190` prepares optional multi-entry list data when the line/list object has optional data at `+0x298`. Importer implication: exact rough-line/vector rendering cannot be recovered from compact object headers alone. The next native-to-importer bridge has to expose or emulate the compiled line/list resource state that sits between `line_style_id` and the `0x1422CC1E0` sampler.

Compiled line-style model refinement: `line_style_id` enters native rendering through resolver `0x1424A4450`, which caches compiled line styles and builds misses with `0x1422D9BE0`. That builder constructs the full `0x720` byte `PWBrushStyle` runtime object via `0x1422D7240` and `0x1422DA100`; the compile/copy path resolves effector/resource blocks through `0x1425682A0`, material/list resources at `style+0x298/+0x2a0`, and the single texture-like resource at `+0x2b0/+0x2b8`. Cache equality `0x1422DCAD0` compares the deep runtime style, not just a style id. Importer policy: SQLite `BrushStyle` rows remain the right source for diagnostics and conservative fallbacks, but broad exact vector rendering should wait until this compiled runtime layer and resource wrappers are modeled.

Compiled line-style diagnostics: `inspect_vector_dynamics.py` now attaches `compiled_line_style_note` to 92-byte strokes and `line_compiled_style_note` to 100-byte object line styles whenever the source `BrushStyle` has pattern/material or texture evidence. The note records the native compile chain (`0x1424A4450 -> 0x1422D9BE0 -> 0x1422DA100`), the `PWBrushStyle 0x720` runtime object boundary, and the native state that is not serialized directly in SQLite (`+0x60/+0x68` secondary resource, optional multi-entry list preparation, deep cache equality). Summary output now also includes `compiled_line_style_notes_by_kind`: in `tmp_vector_probe/compiled_line_style_summary_v2.json`, `test_Filters_Vector_Text` separates style `5` ordinary strokes as texture-descriptor evidence and object line styles `6/7` as pattern/material-list evidence. This gives future corpora a quick split between texture strokes and rough-line pattern objects before any exact renderer is implemented.

Brush StyleFlag diagnostics: summary output now includes `style_flags_by_kind`, which exposes native-observed bits such as `0x20` retained-state path, `0x200` segment-start flag, and `0x1000` direct/max accumulation by stroke/object-line style id. In `tmp_vector_probe/style_flag_summary_v1.json`, `test_Filters_Vector_Text` shows ordinary stroke style `5` without `0x20`, while object line styles `6/7` have `0x1c230` and set `native_retained_state_path_0x20`; this matches the native `PWBrushDraw +0x20 -> 0x14255C980` dispatch into the retained material/texture phase path.

Vector sampler output contract: native `0x1422CCA10` is the segment sample emitter under `0x1422CC1E0`. It prepares a sample record from vector point fields, lets the output sink adjust step length through vtable `+0x18`, calls sink `+0x10` at segment start, and submits every interpolated sample through sink `+0x20`. The surrounding bridge (`0x1430B8640` / `0x1430B89E0` / `0x1430B8F50` -> `0x14255A7E0`) feeds those samples into `PWBrushDraw` buffers prepared by `0x142558580`. The native sink vtable is now resolved as `PWBrushDraw @ 0x1444E9110`: `+0x10 -> 0x14255C510` segment init/bounds seeding, `+0x18 -> 0x14255C440` secondary-resource step rescale, and `+0x20 -> 0x14255C980` per-sample dispatch into ordinary `0x14255DFE0` or retained-state `0x142558A90`. The retained-state branch uses `0x1422D8BB0` to keep material/texture phase state and select pattern resources through `0x14256ACE0`, so texture-like vector strokes cannot be treated as independent centerline capsules. This confirms current importer vector fallbacks are skipping an entire native sink layer: they draw compact centerlines directly, while CSP evaluates compiled `PWBrushStyle` through the sink/draw-buffer layer before row rendering.

Pattern selector diagnostics: IDA `0x14256ACE0` decodes the native pattern/material image selector. It chooses entries from the compiled pattern vector using `OrderType` modes `0..5` (sequence modulo, ping-pong, clamp-to-last, random LCG, stop-when-exhausted, explicit-index-list) and applies `Reverse2` axis flags for fixed, random, or ping-pong-derived flips. `inspect_vector_dynamics.py` now reports `order_type_label` and `reverse2_decoded` for pattern material references; `tmp_vector_probe/pattern_selector_summary_v1.json` shows the rough bubble/frame line style uses `OrderType=3/random_lcg` and `Reverse2=0x22`, meaning both axes use random flip decisions.

Vector point sampler-record diagnostics: `0x1422CC1E0` confirms the first seven compact point float fields are submitted to the `PWBrushDraw` sink as dynamic sample channels, not just inert metadata. The sink record uses `+0/+8` for x/y, `+16..+64` for compact `f32+36/+40/+44/+48/+52/+56/+60`, `+72` for an optional path/neighbor metric, and `+80` for flags derived from internal `0x1000/0x2000`. Compact `f32+48` is angle-like and interpolated with wrapped 0..360 helper `0x14206D6E0`; compact `f32+56` and `f32+60` are size/flow-like factors whose interpolation is gated by point flags. `inspect_vector_dynamics.py` now adds `native_sampler_record_0x1422CC1E0` to point probes; `tmp_vector_probe/sampler_record_detail_v1.json` confirms the field note on `Vector_SizePressure`.

Vector sampler spacing feedback: the apparent zero step in `0x1422CCA10` is not a dead loop. The segment emitter passes a compact seed/state block to sink `+0x20`; `0x14255C980 -> 0x14255DFE0 -> 0x1422D8550` writes the next interval into that state at `+8`, and the sampler adds that value after each submitted sample. The larger curve sampler `0x1422CC1E0` uses the same idea with its caller-provided state: residual distance lives at `state+8`, each submitted sample lets the brush evaluator update the next interval, then the curve loop advances and carries the remainder across segments. `AutoIntervalType=0` writes `2 * max(0.1, effective_size) * evaluated IntervalBase/Effector`, with small AA/softness clamps. `AutoIntervalType` `1/2/3` calls `0x1422DBF80` to derive the interval scalar from auto type, hardness-like softness, and the thickness/stretch-like scalar; the outer evaluator still multiplies by effective size afterward. `AutoIntervalType` `4/5` uses the separate `(1 - hardness_like_value) * effective_size` path, with type `5` capped by `IntervalBase`. `0x1422D9910` is an effective footprint estimator used for bbox and secondary-resource rescaling, while `0x1422D9200` only seeds start-of-segment `+56/+64` factors. Importer implication: fixed preview sample steps are still compensation constants, because native spacing is an evaluator feedback loop involving size, interval, AA, auto-interval/hardness, thickness, and dynamic point channels. Summary JSON now exposes this as `native_vector_sampler_spacing`.

Native spacing estimate diagnostics: `inspect_vector_dynamics.py` now emits `native_spacing_combos_by_kind` and `native_spacing_estimates_by_kind`. The estimate table is intentionally conservative: it uses the compact stroke/object width as the best-effort base size, multiplies compact `f32+56` size-factor range, and separately reports a best-effort `0x142568040` `SizeEffector` multiplier range from graph outputs. The first focused probe `tmp_vector_probe/sampler_spacing_summary_v3.json` showed why fixed importer spacing is fragile: `Vector_SizePressure` style `10` had a baseline `state+8鈮?.2px` before its dynamic size graph, while a very wide `Test_Vector` normal-interval row with `IntervalBase=0.001` estimated `state+8鈮?.3272px`, and another auto-interval wide row estimated `26.176px`. After adding the graph multiplier estimate in `sampler_spacing_summary_v5.json`, `Vector_SizePressure` style `10` becomes `state+8鈮?.016..3.187px`, and `Test_Vector` style `4` becomes `0.509..1.264px` instead of the baseline `2.7px`. These values are diagnostics for experiment planning, not pixel behavior yet.

Spacing estimate bucket layer: `tmp_vector_probe/current_vector_summary_v68.json` adds `native_spacing_estimate_bucket_counts` and `native_spacing_estimate_recommendations` on top of the detailed rows. In the focused vector/bubble/frame corpus, `9` rows have estimated native `state+8` intervals below `0.5px`, and `6` rows depend on dynamic `SizeEffector`. This turns the raw estimates into an implementation warning: any future vector raster-preview sampler should be adaptive and evaluate active size/pressure/tilt/velocity effectors per sample; a single fixed centerline step will overdraw tiny-interval rows and undersample wide dynamic rows.

Per-clip spacing detail: the same estimator is now attached directly to scanned vector records. Ordinary 92-byte strokes expose `native_spacing_estimate`, and 100-byte vector objects expose `line_native_spacing_estimate`. `tmp_vector_probe/vector_sizepressure_detail_v2.json` shows the practical payoff: the `Vector_SizePressure` stroke carries baseline `state8_without_size_effector_range=[3.2,3.2]` and estimated dynamic `state8_with_estimated_size_effector_range=[0.016,3.186707]` in the stroke record itself.

Importer-vs-native spacing comparison: `tmp_vector_probe/current_vector_summary_v71.json` adds `importer_spacing_comparisons_by_kind` and `importer_spacing_comparison_recommendations`. This compares the current centerline fallback subdivision constants (`VECTOR_FALLBACK_SAMPLE_STEP=5.0`, `VECTOR_OPACITY_PRESSURE_SAMPLE_STEP=2.0`) to the estimated native `state+8` interval range. Weighted focused-corpus counts are `13` rows where fallback subdivision is coarser than the estimated native interval, `7` rows where the ranges overlap, and `1` row where fallback subdivision is finer than the native interval. Because the importer currently draws continuous capsules while native spacing controls dab submission, this is still diagnostic-only; it says the next renderer experiment should be adaptive/per-branch, not a global retune of `5.0`.

Adaptive spacing experiment: `clip_loader.py` now has a default-off branch guarded by `RIZUM_CLIP_EXPERIMENTAL_VECTOR_ADAPTIVE_SPACING=1`. It estimates a native-style interval from `AutoIntervalType` `0/1/2/3`, `IntervalBase`, `Hardness`, `ThicknessBase`, compact `f32+56`, and the already-applied pressure-size taper, then only reduces the current fallback step down to a floor of `0.5px`. A broad version also touched simple-size rows and regressed `Test_Vector` (`1.043658 -> 1.075104`), so the kept experiment is limited to `SizeEffector=0x31` pressure-size strokes. Current metrics with the env enabled: `Vector_SizePressure` improves `mean 0.115578 -> 0.112669` and visible pixels `715 -> 697`; `Vector_OpacityPressure` stays `0.122786`, `Test_Vector` stays `1.043658`, and AA none/medium, texture, and no-texture guards remain unchanged. `tmp_vector_probe/current_vector_summary_v72.json` exposes the new env and `VECTOR_ADAPTIVE_SPACING_MIN_STEP=0.5` in `importer_vector_fallback_policy`.

Adaptive spacing candidate diagnostics: `tmp_vector_probe/current_vector_summary_v73.json` adds `experimental_adaptive_spacing_candidates_by_kind`, and per-clip detail adds `experimental_adaptive_spacing_candidate` / `line_experimental_adaptive_spacing_candidate`. The focused corpus currently has exactly one active candidate: `stroke_92` style `10` in `Vector_SizePressure`, with fallback step `5.0`, native interval estimate `0.016..3.186707`, and approximate adaptive step range `0.5..3.186707`. The other twenty comparable rows are explicitly inactive, mostly because broad adaptive spacing was already rejected for non-`0x31` size rows.

Experiment comparison runner: `verify_vector_experiments.py` now automates default-vs-experimental checks for the focused vector guard set. It runs `verify_one_clip.py` twice per clip, once with no extra environment and once with `RIZUM_CLIP_EXPERIMENTAL_VECTOR_ADAPTIVE_SPACING=1`, then records per-metric deltas. The first snapshot is `tmp_vector_probe/adaptive_spacing_verify_v1.json`: `Vector_SizePressure` improves by `mean_delta=-0.002909` and `visible_px_delta=-18`, while `Vector_OpacityPressure`, `Test_Vector`, AA none/medium, texture, and no-texture guards all report `mean_delta=0` and unchanged visible pixels.

Experiment regression gate: the same runner now emits a `summary` block and supports `--fail-on-regression`, `--max-mean-regression`, and `--max-visible-regression`. `tmp_vector_probe/adaptive_spacing_verify_v2.json` reports `improved=1`, `unchanged=6`, `regressed=0`, and `tmp_vector_probe/adaptive_spacing_verify_gate_v1.json` confirms that `python verify_vector_experiments.py --fail-on-regression --max-visible-regression 0` exits successfully for the current narrowed adaptive-spacing experiment. This gives future default-off vector experiments an exit-code guard instead of relying on manual JSON inspection.

Experiment activation gate: `verify_vector_experiments.py` also supports `--min-improved`, so a guard can require that the default-off branch actually changes at least one intended sample. The recommended current command is `python verify_vector_experiments.py --fail-on-regression --max-visible-regression 0 --min-improved 1 --out tmp_vector_probe/adaptive_spacing_verify_gate_v2.json`; it exits `0` with `improved=1`, `unchanged=6`, `regressed=0`. A deliberate negative check with `--min-improved 2` exits `1` and records a `__summary__` failure in `tmp_vector_probe/adaptive_spacing_verify_gate_negative_v1.json`.

Experiment target gate: `verify_vector_experiments.py` now supports `--require-improved <clip-name>` so the intended sample must improve, not merely any sample. The current recommended guard is `python verify_vector_experiments.py --fail-on-regression --max-visible-regression 0 --min-improved 1 --require-improved Vector_SizePressure.clip --out tmp_vector_probe/adaptive_spacing_verify_gate_v3.json`; it exits `0`. A deliberate negative check with `--require-improved Test_Vector.clip` exits `1` and records the missing required improvement in `tmp_vector_probe/adaptive_spacing_verify_gate_negative_v2.json`.

Adaptive-spacing guard preset: `verify_vector_experiments.py --adaptive-spacing-gate` now expands to the current recommended adaptive-spacing thresholds: fail on regression, disallow positive visible-pixel deltas, require at least one improved clip, and require `Vector_SizePressure.clip` to be improved. `tmp_vector_probe/adaptive_spacing_verify_gate_v4.json` confirms the preset exits `0` with `improved=1`, `unchanged=6`, `regressed=0`, matching the longer explicit command.

Experiment error gate: `verify_vector_experiments.py` now supports `--fail-on-error`, and `--adaptive-spacing-gate` enables it automatically. This makes missing references, failed renders, or absent comparable metrics fail by exit code instead of appearing only as `unknown`. `tmp_vector_probe/adaptive_spacing_verify_gate_v5.json` still passes with `fail_on_error=true`, while `tmp_vector_probe/adaptive_spacing_verify_error_negative_v1.json` uses a missing clip to confirm the gate exits `1` and records default/experiment render failures plus missing `mean_direction`.

Experiment table view: `verify_vector_experiments.py --print-table` now prints a compact delta table to stderr while preserving JSON output for files/stdout. The current short command `python verify_vector_experiments.py --adaptive-spacing-gate --print-table --out tmp_vector_probe/adaptive_spacing_verify_gate_v6.json` exits `0` and prints rows for `direction`, `mean_delta`, and `visible_delta`; only `Vector_SizePressure` is marked improved, all other guard samples remain unchanged.

Guard status metadata: `verify_vector_experiments.py` now writes `guard_preset` and `summary.passed` into the JSON payload. `tmp_vector_probe/adaptive_spacing_verify_gate_v7.json` has `guard_preset="adaptive_spacing"` and `summary.passed=true`, while retaining `failure_count=0`, `improved=1`, and `unchanged=6`. This makes the output easier for future automation to consume without reconstructing pass/fail from individual counters.

Run configuration metadata: `verify_vector_experiments.py` now writes a `run_config` block containing resolved clip paths, experiment environment, preset flag, fail/error settings, thresholds, and required-improvement targets. `tmp_vector_probe/adaptive_spacing_verify_gate_v8.json` confirms the adaptive-spacing preset ran over the seven focused vector guard clips with `RIZUM_CLIP_EXPERIMENTAL_VECTOR_ADAPTIVE_SPACING=1`, `fail_on_regression=true`, `fail_on_error=true`, `max_visible_regression=0`, `min_improved=1`, and `require_improved=["Vector_SizePressure.clip"]`.

Non-target unchanged guard: `verify_vector_experiments.py` now supports `--require-unchanged`, and `--adaptive-spacing-gate` requires the six non-target vector guards (`Vector_OpacityPressure`, `Test_Vector`, AA none/medium, texture, no-texture) to remain unchanged. `tmp_vector_probe/adaptive_spacing_verify_gate_v9.json` passes with the target pressure-size sample improved and all six non-target rows unchanged. A deliberate negative check requiring `Vector_SizePressure.clip` to be unchanged exits `1` and records its improvement delta in `tmp_vector_probe/adaptive_spacing_verify_gate_negative_v3.json`.

Expected-direction metadata: `verify_vector_experiments.py` now records `summary.expected_directions` and includes an `expected` column in `--print-table` output. `tmp_vector_probe/adaptive_spacing_verify_gate_v10.json` shows the intended contract directly: `Vector_SizePressure.clip -> improved`, and the six non-target guard samples -> `unchanged`. The printed table displays expected and actual directions side by side before the delta columns.

Per-row expectation annotations: `verify_vector_experiments.py` now copies the expectation into each clip row as `expected_direction` and records `matches_expected`. `tmp_vector_probe/adaptive_spacing_verify_gate_v11.json` confirms all seven adaptive-spacing guard rows match their expected direction, so downstream scripts no longer need to join each row back to `summary.expected_directions`.

Duplicate basename guard: expectation rules are intentionally keyed by clip basename, so `verify_vector_experiments.py` now detects ambiguous duplicate basenames before interpreting expected directions. The JSON records duplicates under both `run_config.duplicate_clip_basenames` and `summary.duplicate_clip_basenames`; default runs emit a warning, while `--fail-on-error` and the adaptive-spacing preset promote duplicates to `config_error` failures. `tmp_vector_probe/adaptive_spacing_verify_gate_v12.json` passes with no duplicates, and `tmp_vector_probe/adaptive_spacing_verify_duplicate_negative_v1.json` deliberately repeats `Vector_SizePressure.clip` and exits `1`.

Effector channel diagnostics: native `0x142568040` maps the effector branch bits to the sampler record. Bit `0x10` evaluates graph pointer `+0x28` with sample channel `+0x10` (compact `f32+36`), bit `0x20` evaluates graph pointer `+0x38` with sample channel `+0x18` (compact `f32+40`), bit `0x40` evaluates graph pointer `+0x48` with sample channel `+0x20` (compact `f32+44`), bit `0x80` applies random, and bit `0x100` multiplies by sample channel `+0x30` (compact `f32+52`, velocity-like). `inspect_vector_dynamics.py` now names these sample channels in `native_runtime_branches`; `tmp_vector_probe/effector_channel_detail_v1.json` shows the opacity-pressure sample's `0x31` effector uses compact `f32+36` and `f32+40`.

Opacity-pressure secondary-lane preview: after the native channel map, the importer now applies a narrow `0.29` blend of the native `0x20` lane factor only for the `OpacityEffector=0x31` pressure-opacity fallback. Full native multiplication still regresses because the capsule fallback lacks CSP's dab geometry, row coverage, and accumulation pipeline, but a tuned share improves the isolated pressure-opacity sample. Retuning only that branch's sample step from `3.5px` to `2.0px` and alpha curve from the earlier `scale=1.55, power=0.65` to `scale=1.72, power=0.88` improves it further: `Vector_OpacityPressure` moves from mean `0.262608` / visible `26314` / max `219` to mean `0.122786` / visible `18175` / max `218`. Guards stayed stable: `Vector_SizePressure=0.115578`, `test_Filters_Vector_Text=0.569085`, `Test_Vector=1.043658`, AA none/medium `0.864298/0.663973`, `Vector_Texture=0.900940`, `Bubble_Frame=0.296084`, and `Test_Ballon` premul mean `2.317777`.

Opacity-pressure corpus scope: `tmp_vector_probe/small_effector_corpus_v1.json` scans the current small vector/bubble/frame corpus and finds exactly one `OpacityEffector` `dual_graph_0x31` row and one `SizeEffector` `dual_graph_0x31` row. The opacity row is therefore currently isolated to `Vector_OpacityPressure`; keep the `0x31` opacity preview compensation sample-gated until another file proves the same constants generalize.

PWBrushStyle evaluator formula checkpoint: the current IDA comments on `0x1422D8550` now pin the native scalar roles tightly enough to guide future importer work. Effective dab size is `baseSize * evaluated(SizeEffector or SpraySizeEffector) * sample_context+0x38` (compact `f32+56`), opacity cap is evaluated from runtime `OpacityEffector` into plot `+8`, and flow/coverage is `evaluated(FlowBase/Effector) * sample_context+0x40` (compact `f32+60`) before row accumulation. Plot `+44` carries the style AntiAlias ordinal and plot `+72` carries texture/color auxiliary density only through the texture gate. Importer policy: keep compact `+36/+40/+44/+52/+56/+60` visible in diagnostics, but do not promote any one channel to direct alpha/size/flow rendering unless an isolated sample and the native row/dab path both support it.

Evaluator formula diagnostics: `inspect_vector_dynamics.py` summary output now includes `native_pwbrushstyle_evaluator_0x1422D8550`, a compact JSON copy of the native input-channel and plot-record map above. A fresh sanity output is `tmp_vector_probe/evaluator_formula_summary_v1.json` from `Vector_OpacityPressure` plus `Vector_SizePressure`, and the refreshed focused corpus snapshot is `tmp_vector_probe/current_vector_summary_v49.json` (`16` clips, `22` vector layers). This is documentation metadata only and does not change renderer behavior.

Vector fallback guard cleanup: the remaining local vector-stroke guard numbers have been promoted to named module constants without changing pixels. `VECTOR_STROKE_MAX_POINTS`, `VECTOR_STROKE_MAX_WIDTH`, and `VECTOR_STROKE_MIN_VISIBLE_SPAN` are parser/fallback sanity limits; `VECTOR_FILLED_CURVE_MAX_SOLID_WIDTH`, `VECTOR_FILLED_CURVE_DARK_RGB_THRESHOLD`, `VECTOR_FILLED_CURVE_ELLIPSE_INSET`, and `VECTOR_FILLED_CURVE_ELLIPSE_POWER` are filled-curve preview tuning for the current fallback shape. These are deliberately labeled as importer guard/tuning values, not serialized `.clip` fields or recovered CSP renderer constants.

Fallback policy diagnostics: `inspect_vector_dynamics.py --summary` now emits `importer_vector_fallback_policy`, populated from the currently loaded `clip_loader.py` constants. `tmp_vector_probe/fallback_policy_detail_v1.json` confirms the summary includes stroke guard limits, fallback sample steps, filled-curve geometry tuning, AA feather tuning, pressure-size/opacity preview constants, and text/balloon fallback insets. This keeps the hardcoded-preview boundary visible in every future corpus summary.

Test_Ballon point-node consumer and PatternStyle-10 interval-dab follow-up:
reverse-side r2 evidence in the adjacent native workspace shows the compact
point records are the real point-node list. `0x142629750` walks `object+0x30`
and calls `0x1422CC1E0` per node; that consumer uses node vtable `+0x70` for
segment length, `+0x68` for distance-to-parameter mapping, and `+0x78` for
position evaluation before calling the writer `+0x20` for each emitted sample.
For flag `0x20` nodes, `+0x78` reaches `0x142623230 -> 0x1431F9420`, which
computes the ordinary quadratic Bezier from current anchor, current tail
control, and next anchor. The importer already had the correct control-point
orientation.

The accepted change is intentionally narrow. In the multi-object balloon native
point-family renderer, only `PatternStyle=10` outlines are now drawn as
closed-path interval dabs with step `record.width * line_style.interval_base`.
For `Test_Ballon` style 4 this is `2.5 * 1.0`, matching the native
segment-sampled outline evidence. `PatternStyle=11` keeps the existing weak
retained-preview outline, single-object balloons/frames keep their fallback,
and ordinary vector strokes are untouched. Verification: `Test_Ballon` full
improves to `premul_mean=1.181103`, `visible=14326`; layer 8 improves to
`0.786705/12602`; layers 9/10 stay `0.325141/1342` and `0.069257/382`;
`Vector_OpacityPressure` and `Vector_OpacityVelocity_50` remain exact, while
`Vector_SizePressure` remains the known `0.024732/153`.

Rejected diagnostics from this pass: fill supersampling worsens layer 8
(`fill_scale=2/4` around `0.826/12818..12839`), direct phase-scrolled resource
alpha worsens badly, and a shallow retained row/segment-quad overlay over the
current path regresses to about `2.13..2.63` premul mean with roughly `26k`
visible pixels. The remaining exactness work is still the real retained
material quad/row pipeline, not more fallback outline tuning.

Vector renderer policy diagnostics: summary output now also includes `importer_vector_renderer_policy`, which separates current pixel-affecting inputs from native-mapped-but-not-rendered fields. Current pixel inputs are limited to layer routing fields, vector object headers, stroke colors/widths/points, compact point `f32+36/+40/+52` in narrow branches, `BrushStyle.AntiAlias`, `SizeEffector=0x31`, `OpacityEffector=0x31`, simple-size branch selection, and a one-point pattern fallback. `FlowBase/FlowEffector`, `Hardness`, texture/material fields, pattern order/reverse state, `StyleFlag` accumulation/retained bits, interval/thickness/rotation, spray/fixed-spray, color mix/watercolor, HSV/subcolor/blur, and compiled resource wrappers remain diagnostic/native-mapped only. Fresh snapshots: `tmp_vector_probe/renderer_policy_detail_v1.json` and `tmp_vector_probe/current_vector_summary_v51.json`.

Per-style renderer status diagnostics: `BrushStyle` and `FillStyle` detail records now include `importer_renderer_status`, and summary output aggregates this as `renderer_status_by_kind`. The status separates `pixel_affecting_inputs`, `preview_only_inputs`, and `diagnostic_only_inputs`, with coverage labels such as `partial_preview`, `native_mapped_not_rendered`, and `context_only`. `tmp_vector_probe/style_renderer_status_detail_v2.json` confirms the isolated pressure-opacity style is `partial_preview`: its `OpacityEffector=0x31` affects pixels, while interval/rotation/styleflag paths remain diagnostic. The refreshed focused corpus snapshot is `tmp_vector_probe/current_vector_summary_v52.json` (`16` clips, `22` vector layers, `19` strokes, `8` object_100 records).

Renderer gap priority diagnostics: summary output now includes `renderer_coverage_counts` and `renderer_gap_inputs_by_kind`, derived from the same per-style renderer status records. A first pass (`tmp_vector_probe/current_vector_summary_v53.json`) showed spray-only fields were noisy when `SprayFlag=0`, so `RotationEffectorInSpray`, `SpraySizeEffector`, and `SprayDensityEffector` are only reported as gaps when spray is enabled. The refreshed `tmp_vector_probe/current_vector_summary_v54.json` keeps the same focused corpus size and shows the most frequent remaining stroke gaps are `RotationEffector`, the texture/water `StyleFlag` family, and interval state; object line gaps are concentrated in pattern/material list rendering, flow, texture/material row paths, retained-state style flags, and thickness.

Renderer gap recommendation layer: `inspect_vector_dynamics.py --summary` now emits `renderer_gap_recommendations`, which groups gap counters into action buckets instead of leaving them as a flat frequency list. `tmp_vector_probe/current_vector_summary_v56.json` classifies the focused corpus as: priority-1 `native_dab_geometry_spacing` (`82` hits: rotation, size velocity, interval, thickness, texture/water style flags), priority-1 `retained_pattern_material_path` (`11` hits: pattern/material list and texture/material row path), priority-2 `coverage_alpha_row_accumulation` (`15` hits: opacity `0x11`, flow, hardness), and priority-3 `fill_composite_context` (`3` hits). This keeps the next workstream honest: analyze PWBrushDraw dab/spacing and retained material paths before turning more parsed fields into importer pixels.

Renderer gap provenance: summary output now records `renderer_gap_inputs_by_clip`, and each `renderer_gap_recommendations` bucket includes `sample_clip_count` plus up to twelve `sample_clips`. The expanded focused sweep `tmp_vector_probe/current_vector_summary_v75.json` covers 18 clips / 27 vector layers and shows `native_dab_geometry_spacing` across 18 clips, retained pattern/material across 8 clips, coverage/alpha row accumulation across 8 clips, and fill composite context across 4 clips. The same sweep confirms the current corpus has no active spray path and no active color-mix/watercolor/blur effectors, so those remain sample requests rather than importer code targets.

Sample request recommendations: `inspect_vector_dynamics.py --summary` now emits `sample_request_recommendations`, a machine-readable layer over the gap provenance. `tmp_vector_probe/current_vector_summary_v76.json` ranks current evidence as priority-1 `native_dab_geometry_spacing`, priority-1 `retained_pattern_material_path`, and priority-2 `coverage_alpha_row_accumulation`; it separately marks `color_water_blur_effectors` and `spray_dab_distribution` as `missing_from_current_corpus`. This means the next practical samples should isolate rotation/interval/thickness/material-list or flow/hardness/opacity before spending time on spray or watercolor/color-mix variants.

Clean baseline-knob vector samples: the new six-file set (`Vector_Baseline`, `Vector_Gap_Fixed_50`, `Vector_Hardness_50`, `Vector_Thickness_50`, `Vector_Angle_45`, `Vector_Opacity_50`) gives the first controlled ordinary 92-byte stroke matrix. `tmp_vector_probe/vector_baseline_knob_summary_v2.json` confirms the intended field mapping: fixed gap changes `BrushStyle.IntervalBase` from `0.1` to `0.5`; hardness changes `Hardness` from `1.0` to `0.5`; thickness changes `ThicknessBase` from `1.0` to `0.5`; angle changes `RotationBase` from `0.0` to `45.0`; fixed ink opacity does not change `BrushStyle.OpacityEffector` and instead writes stroke header `f64+64` / `object_opacity` as `0.5`. The importer now multiplies that header opacity in the ordinary capsule fallback, improving `Vector_Opacity_50` from max `177` / mean `4.626244` to max `133` / mean `4.149894`. The remaining `Opacity_50`, `Thickness_50`, and `Gap_Fixed_50` residuals remain dab/row/geometry work rather than field-location uncertainty.

Clean thickness scalar follow-up: the ordinary no-pattern capsule fallback now consumes `BrushStyle.ThicknessBase` only when it is below `1.0` and there is no pattern or texture path. This is deliberately narrow, because native thickness eventually belongs to stretched/rotated dab geometry. On the clean sample, `Vector_Thickness_50` improves from mean `4.768413` / visible `2350` to mean `0.594538` / visible `725`; baseline, gap, hardness, angle, opacity, pressure-size, pressure-opacity, AA, texture/no-texture, `Test_Vector`, and mixed `test_Filters_Vector_Text` guards stayed stable. `tmp_vector_probe/vector_baseline_knob_metrics_v3.json` captures the six-file post-change metrics, and `inspect_vector_dynamics.py` now reports `ThicknessBase < 1` as a conditional pixel-affecting preview input instead of lumping every thickness field into diagnostics.

Fixed-opacity stroke accumulation follow-up: the clean `Vector_Opacity_50` sample exposed that reading header `object_opacity` was not enough. The feathered capsule writer used normal alpha-over for each short subsegment, so a 50% opaque stroke repeatedly accumulated back toward full opacity along the centerline. The fallback now switches low-opacity stroke segments to max/capped alpha writes, matching the intended opacity cap rather than self-building. `Vector_Opacity_50` improves from the post-header result mean `4.149894` / visible `3667` / max `133` to mean `0.272156` / visible `786` / max `89`; the other five clean knob samples and the pressure-size, pressure-opacity, AA, texture/no-texture, `Test_Vector`, and mixed filter/vector guards remain unchanged. `tmp_vector_probe/vector_baseline_knob_metrics_v4.json` captures the updated six-sample metrics.

Clean hardness scalar follow-up: the earlier broad hardness retune was rejected when only `Hardness=0.95` existed, but the new isolated `Vector_Hardness_50` sample gives a safer boundary. The importer now applies a narrow no-pattern/no-texture capsule softness preview when `BrushStyle.Hardness < 1`, using the native-derived `threshold = hardness * 1.3 - 0.3` as the softness source and only widening feather / slightly reducing radius. `Vector_Hardness_50` improves from mean `0.787563` / visible `1557` / max `153` to mean `0.408562` / visible `1474` / max `95`; baseline, gap, thickness, angle, opacity, pressure-size, pressure-opacity, AA, texture/no-texture, `Test_Vector`, and mixed filter/vector guards stay stable. `tmp_vector_probe/vector_baseline_knob_metrics_v5.json` captures the post-hardness six-sample metrics, and `tmp_vector_probe/current_vector_summary_v79.json` exposes the new hardness preview constants in importer policy diagnostics. This is still a capsule-preview bridge, not exact native radial profile sampling.

Clean fixed-gap follow-up: the controlled gap sample confirms `Gap_Fixed_50` maps to `BrushStyle.IntervalBase=0.5`, but the simplified capsule fallback cannot use native interval feedback directly because CSP spacing drives dab submission rather than a continuous centerline. A narrow no-pattern/no-texture bridge now treats `IntervalBase > 0.1` with off/default interval effector as a capped radius softening, limited to an 8% radius reduction at the clean sample's `0.5` value. `Vector_Gap_Fixed_50` improves from mean `1.196100` / visible `1035` / max `177` to mean `0.672113` / visible `960` / max `170`; baseline, hardness, thickness, angle, opacity, pressure-size, pressure-opacity, AA, texture/no-texture, `Test_Vector`, and mixed filter/vector guards stay stable. `tmp_vector_probe/vector_baseline_knob_metrics_v6.json` and focused `tmp_vector_probe/current_vector_summary_v81.json` capture the post-gap metrics and renderer-policy update. This remains a radius preview compensation until the native spacing/dab loop is modeled.

Clean auto-gap mode follow-up: the non-fixed gap UI modes do not expose a numeric gap field and serialize as `AutoIntervalType=1/2/3`, `IntervalBase=1.0`, and disabled `IntervalEffector`. New samples map Wide/Normal/Narraw to auto types `1/2/3` respectively. A fixed auto-interval radius/feather rule improved these samples but regressed `Vector_NoTexture` because that older black no-AA stroke also uses `AutoIntervalType=2`; the kept rule is therefore scaled by the active `AntiAlias` ordinal. At AA 0 it is a no-op, at AA 1 it applies half of the preview compensation, and at AA 2+ it reaches `radius*0.96` / `feather*2.0`. Metrics improve `Vector_Gap_Wide` mean `0.549044->0.368287`, `Vector_Gap_Normal` `0.355013->0.192938`, and `Vector_Gap_Narraw` `0.300606->0.241287`, while `Vector_NoTexture` remains `0.235198`, `Test_Vector` stays `1.043658`, and AA=1 guard samples do not regress. This is still a preview compensation, not the native auto-interval feedback loop.

Clean thickness+rotation follow-up: the added `Vector_Angle_90` and `Vector_Thickness_50_Angle_90` samples separate rotation alone from stretched brush-tip geometry. `RotationBase=90` with `ThicknessBase=1` stays unchanged at mean `0.489806`, confirming that rotation alone should not affect the capsule preview. With `ThicknessBase=0.5`, the previous uniform radius shrink rendered the stroke with the wrong thick axis (`Vector_Thickness_50_Angle_90` mean `3.628388` / visible `1966`). The importer now reads `BrushStyle.RotationBase` and, only for ordinary no-pattern/no-texture strokes where `ThicknessBase<1` and the rotation is not horizontal, draws a rotated ellipse footprint; the 90-degree sample improves to mean `1.243513` / visible `1020`, while the original horizontal `Vector_Thickness_50` remains at mean `0.594538` / visible `725`. The remaining error is mostly the continuous capsule approximation versus CSP's per-dab row writer.

Clean thickness+45-degree follow-up: `Vector_Thickness_50_Angle_45` validates the same rotated-ellipse rule at an intermediate angle. Current metrics are mean `0.986938` / visible `942`; a runtime sweep without code changes shows the previous uniform-radius shrink would be mean `2.041506` / visible `1272`, and reversing or offsetting the rotation direction worsens to mean `1.256394` / visible `1062`. Keep the current `RotationBase` sign/orientation for the preview bridge.

Clean flow scalar follow-up: the new `Vector_Flow_50` sample confirms `Brush tip > Brush density=50` serializes as `BrushStyle.FlowBase=0.5` with default `FlowEffector`, while stroke opacity and the other clean knob fields stay at baseline. Treating Flow as a direct alpha cap is wrong for the current capsule preview (`Vector_Flow_50` regressed to mean `3.483413`), because native Flow participates in per-dab row coverage and overlapping samples. A narrow no-pattern/no-texture radius preview is the best tested bridge: `radius *= 1 - (1 - FlowBase) * 0.12` improves `Vector_Flow_50` from mean `0.975675` / visible `1744` / max `171` to mean `0.561444` / visible `1729` / max `150`, while baseline, `test_Filters_Vector_Text`, `Test_Vector`, and `Vector_NoTexture` stay unchanged. Exact flow still belongs to the native dab/row accumulation model.

Clean flow dynamic follow-up: `Vector_FlowPressure_50` and `Vector_FlowVelocity_50` identify the Flow dynamic storage without promoting it to pixels. Pressure writes `FlowEffector=0x11` with floor/scalar `0.5` and graph `4`, using the primary compact channel (`+36` / sample context `+0x10`). The velocity UI sample writes `FlowEffector=0x41` with the same floor/scalar and graph, using the auxiliary compact channel (`+44` / sample context `+0x20`), not the native velocity bit `0x100` / compact `+52` path. Current capsule metrics are already moderate (`FlowPressure_50` mean `0.538100`, `FlowVelocity_50` mean `0.846538`), but exact dynamic flow should wait for the native row/dab model; `inspect_vector_dynamics.py` now names `0x41` as `aux_graph_0x41` instead of an unknown form.

Clean flow dynamic preview follow-up: the importer now consumes isolated ordinary no-pattern `FlowEffector` forms `0x11` and `0x41` as a narrow radius preview, not a direct alpha cap. `0x11` evaluates graph `4` from compact point `+36`; `0x41` evaluates graph `4` from compact point `+44`; both use the serialized floor/scalar `0.5` and apply the same capped preview slope as scalar flow (`VECTOR_FLOW_DYNAMIC_RADIUS_SOFTEN_MAX=0.12`). This improves `Vector_FlowPressure_50` mean `0.538100->0.484088` / max `177->158` and `Vector_FlowVelocity_50` mean `0.846538->0.735319` while leaving `Vector_Flow_50` at `0.561444`, `Vector_Baseline` at `0.509006`, and the older vector guards stable. A sweep showed larger slopes overfit pressure and quickly regress the aux/velocity sample, so exact flow remains native dab/row work.

Taper / starting-ending sample follow-up: the re-exported `Vector_StartEnd_20` is now a valid ordinary vector stroke, and it confirms the same boundary as `Vector_Taper_4` and `Vector_StartEndSpeed_20`. All three serialize with BrushStyle rows that are identical to the clean baseline except for `MainId`; no `BrushStyle` column captures the UI Taper or starting/ending values. Their differences are in the sampled vector point stream: `Vector_StartEnd_20` has compact point `f32+36 = 0.091930..0.508180`, `Vector_Taper_4` has `0.0..0.341717`, and `Vector_StartEndSpeed_20` has `0.0..0.462154`, while baseline stays `0.412282..0.635270`. Forcing `+36` into the existing simple-size taper branch regresses badly (`StartEnd_20` mean `0.493150 -> 5.524913`, `Taper_4` `0.418131 -> 5.964038`, `StartEndSpeed_20` `0.445463 -> 6.352650`, baseline `0.509006 -> 4.509000`). The current errors are mostly outline/hand-drawn geometry mismatch in the capsule fallback, not a missing stored brush-style renderer parameter, so no importer rule was added.

Clean texture preview follow-up: the new `Vector_Texture_50` sample maps `Texture` to `BrushStyle.TexturePattern=2` and `TextureDensityBase=0.5` with `TextureComposite=0`, scale `1.0`, rotation/offset/brightness/contrast all zero, and no `PatternStyle`. Its Granite mipmap decodes as a single-channel tile blob (`200x200` offscreen, one `256x256` tile), so the importer now has a narrow texture-only vector preview: draw the ordinary capsule into a temporary stroke buffer, multiply its alpha by `1 - density*0.70 + density*0.70*sqrt(textureByte/255)`, then composite normally over paper. This improves `Vector_Texture_50` mean `1.547956 -> 0.797644` and the older `Vector_Texture` mean `0.900940 -> 0.833431`, while `Vector_Baseline`, `Vector_NoTexture`, `Test_Vector`, and full `test_Filters_Vector_Text` remain stable. `PatternStyle` material brush tips are deliberately untouched: `Vector_BrushTip_Material` remains `5.286094` and `Vector_BrushTip_Material_Gap` remains `7.915981`, because those require stamp/material spacing rather than a post-capsule texture mask.

Clean material brush-tip preview follow-up: `Vector_BrushTip_Material` and `Vector_BrushTip_Material_Gap` isolate `BrushStyle.PatternStyle=2` with one `BrushPatternImage`. The material mipmap external block decodes to four single-channel tiles (`512x512`), with the left lane/crop containing the visible high, tapered stamp. The importer now has a narrow ordinary-stroke material preview: resolve the first `ImageIndex`, crop the left single-channel lane to its nonzero bbox, scale it from stroke width, and alpha-over stamps along the stroke. Continuous material tips resample by the native-style `2 * width * IntervalBase` spacing (with a 1px floor); the fixed-gap branch keeps the shorter/lighter point-sampled preview keyed by `IntervalBase > 0.1`. Metrics improve `Vector_BrushTip_Material` mean `5.286094 -> 0.842944` and `Vector_BrushTip_Material_Gap` `7.915981 -> 1.106737`, while `Vector_Texture_50`, old `Vector_Texture`, baseline, no-texture, pressure-opacity, `Test_Vector`, full `test_Filters_Vector_Text`, and the adaptive-spacing gate stay stable. This is still a preview bridge: exact CSP material rendering needs rotation, OrderType/Reverse2/list handling, row accumulation, and the full native retained/material path.

Compact 120-byte curve-tail rendering: the AA samples show why `92/76/120/88`, `flags=0x41` dark objects still had residuals. The right-hand curve record has two point records whose normal `x/y` endpoints are only a diagonal chord, while each 120-byte point also carries tail doubles at `+88/+96` and `+104/+112`. For `Vector_AA_None`, the first point tail is `(266.666668,399.333335)` and `(586.66667,300.000001)`, and the second point tail is `(350.666668,988.000005)` and `(468.666669,644.000003)`. The wrong cubic interpretation was current `+104/+112` plus next `+88/+96`; the kept preview uses current `+104/+112` as the second cubic control and fixes the first control at the current point. This improves AA means: None `0.864298 -> 0.410123`, Weak `0.731469 -> 0.287546`, Medium `0.663973 -> 0.238421`, Strong `0.621886 -> 0.200072`, while `Test_Vector` and full `test_Filters_Vector_Text` stay unchanged. `inspect_vector_dynamics.py` now marks `compact120_curve_tail_candidate` as pixel-affecting for this narrow dark-curve preview.

Global pressure curve probe: `Vector_GlobalPressure_Default` and `Vector_GlobalPressure_StrongCurve` isolate CSP's global pen-pressure graph with the same document BrushStyle row. Both files keep `BrushStyle` MainId `2` at `SizeEffector=OpacityEffector=FlowEffector=ThicknessEffector=IntervalEffector=00000001`, `AntiAlias=2`, `IntervalBase=0.1`, and no texture/pattern. The point stream changes anyway: default has compact `f32+36` range `0.0..0.579905` / avg `0.393931`, while the strong global curve has `0.0..0.320589` / avg `0.235817`; `+44` also shifts from avg `0.352894` to `0.539136`. This supports the current importer policy that global device pressure adjustment is baked into serialized vector point scalars rather than needing the importer to read the user's machine-wide CSP setting. Caveat: these samples do not enable brush-local size/opacity pressure effectors, so they prove point-channel baking but not the full `global curve -> brush effector graph -> dab size/opacity` stack.

Tilt/velocity size-opacity dynamics follow-up: the four 2026-05-19 samples map the UI dynamic inputs to compact effector forms cleanly. `Vector_SizeTilt_50` writes `SizeEffector=0x21` with low/high `0.5..1.0`, graph `2`, and compact point `f32+40` as the input; `Vector_OpacityTilt_50` uses the same `0x21/+40` form on `OpacityEffector`. `Vector_SizeVelocity_50` writes `SizeEffector=0x41` with floor `0.0`, graph `2`, and compact point `f32+44` as the input; `Vector_OpacityVelocity_50` uses the same `0x41/+44` form on `OpacityEffector`. These velocity UI samples do not set the native `0x100/+52` branch. The importer now evaluates only these observed no-pattern forms for size and opacity preview: `0x21` applies `low + (high-low) * graph(+40)`, and `0x41` applies `floor + (1-floor) * graph(+44)`. Metrics improved for the strongest geometry cases: `SizeTilt_50` mean `4.800775 -> 0.558094`, `OpacityTilt_50` `2.288887 -> 0.432519`, and `SizeVelocity_50` `10.033888 -> 1.075069`; `OpacityVelocity_50` remains `1.394175`, so exact opacity accumulation still belongs to the native dab/row path.

Random size-opacity dynamics follow-up: `Vector_SizeRandom_50` and `Vector_OpacityRandom_50` confirm the random-only UI path serializes as an 8-byte compact effector `0x81` with one scalar payload. The payload is `0.5`, matching the UI minimum/floor, and the native branch decoder marks only `random_0x80` active with no graph refs or point-channel input. `inspect_vector_dynamics.py` now names this form `random_floor_0x81`. The importer adds a narrow no-pattern preview using a deterministic pseudo-random value per vector point, so renders stay stable across runs while approximating CSP's random dab variation. Metrics improve `Vector_SizeRandom_50` mean `2.434294 -> 1.301906` and `Vector_OpacityRandom_50` `1.916644 -> 1.498369`; the residual is still expected because CSP's exact RNG sequence, dab emission grain, and row accumulation are not reproduced. A temporary experiment using compact point `f32+44` as the random source improved `SizeRandom_50` a little further (`1.301906 -> 1.207088`) but regressed `OpacityRandom_50` (`1.498369 -> 1.757944`), so it stays rejected as a general rule.

Random size seed follow-up: the random-only samples expose the file-backed seed for size random. `Vector_Baseline` keeps compact point trailing `u32+80` constant across its 30 points, while `Vector_SizeRandom_50` and `Vector_OpacityRandom_50` store high-entropy per-point `u32+80` values. IDA confirms why: compact point writer `0x1422CD270` serializes internal point `+112/+116` as trailing `u32+80/+84`, sampler `0x1422CCA10` passes internal `+112` as the first dword in the random-state pair to sink vtable `+0x20`, and `PWBrushParameterEffector` evaluator `0x142568040` advances that state with `state = 1103515245 * state + 1234567`, then uses `((state >> 16) & 0x7fff) / 32768.0`. Using this native LCG value only for `SizeEffector=0x81` improves `Vector_SizeRandom_50` again (`mean 1.301906 -> 0.914006`, visible `851 -> 804`) while leaving `Vector_OpacityRandom_50`, `Vector_Baseline`, `Test_Vector`, `Vector_SizePressure`, `Vector_OpacityPressure`, and `Vector_NoTexture` unchanged. Applying native LCG directly to `OpacityEffector=0x81` regresses opacity random (`mean 1.498369 -> 2.183269`), so opacity random stays on the previous stable preview hash until the native opacity cap / flow / row accumulation path is modeled.

Opacity-random rejection pass: `tmp_vector_probe/opacity_random_rejection_probe_resume.json` records an in-memory sweep over opacity random source and curve variants. The current stable pseudo-random source remains the best source candidate (`mean 1.498369`, visible `2850`); direct `u32+80`, inverse `u32+80`, xorshift, byte lanes, and seed-hash variants all regress. Curve compensation can reduce average error slightly (`floor + amp * r` with `0.6 + 0.35*r` gives `mean 1.451175`) but increases visible pixels to `3653` and flips many residuals from too-dark to too-light. Signed diffs show the current preview is mostly too dark on visible residuals (`2139` too-dark vs `711` too-light pixels), while floor-only is strongly too light. No importer rule was kept; the next opacity-random improvement should target native plot `+8` opacity cap, plot `+16` flow, and row accumulation rather than another point-seed guess.

Native no-pattern dab pipeline checkpoint: IDA now has comments on the ordinary queue consumer and row writers. `0x14260F550` copies plot record fields to the queue (`+0` size, `+8` opacity cap, `+16` flow, `+24` stretch/thickness, `+32` rotation, `+44` AA, `+72` texture/color aux). `0x14263F410` consumes no-pattern queue items, sets the dab context, then dispatches to hard circular `0x142640150`, circular AA `0x14263FC50`, stretched/rotated AA `0x142640420`, stretched/rotated hard `0x142640C90`, or narrow fallback `0x1426427D0`. All of those feed `0x14263AC30`, where row coverage is `flowCoverage=(flow*geometricCoverage)>>15`; default accumulation is `old + flowCoverage*(opacityCap-old)>>15`, while `StyleFlag 0x1000` switches to a direct/max candidate `(opacityCap*flowCoverage)>>15`. `tmp_vector_probe/current_vector_summary_v57.json` now emits this as `native_no_pattern_dab_pipeline`, reinforcing that rotation/thickness/flow/hardness belong to a dab/row model, not direct line-capsule alpha tuning.

No-pattern dab migration scope: `inspect_vector_dynamics.py --summary` now emits `no_pattern_dab_candidate_counts` and `no_pattern_dab_candidate_category_counts`. The refreshed `tmp_vector_probe/current_vector_summary_v60.json` shows `stroke_92` has `8` ordinary no-pattern dynamic dab candidates, `5` texture/material-path strokes, and `6` non-ordinary compact strokes (mostly filled-curve shape `0x41`), while `object_100_line` has `3` ordinary no-pattern dynamic candidates, `3` retained-state paths, and `2` pattern-material paths. This narrows the first safe implementation target to ordinary no-pattern dynamic stroke dabs; texture/material, retained-state, and filled-curve paths should stay on their current fallback or diagnostics until their native paths are modeled.

Experimental no-pattern dab renderer rejection: the importer now has a default-off diagnostic branch guarded by `RIZUM_CLIP_EXPERIMENTAL_VECTOR_DAB=1`. It reads `BrushStyle` routing fields plus compact point `+56` size and `+60` flow factors, then uses a minimal circular dab writer with native-style build-up/direct-max accumulation. This confirms the row formula path is executable, but the first metrics are worse and should not replace the capsule fallback: `Vector_OpacityPressure` regresses from `0.122786` to `0.170274`, and `Test_Vector` regresses from `1.043658` to `1.310538`; the mixed `test_Filters_Vector_Text` remains `0.569085` because its active vector gaps are mostly texture/material/object-line paths. Keep this branch as an experiment only until native spacing/curve sampling and exact dab geometry are recovered.

Opacity-random native-dab resume: reconnecting to the IDA MCP on 2026-05-26 confirmed `CLIPStudioPaint.exe.i64` at `127.0.0.1:13337` and re-saved the no-pattern dab evidence under `tmp_ida_random_opacity/`. The recovered row formula still stands: `0x14263F410` stores plot `+8/+16` as 32768-scaled `ctx+0x1e0/+0x1e4`, and `0x14263AC30` computes `flowCoverage=(flow*geometricCoverage)>>15`, then either builds up toward the opacity cap or uses `StyleFlag&0x1000` direct/max. New runtime sweeps show the default-off native dab writer is directionally useful for `Vector_OpacityRandom_50`: with current 5px curve sampling it improves mean `1.498369 -> 1.372256`, and with `VECTOR_FALLBACK_SAMPLE_STEP=1.0` it improves further to mean `0.915225` / visible `2916` / max `82`. This also improves `Vector_Baseline` (`0.509006 -> 0.243400`) and `Vector_SizePressure` (`0.115578 -> 0.087128`), but regresses `Test_Vector` (`1.043658 -> 1.106109`), `Vector_OpacityPressure` (`0.122786 -> 0.170274`), and `Vector_NoTexture` (`0.235198 -> 0.253464`), so the branch remains experimental rather than enabled.

Opacity-random state-grain follow-up: direct native LCG remains wrong for opacity when applied at the current importer grain. In-memory tests recorded in `tmp_vector_probe/opacity_random_lcg_dab_variant_resume.json`, `opacity_random_native_dab_step_sweep_resume.json`, and `opacity_random_seq_original_point_experiment_resume.json` show per-point `u32+80` LCG regresses both capsule and native-dab previews (`2.183269` capsule, `2.804550` native-dab at 5px, `2.075325` native-dab at 1px), and a simple sequential original-point LCG is still worse (`1.545656` at 1px native-dab). IDA explains the mismatch: `0x1422CCA10` initializes a random-state pair from internal point `+0x70`, passes that same state pointer to each sink call, and the evaluator advances it per emitted dab while the sampler uses `state+8` interval feedback. The file-backed seed is therefore stable and real, but opacity random needs native dab emission spacing/state advancement before the importer can replace the stable opacity hash with LCG.

Per-dab opacity-random experiment: the default-off native dab branch now has `RIZUM_CLIP_EXPERIMENTAL_VECTOR_NATIVE_RANDOM_OPACITY=1`, which applies the `0x142568040` advance-before-use LCG to each emitted experimental dab instead of interpolating the stable per-point opacity preview. This is gated behind `RIZUM_CLIP_EXPERIMENTAL_VECTOR_DAB=1`, and normal rendering is unchanged. The first unadvanced-start variant was rejected (`Vector_OpacityRandom_50` mean `2.016375` at the 5px fallback step). After matching native advance-before-use semantics, a step sweep recorded in `tmp_vector_probe/opacity_random_native_rng_segment_sweep_v2.json` improves the sample: best mean is `0.715912` at `1.5px`, while the native interval estimate path (`RIZUM_CLIP_EXPERIMENTAL_VECTOR_ADAPTIVE_SPACING=1`) gives mean `0.796444` / visible `1949` / max `123`. The same adaptive path without native random is weaker (`1.002581`), so the per-dab LCG grain is now supported by pixels. It is still default-off because exact interval, duplicate segment-start emission, and mixed guard behavior are not fully proven.

Native opacity-random guard matrix: `tmp_vector_probe/native_rng_adaptive_verify_matrix_v1.json`, `tmp_vector_probe/native_rng_no_adaptive_verify_matrix_v2.json`, and `tmp_vector_probe/dab_only_no_adaptive_verify_matrix_v1.json` separate the default-off knobs. Native opacity LCG without native-like spacing is not valid yet: with `RIZUM_CLIP_EXPERIMENTAL_VECTOR_DAB=1`, `RIZUM_CLIP_EXPERIMENTAL_VECTOR_NATIVE_RANDOM_OPACITY=1`, and adaptive spacing explicitly disabled, `Vector_OpacityRandom_50` regresses by `mean_delta=+0.517312` / `visible_px_delta=+382`. With all three knobs enabled, `Vector_OpacityRandom_50` improves strongly (`mean_delta=-0.701925`, `visible_px_delta=-901`), but the broader experimental dab/spacing branch still regresses non-target guards (`Vector_OpacityPressure`, `Vector_NoTexture`, and `Test_Vector`). This locks the current conclusion: the file-backed random seed and per-dab LCG grain are recovered, but the native row/sampler path is not yet safe to enable broadly.

Native interval source follow-up: the reverse project confirms `0x14255DFE0` supplies `0x1422D8550` with base size from draw-state `+0x68` in ordinary mode, or `+0x70` in the secondary-resource branch. The evaluator then multiplies that by Size/SpraySize effector output and sample-context `+0x38` before writing `state+8`. The importer should not treat serialized stroke width alone as the recovered native interval size; the remaining mismatch between the `1.5px` sweep best and the `2px` estimate belongs to draw-state setup and dab geometry.

Draw-state width source follow-up: the reverse project now maps ordinary `PWBrushDraw+0x68` back to the compact stroke width. `0x14255D810` takes it from `PWVectorStroke` vtable `+0x80` (`0x14243E100`), which returns native `stroke+0x98`; compact stroke tail reader `0x1425A4760`, called from `0x1422CF5F0` virtual `+0x100`, reads ordinary stroke tail `u32+76` as the line/list id, compact `f64+80` directly into `stroke+0x98`, and compact `u32+88` into `stroke+0xa0`. Secondary `draw+0x70` remains a separate compiled line/list resource path (`sub_1422DC6F0 -> sub_1422DC990`). Importer implication: the experimental native interval estimate's base width is file-backed and stable; opacity-random still depends on exact sampler emission, resource branch selection, and dab/row geometry rather than any missing random/width persistence.

Stroke-seeded opacity-random experiment correction: the default-off native opacity-random branch now advances one continuous state initialized from the ordinary stroke tail `u32+88` instead of resetting from each point's compact `u32+80`. Reverse evidence shows `0x1425A3D60` seeds sampler state from `PWVectorStroke+0xa0`, and normal `0x1422CC1E0` carries that state plus interval feedback at `state+8`; the point-local `0x1422CCA10` path is gated by internal point/list flag `bit 2` and is not the ordinary random sample's main traversal. New verification files are `tmp_vector_probe/native_rng_continuous_stroke_seed_opacity_only_v1.json`, `native_rng_continuous_stroke_seed_no_adaptive_opacity_only_v1.json`, `native_rng_continuous_stroke_seed_random_trio_v1.json`, and `native_rng_continuous_stroke_seed_guard_matrix_v1.json`. With native-dab + adaptive spacing, `Vector_OpacityRandom_50` improves from mean `1.498369` to `1.053937` / visible `2902`; without adaptive spacing it regresses to mean `1.794600`. This is more native-faithful but less numerically flattering than the earlier per-segment point-seed fit, so the branch remains experimental.

Sampler-feedback opacity-random experiment: the default-off native dab branch now uses interval feedback when adaptive spacing is enabled, instead of generating a fixed subdivision first. It carries a continuous stroke-seeded RNG plus residual distance, emits a dab at the current distance, computes the next interval with the native `0x1422D8550` normal-spacing formula, and carries `state+8` residual across segments. New verification files are `tmp_vector_probe/native_rng_feedback_sampler_opacity_only_v1.json`, `native_rng_feedback_sampler_no_adaptive_opacity_only_v1.json`, `native_rng_feedback_sampler_random_trio_v1.json`, and `native_rng_feedback_sampler_guard_matrix_v1.json`. `Vector_OpacityRandom_50` improves to mean `0.724387` / visible `1708` (`mean_delta=-0.773982`, `visible_delta=-1142`), while no-adaptive still regresses to `1.794600`. The random trio also improves mean for baseline and size-random, but guard clips still regress in the broader native-dab branch, so this remains proof of direction rather than default renderer code.

Native circular-AA center rejection: IDA decompilation of `0x14263FC50` shows the native circular AA dab measures distance from `ctx+0x1b0/0x1b8 - 0.5` and uses `0x1422DBF30` for the AA width cap (`AA1/2/3 -> min(radius, 1.5/2.5/3.5)`). A default-off importer probe that applied only this half-pixel center shift to the simplified experimental dab made `Vector_OpacityRandom_50` worse: all-on feedback/native-random/adaptive spacing changed from the previous `mean_delta=-0.773982` to `mean_delta=-0.414169` (`tmp_vector_probe/native_rng_feedback_sampler_aa_center_opacity_only_v1.json`). Keep the half-pixel shift as evidence for a future exact span rasterizer, but do not graft it alone onto the current simplified dab model.

Native plot-size radius correction: the default-off native dab experiment now treats the compact stroke width as the plot/dab radius base inside that branch, instead of reusing the fallback capsule compensation `VECTOR_STROKE_RADIUS_SCALE=0.95`. This follows the native chain `0x1422D8550 plot+0 -> 0x14260F550 queue+0x128 -> 0x14263F410 ctx+0x1c0`, where plot `+0` is passed as the no-pattern dab size/radius. `tmp_vector_probe/native_rng_feedback_plot_radius_opacity_only_v1.json` improves `Vector_OpacityRandom_50` from mean `1.498369` to `0.320775` (`mean_delta=-1.177594`, `visible_delta=-1514`), better than the previous feedback result `0.724387`. `native_rng_feedback_plot_radius_random_trio_v1.json` improves baseline, size-random, and opacity-random means. The guard matrix `native_rng_feedback_plot_radius_guard_matrix_v1.json` still blocks default enablement: `Vector_OpacityPressure` regresses by `+0.041060` mean and `Test_Vector` by `+0.126822`, while `Vector_SizePressure`, AA none/medium, and `Vector_NoTexture` improve and texture remains unchanged. Conclusion: plot-size radius is native-aligned and kept in the default-off experiment, but the full native row/span/branch model is still incomplete.

Native dab flow-effector correction: the default-off native dab experiment now multiplies plot flow by the evaluated `FlowEffector` factor before compact sample `f32+60`, matching the native `0x1422D8550` flow chain more closely. The useful diagnostic was `Test_Vector` style 4, which uses `FlowEffector=0x11`, graph `4`, floor `0.0`; the importer previously only considered the older floor `0.5` preview form. Keeping default preview gating unchanged but applying the full graph/floor value inside the experimental dab path moves `Test_Vector` from a regression to an improvement under the broad all-on matrix (`mean_delta=+0.126822` before, `-0.055819` after). This leaves `Vector_OpacityRandom_50` unchanged at the improved `mean_delta=-1.177594`.

Native opacity-effector rejection and random-specific scoping: applying full native `OpacityEffector=0x31` output directly to the simplified dab experiment badly regresses `Vector_OpacityPressure` (`mean_delta=+1.121983`, `visible_delta=+10342`), so the current capsule-tuned pressure-opacity preview remains in place until the exact row/span profile is recovered. Instead, when `RIZUM_CLIP_EXPERIMENTAL_VECTOR_NATIVE_RANDOM_OPACITY=1` is set, the importer now scopes the native dab/adaptive-spacing experiment to true `OpacityEffector=0x81` strokes. `tmp_vector_probe/native_rng_random_scoped_gate_v1.json` passes with `Vector_OpacityRandom_50` improved (`mean_delta=-1.177594`, `visible_delta=-1514`) while `Vector_OpacityPressure`, `Test_Vector`, AA none/medium, texture, and no-texture guards are all unchanged. This makes the random-opacity native-state path testable without pretending the broader dab/row renderer is complete.

Native row fixed-point quantization follow-up: `0x14263F410` stores opacity cap and flow as `int(value * 32768.0 + 0.5000000100000001)` at ctx `+0x1e0/+0x1e4`. `0x14263AC30` then computes `flowCoverage=(flow_i * geometricCoverage_i) >> 15`; the normal branch writes `old + ((flowCoverage * (opacityCap - old)) >> 15)`, while `StyleFlag&0x1000` writes `max(old, (opacityCap * flowCoverage) >> 15)`. The default-off importer dab experiment now mirrors that fixed-point row math instead of doing the same formula in floats. The random-specific gate `tmp_vector_probe/native_rng_fixed_row_gate_v1.json` still passes: `Vector_OpacityRandom_50` improves by `mean_delta=-1.177613` / `visible_delta=-1513`, and the pressure/Test/AA/texture/no-texture guards remain unchanged. Retesting the native `center-0.5` circular-AA distance after this row quantization still regresses the random target (`tmp_vector_probe/native_rng_fixed_row_center_shift_opacity_only_v1.json`, `mean_delta=-0.407813`), so the center shift remains deferred to an exact span rasterizer rather than the simplified dab preview.

Broad dab status after row quantization: with only `RIZUM_CLIP_EXPERIMENTAL_VECTOR_DAB=1` and adaptive spacing enabled, `tmp_vector_probe/native_dab_adaptive_fixed_row_broad_v1.json` shows `Test_Vector` now improves (`mean_delta=-0.055836`) thanks to native flow-effector multiplication, and `Vector_SizeRandom_50` / `Vector_OpacityRandom_50` also improve. `Vector_OpacityPressure` still regresses by `+0.041060` mean while losing visible pixels, so the remaining broad-native blocker is not fixed-point quantization; it is the exact opacity-pressure cap/profile/span interaction.

Native hard-span and 16-bit accumulation follow-up: `0x142640150` uses a hard circular row span, not a pixel-centre mask: each row computes `sqrt(radius^2 - (y - (cy - 0.5))^2) - 0.4`, truncates the left/right span to ints, and sends the whole span to `0x14263AC30` with `0x8000` coverage. The default-off importer dab experiment now mirrors this for hard/no-AA dabs and also accumulates each experimental stroke in a 32768-scale alpha buffer before converting back to 8-bit RGBA. Broad dab verification `tmp_vector_probe/native_dab_16bit_alpha_broad_v1.json` improves the remaining pressure blocker from `mean_delta=+0.041060` to `+0.011595`; `Test_Vector`, AA none/medium, and no-texture also improve. The random-specific gate `tmp_vector_probe/native_rng_hard_span_16bit_scoped_gate_v2.json` passes with `Vector_OpacityRandom_50` improved by `mean_delta=-1.177781` / `visible_delta=-1521` and all scoped non-target guards unchanged.

Pressure-opacity cap rejection after hard-span fix: after hard-span geometry and 16-bit row accumulation were aligned, the full native `OpacityEffector=0x31` cap was retested in the simplified dab experiment. It still badly regresses `Vector_OpacityPressure` (`tmp_vector_probe/native_dab_full_opacity_after_span_16bit_v1.json`, `mean_delta=+1.098286`, `visible_delta=+10356`). This proves the earlier rejection was not just a stale geometry artefact. Keep the sample-tuned pressure-opacity preview for now; the missing broad-native piece is still a deeper pressure opacity/profile/coverage interaction, not the random seed, row fixed-point formula, hard span geometry, or 16-bit alpha work buffer.

Native size multiplier tightening: the default-off native dab experiment now only multiplies dab radius/interval by the `taper` channel when a real SizeEffector path is active. This follows `0x1422D8550`, where plot `+0` is `baseSize * evaluated(SizeEffector/SpraySizeEffector) * sample_context+0x38`; pure opacity-pressure or opacity-random strokes do not use endpoint taper as dab size. `tmp_vector_probe/native_dab_size_only_radius_broad_v1.json` shows this is behaviorally neutral for the current pressure/random samples, but it removes a non-native coupling from the experiment.

Random-opacity residual probes: the remaining `Vector_OpacityRandom_50` error is not explained by random phase or interval scale. In-memory sweeps show interval scale `1.0` is the best tested value (`tmp` probe: mean `0.320588`; nearby `0.975 -> 0.489769`, `1.025 -> 0.515906`), and LCG phase `0` is best (`extra_lcg_steps=0 -> 0.320588`; `-1 -> 0.552488`, `+1 -> 0.482681`). Scaling opacity/flow/radius also worsens; only a small AA-width increase around `1.025..1.05` gives a tiny improvement (`0.315900..0.314738`), which points to residual AA geometry/coordinate iteration rather than opacity state.

AA row-span rejection after hard-span fix: a direct implementation of the circular-AA row-span structure from `0x14263FC50` was retested after hard-span and 16-bit fixes. It still regresses the random target in the scoped path (`tmp_vector_probe/native_rng_aa_span_scoped_probe_v1.json`, `mean_delta=-0.408094` versus `-1.177781` for the kept simplified AA path) and makes broad random/size-random worse (`tmp_vector_probe/native_dab_aa_span_broad_v1.json`). Keep the native AA span decompilation as evidence for a future exact block/bbox/sampler implementation, but do not half-graft it into the current simplified experiment.

AA span boundary and point-quantization probes: correcting the AA row-span probe to use `0x14263FC50`'s half-open right boundary did not change the rejection (`tmp_vector_probe/native_rng_aa_span_halfopen_scoped_probe_v1.json` remains `mean_delta=-0.408094`, and `native_dab_aa_span_halfopen_broad_v1.json` still regresses broad random/size-random). A separate point-centre sweep confirms the current `int(point)` sampler is also the best tested centre for `Vector_OpacityRandom_50`: `int -> mean 0.320588`, raw double `1.017281`, round/floor-half `1.134975`, `int+0.5 -> 0.964575`, `int-0.5 -> 1.090275`. The clean broad probe `tmp_vector_probe/native_dab_clean_broad_v1.json` keeps improving random/size-random/AA/Test, while the scoped gate `tmp_vector_probe/native_rng_hard_span_16bit_scoped_gate_v5.json` still passes. These probes leave exact AA block iteration as the remaining geometry target, not centre quantization or RNG phase.

AA block-iterator and effective AA-level probes: the iterator under `0x14263FC50` is now mapped far enough to rule out a simple bbox mismatch. `0x14206C980` converts the dab double bbox to `floor(left/top), ceil(right/bottom)`, `0x14206CBB0` expands that integer rect by one pixel, `0x1432E9050` intersects it with the target extents and converts to tile/block ranges, and `0x1432E9230` yields half-open block rectangles clipped against that expanded integer bbox. This matches the importer's broad clipping closely enough that bbox off-by-one is unlikely to explain the random residual. For `Vector_OpacityRandom_50`, forcing effective AA levels in the experiment gives `AA0 mean=1.861913`, `AA1=0.673200`, `AA2=0.320588`, `AA3=0.512062`, so the style AA=2 path is the correct effective level; the caller is not suppressing AA for this sample.

Final alpha quantization probe: keeping the experimental stroke alpha in 32768-scale until the stroke ends is still the right boundary, but the last 32768-to-8-bit conversion is not proven native. An in-memory offset sweep for `Vector_OpacityRandom_50` shows ceil-like conversion can improve the random sample (`offset 32767 -> mean 0.307088` vs the kept round offset `16384 -> 0.320588`), and the scoped gate with ceil improves to `mean_delta=-1.191281`. However, broad dab verification with the same ceil conversion worsens `Vector_OpacityPressure` visible pixels (`tmp_vector_probe/native_dab_alpha8_ceil_broad_v1.json`, `visible_delta=+1461`), and there is no native final-conversion evidence yet. The importer therefore keeps round-to-nearest for the default-off dab experiment and records ceil as rejected/insufficiently proven rather than a general row-path rule.

Native color row writer boundary: the 2026-05-26 IDA reconnect confirms both MCP sessions are still live (`CLIPStudioPaint.exe.i64` on `127.0.0.1:13337`, `iswCoreTG.dll.i64` on `127.0.0.1:13338`). Decompiling `0x14263DDB0` shows it is the material/color row pixel writer, not the final flattened layer composite: it computes `candidate = min(0x40000000, inputCoverage * ctx+0x1e0) >> 15`, updates a 16-bit coverage plane, and writes/mixes BGR bytes using the coverage increment ratio when build-up mode is active. Its callers are the color/material writer variants `0x14263C060` and `0x14263C3A0`; the ordinary no-pattern path in `0x14263AC30` still has a simple coverage-only branch when no color buffer `ctx+0x150`/`ctx+336` is present. This supports the current importer boundary: keep the random-opacity experiment stroke-local in 32768 alpha, reject broad alpha-ceil and direct RGB-composite shortcuts until the layer/final compose path is traced, and treat `0x14263DDB0` as evidence for material/color paths rather than the plain random-opacity sample.

Draw-unit plane wiring follow-up: `sub_14260D4F0` constructs the two `PWBrushPlot` draw contexts. The primary plot at draw-unit `+8` is built by `sub_142636380(v38, a1[7], a1[11], a1[19], a1[15], a1[5])`, while the secondary plot at `+24` swaps in the corresponding secondary accessors. During block preparation, `sub_14260E310` resolves live accessors from the draw resource wrapper through helpers such as `0x142611330`, `0x1426113F0`, `0x142611040`, `0x142611120`, `0x142610CD0`, and `0x142610FA0`, then `0x142643E80` writes them into no-pattern context slots: `a2 -> ctx+0x130`, `a3 -> ctx+0xd0`, `a4 -> ctx+0x100`, `a5 -> ctx+0xe0`, and `a6 -> ctx+0x110`, later clipped into `ctx+0xf0/+0xf8/+0x120/+0x128/+0x140`. `0x14263F410` then chooses active row buffers with `plot+88` / `a12`: false selects `ctx+0xf0` coverage plus optional `ctx+0x120` color, true selects `ctx+0xf8` plus optional `ctx+0x128`. This reinforces that `ctx+0x150` is optional current-row color storage, not proof that every plain opacity-random dab writes BGR directly.

Composite flush quantization candidate: `0x14260D060` flushes queued draw units through `0x14260DB90` and then the composite flush family reached by `0x142660410`. In `0x1426619B0`, one format branch reads a 16-bit coverage plane from `sub_1419D5CD0(a1+0x168)`/`sub_1419D5CD0(a1+0x360-ish live accessor)` and copies to an 8-bit target alpha plane with `HIBYTE(*u16)` when `a1+0x1a0` requests alpha output; the paired BGR output is copied from the high bytes of the 16-bit color plane exposed by `sub_1432C58A0`. A separate no-color-style branch vector-shifts 16-bit coverage by 8 before storing bytes. `0x1426484A0` is another flush-side path that, for some composite modes, treats any nonzero 16-bit coverage as full 8-bit target alpha (`0xff`) and optionally writes a second byte lane. This is real native evidence for final 16-to-8 quantization being truncation/format-dependent, but it is not yet proof for `Vector_OpacityRandom_50`: the exact plain no-pattern random path through `0x142660410 -> 0x1426619B0/0x14264F060/0x1426484A0` still needs to be tied to the sample's draw-unit flags before changing the importer conversion.

Plain composite flush path confirmation: `0x142644610` constructs `PWBrushComposite` with `+400=0`, `+404=0`, `+408=0x21`, `+412=0`, `+416=1`, `+420=0x8000`, `+424=0`, and `+428=0`. `0x1426634A0` wires the live planes and only refreshes `+408` from the target accessor format; it does not set `+400/+404/+424`, so the ordinary no-pattern composite reaches the default `0x142660410 -> 0x142653A40` branch unless later resource state explicitly changes those flags. In that default branch, mode `+412=0` and `+408&0x20` use either the inlined direct writer or `0x14264BC90`: both convert nonzero 16-bit coverage to 8-bit alpha as `(coverage - 1) >> 7`, with full coverage becoming `0xff`, and write the brush BGR bytes when an output color lane is present. The importer now mirrors this only inside the gated native random-opacity experiment; `tmp_vector_probe/native_rng_flush_trunc_scoped_gate_v9.json` passes with `Vector_OpacityRandom_50.clip mean_delta=-1.193344 visible_delta=-1548` and the scoped pressure/AA/texture samples unchanged.

Post-flush AA center recheck: after adopting the native `(coverage - 1) >> 7` flush quantization, the isolated `0x14263FC50` center-shift graft was retested. `tmp_vector_probe/native_rng_flush_trunc_center_shift_scoped_probe_v1.json` still worsens the random target (`mean_delta=-0.422381` versus `-1.193344` for the kept path), while the non-target guards remain unchanged. This reconfirms that the residual is not fixed by applying `center - 0.5` alone; exact AA recovery must include the whole native row/block interaction, not a single coordinate tweak.

Post-flush exact-AA row-span rejection: a more literal temporary port of `0x14263FC50` was retested after the native flush fix. It used the native outer span `int(cx - sqrt(r^2-dy^2))-1 .. int(cx + sqrt(...))+1`, the inner full-coverage span `int(cx - sqrt((r-aa)^2-dy^2))+1 .. int(cx + sqrt(...))-1`, `center-0.5` distance, and `int(coverage*32768)` edge coverage. `tmp_vector_probe/native_rng_flush_trunc_exact_aa_span_scoped_probe_v1.json` still matched the bad center-shift result (`mean_delta=-0.422381`, `visible_delta=-1172`) instead of the kept simplified-AA result (`mean_delta=-1.193344`). The exact-AA graft was reverted. This points away from isolated circular-AA math and toward either the caller selecting a different no-pattern subpath for the sample or another upstream coordinate/plot preparation detail.

Exact-AA center-origin probe: the sample's `ThicknessBase=1.0` and default `ThicknessEffector=1` imply plot `+24`/queue `+320` is `1.0`, so `0x14263F410` should select circular `0x14263FC50` rather than the stretched `0x142640420/0x1426427D0` family. Retesting the literal row-span with `ctx_center = sampler_point + 0.5` makes the native internal `ctx_center-0.5` distance line up with the importer's current integer sample point and returns to the kept metric (`tmp_vector_probe/native_rng_flush_trunc_exact_aa_span_center_plus_half_probe_v1.json`, `mean_delta=-1.193344`). A broad probe did not improve over the simpler kept AA approximation (`native_dab_exact_aa_center_plus_half_broad_probe_v1.json`; random broad `mean_delta=-0.492488` vs old clean broad `-0.506756`), so this graft was also reverted. The useful conclusion is coordinate-origin evidence: native plot centres are likely half a pixel above/right of the compact integer sample coordinates the importer currently uses.

Native random sampler-linearity correction: `0x1422CCA10`'s segment emitter uses a linear sample context (`start double + direction float * scalar`) and `0x1422CC1E0` passes the resulting `v85` sample record to the draw sink; the importer had been using Catmull-Rom for the experimental dab centreline. A targeted random-opacity-only probe showed that linear centres are closer for `Vector_OpacityRandom_50`. The kept change applies linear interpolation only when `RIZUM_CLIP_EXPERIMENTAL_VECTOR_NATIVE_RANDOM_OPACITY=1` and the stroke has true random opacity dynamics, including the adaptive feedback loop that supplies the native dab samples. `tmp_vector_probe/native_rng_feedback_linear_sampler_scoped_gate_v3.json` passes with `Vector_OpacityRandom_50.clip` improving to experiment `mean=0.267731`, `visible=1148` (`mean_delta=-1.230638`, `visible_delta=-1702`) while `Vector_OpacityPressure`, `Test_Vector`, AA none/medium, texture, and no-texture remain unchanged. This moves the remaining residual from random/flush/spacing toward the still-unmapped curve-vtable interpolation and exact plot-centre origin.

Curve sampler vtable slot confirmation: after reconnecting IDA, the `PWVectorCurve` vtable can be aligned from the known `+0x78` point slot `0x1422CA190`. For the 88-byte point family used by `Vector_OpacityRandom_50`, slot `+0x68` is `0x1422C9C80`, which writes `start * (1 - a2/a3) + end * (a2/a3)`, and slot `+0x70` is `0x1422C96D0`, which returns the Euclidean distance from `this+8` to the next point `a3+8`. The Bezier and cubic subclasses have matching slot triplets (`Bezier +0x68/+0x70/+0x78 = 0x1426231E0/0x142623150/0x142623280`, Cubic = `0x142625370/0x1426252B0/0x142625450`) that call the curve helpers, but the compact 92/76/88/88 random-opacity sample is the base linear family. This upgrades the linear random-opacity sampler change from metric-guided to native-backed; the remaining residual should no longer be assigned to Catmull-Rom centreline mismatch.

Sampler endpoint boundary check: xrefs show the ordinary stroke traversal reaches `0x1422CC1E0` from `0x1425A3D60` as `sub_1422CC1E0(i, sink, 0, a4, style+120&2, 0, state, 1, 0, 1)`. With `a6=0`, the `v42 >= segment_length` case returns after storing residual instead of forcing a final endpoint sample; the endpoint-forcing branch only runs in other callers that pass `a6` nonzero and have no next segment. `Vector_OpacityRandom_50` also does not have the `style+120&2` condition that would rewrite exact-boundary residual to `-1e-8`. Therefore the importer's adaptive loop using `while walk < distance` and no explicit final endpoint dab matches the normal random-opacity stroke path; do not add an endpoint-compensation probe for this sample unless a different caller/style flag is identified.

No-pattern plot-centre chain and post-linear sweeps: the ordinary sink path keeps the sample centre as a draw-local double, not an internal half-pixel-shifted point. `0x14255DFE0` converts the sampler record to local queue coordinates as `(sample_x - draw_origin_x) * draw_scale` and `(sample_y - draw_origin_y) * draw_scale` (`a1+24/+28` origin, `a1+264` scale), passes that as `a4` to `0x14260F550`, which copies it to queue `+600`, and `0x14260DB90` passes queue `+600` straight to `0x14263F410`. `0x14263F410` stores it unmodified at ctx `+0x1b0/+0x1b8`; the `center-0.5` behaviour is confined to the AA/profile distance math after setup. Fresh in-memory sweeps after the linear sampler fix confirm the current integer compact centre remains best for `Vector_OpacityRandom_50`: `int mean=0.267731`, `x-0.25=0.276881`, `y-0.25=0.285319`, `x+0.25=0.375994`, and `+0.5,+0.5=1.071094`. Re-sweeping AA width and synchronized radius/AA scale also keeps native factor `1.0` as best (`AA factor 1.0 mean=0.267731`; radius/AA factor 0.99 already worsens to `0.284006`, 1.01 to `0.300375`). Conclusion: after the linear-centre correction, the remaining residual is not a global centre offset, AA-width scalar, or radius scalar; it belongs to exact coverage/profile behaviour or an upstream compact-to-native coordinate projection detail.

Random-opacity post-linear residual split: after fixing the native random sampler to linear segments, the rejected knobs were re-swept. Interval scale `1.0` remains best (`0.99 -> mean 0.420825`, `1.01 -> 0.486881`), and LCG phase `0` remains best (`phase 0 mean=0.267731`, `+1 -> 0.481256`, `-1 -> 0.565706`, with wider `-4..+4` phases all worse). A direct exact-AA row-span implementation also no longer buys anything: the simplified kept AA path and literal `0x14263FC50` row-span with `ctx_center = sampler_point + 0.5` both land at `mean=0.267731` / `visible=1148`, while same-centre and other half-pixel variants are worse. The sample's `BrushStyle.Hardness=1.0`, so the native hardness/profile table is inactive for this case. Isolating layer 5 and inferring CSP stroke alpha from paper/stroke RGB shows the alpha total is essentially solved (`out_alpha_sum=809278` vs inferred reference `808962.5`, signed mean `0.082443`), but local distribution remains (`1342` union pixels differ by more than 1, `out_only=61`, `ref_only=38`). The remaining random-opacity work is therefore not seed persistence, LCG phase, interval scalar, global centre offset, radius/AA scalar, or total opacity cap; it is the exact per-pixel coverage/profile/coordinate distribution.

Native AA half-pixel centre correction: the previous conclusion missed the combination of raw sampler doubles with the native AA `ctx_center - 0.5` distance convention. `0x14255DFE0` passes raw draw-local sample doubles to the queue, and `0x14263FC50` applies the half-pixel shift inside circular-AA distance math. Updating the gated native-dab AA path to measure distance from `center - 0.5`, and keeping raw feedback sample doubles for true native random-opacity dabs, improves `Vector_OpacityRandom_50` further: `tmp_vector_probe/native_rng_raw_center_minus_half_scoped_gate_v1.json` passes with experiment `mean=0.122681`, `visible=891`, `max=31` (`mean_delta=-1.375688`, `visible_delta=-1959`) while the scoped non-target guards remain unchanged. The isolated alpha comparison also tightens (`out_alpha_sum=808423` vs inferred reference `808962.5`, abs mean `2.323398`, `out_only=11`, `ref_only=18`). The remaining residue is now small enough that the next native target should be literal circular-AA row-span/block iteration or final compose minutiae, not random-state or centreline sampling.

Post-correction AA row-span and final quantization recheck: after the raw-sample/`center-0.5` fix, literal circular-AA row-span variants were re-run with the corrected coordinate convention. They tie the current vectorized AA result exactly for `Vector_OpacityRandom_50` (`mean=0.122681`, `visible=891`, `max=31`), including `dist_center=center-0.5`, `span_center=center`, and no outer `+/-1` variants. This confirms the current vectorized AA formula is already equivalent for the sample. A final 32768-to-8-bit sweep found a tiny metric improvement from ceil-scaled `((alpha_i * 255 + 32767) // 32768)` (`mean=0.121594`, `visible=889`), but IDA confirms the native plain default flush `0x142653A40 -> 0x14264BC90` uses `*dst_alpha = (*src_u16 - 1) >> 7` when the destination alpha byte is zero, full coverage maps to `0xff`, and only the existing-alpha blend path uses `(255 * src + (0x8000 - src) * dst + 0x4000) >> 15`. Therefore the importer keeps the native `(alpha_i - 1) >> 7` conversion and records the ceil-scaled variant as a metric-only overfit, not a native-aligned fix.

Sampler and colour residual probes after the AA fix: the remaining random-opacity error is also not explained by a missing initial interval offset, endpoint dab, or stroke colour byte. In-memory probes with initial residual `0.25/0.5/1.0/1.5/2.0px` all regress from the current `mean=0.122681` to `0.160763/0.203325/0.280012/0.439669/0.451050`; changing the loop to `walk <= distance` or adding a tiny endpoint epsilon is neutral for this sample. This matches the recovered native `state+8=0` first-dab and no-forced-endpoint path. Forcing the native dab colour to grey `16` or `18` also worsens (`0.166612` / `0.169931` versus current grey `17`), so the decoded stroke colour and paper composite are not the remaining source. The next useful target is therefore deeper overlap/accumulation state or another small native plot/composite flag, not sampler boundary, endpoint compensation, colour decode, or final alpha quantization.

Post-correction random and projection probes: after reconnecting IDA (`CLIPStudioPaint.exe` on `127.0.0.1:13337`, `iswCoreTG.dll` on `127.0.0.1:13338`), the last random-opacity knobs were re-swept around the current `mean=0.122681` result. Interval scale `1.0` remains best (`0.9975 -> 0.143456`, `1.0025 -> 0.168094`, wider `0.995/1.005` worse), LCG phase `0` remains best (`+1 -> 0.358219`, `-1 -> 0.441038`), high-15-bit variants quantize identically, and low/16-bit variants regress (`rand_16bits_div65536 -> 0.621769`, `rand_low15 -> 0.737156`). A diagnostic-only global sample offset can reduce the metric (`dx=-0.10, dy=+0.02 -> mean=0.093000`), but fresh IDA tracing shows `0x142E65B20` builds the brush bridge packet as `sample_x/y + tool integer offsets`, while `0x14255DFE0` later uses `(sample - draw_origin) * draw_scale`; the nearby `0.05 / scale` fields in `0x1430B8AF0/0x1430B9BE0` have not been proven to enter the dab centre. Follow-up decompilation shows `0x1430B7DA0` constructs `Planeswalker::PWSnapSymmetry`, `0x1430B79D0` builds a symmetry/snap matrix around an anchor from `sub_142497120`, and `0x1430B8280` applies that optional matrix only to sample x/y before the brush packet; the current random-opacity fixture diagnostics contain only one ordinary 92-byte stroke style and no symmetry/snap evidence. Therefore no offset is implemented; the subpixel result is only a clue for future upstream transform/origin tracing.

Random-opacity segment-context rejection: `0x14255C510` calls `0x1422D9200` during sampler segment init and the random-opacity style has `StyleFlag 0x200`, so a tempting hypothesis was that the current importer misses a native `sample_context+0x38/+0x40` size/flow rewrite before each segment. The relevant native pieces are real: `0x1422D9200` computes two factors from style hardness/flow and the incoming sample context, and `0x1422CCA10` can then interpolate those fields into emitted samples. However a default-off probe that applied this literal hard-brush segment factor to the random-opacity adaptive path badly regressed the target (`tmp_vector_probe/native_rng_segment_factors_probe_v1.json`: `Vector_OpacityRandom_50 mean_delta=+2.084193`, `visible_delta=+737`) while the unchanged baseline gate still passed (`native_rng_segment_probe_baseline_gate_v1.json`: `mean_delta=-1.375688`). The probe was removed from code. Treat this segment-init path as bounds/secondary-context evidence until its exact caller semantics are understood, not as a missing random-opacity size taper.

Random-opacity residual triage after segment rejection: three remaining small explanations were checked and rejected. First, `StyleFlag 0x40` is copied to plot `+40` and reaches `0x14263F410` as `a8`, but the no-pattern setup only changes the axis orientation when the thickness/stretch ratio `a6 > 1.00000001`; the current sample has default `ThicknessBase=1.0`, so it stays on the circular AA path. Second, paper compositing over the decoded paper RGB `226` was swept with integer `/255` and `/256` round/floor/ceil variants; the current round-to-nearest `/255` blend remains best (`mean=0.122681`, `visible=891`), while floor/ceil and `/256` variants are worse. Third, the `0x1422CC1E0` residual `-1e-8` boundary path was tested by allowing negative initial residuals in memory: `-1e-8` is identical to baseline and `-0.0001` only gives a noise-level metric change (`0.122681 -> 0.122625`). Do not implement any of these; the remaining error is still local coverage/sample distribution rather than a global branch, final paper blend, or residual clamp.

Random-opacity sampler-channel rejection: the `0x1422CC1E0` main loop was rechecked against IDA after reconnecting. The in-function block at `0x1422CC8C0` is part of `0x1422CC1E0`, not a separate helper; the base point-family slot `+0x68` (`0x1422C9C80`) writes the linear point and leaves `xmm0 = t`, which the caller uses as `v44` to interpolate sample channels. However the current `Vector_OpacityRandom_50` compact stroke has `f32+56 == 1.0` and `f32+60 == 1.0` for all 14 points, and compact `u32+32` is `0` for 13 points and `1` for one point, so the native `0x1000/0x2000` fixed-vs-interpolated logic cannot explain the remaining pixel distribution. `0x1422D8550` was also rechecked for extra random consumption: `SizeEffector`, `FlowEffector`, and `IntervalEffector` are default `0x01`, and `RotationEffector=3` does not set the random-rotation bit `0x80`, so opacity remains the only active LCG consumer for this fixture. The scoped gate `tmp_vector_probe/native_rng_sampler_flags_recheck_gate_v1.json` matches the previous best (`Vector_OpacityRandom_50 mean_delta=-1.375688`, all guard clips unchanged).

Random-opacity overlap/AA triage: in-memory row-writer probes show the remaining error is not a simple 16-bit rounding mode. Changing AA coverage to round/ceil, flow coverage to round/ceil, or accumulation deltas to round/ceil leaves `Vector_OpacityRandom_50` unchanged or slightly worse around the current `mean=0.122681`; `cap_floor` gives only a tiny metric nudge (`0.122606`, visible `890`) and contradicts the native `0x14263F410` opacity-cap rounding with `+0.5000000100000001`, so it is rejected as overfit. A fresh literal `0x14263FC50` AA span port also ties the current vectorized AA result exactly when using `x/y center - 0.5`; boundary variants such as an extra inner-right pixel or different x center regress. Instrumenting dab overlap shows the residual is mixed local distribution rather than a monotone overlap formula error: 1-6 hit pixels have both over- and under-coverage, while high-overlap pixels are only slightly dark. Finally, compact point `u32+32=1` was checked against `0x1422CBAE0`, `0x1422CD520`, and `0x1422CD3C0`: bit0 only controls internal `0x1000`, bit1 controls `0x2000`, and this fixture has no bit1, so it does not trigger the `a1[8] & 2` `0x1422CCA10` side emitter. `sub_1422D8140/sub_1425597A0` were also checked for this style; `StyleFlag 0x10000` clears the color-change/mix descriptor and does not add a centre, opacity, or random adjustment.

Random-opacity draw-pass gate rejection: the suspected two-pass submit loop in `0x14255DFE0` was traced through `0x14255D810` and `0x1425590B0`. `0x14255D810` derives `draw+192` from the small descriptor written by `sub_1422DC770`: descriptor `+12` is the active water/color-change gate and descriptor `+16` is `WaterColorType`; only active descriptor cases set `draw+192` to `1/2/3`. `0x1425590B0` immediately returns `1` unless `draw+192` is `1` or `3` (or unless the secondary-style case has `draw+200`), and `0x14255DFE0` only repeats when that helper returns `0`. The current `Vector_OpacityRandom_50` style is plain no-pattern/no-water (`PatternStyle=0`, `TextureFlag=0`, `UseWaterColor=0`, `BrushColorMixingMode=0`, `StyleFlag=0x1c240`), so descriptor `+12=0`, `draw+192=0`, and the helper returns `1`; the sample is submitted once through the ordinary queue. Therefore the remaining random-opacity residue is not a hidden second pass, retained secondary queue, or water/color-mix flush gate.

Random-opacity sampler caller refinement: the current bridge path reaches `0x14255A7E0 -> 0x1425A4100`, not only the older standalone stroke helper `0x1425A3D60`. This matters for naming but not for the current importer behavior. `0x1425A4100` calls `0x1422CC1E0(curve, sink, secondary, style_has_bounds, style&2, style&0x20, state, 1, 0, 0)`: `a8=1` still seeds/restores the shared residual/random state, `a9=0` skips the optional point-visibility test, and `a10=0` makes the per-sample draw gate initially true before it is passed to sink `+0x20 -> 0x14255C980`. With current `StyleFlag&2 == 0` and no retained `0x20` branch, this caller still does not force an endpoint dab or consume extra LCG state. The previous endpoint rejection remains valid, but future notes should cite `0x1425A4100` for the active bridge route and reserve `0x1425A3D60` for the other stroke traversal route.

Random-opacity resume and point-cleanup rejection: after reopening this thread, both IDA MCP sessions were live again (`CLIPStudioPaint.exe` on `127.0.0.1:13337`, `iswCoreTG.dll` on `127.0.0.1:13338`) and the scoped random gate still passes as `tmp_vector_probe/native_rng_resume_gate_v1.json` (`Vector_OpacityRandom_50 mean_delta=-1.375688`, `visible_delta=-1959`, all scoped guards unchanged). The file-persistent random source remains the compact stroke seed at `u32+88` / native `PWVectorStroke+0xa0`, so reopening the same `.clip` does not resample a new random sequence. A tempting coordinate-cleanup path was also rejected: `0x1422CFC50 -> 0x1422CD170` can snap nearly horizontal/vertical neighbouring curve points to `int+0.001` or `int+0.5+0.001`, and `0x1422CBF70` rounds point x/y to `1e-6`, but this fixture has no adjacent point with x/y equality under the native epsilon and in-memory `1e-6` rounding probes are exactly neutral (`mean=0.122681`, `visible=891`, `max=31`). The current residual remains a local distribution issue: inferred isolated alpha has near-zero total error (`out_alpha_sum=808423`, inferred reference about `808717..808963` depending on reference extraction), with output brighter on the upper/right side and darker on the lower/left side, so keep treating the diagnostic `dx=-0.10,dy=+0.02` improvement as a clue rather than an implemented native rule.

Random-opacity sample-array/snap-ruler branch rejection: `0x142E73180` can install `a1+112` from `0x1430BB030`, after which the render loops call the object's `vtable+0x10` method before `0x1430BAFB0` copies the generated 48-byte records. That branch dispatches concrete `PWSnap*` objects (`PWSnapRuler`, `PWSnapRulerParallel`, `PWSnapRulerEmit`, `PWSnapRulerGuide`, grid/vector/curve variants), and their methods append records through the common `a1+8/+16/+24` vector. It is useful structure but does not match the current random-opacity fixture, which still has no symmetry/snap/ruler evidence. The important confirmed layout is `0x1430BAFE0`: a 48-byte vector sample record receives x/y from the bridge packet at `+128/+136`, scalar/channel fields from `+160/+168/+172/+176`, zeroes sample `+36`, and copies the state/random qword from `+208`. The active ordinary stroke route remains `0x1425A4100 -> 0x1422CC1E0 -> sink+0x20 (0x14255C980) -> 0x14255DFE0`, not a snap-ruler projection.

Random-opacity spray-only effector re-evaluation rejection: `sub_1422E04C0` can re-evaluate plot opacity when its flag byte at `a1+224` is negative, which initially looked relevant because `OpacityEffector=0x81` has the high bit set. The caller context rules it out for the current fixture: in `0x14255DFE0`, `sub_1422E04C0` is reached only inside the spray/pattern branch guarded by `style+0x61c & 1`. The plain `Vector_OpacityRandom_50` style is no-pattern/no-spray, so it follows the ordinary branch through `sub_1425597A0`, `sub_14255C2A0`, draw-local coordinate conversion, `0x14260F550`, and `0x14260DB90`. There is no extra `0x81` random draw or opacity re-evaluation on this path.

Random-opacity draw-local scale/origin rejection: the no-pattern branch in `0x14255DFE0` converts sample centres as `(sample_x - int(draw+24)) * draw+264` and `(sample_y - int(draw+28)) * draw+264`, then scales the plot size by the same `draw+264`. The apparent `sub_14255D370(a1)` call in `0x142558580` is a decompiler shorthand: assembly shows it passes `xmm1 = sub_142611310(offscreen_set)` and `xmm2 = sub_142611320(offscreen_set)`, i.e. `PWBrushOffscreenSet+0x1b0/+0x1b8`, into `draw+264/+272`. `PWBrushOffscreenSet` construction/reset initializes both to `1.0`; the only observed setter in the active tool area updates the y-scale field `+0x1b8`, while ordinary no-pattern centre conversion uses the x-scale field `+0x1b0`/`draw+264`. This leaves the metric-only `dx=-0.10, dy=+0.02` improvement as an upstream projection clue, not a native-backed importer offset or scale edit.

Random-opacity first-sample and curve-feedback rejections: `sub_1422DD7A0` is the sampler predicate `(style+0x270 & 0x40) || (style+0x288 & 0x40) || ((style+0x78 & 0x30) == 0x30)`. The current `Vector_OpacityRandom_50` `StyleFlag=0x1c240` does not satisfy the `0x10/0x20` flag case, and the fixture still has no optional list/material evidence for the `+0x270/+0x288` cases. A forced importer probe that consumed the first zero-distance random sample but skipped drawing it kept guards unchanged but worsened the random target versus the current best (`tmp_vector_probe/native_rng_skip_zero_draw_probe_v1.json`: experiment mean `0.133331`, visible `906`, versus current `0.122681`, visible `891`). A second probe that changed the native random feedback centreline from the recovered linear segment interpolation back to Catmull-Rom also regressed (`tmp_vector_probe/native_rng_catmull_feedback_probe_v1.json`: experiment mean `0.134944`, visible `960`). Both changes were reverted; the current scoped gate after the rejections is `tmp_vector_probe/native_rng_resume_gate_v3_after_rejects.json` with `Vector_OpacityRandom_50 mean_delta=-1.375688`, `visible_delta=-1959`, and all scoped guards unchanged.

Random-opacity compact tail-field rejection: the varying compact point `f32+76` initially looked like a possible local offset/coverage clue, but IDA maps it to internal point `+0x6c`, not to the submitted brush sample x/y. The base `PWVectorCurve` sample slot `0x1422C9C80` computes only linear xy from `this+8/+16` and next `+8/+16`; `0x1422CD520/0x1422CD3C0` copy only packet doubles `+0x10..+0x40` into internal floats `+0x44..+0x5c` for brush evaluator channels. The archived floats at internal `+0x60/+0x64/+0x68` are tangent/scale helpers touched by branches such as `0x1422CDA90/0x1422CDCD0` when point flag `0x40` is active; this fixture has no such point flag (`compact u32+32` is only `0` or bit0 once). Internal `+0x6c` is also overwritten at the start of the active sampler call: `0x1422CC1E0` with `a8=1` writes the shared residual `state+8` into `point+0x6c`, while the caller's `state+8` starts at `0`. Therefore the file's per-point `f32+76` values are persisted sampler metadata, not active dab-centre or random-opacity inputs for the current render.

Random-opacity circular-path recheck: `0x14263F410` still selects the circular no-pattern path for this fixture. `ThicknessBase=1.0` leaves the `a6/v15` ratio inside `1.0 +/- 1e-8`, so the stretched/rotated helpers `0x142640420/0x1426427D0` are bypassed; `Hardness=1.0` keeps the softness/profile mask disabled; and nonzero AntiAlias reaches `0x14263FC50` through `0x1422DBF30`. This means the remaining local residual is not explained by compact tail floats, point flag `0x40`, an accidental stretched-brush branch, or a hardness profile.

Random-opacity queue-order rejection: the ordinary sink callsites in `0x14255DFE0` write one plot record with `0x14260F550` and immediately consume it through `0x14260DB90` in the same sample path (`0x14255E571 -> 0x14255E580` and `0x14255E787 -> 0x14255E796`). The related helpers show the same shape for the non-random draw routes; only the async flush loop `0x14260D060` consumes a pending queue record later, and the current ordinary path does not leave a multi-record list to reorder. A temporary importer probe that reversed dab draw order confirmed this is not the remaining pixel source: `tmp_vector_probe/native_rng_reverse_dabs_probe_v1.json` still improved the target but regressed versus the kept order (`mean_delta=-1.234106`, `visible_delta=-1336` vs current `-1.375688` / `-1959`). The restored scoped gate `tmp_vector_probe/native_rng_queue_order_recheck_gate_v1.json` matches the current best and leaves all scoped guards unchanged.

Random-opacity `0.05/scale` projection rejection: the previously suspicious `0.05 / scale` value is real, but it is not a native-backed dab-centre offset for the current render. `0x1430B8AF0` and `0x1430B9BE0` store it at bridge object `+0x130`; `0x1430B8640` reads that field only as the fourth argument to `0x1425B0AD0`, a geometric point/segment reuse predicate that decides whether to update the internal curve point through vtable `+0x30/+0x38/+0x48`, then calls `0x14255A7E0`. The active brush submit still reaches `0x1425A4100 -> 0x1422CC1E0 -> 0x14255DFE0` with raw sample doubles; this tolerance is construction/simplification plumbing, not an additive sample transform. The current `Vector_OpacityRandom_50` 14-point stroke also has no simple `0.05px` collinear deletion candidate: the smallest middle-point distance to its neighbour chord is about `0.367687px`, and zero points fall within `0.05px`. Keep the diagnostic `dx=-0.10,dy=+0.02` as an unresolved metric clue, not an implemented offset.

Random-opacity edge residual triage: a no-edit instrumentation pass over the current gated native-random path shows the remaining error is concentrated on low-overlap circular-AA edge pixels. Of the inferred alpha mask's `3777` active pixels, only `15` have zero experimental dab hits; `13` of those are CSP-undercovered-by-us pixels, and every one sits just outside the current nearest native dab boundary (`nearest (radius - distance) / aa_width` from `-0.052471` to `-0.001792`, i.e. at most about `0.13px` outside for AA width `2.5`). Tiny in-memory metric tweaks such as `radius +0.005px` or `aa_width -0.01px` can move `Vector_OpacityRandom_50` by only noise-level amounts (`mean 0.121969` / `0.122325` versus current `0.122681`) and trade off visible-pixel counts. Fresh IDA decompilation reconfirms the native constants and formulas: `0x1422DBF30` maps AA `1/2/3` to `min(radius, 1.5/2.5/3.5)`, `0x14263FC50` uses `ctx_center - 0.5`, `1.0 / aa_width`, and `(int)(coverage * 32768.0)`, and `0x14263F410` rounds opacity/flow caps with `+0.5000000100000001`. Therefore do not encode micro radius/AA scalar fixes; the residue remains an upstream coordinate/profile/distribution issue rather than random state, row fixed-point, AA scalar, or row-span math.

Random-opacity style-flag residual rejection: the remaining unknown bits in this sample's `StyleFlag=0x1c240` were checked against the active native path. `0x1422D8550` does read `StyleFlag & 0x8000`, but only to decide whether caller argument `a5` can suppress the normal auto-interval cap: when the flag is set, local `v10=a5`, and if that later tests false the interval scalar `v35` is clamped with `fmin(0.25, v35)` before writing the next `state+8` distance. The current random-opacity brush has `IntervalBase=0.1`, so either side of that branch still leaves the native interval scalar at `0.1`; it cannot explain the current pixels. `StyleFlag & 0x4000` did not appear in the decompiled active sampler/evaluator/draw-submit path (`0x14255D810`, `0x1422D8550`, `0x14255DFE0`, `0x1422CC1E0`, `0x1425A4100`, `0x14255C980`, `0x14255C510`, `0x1425597A0`, `0x1422DC770`, `0x1422DD630`, `0x1422DC740`); the nearby live `0x40000` check belongs to the secondary color-mix writer, not this primary no-pattern sample. The diagnostic center-shift sweep is still stable (`dx=-0.10, dy=+0.02 -> mean=0.093000`, current `0.122681`), but these style bits do not provide a native-backed offset/profile rule.

Random-opacity sampler precision rejection: the residual is not caused by float-vs-double arithmetic in the recovered native sampler loop. `0x1422CC1E0` computes the segment interpolation scalar in double, while also mirroring `state+8` into internal point float `+0x6c` at entry for persistence/serialization. A targeted in-memory renderer that replaced the current dab centres with variants using float32 segment deltas, float32 residual/next-step updates, or fully float32 `distance/t/center` generation produced the same metric as current (`mean=0.122681`, `visible=891`, `max=31`). The largest centre movement in the strongest float32 variant was only about `1.0e-5px`, far below the diagnostic `dx=-0.10,dy=+0.02` clue. Separately, `0x14255A7E0` shows `sub_1422CA9A0` is only used in the cached/bounds path when `draw+212` is set; the active ordinary path calls `0x1425A4100 -> 0x1422CC1E0` and then reads queue bounds. Keep the remaining coordinate investigation focused on draw-origin/upstream projection or persisted point construction, not sampler numeric precision or the cached bounds builder.

Random-opacity draw-origin bridge rejection: `0x142558580` receives its draw-local origin from the caller's `a2+72` field, passed through `0x1430B8AF0/0x1430B9BE0` as `a1+320`, and stores the qword directly at `draw+24/+28` before `0x14255DFE0` later subtracts those values as integers. The same setup stores clip/request origin `a2+64` at `draw+248/+252`; both only affect integer bounds/origin subtraction and cannot explain the stable fractional `dx=-0.10,dy=+0.02` clue. The packet builder `0x142E65B20` also only adds integer tool offsets `a1[72]/a1[73]` to sample x/y. The only confirmed fractional upstream coordinate transform in this bridge is optional `0x1430B8280`, which applies a matrix to the sample block when render state `+128` is present, but the current random-opacity fixture has no snap/ruler/transform evidence and follows the ordinary sample-array route. Therefore keep looking at persisted point construction or another upstream projection source, not draw-local integer origin or the basic bridge packet offset.

Random-opacity affine/queue-path refinement: an in-memory centre-transform probe makes the residual's shape clearer but still does not justify an importer rule. Translation alone keeps the old clue (`dx=-0.10,dy=+0.02 -> mean=0.093000`), and adding a tiny first-order rotation around the rendered alpha centroid improves the target further (`rot=0.001 -> mean=0.080175`, visible `669`, max `29`). The native queue path does not support baking that transform into the reader: `0x14260F550 -> 0x14260DB90 -> 0x14263F410 -> 0x14263FC50` copies raw draw-local centre doubles through queue `+600` and only applies `ctx_center-0.5` inside circular-AA distance math. `0x1430BBA20/0x1430BBC40` are spacing/history scalar helpers rather than x/y mutators, and the optional `0x1430B8280` matrix lives in the input/sample-array path gated by `sub_1430B7CA0/sub_1429C6EE0`, not in the active saved-stroke render route. Treat the affine result as a diagnostic pointer toward persisted point construction or compiled line-list projection. The next native-backed question is whether the compact renderer's `0x1425A4100` compiled line-list object (`v12[6]`) is byte-for-byte equivalent to the importer-visible 92-byte point stream.

Random-opacity compiled line-list refinement: `0x1425A4100`'s `v12[6]` is not a separate hidden projection layer for the current stroke. The compact reader chain `0x142554920 -> 0x1422CF5F0` allocates the stroke/list object through `sub_142552AF0`, then allocates each node from list factory `v14+72`, links the first/last nodes at `v14+48/+56`, and calls `0x1422CBAE0` for each record. `0x1422CBAE0` reads x/y doubles into node `+8/+16`; the later allocator recheck corrected the node family to `PWVectorSplineCurve` for `0x2081`, so the persisted compact points are still the source, but their active sampling is spline-based rather than base-linear. The cleanup/projection helpers `0x1422CFC50 -> 0x1422CD170`, `0x1422CBF70`, `0x1422CDA90`, and `0x1422CDCD0` are reached from editor/transform/post-processing callsites, not the file-read path for this fixture. Therefore there is still no second compiled line-list representation; the resolved centreline gap was the selected spline point-family sampler, not hidden rewritten point coordinates.

Random-opacity layer/document transform rejection: the saved `.clip` tables do not expose a layer-level transform for the current vector layer. `Layer 2` has `LayerOffsetX/Y=0`, `LayerRenderOffscrOffsetX/Y=0`, mask offsets `0`, opacity `256`, composite `0`, and visibility `1`; `VectorObjectList` contains only the layer's `VectorData`, and the related `MipmapInfo`/`Offscreen` rows for layer id `5` carry normal 100%/50% offscreen resources rather than a matrix. This rules out the small affine probe as a serialized layer/object transform in the current fixture. Keep the importer centred on compact point geometry plus native no-pattern dab/composite behaviour.

Random-opacity downstream composite/cache rejection: the saved vector-layer offscreen cache does not contain CSP's rendered stroke for this fixture. `decode_layer(5)` returns no usable raster cache; layer id `5` offscreen ids `24/25` do not decode as RGBA, and offscreen id `26` decodes as an empty 512x512 tile set (`alpha_sum=0`). A no-edit probe that captured the gated native-random `native_alpha_i` 32768-scale buffer and composited it directly over paper as a hypothetical 16-bit brush-to-paper path was also worse than the current 8-bit layer-alpha composite (`16bit_direct off0 mean=0.129150`, rounded `0.139631`, versus current `0.122681`). This matches the native branch evidence: `0x14264BC90` only uses the existing-destination-alpha colour blend when the target alpha byte is nonzero; the vector layer's own offscreen starts empty and writes brush BGR plus `(*coverage - 1) >> 7` alpha. The remaining residue is therefore not recoverable from a saved vector cache or by bypassing the layer alpha byte during final paper composition.

Random-opacity AA precision and cap-floor rejection: replacing the current circular-AA distance with `np.hypot`, explicit float64 square-root, or float32 distance/coverage leaves `Vector_OpacityRandom_50` exactly unchanged (`mean=0.122681`, visible `891`, max `31`), so the residue is not NumPy double versus native float/SSE sqrt precision. Tiny opacity-cap nudges can still find metric-only local minima (`cap_floor`, denominator `32769`, or floor `0.4999847` all give `mean=0.122606`, visible `890`), but IDA rejects them: `0x142568040` advances `state = 1103515245*state + 1234567` and multiplies by `((HIWORD(state) & 0x7fff) * 0.000030517578125)`, while `0x14263F410` rounds opacity/flow caps with `+0.5000000100000001` and `0x14263AC30` uses unrounded `>>15` products for flow coverage and build-up accumulation. Keep the native round/trunc chain rather than encoding the cap-floor overfit.

Random-opacity endpoint distribution triage: the current gated native-random path emits 91 dabs from the file-persistent stroke seed. Inferred isolated alpha has `out_alpha_sum=808423` versus reference about `808962.5`, with the left/start half too transparent (`sum=-1211.9`) and the right/end half too opaque (`sum=+672.4`). Nearest-dab residual attribution is strongest at the final dab around `(154.897,25.614)` (`cap=0.9058`, positive residual) and early dab 4 around `(18.684,127.122)` (`cap=0.7186`, negative residual). However cap-order probes reject sequence misalignment: reversing caps worsens to `mean=0.673013`, shifting by one worsens to `0.316688`/`0.438019`, and sorted/reversed variants are larger still. Compact point channels also rule out a hidden opacity curve for this fixture: `f32+52/+56/+60` are all `1.0`, while `OpacityEffector=0x81` decodes as the random-floor branch only. The residue is endpoint/edge distribution, not a wrong random order or extra channel-driven cap.

Random-opacity seed and spacing equivalence: per-point `u32+80` values match the continuous LCG state at each original point boundary, so a segment-local reset to the saved point seed is equivalent to the current continuous stroke-seeded walk for this sample (`mean=0.122681`, visible `891`). Other seed/phase variants are rejected: using the next point seed, no-advance first value, per-segment spacing reset, first step only, or midpoint phase all regress (`mean` from about `0.280013` to `0.701663`). Carrying residual distance across segment boundaries is therefore correct, and the persisted per-point random states explain deterministic reopening without changing the active output.

Random-opacity row accumulation and active-caller rejection: no-edit draw probes confirm the current row composition roles are correct. Direct/max accumulation, swapping cap and flow, half-cap flow, cap-as-flow, and cap-squared variants all worsen the target (`mean` about `0.330356..0.917381` versus current `0.122681`). IDA also narrows the bridge caller: `0x14255A7E0` passes caller `a6` to `0x1425A4100`; ordinary saved-stroke callsites `0x1430B8640` and `0x1430B89E0` pass `a6=0`, so `0x1425A4100` submits only `v12[6]` (the first-node list). `a6=1` callsites exist in `0x1430B8F50` and `0x1430B9BE0`, but they belong to editor/update or temporary-object routes, not the current saved-stroke render. There is no hidden second list submission for `Vector_OpacityRandom_50`.

Random-opacity PSD-cache absence: there is no `img\Vector_OpacityRandom_50.psd` layer export available in the current corpus, so PSD layer alpha cannot be used as an independent target for this specific random-opacity fixture. Continue treating the PNG as the sole pixel oracle and the saved `.clip` vector/offscreen data as authoritative metadata.

Random-opacity bbox/clip rejection: compact point bboxes are present (`point+16..28`, read by `0x1422CBAE0` into native node `+24..36`), and this stroke header bbox is `(8,14)-(166,138)`. The active saved-stroke route still calls `0x1425A4100 -> 0x1422CC1E0` with `a9=0`, so the optional point/list visibility vtable `+0xf0` is skipped. The draw-side region check is only `0x14255C2A0`: it rounds the sample centre with `+/-0.5000000100000001`, adds integer draw offsets, expands by `ceil(radius)` (or `1.5*radius` for pattern), and asks the draw region whether that coarse footprint touches. No-edit clipping probes match that reading: clipping every dab to the record bbox, segment-union point bbox, or interpolated point bbox leaves `Vector_OpacityRandom_50` exactly unchanged (`mean=0.122681`, visible `891`), while nearest-point bbox clipping regresses badly (`mean=0.514875`, visible `1174`, max `206`). The endpoint residual is therefore not a saved point-bbox/clip truncation issue.

Random-opacity full-affine diagnostic and optional-transform rejection: fitting a general tiny centre transform around the current dab centroid `(80.084466,74.090832)` improves the target from current `mean=0.122681` to `mean=0.069488`, `visible=597`, `max=26` with parameters `tx=-0.0935547`, `ty=0.0125`, matrix `[[1.001125,0.001],[0.00215625,1.001875]]` (singular values about `1.003122/0.999878`, rotation about `0.000577rad`). This remains a diagnostic only. Native reinspection shows the concrete matrix applier `0x1430B8280` transforms the first two doubles of a 48-byte sample block before `0x142E65B20`, but the producer chain `sub_1430B7CA0 -> sub_1430B7DA0/sub_1430B79D0` is the optional snap/symmetry path (`Planeswalker::PWSnapSymmetry`) and depends on render state `+128`. The current fixture has no snap/ruler/symmetry tables or saved transform evidence, and `0x142E65B20` itself only adds integer tool offsets to sample x/y. Therefore do not implement the fitted affine; keep looking for a native-backed ordinary saved-stroke coordinate/profile source.

Random-opacity live sample-array route rejection: `0x142E762F0` and sibling `0x142E77A40` do consume the live 48-byte sample array through `sub_1430BAF50/sub_1430BAFB0`, optionally apply `0x1430B8280`, build a bridge packet with `0x142E65B20`, and submit via `0x1430B8640`. Their immediate callers (`0x142CC1020`, `0x142CC0E30`, `0x142F8ABE0`) are brush event/update wrappers that turn input samples into vector stroke state; they are not the saved `.clip` compact-stroke render path. Reopened vector geometry still comes from `0x142554920 -> 0x1422CF5F0 -> 0x1422CBAE0`, where each 92-byte compact point is read into a node and then drawn through `0x14255A7E0 -> 0x1425A4100 -> 0x1422CC1E0`. Thus the residual cannot be fixed by reading a hidden persisted 48-byte sample array for `Vector_OpacityRandom_50`; that array is live input construction state.

Random-opacity AA scan-loop and final-flush rejection: `0x14263FC50` was rechecked at pseudocode level. It computes `inv_aa = 1.0 / aa_width`, `center = ctx_center - 0.5`, `inner = max(0, radius - aa_width)`, scans each touched row with outer `int(cx +/- sqrt(radius^2-y^2)) +/- 1`, writes the inner span at `0x8000`, and only computes `int(((radius - sqrt(dx^2+dy^2)) * inv_aa) * 32768.0)` on the two AA edges before calling `0x14263AC30`. A no-edit exact scan-loop monkeypatch is metric-neutral (`mean=0.122681`, `visible=891`, `max=31`), as were coverage round/ceil/32767 and flow/accumulation rounding variants. Final alpha-byte probes also reject the 32768-to-8-bit flush as the main source: byte `+1` worsens to `mean=0.162112`, byte `-1` only changes noise-level to `0.122662`, and LSB forcing worsens to `0.139931`. The remaining random-opacity residue is not AA row-span, coverage quantization, row accumulation rounding, or final alpha-byte conversion.

Random-opacity sampler endpoint and composite rejection: `0x1422CC1E0` uses `a7+8` as the carried next-sample distance, emits while that value is below the segment length, then stores `next_distance - segment_length` on exit. It can force one endpoint sample only when caller `a6` is true and the current node has no side chain; ordinary `0x1425A4100` passes `a6 = runtime_style+120 & 0x20`, and this fixture's `StyleFlag=0x1c240` does not set `0x20`. In-memory endpoint-inclusive loop probes are exactly neutral. The point vtable was also rechecked: `0x1422C9C80` linearly interpolates x/y as `a2/a3`, `0x1422C96D0` measures Euclidean segment length, and compact reader `0x1422CBAE0` reads x/y doubles directly before storing floats for the other point channels. Finally, the reference PNG is fully opaque after paper compositing; channel diffs are RGB-only, but normal paper composite variants (`float64`/integer `/255` round/floor) show only noise-level `0.122662` or regressions. The remaining residue is therefore still upstream stroke coverage/coordinate distribution, not endpoint phase, hidden curve interpolation, compact x/y conversion, or final normal composite rounding.

Random-opacity evaluator size/flow/opacity sensitivity rejection: a no-edit scalar grid confirms the kept plot parameters are locally stiff. Spacing scale changes of only `0.5%` regress strongly (`mean=0.189787` or `0.219450`), opacity cap `+/-0.5%` regresses to about `0.168..0.171`, and flow decreases regress while flow increases are neutral because the opacity cap dominates. Tiny radius nudges can find only local metric noise (`radius +0.01px -> mean=0.122213`; earlier `+0.005px` was `0.121969`), but IDA does not support such an adjustment: `0x1422D8550` writes effective size directly to plot `+0`, and `0x14263F410` stores that value at ctx `+0x1c0` before calling the circular branch. `plot+24` / thickness only switches to stretched/rotated helpers when the scale leaves `1.0 +/- 1e-8`; this fixture has `ThicknessBase=1.0`, so the ordinary circular branch calls `0x1422DBF30(ctx+0x1d0, ctx+0x1c0)` and `0x14263FC50` without an additive radius term. Keep the radius/spacing/opacity formulas native-backed rather than encoding the micro-radius overfit.

Random-opacity compact `f32+76` carry rejection: the current fixture's compact point `f32+76` values look spacing-like (`0.286..1.832`), and a no-edit probe that restarts each segment with the segment-start `f32+76` improves the metric to `mean=0.114825`, `visible=834`, `max=30`. Native evidence still rejects using it as active spacing state. `0x1422CBAE0` reads compact `f32+76` into internal point `+0x6c`, but active `0x1422CC1E0(a8=1)` writes the shared `a7+8` carry into the first point at entry and then uses only `a7+8` for the loop's `v42` distance. Sink slot `+0x10` (`0x14255C510`) writes segment-init factors only to sample context `+56/+64`; sink slot `+0x18` (`0x14255C440`) rescales step only when a secondary resource exists; sink slot `+0x20` (`0x14255C980`) dispatches to the ordinary draw loop. The side emitter `0x1422CCA10` is gated by point/list bit `a1[8] & 2`, absent in this fixture, and even there it uses compact `f32+64/+68/+72` for offset stepping rather than `f32+76`. Treat the `f32+76` probe as another residual-shape clue, not a native-backed importer rule.

Random-opacity sample-channel rejection: after reconnecting IDA, `0x142568040` was rechecked at pseudocode level. Effector bits `0x10/0x20/0x40` read graph inputs from sample context `+0x10/+0x18/+0x20`, bit `0x100` reads context `+0x30`, but the random branch is only `a3 && signed(flags byte) < 0`: it advances `state = 1103515245 * state + 1234567` and multiplies by `floor + (1-floor) * ((HIWORD(state)&0x7fff)/32768.0)`. The current `OpacityEffector=0x81` therefore has no hidden `f32+36/+40/+44` input. A no-edit in-memory probe confirmed the metrics: multiplying the current native random opacity by interpolated compact `+36`, `+40`, or `+44` worsens `Vector_OpacityRandom_50` to mean `9.518062`, `4.595756`, or `1.630631` respectively, while `+52/+56/+60` are neutral because they are all `1.0` in this fixture. Keep the importer rule as random-floor-only; the remaining residue is not a missing sample-channel opacity multiplier.

Random-opacity draw-scale rejection: the no-pattern queue path really does scale draw-local centres and sizes by `draw+264` before submitting a dab. The setup is now mapped more tightly: `0x142558580` calls `sub_142611310/0x142611320` on the target draw object, passing the returned doubles to `0x14255D370`, which stores them at `draw+264/+272` only when they are in `(0, 0.9999999899999999)`; otherwise it writes exactly `1.0`. `0x14255DFE0` then uses `draw+264` for x, y, and effective size in the ordinary no-pattern branch before `0x14260F550 -> 0x14260DB90 -> 0x14263F410`. The saved fixture's vector layer has `MipmapInfo` scale `100.0` with offscreen size `200x200` for layer id `5`, plus a secondary `50.0` / `100x100` mipmap that is not the full-size PNG target. A forced in-memory draw-scale probe shows why this is tempting but rejected: scale `0.9995` improves the metric to `mean=0.107400`, but scale `0.999` already ties/regresses (`0.122737`), `0.9975` is bad (`0.308325`), and the real half-scale mip target is very wrong (`15.537525`). Because the native full-size path clamps scale to `1.0` and the current oracle is the `200x200` PNG, do not encode a sub-1 draw-scale fix.

Random-opacity color/mix descriptor rejection: the active saved-stroke route reaches `0x1425A4100`, which first submits the primary list through `0x1422CC1E0(..., a3=0, a8=1, a9=0, a10=0)` and only submits a secondary `a3=1` pass when `sub_1422DC6F0` returns a secondary style/resource. For the current `Vector_OpacityRandom_50` style, `0x1422D8140` sees `StyleFlag=0x1c240`: bit `0x10000` is set and `0x20000` is clear, so descriptor `+0/+8/+16/+24` for SubColor/Hue/Saturation/Value are forced to zero before any color-change evaluation. Because `UseWaterColor` is also `0`, descriptor `+48/+56` for MixColor/MixAlpha are forced to `1.0/1.0`; `BrushColorMixingMode` and `BrushLMSLinearity` copy as zero on the primary path. The consumer `0x1425597A0` therefore has no HSV/sub-color delta to apply, no partial MixColor branch (`+48 < 0.99999999`) to enter, and MixAlpha `+56=1.0` selects the normal fallback/full-alpha side rather than sampled-canvas alpha. The remaining random-opacity residue is not a hidden color-mix, watercolor, blur, or secondary-descriptor adjustment.

Random-opacity pen-head/profile rejection: `0x14263F410` was rechecked after the color/mix pass. The current style stores `Hardness=1.0`, so the `ctx+0x1d4` softness/profile-mask branch at `0x14263F410` lines that call `0x142664050/0x142663A40` is skipped. `ThicknessBase=1.0` leaves `v15` within `1.0 +/- 1e-8`, so the stretched/rotated helpers `0x142640420/0x142640C90` and narrow fallback `0x1426427D0` are skipped as well. Because `AntiAlias=2`, the active draw helper is the circular AA path `0x14263FC50` with `0x1422DBF30` returning `min(radius, 2.5)`, not the hard circular `0x142640150` span with its `sqrt(radius^2-dy^2)-0.4` shrink. Therefore the remaining random-opacity residue is not a hidden hardness mask, stretched pen-head profile, or hard-span geometry rule; it stays in ordinary circular-AA sample coverage/distribution.

Random-opacity sampler-state initializer confirmation: the bridge initializer `0x1425A4B20` resolves the last ambiguity about initial random state and interval carry. It writes primary sampler state `a2+0 = *(u32 *)(stroke+160)`, `a2+4 = 0`, and `a2+8 = 0.0`, then writes the secondary sampler state at `a3` as one LCG advance from the same seed with `a3+8 = 0.0`. The saved-stroke route calls this as `0x1430B8AF0/0x1430B9BE0 -> 0x1425A4B20(stroke, bridge+40, bridge+72)`, and `0x1425A4100` submits the current primary list through `0x1422CC1E0(..., a7=bridge+40, a8=1, a3=0)`. At `0x1422CC1E0` entry, `a8=1` copies that primary state into the first point (`point+0x70/+0x74` and float carry `+0x6c`) before sampling. Because the current fixture has no active secondary style/resource, the importer should seed the primary walk from compact stroke tail `u32+88` / native `stroke+160`, start distance carry at `0.0`, and advance the LCG inside the opacity effector before using each emitted dab. This confirms the current random phase and initial carry; the residue is not a hidden pre-advance or saved nonzero carry.

Random-opacity draw-flag / first-sample skip rejection: `0x1422CC1E0` passes its per-sample `v45` flag to sink vtable `+0x20`; in the active sink `0x14255C980 -> 0x14255DFE0`, that flag becomes `a4`, and the ordinary no-pattern branch only queues a dab when `a4` is true. The only native first-sample suppression in this path is gated by caller predicate `sub_1422DD7A0(style)`: it returns true when style/list bytes `+624` or `+648` have bit `0x40`, or when `(StyleFlag & 0x30) == 0x30`. The current style has `StyleFlag=0x1c240`, so `0x30` is clear, and it has no active optional secondary/list evidence for the `+624/+648` gates. Therefore every ordinary emitted primary sample keeps `v45=1` and reaches the queue; the previously tested forced first-sample skip remains a rejected mismatch, not a native rule for this fixture.

Random-opacity interval writeback confirmation: `0x1422D8550` writes the next sampler distance to `a8+8` before evaluating opacity. In the ordinary interval path, it evaluates `IntervalBase/Effector` at `style+0x1a0/+0x1a8`, applies the `StyleFlag&0x8000` caller-flag cap only when the sampler-provided `a5`/`v75` flag is false, then writes `a8+8 = 2 * max(0.1, effectiveSize) * interval`. A lower spacing clamp can raise the result to `1.0`, or to `max(0.5, ThicknessBase)` when AA is active and thickness is below `1.0`, but the current random-opacity fixture has `effectiveSize=10`, `IntervalBase=0.1`, `ThicknessBase=1.0`, `AntiAlias=2`, and sampler draw flag true. Therefore the native writeback is exactly `2.0px`, matching the importer feedback loop; the remaining residue is not an interval clamp, `StyleFlag&0x8000` cap, or hidden AutoIntervalType `4/5` path.

Random-opacity row-writer recheck: `0x14263AC30` was re-read against the current native-random branch. For the active no-water/no-material/no-hardness case, it computes `flowCoverage = (ctx+0x1e4 * geometricCoverage) >> 15`, treats the passed x-span as inclusive (`count = end - start + 1`), and then either applies direct/max `candidate = (ctx+0x1e0 * flowCoverage) >> 15` when `ctx+0x1dc` is set or the normal build-up `old + ((flowCoverage * (ctx+0x1e0 - old)) >> 15)` when it is clear. Current `StyleFlag=0x1c240` leaves `ctx+0x1dc` clear, `Hardness=1.0` leaves `ctx+0x1d4` clear, and `UseWaterColor=0` leaves the color/material writer-family flags clear. `0x14264BC90` still flushes the layer alpha byte as `(*u16 - 1) >> 7` for empty target pixels. This reconfirms the importer alpha-buffer formula and inclusive row span; the residue is not a swapped row role, half-open span, water/material writer branch, or final alpha flush mode.

Random-opacity build-up clamp confirmation: the row writer wording above is conditional in native code. In the normal `ctx+0x1dc == 0` branch of `0x14263AC30`, it reads old coverage, loads `ctx+0x1e0` as the opacity cap, and only executes `old + ((flowCoverage * (opacityCap - old)) >> 15)` when `old < opacityCap`; if the existing pixel coverage is already at or above the current random opacity cap, it leaves that pixel unchanged. A no-edit monkeypatch that removed this guard and allowed lower-cap dabs to reduce existing alpha regressed `Vector_OpacityRandom_50` from current `mean=0.122681`, `visible=891` to `mean=1.944862`, `visible=3459`, `max=93`. Keep the importer's `max(opacityCap - old, 0)` behaviour; the remaining residue is not a missing downward blend toward lower random caps.

Random-opacity sampler caller and dry-run phase rejection: `0x1425A4100` calls the active primary list as `sub_1422CC1E0(v12[6], sink, a3=0, a4=sub_1422DD7A0(style), a5=style+120&2, a6=style+120&0x20, a7=bridge+40, a8=1, a9=0, a10=0)`. For `StyleFlag=0x1c240`, both `a5` and `a6` are clear, so there is no boundary residual rewrite or forced endpoint pass. The nearby `0x1425A3D60` route does call `sub_1422CC1E0(..., a10=1)` for a no-draw/cache-style traversal, but it uses a local state packet and is not the active pixel route. A no-edit probe that simulated hidden pre-consumption of the random stream made the target much worse (`extra_lcg_shift_14 mean=0.673931`, `extra_lcg_shift_91 mean=0.704325`, `extra_lcg_shift_92 mean=0.684113`, versus current `0.122681`). Therefore the remaining residue is not a dry-run/cache traversal advancing the actual random opacity sequence.

Random-opacity `draw+616` / plot `+88` saved-route rejection: `0x14255DFE0` passes `draw+616` as the evaluator's final argument to `0x1422D8550`, which writes it into plot `+88`; `0x14260F550` copies it to queue `+0x180`, and `0x14263F410` uses it as `a12` to choose the primary or alternate no-pattern row plane. Rechecking the saved-stroke caller shows `sub_1430B8AF0` / `sub_1430B9BE0` pass packet `a2+260` to `sub_142558580`, and in the ordinary render packet built by `0x142E73180` this offset is the high dword of `v458`, initialized to zero and not assigned before the saved-stroke calls. Thus the current `Vector_OpacityRandom_50` PNG route selects the primary plane (`a12=0`); the residual is not a hidden alternate-plane/sample-id branch.

Random-opacity scalar/color/composite and exact-AA recheck: the remaining PNG residual is a pure equal-channel grey delta, so it still corresponds to stroke alpha over paper rather than hue/color mixing. No-edit probes confirm the serialized stroke color and paper color are already correct: forcing line grey `16` or `18` regresses versus the stored `17`, and paper `225`/`227` regresses versus stored `226`. Replacing the vectorized AA coverage with a literal `0x14263FC50` scan-loop implementation is metric-neutral (`mean=0.122681`, visible `891`), and final Normal composite variants also fail to explain the gap: current u8 alpha flush plus integer/round Normal blend is best, while direct 16-bit alpha-to-paper floor/round/ceil variants are worse or only trade signed bias. The remaining residue is therefore not stroke RGB, paper RGB, final layer composite rounding, or a simplified AA scan-loop boundary.

Random-opacity live point-simplification rejection: `sub_1430B8AF0` stores `0.05 / scale` at bridge `+304` and may set bridge `+312`, and `sub_1430B8640` uses `+312` with `sub_1425B0AD0` to simplify/replace live input points based on a three-point angle/distance test. That looked promising because a metric-only `x -= 0.10` dab-center probe improves the target (`mean=0.094219` versus current `0.122681`). Rechecking xrefs shows this simplification path is reached from live sample-array builders such as `0x142E762F0` / `0x142E77A40`, not from the saved compact-stroke render. The reopened `.clip` route still builds compact points through `0x1422CBAE0`, uses Euclidean segment length `0x1422C96D0 -> 0x141A8E410`, linear interpolation `0x1422C9C80`, and submits via `0x1425A4100 -> 0x1422CC1E0`. Keep the `x -= 0.10` result as a residual-shape diagnostic only; do not implement live bridge simplification or `0.05/scale` offsets for saved random opacity.

Random-opacity active coordinate/region gate recheck: the reconnected IDA pseudocode keeps the ordinary no-pattern submission simple. `0x14255DFE0` subtracts integer draw origin fields `draw+24/+28`, multiplies x/y/size by `draw+264`, then passes raw centre doubles through `0x14260F550`; the queue copy preserves those doubles until `0x14260DB90 -> 0x14263F410 -> 0x14263FC50`. The only rounded-centre path in this area is `0x14255C2A0`, which does a coarse draw-region touch test using rounded x/y plus `ceil(radius)` and does not crop or quantize the dab coverage. The post-reconnect scoped gate `tmp_vector_probe/native_rng_post_reconnect_gate_v1.json` still passes (`Vector_OpacityRandom_50` mean delta `-1.375688`, visible delta `-1959`, all six guards unchanged). The remaining residue is therefore not ordinary queue quantization, draw-region clipping, or an extra active coordinate transform between sink and row writer.

Random-opacity post-compaction resume: after reopening the investigation thread, both IDA MCP sessions were reachable again (`CLIPStudioPaint.exe` on `127.0.0.1:13337`, `iswCoreTG.dll` on `127.0.0.1:13338`). The syntax check still passes, and the scoped random gate `tmp_vector_probe/native_rng_resume_after_compaction_gate_v1.json` preserves the current result: `Vector_OpacityRandom_50 mean_delta=-1.375688`, `visible_delta=-1959`, while `Vector_OpacityPressure`, `Test_Vector`, `Vector_AA_None`, `Vector_AA_Medium`, `Vector_Texture`, and `Vector_NoTexture` remain unchanged. A broad native-dab/adaptive probe for neighbouring opacity/size pressure samples shows the row path is directionally useful (`Vector_OpacityPressure 0.122786 -> 0.108165`, `Vector_SizePressure` and AA guards also improve), but it is still not a default-safe claim about full `OpacityEffector=0x31`. Re-decompiling `0x142568040` reconfirms the native effector math: flag `0x10` multiplies the base by graph `+0x28` from sample `+0x10`, flag `0x20` applies the range lane from graph `+0x38` / sample `+0x18`, flag `0x40` multiplies from sample `+0x20`, random advances the LCG only under the signed high-bit branch, and flag `0x100` multiplies by sample `+0x30`. Therefore the kept random-opacity rule remains file-seeded `0x81` plus native dab feedback, and the pressure-opacity gap remains a broader dab/profile/coverage problem rather than an effector-formula or random-persistence problem.

Pressure-opacity native-dab rejection pass: `Vector_OpacityPressure` is a useful non-random hard-dab control (`OpacityEffector=0x31`, `AutoIntervalType=2`, `AntiAlias=0`, `Hardness=1.0`, `StyleFlag=0x1c240`). Several tempting ways to improve the broad native-dab experiment are still rejected. Forcing linear centres for every non-random experimental dab improves some AA/size guards but worsens pressure opacity (`0.108165 -> 0.109101`), so the earlier random-only linear sampler correction should not be broadened from this evidence. Skipping the first pressure dab is only a tiny metric improvement (`0.107426`) and IDA rejects it: `sub_1422DD7A0` only suppresses the first sample when `style+0x270/+0x288 & 0x40` or `(StyleFlag & 0x30) == 0x30`, neither of which holds for `0x1c240`. Direct/max versus build-up is neutral for this sample, while radius scale, forced AA, and centre-offset sweeps all keep the native values best (`radius_scale=1.0`, `AA=0`, `dx=0,dy=0`). Re-reading `0x142640150` confirms the hard circular span uses x centre `ctx+0x1b0`, y centre `ctx+0x1b8 - 0.5`, `sqrt(radius^2-dy^2)-0.4`, and full `0x8000` coverage, matching the importer hard path. Finally, smaller AutoIntervalType-2 scalars such as `0.05` can slightly improve the pressure metric (`0.104878`), but `0x1422DBF80` proves the native type-2 scalar is exactly `0.08 * (3.5 - max(hardness,0.2)*2.5)`, giving `0.08` for `Hardness=1.0`; treat the smaller interval as an overfit, not a renderer rule.

Random-opacity double/int precision rejection: a focused no-edit probe tested whether the remaining random-opacity residue is caused by Python double versus native float/int conversion. It is not. With the current scoped native-random path, `Vector_OpacityRandom_50` stays at `mean=0.122681`, `visible=891`, `max=31`. Forcing centre floor-to-int worsens to `0.893719`, round-to-int worsens to `0.314269`, and `int+0.5` worsens to `0.267731`; centre `float32`, radius `float32`, opacity `float32`, and flow `float32` are exactly neutral. Flooring the opacity cap to a 32768 grid gives only a tiny metric nudge (`0.122606`, visible `890`) and contradicts native evidence: `0x14263F410` rounds opacity/flow caps with `+0.5000000100000001`, while `0x142568040` normalizes random as high-15 bits divided by `32768.0`. Keep the current double/raw-centre and native rounded fixed-point behaviour; the remaining residue is not a simple double/int cast bug.

Random/pressure residual distribution split: `tmp_vector_probe/residual_distribution_random_pressure_v1.json` shows the current random target and the broad pressure probe fail in different shapes. `Vector_OpacityRandom_50` has only `1195` active residual pixels and almost solved total alpha (`out-ref=-594.19`), with most absolute error on circular-AA edge pixels (`inside_edge abs=7285.19`, `just_outside px=15`) and mixed early/late dab signs (`first_10pct=-740.60`, `last_10pct=+275.74`). In contrast, the broad `Vector_OpacityPressure` native-dab probe has a much larger alpha deficit (`out-ref=-107113.54`) concentrated in deep interior hard-dab pixels (`inside_deep px=24771`, `sum=-104121.24`). Therefore the remaining random residue should not be chased as the same bug as pressure's broad `0x31` opacity/profile gap; random is a small edge/coordinate distribution problem, while pressure still needs the larger opacity/flow dab model solved.

Random-opacity AA profile/grid rejection: a no-edit AA coverage profile grid around the current native-random result only found a tiny metric-only nudge from `gamma=1.0`, `scale=1.005`, `bias=0` (`mean 0.122362`, visible `887` versus current `0.122681`, visible `891`). Other gamma, bias, and coverage-shape changes were worse, and the improvement is too small and unsupported by native code. Fresh IDA rechecks keep `0x1422DBF30` as `AA 1/2/3 -> min(radius, 1.5/2.5/3.5)` and `0x14263FC50` as a linear edge profile using `ctx_center - 0.5`, `1.0 / aa_width`, full `0x8000` inner spans, and `int(coverage * 32768.0)` before `0x14263AC30` applies the native build-up row formula. Keep the linear AA profile and current cap/flow rounding; do not encode the `1.005` overfit.

Random-opacity per-dab residual direction probe: `tmp_vector_probe/native_rng_per_dab_residual_direction_v1.json` captures the `91` native-random dabs and assigns the remaining inferred-alpha residual pixels to their nearest circular-AA dab. The old global offset clue is reproduced (`dx=-0.10`, `dy=+0.02` gives `mean=0.093000`, `visible=780`), but adding a tiny progress ramp improves the metric further (`dx1=0.08`, `dy1=0.08` around that offset gives `mean=0.079331`, `visible=650`). A tangent/normal split in `native_rng_tangent_normal_offset_probe_v1.json` makes the shape clearer: pure local tangent/normal offsets only reach `mean=0.097294`, while the best extra term on top of the translation is a progress-dependent normal ramp (`normal_ramp=0.16`, `mean=0.071231`, `visible=619`). This points away from sampler phase/spacing as the main residue and toward a tiny upstream projection/centreline alignment difference. It remains diagnostic only because the native queue path still copies raw centres and no persisted transform has been found.

Random-opacity compact bbox centre rejection: `tmp_vector_probe/native_rng_point_bbox_center_probe_v1.json` tests whether the saved point bbox fields hide the better centreline implied by the normal-ramp diagnostic. They do not. Reprojecting dabs through the raw compact point polyline is neutral (`mean=0.122681`), but replacing centres with interpolated point bbox centres regresses to `mean=0.921750`, bbox top-left/right-bottom radius proxies explode to about `13`, and even a half blend toward bbox centres regresses to `0.680025`. Therefore the remaining alignment clue is not the compact point bbox stream or another obvious persisted centre in the 92-byte point records.

Random-opacity queue/centre IDA recheck after the normal-ramp clue: `0x14260F550` copies the plot centre block to queue `+600`, `0x14260DB90` forwards that pair, and `0x14263F410` writes `*a2` / `a2[1]` directly into ctx `+432/+440` before selecting the circular path. `0x14263FC50` then applies only the known `ctx_center - 0.5` convention. This recheck gives no native-backed place to add the metric-only translation/normal-ramp correction in the importer. The remaining random-opacity task is now narrower: find the upstream projection/compiled-line or export-space reason for this tiny centreline mismatch, not tweak RNG, AA profile, row accumulation, or compact bbox centres.

Random-opacity point-vtable correction: the earlier base-vtable conclusion was superseded by the stroke flag allocator. `0x1422D0510` selects the point allocator from the stroke flag byte, and the current `0x2081` low byte is signed/high-bit, so the active node family is `PWVectorSplineCurve` from `sub_142567A00`, not the plain `PWVectorCurve` vtable at `0x1444DE3C0`. The spline vtable at `0x1444efc50` uses `sub_143200940` for position sampling and `sub_143200C90`-style distance-to-parameter mapping; `sub_143204EA0` limits far neighbour controls. The importer now mirrors the native projection more closely: `+0x70` first uses quarter-point rough length to choose a clamped subdivision count, then `+0x68` maps the current carry distance back to spline `t` using `int(length * 0.25)` subdivisions. The scoped gate `tmp_vector_probe/native_rng_spline_native_projection_gate_v1.json` improves `Vector_OpacityRandom_50` to `mean=0.035175`, `visible=274`, `max=12` while `Vector_OpacityPressure`, `Test_Vector`, `Vector_AA_None`, `Vector_AA_Medium`, `Vector_Texture`, and `Vector_NoTexture` remain unchanged. The double/int probe remains rejected; the large missing piece was the saved stroke's spline point family and native distance projection.

Random-opacity stroke/list reader tail recheck: `0x1422CF5F0` reads stroke/list headers, allocates the list object with `0x142552AF0 -> 0x1422D0510`, seeks to the header tail, calls the list virtual `+0x100`, then loops over each point and calls `0x1422CBAE0`. `0x1422D0510` selects the point allocator from the stroke flag byte (`0x2081` takes the signed/high-bit family through `sub_142567A00`), but the point reader still writes x/y doubles directly to node `+8/+16`. The observed `+0x100` list-tail virtuals copy or mark auxiliary list state; they do not rewrite the already-unread point coordinates, and the point loop happens afterwards. This further rejects a hidden compiled-line centreline rewrite in the saved reader path.

Random-opacity spline point-flag fix: the remaining `274`-pixel residue was the `PWVectorSplineCurve` point flag rule, not double/int precision. `0x142626B80` / `0x142626EE0` check node `+64 & 1` while collecting previous/next spline controls: when the current point is flagged, the previous-side controls collapse to the current point; when the next point is flagged, the next-side controls collapse to the next point, and flagged neighbour points also stop the second-neighbour lookup. The target compact stroke has point index `2` with compact `+32 flags=0x1`, so ignoring this bit bent the native six-point projection slightly. The importer now reads compact point `+32` into `point_flags` and passes it into `_native_spline_segment_controls`. The native no-pattern `OpacityEffector=0x81` path is enabled by default rather than hidden behind the experimental env flags; the env flags remain for broader brush experiments. The scoped gate `tmp_vector_probe/native_rng_spline_point_flags_gate_v1.json` renders `Vector_OpacityRandom_50` pixel-exact (`max=0`, `mean=0.0`, `visible=0`) while `Vector_OpacityPressure`, `Test_Vector`, `Vector_AA_None`, `Vector_AA_Medium`, `Vector_Texture`, and `Vector_NoTexture` remain unchanged.

Flow/opacity pressure native-dab promotion: the no-pattern `0x2081` path now uses the native dab/spacing loop by default rather than only under `RIZUM_CLIP_EXPERIMENTAL_VECTOR_DAB`. The adaptive feedback centre is always sampled from `PWVectorSplineCurve` controls, not Catmull-Rom, which matches `0x1422CC1E0` for the signed/high-bit point family. This makes the flow targets deterministic without environment flags: `Vector_FlowPressure_50`, `Vector_Flow_50`, and `Vector_Baseline` all render pixel-exact (`max=0`, `mean=0.0`, `visible=0`), and `Vector_Opacity_50` is down to a single non-visible byte (`max=1`, `mean=0.000013`, `visible=0`). The old random-opacity exactness is preserved.

Opacity-pressure `0x31` effector correction: IDA recheck of `0x142CB44D0`, `0x142569450`, `0x1425695F0`, `0x142CB5470`, `0x142CB5410`, and `0x142568040` resolves the compact range lane. Compact `OpacityEffector=0x31` with tail `amount1=5.0` compiles its `0x20` lane as a native range multiplier with low `100%` and high `500%`, then multiplies the primary graph lane. The importer now evaluates that as `primary * (1.0 + (amount1 - 1.0) * secondary)` and removed the prior `0.29` blend, `1.72` alpha scale, `0.88` power, and `0.05` opacity floor heuristics. With the default native dab path, `Vector_OpacityPressure` improves from `mean=0.122786`, `visible=18175`, `max=218` to `mean=0.003591`, `visible=516`, `max=4`.

Pressure-effector supersession note: earlier entries that describe `SizeEffector=0x31` as point `+36` with a tuned radius base, `OpacityEffector=0x31` as a `0.29` secondary-lane blend, or the second graph lane as parsed-but-disabled are historical preview-era notes. The current native-backed state is the per-emitted-sample range-lane formula above and the `Vector_SizePressure` status in the following entries.

Opacity-pressure sample-channel resolution: the last `Vector_OpacityPressure` residual was evaluation order, not double/int precision. Native `0x1422CC1E0` first interpolates compact point channels into the sample context, then `0x1422D8550 -> 0x142568040` evaluates `OpacityEffector=0x31` from sample `+0x10/+0x18`. The importer was evaluating the `0x31` effector at each endpoint and then interpolating the already-clamped opacity result, which is not equivalent for the native range lane. The native no-pattern path now carries raw primary/secondary scalars through adaptive spacing and evaluates `OpacityEffector` per emitted sample. Result: `Vector_OpacityPressure`, `Vector_FlowPressure_50`, `Vector_Flow_50`, `Vector_Baseline`, and `Vector_OpacityRandom_50` are pixel-exact; `Vector_Opacity_50` remains only one non-visible byte (`max=1`, `visible=0`).

Size-pressure `0x31` follow-up: the same sample-channel rule is native-backed for `SizeEffector=0x31`. `0x1422D8550` evaluates the size effector from the interpolated sample context, multiplies by sample `+0x38`, writes the result directly to plot `+0`, and derives the next spacing as `2 * max(0.1, effectiveSize) * interval_scalar`. For the current style, `AutoIntervalType=2`, `Hardness=1.0`, and `0x1422DBF80` give interval scalar `0.08`; `AntiAlias=0`, `ThicknessBase=1.0`, and `StyleFlag=0x1c240` keep the hard circular path with no stretch/profile branch. Moving `SizeEffector=0x31` evaluation from endpoint interpolation to per-emitted-sample evaluation improves `Vector_SizePressure` but does not finish it: current result is `max=226`, `mean=0.024409`, `visible=151`, with all residual pixels output-only on the vector layer.

Size-pressure native rejection set: the remaining `151` pixels are not explained by PSD compositing, bbox clipping, simple radius shrink, scalar interpolation, or interval tuning. The CSP PSD export contains a separate vector layer (`Layer 1`) with bbox `[170,142,533,493]`; comparing the importer vector layer to that PSD layer shows `extra=151`, `missing=0`, so the mismatch is real vector alpha rather than paper/final composite. Clipping to the PSD bbox only removes `31` outside-bbox pixels and leaves `120` inside; stroke header bbox, point pen-bbox union, segment union, and interpolated point-bbox clips are neutral or regress. IDA confirms `0x142640150` hard spans use `y_center = ctx_y - 0.5` and `sqrt(radius^2 - dy^2) - 0.4`, while rect helpers use floor/ceil plus expansion, so a narrower native rect or `-0.5` span shrink is not supported. The tempting scalar-linear probe improves the size metric but breaks exact `Vector_OpacityPressure`; rechecking `0x1422CC1E0` shows the active main loop uses spline distance-to-parameter `v44` for sample-channel interpolation, so scalar-linear stays rejected.

Size-pressure residual attribution: `tmp_vector_probe/sizepressure_residual_per_dab_v1.json` assigns the remaining output-only pixels to the native hard-dab stream. The residual is boundary-like and concentrated late in the stroke: dabs `220..226` cover most of the tail extras, with dab `226` the largest last contributor (`15` pixels) and dab `223` the largest any-contributor (`40` pixels). The rest is scattered along the stroke edge. Span endpoint probes reject half-open or one-sided row spans, which regress to `369..619` visible pixels; `0x142640150`'s inclusive `x0..x1` interpretation is therefore correct. A metric-only span shrink (`-0.45` instead of native `-0.4`) reduces visible pixels to `110` but already introduces missing reference pixels and contradicts IDA, so it stays rejected.

Size-pressure spline/feedback recheck: IDA `0x143200940`, `0x143200C90`, `0x143204EA0`, `0x142626B80`, and `0x142626EE0` reconfirm the six-point `PWVectorSplineCurve` interpolation and the distance-to-parameter loop. `0x143200C90` uses `int(length * 0.25)` clamped to `4..255` for the t search. `0x1431FFEC0` adds one nuance that the importer now mirrors: for length calculation, native first computes quarter-point rough length and returns it directly when `int(rough / 4) <= 4`; only longer spans are resampled, with the step count clamped to `255`. This native length correction is neutral on `Vector_SizePressure` and preserves exact `OpacityRandom`, `OpacityPressure`, `FlowPressure`, `Flow`, and `Baseline`, but it removes a non-native short-span approximation from the feedback path.

Size-pressure small-radius branch recheck: the apparent `v19 < 1.0` branch in `0x1422D8550` only promotes radius to `1.0` and folds the size into flow when either AA (`v20`) or the hardness/profile path (`v26`) is active. For this fixture `AntiAlias=0`, `Hardness=1.0`, and the relevant style bit leaves `v26=0`, so sub-1px size-pressure dabs stay true subpixel hard dabs. Skipping early tiny dabs, forcing a first-sample offset, or resetting segment residuals gives at most tiny metric changes and is not a native-backed fix for the remaining `151` pixels.

Size-pressure base-size cast trace: `draw+104`, the base size argument later passed to `0x1422D8550`, is returned by the `PWVectorStroke` vtable slot `+0x80` (`0x14243E100`), which simply reads stroke `+152`. That field is set by the bridge setup through vtable slot `+0x88` (`0x1419D8FA0`). In the ordinary packet builder path, the packet scalar at `+0x10` is supplied by `sub_142E65B20`; the callsite around `0x142E75B9A` converts caller field `[rsi+0x18c]` from int to double before passing it. This confirms a native int-to-double size ingress exists, but it does not yet explain the current SizePressure residue: the fixture's effective base width is already `20.0`, and earlier radius-scale/cast probes either stayed neutral or introduced missing reference pixels. Keep the remaining work on size feedback / effector semantics rather than a generic float-vs-int center or radius cast.

Size-pressure segment-init and clip/effectors recheck: reconnecting to IDA after the compaction keeps `CLIPStudioPaint.exe` reachable on `127.0.0.1:13337`. The tempting `0x14255C510 -> 0x1422D9200` segment-initializer path is not active for this fixture: the stroke object flags are `0x2081`, so `0x1422CC1E0` does not take the `a1[8] & 0x10` segment-context branch, and every compact point has `u32+32 == 0`, so the point `0x20` sink-init branch is also clear. The no-pattern draw clip was also rechecked: `0x14260F550` copies the queue default rect to queue `+580` when the ordinary caller passes no explicit rect, and `0x14263F410` merely intersects each dab footprint with that rect before `0x142640150`; a PSD-vector-layer bbox clip was already proven to remove only `31/151` extras. Finally, `0x142568040` reconfirms `SizeEffector=0x31` lane semantics: the primary `0x10` lane compiles to the existing compact "amount 1.0 -> graph-only" behavior, and the `0x20` range lane multiplies by `low + (high-low) * graph(sample+0x18)`, matching the importer `1.0 + (amount1 - 1.0) * secondary` form for this compact blob. The residue is therefore not segment-init size/flow rewrites, draw-region clipping, or a reversed range-lane multiplier.

Size-pressure graph and small-profile recheck: `0x1423E8640 -> 0x1431F7F10 -> 0x1431F6480` matches the importer's `BrushEffectorGraphData` evaluator: two-point graphs are linear, longer graphs use midpoint-bounded quadratic spans, and the quadratic root solver uses the same `1e-8` discriminant/root acceptance scale. No-edit probes confirm this is not just syntactic agreement: forcing graph `6` to piecewise-linear worsens `Vector_SizePressure` to `visible=246`, nearest-point worsens to `visible=266`, and removing the size final clamp is neutral (`visible=151`) because the current evaluated size multiplier stays below `1.0`. Re-reading `0x1422D8550` also keeps the small/profile branches inactive: `Hardness=1.0` leaves `v26=0`, `AntiAlias=0` leaves `v20=0`, `StyleFlag=0x1c240` lacks the `0x8` and `0x1004==4` cases, and `ThicknessBase=1.0` keeps `0x14263F410` on the circular hard path. Changing the t-search epsilon to native `1e-8` is metric-neutral, while the deliberately wrong short-length interpretation regresses to `visible=330`. The remaining `151` pixels are still a radius/feedback distribution problem, not graph interpolation, final clamp, small-radius promotion, or t epsilon.

Size-pressure sampler flag recheck: `0x1422CC1E0` does contain native `0x1000/0x2000` fixed-vs-interpolated logic for sampler fields `+0x38/+0x40` (compact point floats `+56/+60`), and the current stroke object flags include `0x2000`. For `Vector_SizePressure`, however, the compact data makes this branch value-neutral: all 29 compact points have `f32+56 == 1.0`, `f32+60 == 1.0`, and `u32+32 == 0`; the diagnostic `point_stats` also keeps `f32+52 == 1.0` and `f32+64/+68/+72/+76 == 0.0`. Therefore the sampler flag path cannot shrink the current hard-dab footprint or change the feedback step for this fixture. The residue remains in the effective-size feedback / hard-span distribution after `0x1422D8550`, not in the `+56/+60` channel interpolation rule.

Size-pressure post-resume geometry probes: the current worktree still renders `Vector_SizePressure` as `max=226`, `mean=0.024409`, `visible=151`, so the residual is stable. A no-edit hard-dab monkeypatch confirms the metric has a strong shrink-shaped overfit but no native support: changing only the hard span constant from native `sqrt(...)-0.4` to `-0.45` gives `visible=110`, `-0.5` gives `visible=93`, and radius scales around `0.9975` give about `107` visible pixels. Small centre offsets are mostly neutral or worse (`dx/dy` probes stay around `149..164` visible). Re-decompiling `0x142640150` keeps the native code unambiguous: `ctx+0x1c0` is squared as the radius, y uses `ctx_y - 0.5`, x uses raw `ctx_x`, span is `fmax(0.0, sqrt(radius^2 - dy^2) - 0.4)`, and the row writer receives inclusive `x0..x1` at full `0x8000` coverage. Therefore the shrink probes remain diagnostic only.

Size-pressure feedback/projection probes: spline length and distance-to-t variants are not the remaining cause. Rough-only length, always-4, always-255, ceil/round rough-step, always-resample, strict `>` target comparison, native `1e-8` segment epsilon, and t-search step-count variants all leave `visible=151` or worsen by one pixel. Float32 quantization of the pressure-effector inputs, graph outputs, intermediate branches, or final factor is also neutral. The `PWVectorSplineCurve` control limiter is active only three times in this stroke, including one tail-side control; no-limit and one-level variants are neutral, while the best non-native variant only nudges `visible=148`. Skipping early/late dabs or sub-3px dabs likewise gives only tiny changes (`148..151`) and does not match a native predicate. The remaining gap is therefore not spline precision, float-vs-double effector math, first/last sample emission, or the recovered control-collapse rule.

Size-pressure queue-rect recheck: the active no-pattern path does pass an explicit per-plot rect through `0x14255C680 -> 0x14260F550`, not just a queue default. `0x14255C680` computes `[cx-r, cy-r, cx+r, cy+r]`, converts it through `0x14206C980` (`floor(left/top)`, `ceil(right/bottom)`), optionally splits at the nearest 256-pixel tile boundary, and stores that rect at queue `+580`; `0x14263F410` later intersects its own expanded footprint rect with it. A no-edit importer simulation of that exact floor/ceil queue clip is metric-neutral (`visible=151`), so the current missing rule is not the ordinary queue rect. Only non-native negative rect expansion/shrink variants improve the metric (`visible=133..145`), matching the earlier shrink-shaped overfit and staying rejected.

Size-pressure post-compaction field audit: re-running the current importer still gives `Vector_SizePressure max=226`, `mean=0.024409`, `visible=151`, so the blocker is stable. A fresh IDA pass over `0x1422D8550`, `0x14255DFE0`, `0x14260F550`, `0x14260DB90`, `0x14263F410`, and `0x142640150` keeps the active field chain direct: `0x1422D8550` computes `v19 = evaluated_size * sample+0x38`, writes that same value to plot `+0`, and writes spacing carry as `2 * max(0.1, v19) * interval`; `0x14255DFE0` only applies draw scale/origin before queueing; `0x14260F550` copies plot `+0/+8/+16/+44/+88`; `0x14263F410` stores radius at ctx `+0x1c0`; and the hard row writer consumes that radius unchanged. This rules out a hidden queue-side radius shrink or a separate plot-radius field for the remaining hard-edge extras.

Size-pressure sampler carry audit: `0x1422CC1E0` matches the importer's carry model. It starts `v42` from sampler state `+8`, maps distance to spline `t` through vtable `+104`, calls the sink vtable `+32`, then adds the sink-updated `state+8` to `v42`; when the segment is exhausted it writes back `v42 - segment_length`. With current caller flags from `0x1425A4100` (`a5=0`, `a6=0`, `a8=1`, `a9=0`, `a10=0`) there is no endpoint forcing, visibility bbox gate, dry-run random consumption, or first-sample skip. The residue is therefore not a reversed residual formula or a skipped native endpoint.

Size-pressure channel/formula probe: a no-edit source-exec probe replaced the `SizeEffector=0x31` raw sample channels and range-lane formula while rendering only `Vector_SizePressure`. The native-backed `primary=f32+36`, `secondary=f32+40` path remains the best result (`visible=151`). Secondary fields `+52/+56/+60` or removing the secondary lane all worsen to `visible=266`; secondary `+36` worsens to `421`; secondary `+44/+48/+64/+68/+72/+76` worsen to about `3140`; primary fields other than `+36` regress from hundreds to tens of thousands of pixels. Formula variants also regress: half secondary gives `154`, square `265`, division `506`, inverse `3071`. This closes the likely compact-channel and range-multiplier explanations for the remaining size-pressure edge.

Size-dynamics corpus check: sibling size-dynamics fixtures are not a single solved family yet. `Vector_GlobalPressure_Default` remains pixel-exact, and `Vector_SizeTilt_50` is near exact (`max=2`, `visible=14`), but `Vector_SizeRandom_50` is still broad (`mean=0.694688`, `visible=869`) and `Vector_SizeVelocity_50` remains non-exact (`mean=0.126456`, `visible=272`). For the current goal this means `SizePressure` should still be treated as the hard `0x31` pressure-size feedback target, while random/velocity size branches need their own native evidence rather than being used to justify a generic radius shrink.

Size-pressure descriptor/dual-pass audit: `0x14255DFE0`'s pre-submit loop was rechecked because it can iterate twice depending on `sub_1425590B0`. `sub_1425590B0` is a draw-buffer/queue state gate, not a BrushStyle evaluator; for this hard/full-opacity no-pattern fixture a second pass would not change the binary shape even if it occurred. `sub_1425597A0` consumes the descriptor from `0x1422D8140`, but the active branches are colour/mix/alpha consumers: for this style `StyleFlag=0x1c240` and `UseWaterColor=0` keep sub-colour/HSV/mix defaults, and no centre, radius, spacing, or hard-span field is rewritten after `0x1422D8550`. This rejects descriptor or dual-submit logic as the cause of the remaining SizePressure hard-edge extras.

Size-pressure point-reader/cache audit: `0x1422CBAE0` maps the saved 92-byte point record directly into the native node: compact x/y doubles to node `+8/+16`, bbox ints to `+24..+36`, compact flags `+32` to node `+64`, floats `+36/+40/+44/+48/+52/+56/+60/+64/+68/+72/+76` to node `+68/+72/+76/+80/+84/+88/+92/+96/+100/+104/+108`, and trailing dwords `+80/+84` to `+112/+116`. The active sampler then uses only node `+68..+92` for the main sample context; the current fixture has compact `+64..+76 == 0.0`, so no hidden size input lives there. The layer metadata also has no mask/offset (`LayerLayerMaskMipmap=0`, all layer offsets zero). The vector layer's render mipmap points to external ids not present in the `.clip` external bodies, so there is no decodable stored render cache to use as a shortcut oracle; CSP must regenerate the vector layer.

Size-pressure numeric micro-probes: replacing `np.hypot` with literal native-shaped `sqrt(dx*dx+dy*dy)` in the spline length/t-search path is neutral (`visible=151`). Raising the segment epsilon to `1e-8`, storing residual carry as `float32`, and adding `float32(next_step)` are also neutral. Together with the previous rough-length/t-search sweeps, this rules out the remaining gap being a Python `hypot`, float-carry, or one-more-ULP residual issue.

Size-pressure compiler and tail-phase recheck: after reconnecting to IDA, `0x142B757B0` confirms the concrete compiler slot for the current brush: `SizeEffector` is read through parameter id `base+0x3e9` (`1001`) and lowered into runtime `PWBrushStyle+0x80`; `0x142CB45F0` / `0x142569330` / `0x1425694D0` then install the `0x10` primary graph lane and `0x20` range lane, while `0x142CB5E80` can only add the separate velocity flag `0x100` if the keyed parameter map contains id `1001`. The current file exposes only the `SizeEffector` blob `0x31, graph5, amount0=1.0, graph6, amount1=1.5`, and the sibling exact `Vector_OpacityPressure` fixture depends on the same `0x31` range-lane semantics with its own high value, so changing the generic `0x31` decode is rejected. Metric-only sweeps reinforce that point: reducing the size secondary high to `1.4` improves `Vector_SizePressure` to `visible=94` but immediately breaks `Vector_OpacityPressure`; interval/secondary-high combinations stay overfits with missing pixels or no native backing.

Size-pressure phase/previous-step rejection: source-exec probes that changed the spacing feedback to use the previous effective size or primary-only spacing improve only to `visible=145`, while interval scalar sweeps bottom out around `visible=137..143` and radius/phase combinations around `visible=99` with missing pixels. IDA assembly for `0x1422CC1E0` resolves the tempting previous-size idea: the loop calls the sink vtable `+0x20` first, then executes `addsd xmm8, qword ptr [state+8]`, so the next sample advances by the newly written current-size interval, matching the importer `walk += next_step` ordering. Therefore the remaining residue is not a one-sample delayed spacing update, a simple initial residual, or a size-pressure-specific interval scalar.

Size-pressure submit-gate and row-clip recheck: `0x14255C2A0 -> 0x142619200` is a whole-dab coverage/bounds gate, not a per-pixel hard-edge shrinker. It rounds the dab centre to integer space, checks the current draw-region structure, and returns a rect/offset used by `0x14260F550`; for the current `Vector_SizePressure` stream all 233 importer dabs pass the document and PSD-vector-bbox approximations, and the tail dabs stay inside the active bbox. The related `0x1422CC1E0` `v76` path can make an initial sample feedback-only when `sub_1422DD7A0` is true, but no-edit probes skipping the first 1..3 dabs or the final sub-1px dabs are metric-neutral (`visible=151`), so this is not the missing predicate. Re-implementing `0x142640150`'s exact block-rect row clipping (`floor/ceil`, `sub_14206CBB0` expansion, and `v13..v12-1` span clipping) is also neutral. Finally, secondary-input overfits such as `sample+0x18 += 0.01` or scaling it upward reduce the visible count (`127` / `115`) but introduce no native explanation and contradict `0x142568040`, whose `0x20` lane reads only sample context `+0x18` with no offset or `+0x20` mix. Keep the remaining search on a native-backed effective-size/feedback nuance, not on coverage gates, row clipping, first-dab skips, or biased secondary scalars.

Size-pressure sampler-state initialization recheck: the saved vector bridge `0x1430B9BE0 -> 0x142558580 -> 0x1425A4B20 -> 0x14255A7E0` initializes the sampler feedback states immediately before geometry submission. `sub_1425A4B20` writes the primary state as `seed, 0, 0.0` at `a1+40` and the secondary/shared state as `LCG(seed), 0, 0.0` at `a1+72`; `0x14255A7E0 -> 0x1425A4100` then passes those state blocks to `0x1422CC1E0`. This confirms the importer starting `residual_distance = 0.0` for the ordinary saved primary path is native-backed, and the remaining residue is not a hidden preseeded carry from the draw bridge.

Size-pressure secondary-width/vtable recheck: `0x14255D810` stores primary draw width at `draw+104` from the stroke vtable slot `+0x80` (`0x14243E100`, returning stroke `+152`) and also reads vtable slot `+0x70` into a local value that later becomes primary writer-state metadata. For the observed vector-stroke vtables, the `+0x70` slot returns stroke `+120` or subclass fields such as `+136`, while the neighboring setter slots write those same secondary fields; it is not the active SizePressure base-size source. `0x14255C980 -> 0x14255DFE0` passes the sampler's `a3` through as the primary/secondary selector: only `a3 != 0` replaces `v11 = draw+104` with `draw+112` before calling `0x1422D8550`. The saved primary route for this fixture is `a3=0`, so the active size argument remains `20.0` from `draw+104`. This closes the reopened double/int and secondary-width branch for the current `151` output-only hard-edge pixels.

Size-pressure sampler-phase diagnostics: IDA `0x1422CC1E0` was re-read around the active sample-context construction. It linearly interpolates node `+68/+72/+76/+84` into sample `+0x10/+0x18/+0x20/+0x30`, and interpolates node `+88` into sample `+0x38` unless the native `0x1000` fixed-field flag combination overrides it; the current stroke has only `0x2000`, and compact `+56/+60` are all `1.0`, so the recovered SizeEffector inputs stay unchanged. The caller is still `0x1425A4100(... a4=sub_1422DD7A0(style), a5=StyleFlag&2, a6=StyleFlag&0x20, a8=1, a9=0, a10=0)`, and `sub_1422DD7A0` requires runtime `+624/+648 & 0x40` or `(StyleFlag&0x30)==0x30`; the SQLite style `0x1c240` does not satisfy the flag case and has no optional list evidence for the other gates. No-edit probes back this up: casting native dab centres to integer/floor/round worsens (`visible=310..431` with missing pixels), while mild metric improvements from initial residual (`visible=145` at `3.0px`) or tangent shifts (`visible=135` at `+0.5px` with no missing) are phase-shaped diagnostics only. They do not override the native zero-carry initialization, raw-centre queueing, or recovered sampler predicate.

Size-pressure post-reopen attribution and graph/t-return recheck: `tmp_vector_probe/sizepressure_extra_dab_attribution_v3.json` captures all `233` current hard dabs and shows every one of the remaining `151` pixels is output-only and first-changed exactly once (`change_count_hist_extra=[[1,151]]`). Tail segment `26` dominates (`extra_changed=53`, dabs `220..226`, radius range about `9.69..18.28`), with smaller edge clusters on earlier segments. A diagnostic that interpolates size inputs by arc-distance fraction instead of native spline `t` improves the metric (`visible=105`) but remains rejected: `0x1422CC1E0` stores the vtable `+0x68` return in the sample-channel interpolation path, and `0x142626EE0 -> 0x143200C90` returns the recovered spline parameter `t` after writing the sampled point to `a9`. Re-reading `0x1423E8640 -> 0x1431F7F10` also confirms the graph evaluator matches the importer's midpoint-bounded quadratic spans; graph-shape is not the missing rule. Finally, `0x1422CBAE0` point read tail for the active `PWVectorSplineCurve` vtable resolves `+0xd8` to `_guard_check_icall_nop`, so there is no hidden saved-point postprocess after compact x/y/field mapping.

Size-pressure redundant-dab and tile-split recheck: no-edit probes that skip individual tail dabs confirm the residual has redundancy but not a native predicate yet. Skipping dab `221`, `223`, `224`, `226`, or `104` removes one to three visible extras without missing pixels, and skipping `221..224` reduces the metric to `visible=131` with `missing=0`; skipping the wider `220..226` range improves extras but creates `49` missing pixels. This is useful attribution only: IDA `0x14255C2A0` is a whole-dab draw-region gate and does not test accumulated alpha/coverage before submitting hard rows. A separate explicit queue-rect simulation also leaves `visible=151` for no rect, exact `[floor/ceil(cx +/- r)]` rect, one-pixel expanded rect, and the `0x14255C680` 256-tile split shape. The residue is therefore not native tile splitting, explicit plot rect clipping, or an accumulated-alpha skip.

Size-pressure style-flag immediate audit: targeted IDA immediate searches over the brush/vector render bands show no active `StyleFlag & 0x4000` branch in the current no-pattern size path. The only nearby `0x4000` bit operation is inside `0x14266FE20`, where it controls a byte mask/stencil threshold loop, not `PWBrushStyle+0x78`. `StyleFlag & 0x8000` appears in `0x1422D8550` and `0x1422E04C0`; in the active `0x1422D8550` path it only decides whether caller argument `a5` may suppress the normal `fmin(0.25, interval)` cap. `Vector_SizePressure` uses `AutoIntervalType=2` with scalar `0.08`, already below that cap, and the spray/pattern post-adjuster `0x1422E04C0` is not on this plain hard no-pattern route. This closes the reopened `0x4000/0x8000` style-bit shortcut.

Size-pressure radius/centre quantisation rejection: `tmp_vector_probe/sizepressure_radius_center_quant_probe_v1.json` tested the remaining simple numeric-cast suspects around `_draw_native_dab_rgba`. Radius-only, centre-only, and combined float32 casts, `nextafter(radius, -inf)`, radius floor/round at `1/256`, `1/1024`, and `1/65536`, centre floor/round at `1/256`, and all-floor `1/256` all produce exactly the current metrics (`visible=151`, `extra=151`, `missing=0`, `mean=0.024409`, `max=226`). Together with the IDA-backed hard-row recheck (`0x142640150` keeps raw x, `y-0.5`, `sqrt(radius^2-dy^2)-0.4`, and inclusive full-coverage spans), this rejects a generic double/int, float32, or saved-radius quantisation explanation for the remaining hard-edge extras.

Size-pressure draw-flag and plot-field recheck: `0x1425A4100` calls `0x1422CC1E0` with `a4=sub_1422DD7A0(style)`, `a5=StyleFlag&2`, `a6=StyleFlag&0x20`, `a8=1`, `a9=0`, and `a10=0`. In `0x1422CC1E0`, the per-sample submit flag `v45` defaults true and is only cleared by the already-rejected first-sample predicate when `sub_1422DD7A0` is true; current `StyleFlag=0x1c240` does not satisfy `(StyleFlag&0x30)==0x30`, and there is no evidence for the optional list bytes `+624/+648 & 0x40`. Re-reading `0x1422D8550 -> 0x14260F550 -> 0x14260DB90` also confirms there is no split between spacing size and raster radius: `0x1422D8550` writes the same effective size to plot `+0` and the next-distance state, `0x14260F550` copies plot `+0` to queue `+296`, and `0x14260DB90` passes queue `+296` as the `0x14263F410` radius argument. The remaining SizePressure pixels are therefore not caused by a hidden no-draw flag or a separate queue-side footprint size.

Size-pressure second-submit gate recheck: `tmp_vector_probe/sizepressure_second_submit_gate_probe_v1.json` closes the saved-bridge `a6=1` concern for this fixture. `0x1430B9BE0` does call `0x14255A7E0(..., a6=1)`, so `0x1425A4100` first submits the container's first node (`v12[6]`, list `+48`) and then submits the list container itself. However, `0x1422CE210` and `0x1422CF5F0` link actual stroke nodes through node `+40`, while the container first-node/last-node/count fields live at `+48/+56/+64`, so the container submit is not a second pass over the curve body. With no next segment, `0x1422CC1E0` only emits its endpoint fallback when sampler carry `state+8` is within `+/-1e-8`; the current primary submit exits with carry `0.007134543929300019`. Therefore the list-body submit is inert for `Vector_SizePressure`, and the remaining hard-edge extras are not a missing second draw pass or reopen-time random/list divergence.

Size-pressure region-bitmask recheck: the earlier "pure bbox" summary for `0x14255C2A0 -> 0x142619200` was too coarse. Reconnected IDA shows `0x142619200` intersects the rounded-centre/ceil-radius bbox with region bounds, then calls `0x14261D190(a1, a1+392 mask, a1+408 mask, rounded_x - offset_x, 1, 1, 1, 0, ...)`, whose entry checks a centre bit in one mask, requires the corresponding allowed/source bit, and updates mask bits while producing an output rect. The packet path can carry a non-null region pair: `sub_142E73180` stores the `sub_142E81580` result into the packet fields later read by `0x1430B9BE0` as `a2+240/+248`. However, simple importer-side analogues do not match the remaining residue: skipping duplicate rounded centres skips 20 dabs but is exactly neutral (`visible=151`), while accumulated-alpha centre, coarse disk, or coarse rect occupancy gates skip most dabs and regress to `visible=786..1706`. The native region object is therefore real, but the current SizePressure gap is not a simple accumulated-coverage or duplicate-centre dab skip.

Size-pressure draw-suppression predicate recheck: `sub_1422DD7A0` is not an unresolved optional-list mystery for this fixture. IDA shows it returns true only when `style+624 & 0x40`, `style+648 & 0x40`, or `(StyleFlag & 0x30) == 0x30`. The runtime constructor `0x1422D7240` and clone/compiler copier `0x1422DA100` place `RotationEffector` at `+624` and `RotationEffectorInSpray` at `+648`; the SQLite style for `Vector_SizePressure` has both fields as `3`, not `0x40`, and `StyleFlag=0x1c240` does not satisfy the `0x30` case. Therefore the first-sample feedback-only path in `0x1422CC1E0` is definitely off for this saved stroke, and the remaining `151` output-only hard-edge pixels are not caused by a hidden no-draw sample predicate.

Size-pressure `0x31` range-lane parser recheck: reconnecting to `CLIPStudioPaint.exe.i64` and following `0x142F028B0` shows the named-dynamics parser rounds percent-like doubles into integer fields, but the relevant helpers keep the compact range lane as low `settings+24` and high `settings+28`. `sub_142CB53E0` only accepts the `0x20` high field and clamps it to `100..1000`, while `sub_142CB5410` clamps the low field to `<=100`; enabling `0x20` also defaults the high field to `500`. This supports the current `primary * (1.0 + (amount1 - 1.0) * secondary)` importer formula for the saved `amount1=1.5`, and rejects the metric-only `1.4` secondary-high overfit.

Size-pressure region-parameter audit: the `0x142619200 -> 0x14261D190` gate really uses the two region bitplanes at context `+392/+408`, and `0x142E81580` can build those planes from document parameters including `5111`, `5112`, `5120`, and `0x139c`. A direct SQLite/blob scan of `Vector_SizePressure.clip` found no `5111`, `5112`, or `0x139c` entries; only ordinary `Offscreen.Attribute` blobs contain `5120`. That makes a file-backed special region unlikely for this fixture. The region object remains real native machinery, but the next SizePressure search should not assume a hidden saved crop/region setting unless a later packet trace proves the fields are non-default.

Size-pressure whole-dab gate closure: a fresh IDA pass through `0x14255D810`, `0x142558580`, and `0x1430B9BE0` shows the region gate is optional draw-context state, not an unconditional ordinary-stroke rule. `sub_14255C2A0` returns true immediately when `draw+288 == 0`; `sub_14255D810` only enables the split-queue path when `!draw+288`; and the saved bridge passes the optional region pair through `a2+240/+248` into `sub_142558580`, which copies it to `draw+288/+296`. A no-edit hard-dab union analysis also rejects a skipped-dab explanation: the current 233 dab masks form exactly the reference stroke plus the 151 extra dark pixels (`union-ref=151`, `ref-union=0`), duplicate rounded-centre skipping is metric-neutral, and greedily removing every dab that can be removed without creating a missing reference pixel still leaves 80 extra pixels after 39 skipped dabs. Therefore the remaining residue is not a native centre-bit, duplicate, or accumulated whole-dab gate; it is still a continuous effective-radius / sample-distribution mismatch.

Size-pressure spacing-floor and tail-feedback audit: a no-edit source-exec probe tested hard minimum feedback steps from `0.16` through `3.0` pixels. Small floors improve only marginally (`visible=148` at `0.16`, `147` at `0.25..0.5`) and strong floors still do not solve SizePressure while breaking guards (`floor=3.0` gives `Vector_SizePressure visible=141`, but `Vector_FlowPressure_50 visible=951`, `Vector_Flow_50 visible=3399`, and `Vector_Baseline visible=729`). This rejects a generic hard spacing floor. A separate runtime trace in `tmp_vector_probe/sizepressure_tail_feedback_audit_v1.json` captures all `233` current feedback samples and records the tail segment `26` as seven dabs (`220..226`) with residual-in `1.079194244`, radius/effective size decreasing `18.275993590 -> 9.694965362`, and feedback step decreasing `2.924158974 -> 1.551194458`. Consecutive centre distances differ from the previous emitted step by only about `-0.090..+0.085px`, which is consistent with spline arc-length inversion tolerance rather than a missing sample or a one-dab delayed feedback rule. The remaining target is still a native-backed continuous size/footprint nuance, not a discrete spacing floor or obvious tail phase error.

Size-pressure draw-scale closure: the ordinary no-pattern draw loop does multiply local x/y/radius by `PWBrushDraw+264` before `0x14260F550`, but this does not justify the metric-only `radius_scale=0.997` shrink. IDA resolves the setup chain as `0x142558580 -> 0x142611310/0x142611320 -> 0x14255D370`: OffscreenSet `+432` becomes `PWBrushDraw+264`, OffscreenSet `+440` becomes `PWBrushDraw+272`, and `0x14255D370` clamps each to exactly `1.0` unless the value is in `(1e-8, 0.9999999899999999)`. The saved-route setter seen in `0x142E73180` is `0x142614C20(qword_1454C2538, sub_1424AEAB0(v261))`; `0x142614C20` writes only OffscreenSet `+440`, while `0x1424AEAB0` returns `1.0 / (1 << v261)`. Therefore this native path can affect `draw+272` with power-of-two secondary/offscreen scale values, but it does not create a `0.997` primary radius scale at `draw+264`; OffscreenSet `+432` defaults to `1.0` in `0x142610220` for the current full-size saved vector target. Continue treating `0.997` as a useful residual-shape probe, not an importer rule.

Size-pressure thickness/axis closure: fresh IDA decompiles saved in the reverse workspace as `tmp_ida_sizeeffector/0x1422D8550_thickness_recheck_decompile.json`, `0x14260DB90_thickness_recheck_decompile.json`, and `0x14263F410_thickness_recheck_decompile.json` close the `StyleFlag&0x40` suspicion for this fixture. `0x1422D8550` derives plot `+24` from runtime thickness ratio `v33`; because the current BrushStyle has `ThicknessBase=1.0` and default `ThicknessEffector=0x01`, that value remains exactly `1.0`. The queue copies it to `+320`, and `0x14263F410` only enters `0x142640420/0x142640C90/0x1426427D0` when the ratio leaves `1.0 +/- 1e-8`. Thus `Vector_SizePressure` still reaches the circular hard helper `0x142640150`; the `native_thickness_axis_split_0x40=true` diagnostic is only an orientation capability bit, not an active elliptical footprint for this sample.

Size-pressure graph-root ordering recheck: `0x1431F6480` returns quadratic roots in native order `(-b - sqrt)/(2a)` then `(-b + sqrt)/(2a)`, while the importer had tested them in the opposite order. A source-exec probe that changed the importer graph helper to native root ordering and native `0 <= t < 1` acceptance was exactly neutral: `Vector_SizePressure` stayed `visible=151`, and `Vector_OpacityPressure`, `Vector_OpacityRandom_50`, `Vector_FlowPressure_50`, `Vector_Flow_50`, and `Vector_Baseline` stayed pixel-exact. For current graph5/graph6, the midpoint-bounded quadratic spans are effectively monotonic in x, so the root-order detail is a real native nuance but not the missing SizePressure rule.

Size-pressure queue `+648` / `0.3*size` state audit (superseded by the `context+628` closure below): an external long-context helper suggested `0x14260DB90` might contain a size-dependent pre-flush crop. IDA rejects the simple crop interpretation but leaves a narrower conditional rect-helper path. The branch tests queue `+648` and chooses either `queue+296 * 0.3` when `queue+464` is set or `queue+472` otherwise, then rounds that value into `0x142643BF0`. `0x142643BF0` writes draw-context `+632/+636/+640` only when context `+628` is enabled. `0x14263F410` consults `context+632` through `0x14266E1E0`, which adjusts/intersects the integer dab/block rect; it is not the hard circular pixel formula in `0x142640150`. Later IDA evidence resolved `context+628` as the water-color row-writer mode flag, and current SizePressure leaves it off.

Size-pressure spline-length short-span audit: `sub_1431FFEC0` has a misleading Hex-Rays return-type artifact around the rough-length shortcut. The decompile appears to return `int(rough/4)` for short spans, but the caller expects the double result in `xmm0`, and after `cvttsd2si` the rough length remains in `xmm0` on that path. No-edit importer probes confirm the current rough/then-refine model: a short-bucket-length variant regresses `Vector_SizePressure` to about `visible=330` and breaks exact guards badly; `ceil_refine_length` is neutral for SizePressure but causes small guard diffs; `t_steps_from_rough` is exactly neutral. Do not reopen spline short-span bucket length unless new native evidence contradicts this.

Size-pressure `context+628` rect-helper closure: the optional queue branch at `0x14260DB90` is real but inactive for the current saved `Vector_SizePressure` route. IDA still shows only one code xref to `0x142643BF0`, from `0x14260DB90`, and `0x142643BF0` merely stores `context+632/+636/+640` when `context+628` is already set. The missing nuance is that `context+628` is not an unknown SizePressure gate: `0x142644180` writes `context+616 = mode != 0`, `context+628 = mode == 2`, and `context+624 = mode == 3`, where the mode comes from the writer-state descriptor built from `UseWaterColor` and `WaterColorType`. Existing row-writer notes already map `UseWaterColor=0` to mode `0`, and the current vector pressure samples keep `UseWaterColor=0`. Therefore `0x14266E1E0` can remain documented as a future water-color/material rect helper, but it cannot explain the present `151` output-only hard-edge pixels and should not be implemented as a SizePressure crop/shrink rule.

Size-pressure sampler precision closure: a no-edit source-exec probe forced the remaining feedback/evaluator boundary values through float32 one at a time: segment residual carry, walk initialisation, returned spline parameter `t`, interpolated primary/secondary size scalars, feedback point x/y, effective size, next step, and an all-core combination. Every variant stayed exactly at the current `Vector_SizePressure` metric (`max=226`, `mean=0.024409`, `visible=151`) and did not explain the tail hard-edge extras. IDA still shows `0x1422CC1E0` carrying distance in `a7+8` as a double and `0x1422D8550` writing the next distance as a double, so there is no native evidence for a float truncation rule. The graph evaluator root order, however, is a real native detail: `0x1431F6480` tests `(-b - sqrt)/(2a)` before `(-b + sqrt)/(2a)`. The importer now matches that order in both `clip_loader.py` copies; verification remains unchanged (`Vector_SizePressure visible=151`; `Vector_OpacityPressure`, `Vector_OpacityRandom_50`, `Vector_FlowPressure_50`, `Vector_Flow_50`, and `Vector_Baseline` exact; `Vector_Opacity_50` one non-visible byte).

Size-pressure tail-only phase rejection: a long-context helper independently pointed back at tail segment `26` density, segment residual, and hard-span integer boundaries as the only plausible remaining suspects. A focused no-edit probe then perturbed only segment `26`: adding length deltas from `-1.0..+1.0px`, multiplying its length by `0.95..1.05`, and shifting its incoming walk/residual by `-1.0..+1.0px`. These variants do not explain the residual shape: almost all stay at `visible=151`, the best length-only case (`distance - 1.0px`) reaches only `visible=150`, and positive walk shifts regress to `152..153`. This rejects a tail-only segment-length or one-segment residual-threshold explanation. The remaining search should stay broader: a native-backed continuous footprint/sample-distribution nuance that applies across the stream while preserving the exact baseline/flow/opacity guards.

Size-pressure sink/cache/graph-return recheck: a fresh static pass revalidated the active submission boundary without changing importer code. `sub_1422CC1E0`'s sampler `0x1000/0x2000` fixed-field logic is value-neutral for this fixture: the object flag includes `0x2000`, but all compact point `u32+32` flags are zero and compact `f32+56/+60` are both constant `1.0`, so size/flow feedback stay unchanged. The sink vtable method `0x14255C980` only chooses the ordinary no-pattern draw loop (`0x14255DFE0`) for the primary saved route; the segment-init methods `0x14255C510/0x14255C440` are gated by inactive object/point flags or secondary-style routing. Re-reading `0x1431F7F10` at assembly level also clarifies the graph evaluator return artifact: after `0x1431F6480` writes up to two quadratic roots, native computes y values for valid roots and returns the first valid y in `xmm0`; this matches the importer's native root order for current graph5/graph6. Finally, `LayerRenderMipmap` is not a shortcut oracle here because `_decode_mipmap_rgba` returns no decodable cache body for `Vector_SizePressure`, `Vector_OpacityPressure`, or `Vector_Baseline`; the PNG targets are regenerated vector output, not embedded cached rasters.

Size-pressure PSD-layer residual triage update: direct comparison against the PSD `Layer 1` hard mask remains the cleanest oracle: current importer mask `15905`, PSD mask `15754`, `extra=151`, `missing=0`, bbox `[170,142,533,493]`. New no-edit probes close three more tempting families. First, the single-shrink explanations are insufficient: overriding `amount1` bottoms out around `1.3995` with `visible=92`, hard-span shrink `0.4 -> 0.5` bottoms out around `visible=93`, and a combined `amount1/span` grid still stays above `90` visible pixels while introducing missing pixels in many cases. These are only residual-shape diagnostics. Second, the `SizeEffector` second-lane source is not the missing mapping: forcing graph6 to read primary, constants, inverted secondary, squared secondary, or sqrt secondary is worse than the current compact `f32+40` / sample `+0x18` source, matching `sub_142568040`. Third, the remaining residue is not a whole-dab skip predicate. A greedy set-cover probe over the 233 current hard dab masks can remove 39 dabs without creating missing PSD pixels, but it still leaves 80 extras; duplicate-centre, accumulated-alpha, and simple skipped-dab explanations are therefore too weak. The open problem remains a native-backed continuous size/footprint or sample-distribution nuance, not a single scalar, span constant, lane-source remap, or dab gate.

Size-pressure hard-row and field-map closure: live IDA MCP checks on `127.0.0.1:13337` and `13338` both reached `CLIPStudioPaint.exe` for this pass, so port identity must still be verified each session. The hard no-AA row path is now closed against the current importer: `sub_142640150` uses raw x, `y - 0.5`, `sqrt(radius^2 - dy^2) - 0.4`, inclusive spans, and full coverage; `sub_14263F410` keeps the circular hard handoff; `sub_14263AC30` matches the 16-bit build-up/direct-max paths; and final alpha flush remains `(*u16 - 1) >> 7`. Therefore the remaining `151` output-only pixels are not row coverage, alpha conversion, or final composite. Re-reading `sub_1422CBAE0` also resolves the compact point flags: compact `u32+32` maps to native node `+64`, while compact tail `u32+80/+84` maps to node `+112/+116` and is not a hidden point-flag field; current compact `f32+64..+76` values are zero. Forced linear/all-corner sampling remains rejected: it reduces the SizePressure metric diagnostically but breaks exact guard samples and contradicts the confirmed `0x2081 -> PWVectorSplineCurve` route.

Size-pressure long-context helper suspect audit: a long-context helper proposed `style+408` material opacity, `StyleFlag&0x1004` flow rescaling, endpoint-result SizeEffector interpolation, and per-segment `PWBrushDraw+104/+112` size switching. IDA and no-edit probes reject these as current causes. In `sub_1422D8550`, `style+408` is the runtime hardness/softness scalar (`BrushStyle.Hardness` maps to `style+0x198`), and the current SQLite row has `Hardness=1.0`; therefore `v26` stays false and the `v19 *= (1.5 - v23 * 0.5)` branch is inactive. The flow-rescale guard is exactly `(*(_DWORD *)(style+120) & 0x1004) == 4`; current `StyleFlag=0x1c240` gives `0x1000`, not `4`, so that branch is also inactive. A no-edit source-exec variant that evaluated `SizeEffector=0x31` at segment endpoints and interpolated the already-evaluated result regressed `Vector_SizePressure` from `visible=151` to `156`, matching the native rule that `0x1422CC1E0` interpolates sample context lanes first and `0x1422D8550 -> 0x142568040` evaluates the effector per emitted dab. Finally, `0x14255C980` passes its `a3` selector through to `0x14255DFE0`; the active caller `0x1425A4100 -> 0x1422CC1E0` uses `a3=0`, so the ordinary route uses `PWBrushDraw+104` as the size argument. `PWBrushDraw+112` belongs to the `a3 != 0` secondary-resource path and is not a hidden per-segment radius source for this saved stroke. `0x1422D8140` / `0x1425597A0` were also re-read in this audit and only prepare inactive color/subcolor/mix descriptors for this fixture, not a coordinate or radius transform.

Size-pressure spline-centre and helper audit follow-up: a tempting remaining centreline hypothesis was that `PWVectorSplineCurve` slot `+0x68` might walk a chord/polyline approximation for distance and leave the emitted sample x/y on that approximation while returning only an approximate `t` for the dynamic lanes. Live IDA rejects that. `sub_142626EE0` forwards the six limited spline controls into `sub_143200C90`; `sub_143200C90` walks `int(length * 0.25)` chords only to locate the target distance, computes `t = ((i + 1) - overshoot / chord_len) * step`, then calls `sub_143200940` again at that refined `t` and writes that true spline point to the sample record. The importer already mirrors this shape by calling `_native_spline_t_at_distance` and `_native_spline_point_from_controls` with the same `t`; forced compact-point/output integer variants regress the target (`visible=309..340`), so this is not the remaining `151`-pixel cause. A small global dab-centre shift sweep is also only diagnostic (`+0.2,+0.2` improves `visible=151 -> 147`) and has no active native transform support. A second external long-context audit produced no new active candidates beyond already closed sampler/row/effector facts. As an extra guard on the earlier global-pressure conclusion, `Vector_GlobalPressure_Default.clip` and `Vector_GlobalPressure_StrongCurve.clip` both still verify pixel-exact in the importer, reinforcing that global/device pressure is baked into saved point scalars for these reopened documents rather than applied as a hidden runtime `SizeEffector` curve.

Size-pressure r2 continuation after switching away from IDA MCP: command-line r2 rechecked the active no-pattern submit and spline sampler. In `0x14255DFE0`, both the offset-list and ordinary submit branches multiply draw-local x/y/radius by `PWBrushDraw+0x108` (`draw+264`) before `0x14260F550`; `PWBrushDraw+0x110` (`draw+272`) is only seen scaling a prepared scalar at `0x14255E14C` before colour/mix descriptor handling, not the geometry fields. In `0x1422CC1E0`, the vtable `+0x68` return is copied from `xmm0` to `xmm6` at `0x1422CC5F3..5F6`, and the following lane interpolation at `0x1422CC603..6AB` uses that same `xmm6` for compact fields `+0x44/+0x48/+...`; later point generation also uses the same parameter. r2 of `0x142626EE0 -> 0x143200C90` confirms the spline helper computes `steps=int(length*0.25)` clamped to `4..255`, walks chords only to find the target, calls `0x143200940` again at the refined `t`, stores the true spline point, and returns that `t`. Therefore the arc-distance-fraction interpolation probe remains a shape diagnostic, not native semantics, and `draw+272` remains rejected as the missing `0.997`-style radius shrink.

Size-pressure residual-shape and small-branch probes: a no-edit wrapper over `_brush_pressure_effector_value` recorded `262` SizeEffector calls during the current render, with primary range `0.0..0.937887`, secondary range `0.588889..0.800000`, and evaluated output range `0.0..0.948196`; no call exceeds `1.0`, so the importer's final `<=1.0` clamp is not active for this fixture. A last-changed hard-dab attribution probe again found all `151` visible pixels are output-only and assigned to the current `233` hard dabs, led by dabs `221/223/220/222`. Relative-direction stats show the tail cluster extras mostly lie on the forward/outside boundary of large declining-radius dabs (for dab `221`, median along `+11.59`, median side `+9.94`; for dab `223`, median along `+9.71`, median side `+7.12`), while smaller earlier clusters lie on mixed side boundaries. Simple global clipping is insufficient: clipping output to the dark reference bbox `[170,142,532,492]` leaves `visible=120`, and clipping to the historical PSD bbox `[170,142,533,493]` leaves `visible=136`, both with no missing pixels. A diagnostic AutoInterval scalar sweep gives only small overfit movement (`0.96x -> visible=150`, `1.04x -> visible=143` with `missing=1`), while r2 `0x1422DBF80` still proves native type `2` with hardness `1.0` and thickness `1.0` returns `0.08`. The current blocker remains a native-backed continuous effective-size/sample-distribution nuance, not SizeEffector clamp, bbox clip, `draw+272`, spline-t mismatch, or interval scalar.

Size-pressure caller/point-field re-audit: Windows r2 disassembly rechecked the active setup around `0x1425A4100`, `0x1422CC1E0`, `0x1422DD7A0`, `0x1422CBAE0`, and `0x1422CD520`. `0x1425A4100` calls the sampler as `0x1422CC1E0(list, sink, a3=0, a4=sub_1422DD7A0(style), a5=StyleFlag&2, a6=StyleFlag&0x20, a7=feedback_state, a8=1, a9=0, a10=0)`. Current `StyleFlag=0x1c240` makes `a5/a6` zero, and `sub_1422DD7A0`'s direct style predicate `(StyleFlag&0x30)==0x30` is false; its remaining true cases are runtime `style+0x270/+0x288` bit `0x40`, with no current evidence that they are active. The compact reader still maps x/y directly, compact `u32+32` to node flags `+0x40`, compact `f32+36..+76` to node `+0x44..+0x6c`; the packet copier `0x1422CD520` only fills node `+0x44..+0x5c` from packet doubles `+0x10..+0x40` and toggles `0x1000/0x2000` from packet `+0x50`. For current SizePressure, compact `f32+56/+60` remain constant `1.0`, tail fields are not active radius inputs, and feedback state initialization remains the already-confirmed zero-carry route. A WSL `pdg` attempt returned no usable pseudocode in this session, so the conclusion is based on Windows r2 disassembly rather than decompiler output. Verification remains unchanged: `Vector_SizePressure max=226`, `mean=0.024409`, `visible=151`.

Size-pressure isolation and radius-vs-feedback diagnostics: a 2026-06-02 pass compared the active `BrushStyle` rows for the two 1024 pressure fixtures. `Vector_SizePressure` and exact `Vector_OpacityPressure` share `AntiAlias=0`, `AutoIntervalType=2`, `Hardness=1.0`, `IntervalBase=1.0`, no pattern/spray/watercolor/texture, `ThicknessBase=1.0`, and `StyleFlag=0x1c240`; the meaningful difference is that the dynamic `0x31` effector is installed in `SizeEffector` instead of `OpacityEffector`. That makes generic canvas size, AA=0, auto interval, row writer, pressure graph, and transform explanations unlikely unless they are gated by size affecting radius/spacing. A no-edit probe also rejected a remaining spline-step ambiguity: computing `_native_spline_t_at_distance` steps from rough quarter-length rather than the refined segment length left `Vector_SizePressure` unchanged at `max=226`, `mean=0.024409`, `visible=151`, while `Vector_OpacityPressure`, `Vector_OpacityRandom_50`, `Vector_FlowPressure_50`, `Vector_Flow_50`, and `Vector_Baseline` stayed exact. Size-only diagnostic perturbations show the residual is more radius-sensitive than feedback-sensitive: applying a SizePressure-only radius scale of `0.997` improves to `visible=98`, a SizePressure-only feedback scale of `1.04` improves only to `visible=143`, and combining both lands at `visible=106`; these remain diagnostic shapes, not native semantics. A fresh r2 read of `0x1422D8550` still shows one effective size value: `xmm7` is multiplied by sample `+0x38`, feeds `state+8 = 2 * max(size, 0.1) * interval`, and is later stored directly to plot `+0`. The only apparent plot/feedback split is the sub-1px branch that can force plot size to `1.0` and compensate flow, but current SizePressure dabs are far above that threshold. The remaining high-value search is therefore a native-backed continuous radius/footprint nuance after SizeEffector, not rough/refined spline steps or a separate hidden plot-size shrink in `0x1422D8550`.

Size-pressure hard-boundary residual shape: a follow-up no-edit capture saved `tmp_vector_probe/sizepressure_extra_boundary_margin_probe_v2.json`. It records the 233 emitted hard dabs during the current render, reattributes all 151 visible extra pixels to a containing dab under the current hard-span rule, and computes the integer-boundary radius shrink needed for the extra pixel to fall out of that dab's inclusive `x0..x1` span. All 151 extras are attributable; the shrink-needed distribution is small (`median=0.059421821`, `mean=0.149644013`, `p90=0.462769898`, `max=1.098327456`), with `89/151 <= 0.1px` and `121/151 <= 0.25px`. This explains why tiny radius diagnostics improve the metric while larger global shrink starts trading extras for missing pixels. It is still not native evidence for a radius scale, but it strengthens the current shape diagnosis: the remaining mismatch is concentrated at hard-circle integer boundaries after dynamic size evaluation, not primarily in feedback density or spline t-search.

Size-pressure size-only tail footprint recheck: a follow-up capture corrected the residual wording: all 151 visible pixels are black ink from the importer over CSP paper (`out=(0,0,0,255)`, `ref=(226,226,226,255)`), with no alpha/premul difference. Effector-call alignment shows the first 29 `SizeEffector` calls are point/end setup and the next 233 correspond to emitted dabs. The broader size-family samples show this should be described as size-dynamics-only rather than all-pressure: `Vector_OpacityPressure` remains exact, while `Vector_SizeTilt_50` has 14 visible pixels, `Vector_SizeVelocity_50` has 272, and `Vector_SizeRandom_50` has 869. However, those size-family samples have different AA/interval/effectors, so they are diagnostic context rather than direct evidence for the `SizePressure` 0x31 case. A new per-dab shrink-window probe saved `tmp_vector_probe/sizepressure_per_dab_shrink_window_v1.json`: 78 dabs contribute the 151 extras, and 77 have a shrink window that removes attributed extras without deleting uniquely native black pixels. Many late/tail dabs have `protected=0`, showing the residue sits in overlapped segment-end footprints rather than row accumulation. Several tempting variants were then rejected by no-edit source probes. Segment-tail lookahead emission barely improves SizePressure (`151 -> 147`) and breaks exact guards if applied generically; a native-looking minimum feedback step around `1.0` preserves opacity/flow/baseline but worsens SizePressure (`151 -> 153` at floor `1.0`); adjacent-sample radius drawing (`avg_prev`, `avg_next`, `prev`, `next`) either only reaches `143` with missing pixels or regresses further. Fresh r2 of `0x1422CC1E0` around `0x1422CC5E5..0x1422CC831` reconfirms compact lane interpolation uses the same refined `xmm6` returned by vtable `+0x68`, so arc-distance-fraction interpolation remains only a diagnostic. The remaining open target is still a native-backed size-dynamic footprint/sample-distribution nuance that can make the current hard dabs a strict black-ink superset without changing generic row, graph, or opacity/flow semantics.

Vector gap narrow feedback-floor acceptance: native `0x1422D8550` clamps the
no-pattern dab feedback distance to at least `1.0` before the next emitted dab.
The importer now mirrors this in both loader entrypoints with
`next_step = max(1.0, 2 * max(size, 0.1) * intervalScalar)`. Verification fixed
`Vector_Gap_Narraw.clip` from `max=59`, `mean=0.08195`, `visible=556` to exact
while preserving `Vector_Gap_Normal`, `Vector_Gap_Wide`,
`Vector_Gap_Fixed_50`, `Vector_Baseline`, `Vector_Flow_50`,
`Vector_FlowPressure_50`, `Vector_OpacityRandom_50`,
`Vector_SizeRandom_50`, and `Vector_OpacityPressure` as exact. It also improves
`Vector_SizeVelocity_50` to `visible=113`, but paused
`Vector_SizePressure` now verifies at `visible=153`; this is accepted native
feedback semantics, not the SizePressure solution.

Vector SizeVelocity sub-1px AA promotion acceptance: r2 around
`0x1422D8A42..0x1422D8A6F` shows native handles evaluated radius below `1.0`
differently only when the AA raster flag or softness/profile flag is active:
it multiplies flow by the original small radius, promotes plot radius to
`1.0`, and then later applies any soft-hardness radius expansion. The importer
now mirrors that in both loader entrypoints, including making the AA width at
least `1.0` for the promoted dab. Verification fixes
`Vector_SizeVelocity_50.clip` from `max=73`, `mean=0.030519`, `visible=113` to
exact. Guard results stay unchanged: `Vector_Baseline`, `Vector_Flow_50`,
`Vector_FlowPressure_50`, `Vector_OpacityRandom_50`,
`Vector_SizeRandom_50`, `Vector_OpacityPressure`, and all four gap samples are
exact; `Vector_Hardness_50` remains invisible (`visible=0`), and paused
`Vector_SizePressure` remains `max=226`, `mean=0.024732`, `visible=153`
because its `AntiAlias=0` / `Hardness=1.0` gates are off.

Vector SizeTilt residual rejection note: after the accepted SizeVelocity fix,
`Vector_SizeTilt_50.clip` remains near-exact (`max=2`, `mean=0.001306`,
`visible=14`). The residual pixels are all final paper-composited RGB edge
differences with no final alpha delta, concentrated around the last AA dabs
with radii about `5.86` and `aa_width=2.5`. The `SizeEffector` compact blob is
the known `0x21` form (`low=0.5`, `graph=2`, `high=1.0`, input compact
`f32+40`). Diagnostic probes show that multiplying only `0x21` output by
`0.9999` makes the sample invisible (`max=1`, `visible=0`) without touching
the main guards, but native-sized float32 percentage errors such as
`0.99999998` are neutral and r2 `0x142568040` shows the lane is ordinary
float low/high plus graph input. Treat the `0.9999` result as an overfit shape
signal, not accepted native semantics.

Vector AA compact-120 and opacity-velocity rejection pass: the next isolated
samples were checked after `Vector_SizeVelocity_50` became exact, but no native
rule was accepted. For `Vector_AA_None/Weak/Medium/Strong`, all visible
residual pixels have final alpha `255` like the PNG reference and differ only
in paper-composited RGB. Sweeping compact-120 line radius (`0.55..0.70`),
filled-curve feather base/per-AA, and native-inspired AA widths only trades
error among the four AA levels; no single setting improves the family cleanly.
Changing vector sample quantization from integer cast to raw float, round,
half-pixel, or `-0.5` also fails: some AA metrics improve, but others regress
and `test_Filters_Vector_Text` is harmed. Keep the current compact
`+104/+112` curve-tail preview until the native object-curve geometry is
recovered. For `Vector_OpacityVelocity_50`, final residuals are all output
darker over paper, with the common RGB delta equivalent to about `4.2/255`
too much inferred stroke alpha. However, scoped probes that scale or bias only
`OpacityEffector=0x41` output all worsen mean or visible count. This rejects a
simple 0x41 opacity multiplier/bias; the remaining issue is likely
coverage/sample distribution or needs a transparent layer oracle.

Opacity-velocity PSD oracle acceptance: Rizum exported
`Vector_OpacityVelocity_50.psd`, which turned the old final-composite residual
into a transparent-layer oracle. PSD `Layer 2` is bbox `[28,97,91,167]`, has
`1618` nonzero-alpha pixels, constant RGB `(167,49,64)`, and mean visible alpha
`88.309642`. Before the fix, the importer layer had the same RGB but `4459`
nonzero-alpha pixels with bbox `[28,0,200,167]`; the extra high-velocity tail
had signed alpha `0..5` and explained the final PNG's 1..3 RGB-level darker
stroke. The matching value `5/255 ~= 0.0196` identified the importer-only
`0.02` floor applied to `0x41`/random opacity dynamics, not an `OpacityEffector`
curve or lane-source problem. Changing that floor to `0.0` in both loader
entrypoints makes `Vector_OpacityVelocity_50.clip` pixel-exact and makes the
PSD layer alpha and premultiplied RGB exact (`alpha_diff_px=0`,
`premul_rgb_diff_px=0`). Guard verification stays exact for
`Vector_Baseline`, `Vector_Flow_50`, `Vector_FlowPressure_50`,
`Vector_OpacityPressure`, `Vector_OpacityRandom_50`, and
`Vector_SizeVelocity_50`; paused `Vector_SizePressure` remains unchanged at
current `max=226`, `mean=0.024732`, `visible=153`. Earlier scale/bias/lane-swap
probes remain rejected diagnostics; the accepted conclusion is simply that
native `0x41`/random opacity output is allowed to reach zero.

Size-tilt PSD oracle and no-edit rejection: `Vector_SizeTilt_50.psd` confirms
the remaining 14 final visible pixels are a real transparent-layer alpha edge
mismatch, not a paper/export composite issue. PSD `Layer 2` has bbox
`[16,4,182,173]`, `2911` nonzero-alpha pixels, constant RGB `(167,49,64)`,
and mean visible alpha `228.660941`; compositing that PSD layer over paper
recreates the PNG exactly. The importer layer has the same bbox and RGB but
`2912` nonzero-alpha pixels, with `105` alpha-diff pixels, `25` over the
visible threshold, all importer-high by `+1..+3` and clustered at the lower
right tail bbox `[167,155,182,173]`. Probe grid results: scaling only
`SizeEffector=0x21` output by `0.9999` makes final output invisible
(`max=1`, `visible=0`) but still leaves PSD-layer alpha diffs (`80` pixels,
max `1`); `0.99995` leaves `visible=2`; stronger scaling creates missing
alpha and quickly regresses. AA width scaling, radius scaling, and center shifts
are less clean or regress sharply. r2 at `0x142568040` reaffirms the native
`0x20` range lane: read `sample+0x18`, evaluate graph `+0x38`, compute
`low + (high-low)*graph` from runtime floats `+0x10/+0x14`, and multiply the
running factor. There is no native-backed `0.9999` shrink constant, so the
sample remains a tiny rejected edge residual rather than an accepted importer
change.

Vector AA compact hard-edge fallback acceptance: the AA fixture family is now
split by layer and stroke form. Each file has Paper plus two vector layers. The
first ordinary `0x2011` stroke layer is already matched by the importer
(`Vector_AA_None` exact against PSD, `Weak/Strong` only alpha-1 differences,
`Medium` one alpha-2 pixel). The remaining visible error comes from the second
compact `0x41` dark filled-curve layer, not from missing layer traversal:
instrumenting `_composite_image` shows the second layer is rendered and
composited. The PSD-layer comparison shows alpha-edge mismatch only; both
sides are premultiplied black. A narrow accepted fallback change now applies
only to compact `0x41` dark filled curves with brush AA level `0`: use
`VECTOR_FILLED_CURVE_HARD_RADIUS_SCALE=0.95` and `feather=0`. Verification:
`Vector_AA_None` improves from `max=226`, `mean=0.151164`, `visible=1654` to
`max=226`, `mean=0.062396`, `visible=386`; `Vector_AA_Weak`,
`Vector_AA_Medium`, and `Vector_AA_Strong` are unchanged. Exact guards remain
exact (`Vector_Baseline`, `Vector_Flow_50`, `Vector_FlowPressure_50`,
`Vector_OpacityPressure`, `Vector_OpacityVelocity_50`,
`Vector_OpacityRandom_50`, `Vector_SizeVelocity_50`), while
`Vector_SizeTilt_50` remains `visible=14` and paused `Vector_SizePressure`
remains `visible=153`. Do not generalize this to AA levels 1..3: no-edit
sweeps for those levels trade mean/visible wins and losses, and compact
`+104/+112` curve-tail geometry remains the likely unrecovered native detail.

Vector AA compact sampled-point phase refinement: dumping the compact 120-byte
point records showed that `+88/+96` looks like an incoming handle candidate
(`point0 +88/+96 == p0`, `point1 +104/+112 == p1`), but a standard cubic
`p0,out0,in1,p1` is much worse than the current degenerate
`p0,p0,out0,p1` preview for every AA fixture. The accepted follow-up is
therefore sampled-point phase, not handle remapping. For compact `0x41` dark
filled curves, AA level `0` keeps integer sampled points but uses denser
sampling plus a `+0.25,+0.25` phase; AA levels `1..3` use raw float sampled
points, denser sampling, and a `-0.25,-0.25` phase while retaining their
existing radius/feather. Full-PNG verification improves the family again:
`Vector_AA_None` `visible=386 -> 226`, `Vector_AA_Weak` `1936 -> 1760`,
`Vector_AA_Medium` `2184 -> 1827`, and `Vector_AA_Strong` `2587 -> 2244`.
Exact guards remain exact (`Vector_Baseline`, `Vector_Flow_50`,
`Vector_FlowPressure_50`, `Vector_OpacityPressure`,
`Vector_OpacityVelocity_50`, `Vector_OpacityRandom_50`,
`Vector_SizeVelocity_50`), while `Vector_SizeTilt_50` remains `visible=14`,
paused `Vector_SizePressure` remains `visible=153`, and `Test_Vector` /
`test_Filters_Vector_Text` are unchanged. Treat this as a narrow PSD-backed
fallback refinement; native compact curve geometry is still open.

Vector texture remap acceptance: after the AA compact pass, the next clean
non-exact target was texture. `Vector_OpacityTilt_50` verifies as non-visible
byte noise (`max=1`, `visible=0`), while `Vector_Texture` and
`Vector_Texture_50` still had large alpha/coverage residuals. PSD-layer
comparison confirms the texture samples have correct black premultiplied RGB;
the mismatch is alpha only. `Vector_NoTexture` retains a separate 141-pixel
hard-edge residual, so the texture question is isolated to texture coverage
preview. Generic density/gamma and tile-phase probes remain rejected, but the
native texture notes already map `TextureFlag & 0x100` to inversion and
`TextureFlag & 0x200` to the average-baseline remap path in `0x142664760` /
`0x1426642D0`. A no-edit formula probe showed the remap-family coverage shape
is a stable improvement for the current `TextureComposite=0`, single-channel
resource: `factor = 1 + density * 3.0 * (textureByte - textureAverage)`.
Accepting that only when `TextureFlag & 0x200` improves `Vector_Texture` from
`max=226`, `mean=0.833431`, `visible=25352` to `max=226`, `mean=0.571271`,
`visible=11683`, and `Vector_Texture_50` from `max=182`, `mean=0.797644`,
`visible=3114` to `max=182`, `mean=0.709181`, `visible=2106`. Guards remain
stable: `Vector_Baseline`, `Vector_Flow_50`, `Vector_FlowPressure_50`,
`Vector_OpacityPressure`, `Vector_OpacityVelocity_50`,
`Vector_OpacityRandom_50`, and `Vector_SizeVelocity_50` are exact;
`Vector_NoTexture`, `Vector_BrushTip_Material`,
`Vector_BrushTip_Material_Gap`, and `test_Filters_Vector_Text` are unchanged.
This is still a preview bridge, not exact native texture rendering; exact
coverage likely needs the full row-writer sampler/footprint.

Superseded texture remap refinement: this `1.5` amplitude conclusion was
replaced by the later `0x142664260` scalar reread on 2026-06-04. At this
checkpoint, the accepted `TextureFlag=0x201` bridge was
re-tested after the thickness work because `Vector_Texture` was still the
largest isolated residual. A no-edit probe that moved texture application from
post-stroke multiplication to per-segment pre-accumulation was rejected: it
regressed `Vector_Texture` from `mean=0.571271` to `0.887783` and
`Vector_Texture_50` from `0.709181` to `1.490394`. Formula probes instead
matched the native density gate more closely. Disassembly around `0x142664760`
keeps `TextureComposite=0` as `coverage * (0x8000 + baseline - texCoverage)`,
and older evidence around `0x1422D8550` / `0x142664260` says the plot texture
density scalar is only active for descriptor flags satisfying `(TextureFlag &
0x11) == 0x11`. The current `0x201` samples lack bit `0x10`, so the remap
amplitude should not be multiplied by SQLite `TextureDensityBase`. The importer
now uses `factor = 1 + 1.5 * (textureByte - textureAverage)` for the remap
preview. Verification improves `Vector_Texture` to `max=226`, `mean=0.499736`,
`visible=11672`; `Vector_Texture_50` remains `max=182`, `mean=0.709181`,
`visible=2106`. Exact vector guards, `Vector_NoTexture`, `Vector_SizePressure`,
the thickness samples, `Test_Vector`, full `test_Filters_Vector_Text`, and the
strict filter-export matrix are unchanged.

Material / SizeTilt / NoTexture rejection pass: after the texture acceptance,
the next isolated residuals were probed without changing importer code.
`Vector_BrushTip_Material_Gap.psd` now has a layer oracle: Paper plus `Layer 1`
with bbox `[17,16,195,40]`, `1727` nonzero-alpha pixels, max alpha `253`, and
constant visible colour near `(192,93,97)`. The current material fallback emits
18 stamps with centers from `(21,25)` to `(191,26)`, resized stamp `12x12`,
opacity `0.3`, and anchor `(0.5,0.3)`, producing bbox `[16,22,197,38]`,
`1099` nonzero-alpha pixels, and visible mean colour `(177,44,44)`. A no-edit
probe varied wide-gap height, opacity, anchor, and simple colour overrides.
The best final-PNG result was only `mean=1.099519`, `visible=1850` versus the
current `mean=1.106737`, `visible=1862`, while variants that made the PSD
layer shape more similar regressed the final PNG (`mean >= 1.25`). Treat
material-tip colour/height/anchor tuning as overfit unless native material row
writer evidence appears.

`Vector_SizeTilt_50` was rechecked against its PSD layer oracle. PSD `Layer 2`
has bbox `[16,4,182,173]` and `2911` nonzero-alpha pixels; the importer layer
has `2912`, with alpha sum only `+133` higher. The final `14` visible pixels
are all paper-composited RGB edge differences of `1..2` clustered in the tail
bbox `[167,155,182,173]`. This remains a tiny accepted residual, not a reason
to add the earlier metric-only `SizeEffector=0x21 * 0.9999` shrink.

`Vector_NoTexture` was also separated from the texture residual. Its PSD layer
is a hard black mask (`bbox [257,242,720,750]`, `26563` nonzero alpha). The
importer layer has `26548` nonzero alpha, with `63` output-only black pixels
and `78` missing black pixels; the largest component is a 74-pixel tail cluster
at `[693,720,720,750]`. Sample-point rounding/phase variants are exactly
neutral. In-memory hard-span probes improve `NoTexture` as the shrink moves
from native `0.4` toward `0.48/0.50` (`visible 141 -> 103`) and also improve
paused `SizePressure` (`153 -> 94/92`), but they break exact
`Vector_OpacityPressure` (`visible 323/409`). Since r2/IDA already ground the
hard circular helper at `sqrt(radius^2-dy^2)-0.4`, keep span/radius shrink as a
diagnostic boundary signal only, not an importer change.

Vector AA compact AA-phase refinement acceptance: a focused no-edit renderer
probe revisited only the second compact `0x41` dark curve layer in
`Vector_AA_None/Weak/Medium/Strong`, keeping the ordinary `0x2011` layer
unchanged. Visualizing `Vector_AA_None` layer 2 shows the object is a compact
curve stroke, not a filled blob: current layer bbox `[264,383,498,648]` vs PSD
`[264,383,498,647]`, with red/blue edge swaps around the same arc. The compact
record still favours the existing degenerate cubic `p0,p0,out0,p1`; variants
using `+88/+96` as a standard incoming handle (`p0,out0,in1,p1`) or other
control-point remaps remain far worse. The accepted improvement is only the
AA-level sampled-point phase: change `VECTOR_FILLED_CURVE_AA_SAMPLE_SHIFT`
from `-0.25` to `-0.5` for compact `0x41` dark curves with AA level `1..3`.
Verification after editing both loader entrypoints: `Vector_AA_None` unchanged
at `max=226`, `mean=0.036532`, `visible=226`; `Vector_AA_Weak` improves to
`max=158`, `mean=0.061561`, `visible=1759`; `Vector_AA_Medium` improves to
`max=102`, `mean=0.021742`, `visible=1691`; `Vector_AA_Strong` improves to
`max=128`, `mean=0.020742`, `visible=2070`. Exact guards remain exact
(`Vector_Baseline`, `Vector_Flow_50`, `Vector_FlowPressure_50`,
`Vector_OpacityPressure`, `Vector_OpacityVelocity_50`,
`Vector_OpacityRandom_50`, `Vector_SizeVelocity_50`, `Vector_SizeRandom_50`).
`Vector_NoTexture`, texture, material brush-tip, `Vector_SizePressure`,
`Test_Vector`, and `test_Filters_Vector_Text` are unchanged. Treat this as a
narrow PSD-backed compact-curve preview refinement; the native compact curve
object geometry is still not fully recovered. A follow-up radius/feather grid
around the new `-0.5` phase was rejected: it can lower aggregate mean, but only
by trading error between AA levels, especially worsening either `Medium` or
`Strong`.

AA phase re-sweep after the texture refinement still rejects further movement:
global `VECTOR_FILLED_CURVE_AA_SAMPLE_SHIFT=-0.5` remains best across
`Vector_AA_None/Weak/Medium/Strong` (`mean_sum=0.140577`,
`visible_sum=5746`). The closest alternative `-0.375` slightly helps Weak but
worsens Medium/Strong (`mean_sum=0.142893`, `visible_sum=5908`), so no AA code
change was kept.

Thickness AA ellipse refinement acceptance: after the compact-AA pass, the
remaining isolated vector table showed `Vector_Thickness_50*` as the narrowest
native-backed non-exact family outside material/texture/SizePressure. Current
styles are clean no-pattern dabs (`AntiAlias=2`, `ThicknessBase=0.5`,
`Hardness=1.0`, `PatternStyle=0`, `IntervalBase=0.1`) with only
`RotationBase` changing across `0/45/90`. A no-edit matrix rejected simple
axis substitutions and rotation changes: the old `minor`-axis AA normalization
was still the best of that simple family by aggregate mean, while
`major`/`avg`/`geom`, raw center, rotation sign, and 90-degree offsets traded
visible count for worse mean or broke the angle samples. r2 disassembly of
`0x14263F410` and `0x142640420` then provided a better native rule:
`0x14263F410` dispatches the current thickness samples to the stretched AA
helper, and `0x142640420` subtracts the AA width from both major and minor axes
before building the inner ramp. Mirroring that as an outer ellipse plus an
inner ellipse with `major-aa_width` and `minor-aa_width` improves all three
means after editing both loader entrypoints: `Vector_Thickness_50` from
`max=91`, `mean=0.295050`, `visible=688` to `max=55`, `mean=0.277906`,
`visible=752`; `Vector_Thickness_50_Angle_45` from `max=64`,
`mean=0.314887`, `visible=1270` to `max=64`, `mean=0.287650`,
`visible=922`; and `Vector_Thickness_50_Angle_90` from `max=62`,
`mean=0.417094`, `visible=1675` to `max=69`, `mean=0.317744`,
`visible=948`. Exact guards (`Vector_Baseline`, `Vector_Flow_50`,
`Vector_FlowPressure_50`, `Vector_OpacityPressure`,
`Vector_OpacityRandom_50`, `Vector_OpacityVelocity_50`,
`Vector_SizeRandom_50`, `Vector_SizeVelocity_50`) stayed exact. Paused
`Vector_SizePressure`, `Vector_NoTexture`, AA, texture, material brush-tip,
`Test_Vector`, and `verify_filter_exports.py img/test_Filters_Vector_Text.clip
--max-max 8 --max-mean 0.06 --max-visible-px 2200` stayed unchanged. A rough
hand-written row-span port of the helper over-darkened the samples badly, so
the accepted implementation should still be treated as a native-backed
inner/outer ellipse approximation rather than a full line-for-line port of
`0x142640420`.

Tone Curve byte-domain correction: the newer standalone `Test_ToneCurve.clip`
sample exercises non-identity RGB compact curves, and it overturns the earlier
direct 16-bit-domain LUT assumption for multi-channel payloads. Direct PSD
inspection of `Test_ToneCurve.psd` shows the `curv` tag stores byte UI points
corresponding to the compact points after byte scaling, while native evidence
still shows the runtime generator consumes four expanded `0x104` curve blocks
and merges R/G/B LUTs with the master. No-edit probes found the balanced native
match by converting compact coordinates with `ceil(value / 257)`, generating
the quadratic B-spline in byte-domain span `/255`, sampling with
`t = sample_idx / 257`, and rounding the resulting byte table. Editing both
loader entrypoints with that rule improves `Test_ToneCurve` from `max=25`,
`mean=0.488793`, `visible=8810` to `max=17`, `mean=0.018200`,
`visible=6107`. `verify_filter_exports.py img/test_Filters_Vector_Text.clip
--max-max 8 --max-mean 0.06 --max-visible-px 2200` still passes with isolated
Tone Curve at `max=1`, `mean=0.000514`, `visible=0`; `Test_Vector`,
`Test_Ballon`, `Test_Frames`, `Test_Gradiation`, exact vector guards,
texture samples, and paused `Vector_SizePressure` are unchanged.

Material brush-tip lane and PSD-layer recheck: `Vector_BrushTip_Material` and
`Vector_BrushTip_Material_Gap` remain PatternStyle `2` / no texture / no
watercolor / no spray samples, differing mainly by `IntervalBase=0.1` versus
`0.5`. The vector blob contains one ordinary `(92,76,88,88)` stroke with 18
points; current fallback renders ordinary Material as 88 resampled `12x20`
alpha stamps and Gap as 18 `12x12` stamps. The Gap PSD layer oracle is stronger
than the final PNG: its native vector layer bbox is `[17,16,195,40]`, alpha
nonzero `1727`, alpha sum `172314`, and visible RGB median `(192,93,97)`, while
the fallback layer is bbox `[16,22,197,38]`, alpha nonzero `1099`, alpha sum
`44425`, and fixed RGB `(177,44,44)`. This proves the native material path is
not merely stroke-color times a stamp alpha.

The persisted material mipmap has offscreen size `169x449` but four 256x256
single-channel tiles, which decomposes cleanly into two apparent `169x449`
lanes. Rechecking the native lane hypothesis against importer probes rejects a
direct lane swap: using the right lane as the stamp alpha worsens
`Vector_BrushTip_Material` from mean `0.842944` to `2.313456` and
`Vector_BrushTip_Material_Gap` from `1.106737` to `1.180225`. Raising only the
Gap alpha also fails (`0.3 -> 0.4` slightly reduces visible pixels but worsens
mean, and `0.5+` regresses further). A geometry/color grid can find a tiny
metric-only Gap improvement around color `(205,110,115)`, alpha `0.6`, and the
current `12x12` geometry (`1.106737 -> 1.099575`), but more PSD-like large
stamps regress the final PNG. Keep current fallback constants; exact material
rendering should continue through native `0x14263B7F0` row dispatch and dynamic
material-color writer analysis (`0x14263C3A0` / `0x142637A70` family), not
through another preview constant.

Material two-lane native-color refinement: direct disassembly around
`0x142637A70`, `0x142637C70`, and `0x14263E970` fills in the missing
material-color semantics for selector/flags `0x10`. The dynamic and fixed
samplers both first use byte/lane `1` as the coverage gate: if it is zero, the
pixel is skipped; otherwise incoming coverage is multiplied by that byte using
the same `0x80808081` divide-by-255 idiom. Byte/lane `0` is separately
expanded to a `0..0x8000` mix weight via `(byte0 * 257 + 1) >> 1`, and the
output BGR is blended between two context color triples:
`ctx+0x24c/+0x250/+0x254` and `ctx+0x25c/+0x260/+0x264`, with fixed-point
rounding `+0x4000 >> 15`. When cached material sampling is active,
`0x14263E970` samples four footprint points, ignores points whose byte/lane
`1` is zero, averages byte/lane `0` over the nonzero samples, and scales
coverage by summed byte/lane `1` before returning to `0x142637A70`.

A no-edit importer probe using the current fallback placement but this
native-ish color model confirms direction but rejects a code change. With the
existing fallback opacity constants, lane mix regresses both samples
(`Vector_BrushTip_Material` `0.842944 -> 1.608850`,
`Vector_BrushTip_Material_Gap` `1.106737 -> 1.134969`). Empirical opacities
can improve the ordinary sample (`0.8` gives `0.758831`) and barely improve
Gap (`0.5` gives `1.102556`), but that is still a preview constant compensating
for the missing native UV quad, four-sample footprint, and row accumulation.
Therefore keep the current material fallback unchanged and treat lane `1`
coverage plus lane `0` color mix as accepted native semantics for the future
native material experiment.

Material quad/aspect continuation: the adjacent reverse workspace dump
`tmp_r2_csp/material_quad_submit_pd_20260604.txt` extends the retained pattern
branch beyond row color. The pattern-only queue case in `0x14260DB90` is still
the `queue+0x158 != 0 && queue+0x178 == 0` branch, which calls
`0x142642010`; the transform/material case with `queue+0x178 != 0` calls
`0x142636CC0`. In `0x142642010`, the helper stores the dab center at draw
context `+0x1b0/+0x1b8`, stores the clip rect at `+0x190`, converts the two
plot scalars to `32768`-scaled fields `+0x1e0/+0x1e4`, resolves the material
through `0x14251CCB0`, then stores material width/height at `+0x1f8/+0x1fc`
and max fixed UV at `+0x200/+0x204`. It then calls `0x142642E90` to publish
the material pixel cache at `+0x238/+0x240/+0x244`.

The quad construction itself is not square-stamp logic. After querying
material width and height again, native computes
`scale = 2 * effective_size / max(width, height)`. It copies that scale to
both axes, multiplies one axis by the caller scale at stack `+0x8a0` depending
on the aspect/scale flag at stack `+0x8b0`, and either calls the axis-aligned
submit helper `0x142641C60` when rounded rotation and flip are zero, or builds
four 0x20-byte vertices `(double x, double y, double u, double v)` and calls
`0x1426410B0(ctx, quad, 0)`. The manual path uses the sine/cosine table at
`0x1444D4AF0`, half extents `width * scaleX * 0.5` and
`height * scaleY * 0.5`, and flips UV `u=0/width` according to the caller flag.
`0x1426410B0` then clips/scans the quad, writes fixed-point UV state at
context `+0x208/+0x20c/+0x210/+0x214`, and dispatches rows through
`0x14263B7F0`.

The zero-rotation/zero-flip fast path `0x142641C60` is probably closer to the
current material samples. It receives `scaleX` in `xmm1`, `scaleY` in `xmm2`,
material width in `r9d`, and material height as the stack argument. It computes
the same center +/- half extent bbox, clips it, then scans axis-aligned spans
directly rather than first materializing four vertices. Its UV increments are
fixed-point `int((1 / scaleX) * 32768)` and `int((1 / scaleY) * 32768)`. The
starting UV for each clipped span is
`(pixel_left - bbox_left + bias) / scaleX * 32768` and
`(pixel_top - bbox_top + bias) / scaleY * 32768`; `bias` is `0.5` only when
draw context `+0x1d0` is zero, otherwise it is `0`. When `ctx+0x1d0` is
nonzero, the helper also stores half x-step fields at `+0x220/+0x224` and
per-row paired UV fields at `+0x218/+0x21c` before calling `0x14263B7F0`.
This makes material alignment a fixed-UV/source-phase problem, not only a
resized alpha-stamp problem.

A no-edit importer probe rejects copying this one aspect formula into the
current preview fallback. For the current `169x449` material and an effective
size around `10`, the literal native aspect is roughly `8x20`, while current
fallback geometry is `12x20` for ordinary Material and `12x12` for Gap.
Monkeypatching preview constants to native-aspect `width_scale=0.75`,
`height_scale=2.0`, `gap_height_scale=2.0` worsens
`Vector_BrushTip_Material` from mean `0.842944` to `1.539219` and
`Vector_BrushTip_Material_Gap` from `1.106737` to `1.269750`. A Gap-height-only
variant (`1.2x2.0`) leaves ordinary Material unchanged but worsens Gap to
`1.326781`; higher Gap opacity with native aspect worsens further. Therefore
the aspect/quad rule is accepted as native retained-material evidence, but it
is not an importer edit until the full UV sampler, four-sample footprint,
row accumulation, and color-mix path are modeled together.

Material axis/row probe continuation: both `Vector_BrushTip_Material` and
`Vector_BrushTip_Material_Gap` have `BrushStyle.AntiAlias=2`, so the
axis-aligned helper's current-sample phase should be the `ctx+0x1d0 != 0`
branch with source bias `0`, not the no-AA `+0.5` bias. A no-edit Python
prototype replaced only `_draw_material_stamp_rgba` with a full-material-lane
fixed-UV sampler while leaving the importer's current point/spacing decisions
intact. The best variant used current fallback placement, `bias=0`, left-lane
coverage, and stroke color; it improved ordinary Material from mean
`0.842944` to `0.797631`, but worsened Gap from `1.106737` to `1.122419`.
Center placement, native-aspect geometry, right/lane1 coverage, max-lane
coverage, material mix-to-white/paper, PSD-median color, and `+0.5` bias all
regressed the family. A separate duplicate/near-point skip probe is also
rejected: removing only the zero-distance Gap tail dab changes Gap
`1.106737 -> 1.108019`; larger thresholds reduce a few visible pixels but
worsen mean.

Fresh disassembly of `0x14263B7F0` and `0x14263C3A0` explains why the RGBA
stamp prototype is still not the native renderer. `0x14263B7F0` is not a
single material writer; it dispatches according to draw-context flags and
material format. The relevant dynamic material-color path is
`0x14263C3A0`: it reads the 16-bit coverage plane from `ctx+0x148`, the BGR
plane from `ctx+0x150`, and the paired prior/secondary planes from
`ctx+0xf0/+0x120`. For each pixel it calls `0x142637A70` when
`ctx+0x1d0 != 0` (AA/material footprint path) and `0x142637C70` when
`ctx+0x1d0 == 0`, receiving updated coverage plus BGR components in scratch
fields. If coverage is nonzero, it optionally applies mask/texture gates, then
updates the 16-bit coverage plane with the usual native accumulation
`old + ((opacityCap - old) * coverage >> 15)` unless the direct/max mode
`ctx+0x1dc` is set. It then passes the BGR output and coverage to
`0x14263DDB0` to write the colour plane. The current importer material preview
still alpha-overs 8-bit RGBA stamps, so exact material support needs a
retained 16-bit coverage/BGR row experiment rather than another preview stamp
constant.

Retained material row prototype: `0x14263DDB0` was re-read directly and saved
in the adjacent reverse workspace as
`tmp_r2_csp/material_color_plane_writer_pd_14263ddb0_20260604.txt`. The writer
receives a 16-bit coverage plane pointer, a BGR byte plane pointer, a pixel
index, an input coverage value, and BGR bytes on the stack. It computes
`candidate = min(inputCoverage * ctx+0x1e0, 0x40000000) >> 15`. In
`ctx+0x1dc` direct/max mode it only replaces the pixel when `candidate` is
larger than the existing 16-bit coverage, then writes the incoming BGR bytes.
In build-up mode, zero old coverage or full candidate coverage stores directly;
otherwise native first updates coverage toward `opacityCap`, then blends
existing BGR with incoming BGR using fixed-point weights derived from the new
coverage. This reinforces that material rows are straight colour plus a
16-bit coverage plane, not ordinary 8-bit RGBA alpha-over.

A no-edit retained-row prototype kept the importer's current material point
placement and UV geometry but replaced per-stamp alpha-over with a persistent
16-bit coverage plane and BGR byte plane. Several bounded variants swept
coverage lane, source color, BGR blend approximation, flush mode
(`(*u16-1)>>7`, rounded `*255/32768`, and high byte), and a few flow scales.
The best combined score used left-lane coverage, PSD-like color `(192,93,97)`,
a simple coverage-ratio blend, `(*u16-1)>>7` flush, and `0.5` flow: ordinary
`Vector_BrushTip_Material` improved from mean `0.842944` to `0.724850`, but
`Vector_BrushTip_Material_Gap` worsened from `1.106737` to `1.142213`. Stroke
color with retained rows gives the same shape: ordinary can improve
(`~0.7995` at `0.4` flow), but Gap still worsens (`~1.142`). Therefore a
half-native row-plane patch is rejected for now. The row-plane direction is
native and useful, but the safe implementation boundary still requires native
material point submission/spacing for the Gap style, exact color context
resolution, and final flush/composite confirmation.

Material Gap submission recheck: `0x14255DFE0` has a `draw+0x160 == 2` branch
that calls `0x14255C680` instead of immediately calling `0x14260F550`. Fresh
r2 disassembly from the reverse workspace shows `0x14255C680` is not a
separate material renderer: it builds the dab bbox, converts the center to a
256-fixed grid coordinate, checks whether the dab crosses a grid/tile boundary,
and either submits one queue record or splits the dab across the paired draw
contexts `draw+0x140` and `draw+0x150`. The actual draw still flows back
through `0x14260F550 -> 0x14260DB90`, then into the material/no-pattern
dispatch. A no-edit importer probe that kept the current Gap height/alpha/anchor
constants but forced wide material strokes to use the same interval resampling
expression as ordinary material strokes was rejected: ordinary
`Vector_BrushTip_Material` stayed at mean `0.842944`, while
`Vector_BrushTip_Material_Gap` worsened from `1.106737` to `1.148144`.
Therefore the current raw-vs-interval switch is not the standalone Gap fix;
keep the remaining target on retained row/color/final flush or deeper
retained-state phase semantics rather than a simple wide-stroke resample patch.

Material native-spline spacing acceptance: the reverse workspace dynamic trace
`tmp_csp_material_gap_frida_trace_20260604.jsonl` contains 87
`queueCopy`/`dispatch`/`materialLeave` triplets for ordinary
`Vector_BrushTip_Material`. The queue centers span roughly
`(16.2638,28.3939)` to `(187.4871,31.6864)`, with step statistics
`min=1.7015`, `max=2.1494`, `mean=1.9970`, `median=1.9975`. Replaying the
importer's recovered `PWVectorSplineCurve` six-control sampler over the saved
18 compact points with `step = 2 * width * IntervalBase = 2.0` matches those
87 native centers to floating-point noise (`max ~= 6.8e-8`, mean
`~= 2.3e-8`). A linear point-distance resampler has the same rough shape but
differs by up to `0.154px`, so native material point submission is spline
distance sampling, not raw polyline distance.

The same native spline sampler on `Vector_BrushTip_Material_Gap`
(`IntervalBase=0.5`, `width=10`, so step `10.0`) yields the Gap centers seen in
the earlier console trace: `(21.151,25.813)`, `(30.981,26.971)`,
`(40.957,28.711)`, `(50.804,29.992)`, ... through about
`(190.652,27.055)`. This proves the previous "force wide material resampling"
rejection was specifically a rejection of the importer's simple linear/rounded
probe, not of native resampling itself.

Accepted importer change: add `_native_spline_resample_points_by_distance()`
and use it only for `IntervalBase > 0.1` material-tip strokes, passing float
centers to the material stamp writer. Ordinary material keeps the previous
preview branch: applying native spline/float centers globally is more native,
but it regresses the current preview constants (`Vector_BrushTip_Material`
`0.842944 -> ~0.95`). The narrow wide-only patch improves
`Vector_BrushTip_Material_Gap` from `max=132`, `mean=1.106737`,
`visible=1862` to `max=132`, `mean=1.030538`, `visible=1844`, while ordinary
`Vector_BrushTip_Material` remains `mean=0.842944`. Verification after the
patch: `Vector_Baseline`, `Vector_OpacityPressure`, and
`Vector_SizeRandom_50` remain exact; `Vector_Texture_50` remains
`mean=0.709181`, `visible=2106`; paused `Vector_SizePressure` remains
`mean=0.024732`, `visible=153`; `test_Filters_Vector_Text` remains
`mean=0.568335`, `visible=48586`. The remaining material Gap residual is now
bounded to material row/color/UV/final flush rather than primary center
spacing.

Material Gap aspect/color acceptance: after native-spline spacing, the next
bounded probes separated metric-only constants from native-backed material
preview semantics. Pure Gap height/alpha/anchor/color sweeps can reduce the
PNG mean, but the accepted variant is the one that also matches native/PSD
evidence. Frida material context records showed resolved material dimensions
`w=42`, `h=112`, `fmt=17`, `opacity_fixed=32768`, `flow_fixed=32768`, main
color `(177,44,44)` in file RGB order, and sub color `(228,224,237)` from the
stroke header offsets `+52/+56/+60`. The native pattern scale formula already
recovered from `0x142642010` is `2 * effective_size / max(material_w,
material_h)`; with size `10`, this gives about `7.5x20`. The importer now uses
that aspect for wide material tips (`width_scale=0.75`, `height_scale=2.0`),
centered anchor, and full material coverage.

The color is also no longer a PSD hardcode. Reconstructing the material lanes
from the mipmap shows the left crop has mean `69.59165/255 = 0.2729`; using
that as the material mix value blends main `(177,44,44)` with sub
`(228,224,237)` into `(191,93,97)`, matching the PSD layer median
`(192,93,97)`. A no-edit source probe with this dynamic mix produced
`Vector_BrushTip_Material_Gap max=100`, `mean=0.289350`, `visible=1374` and
left `Vector_BrushTip_Material` unchanged at `mean=0.842944`. The accepted
patch implements the same rule in both loader entrypoints.

Layer-oracle check after the patch: current Gap fallback layer bbox is now
`[17,16,195,40]`, alpha pixels `1494`, alpha sum `164592`, alpha max `253`,
and median RGB `[191,93,97]`. The PSD oracle is bbox `[17,16,195,40]`, alpha
pixels `1727`, alpha sum `172314`, and median `[192,93,97]`. Guard checks
after the patch: `Vector_BrushTip_Material` stays `mean=0.842944`;
`Vector_Baseline`, `Vector_OpacityPressure`, and `Vector_SizeRandom_50` stay
exact; `Vector_Texture_50` stays `mean=0.709181`, `visible=2106`; paused
`Vector_SizePressure` stays `mean=0.024732`, `visible=153`; and
`test_Filters_Vector_Text` stays `mean=0.568335`, `visible=48586`. Remaining
material exactness is now narrowed to per-pixel lane1 coverage sampling,
retained 16-bit row accumulation, UV phase, and final flush/composite details.

2026-06-04 material Gap native-mip / fixed-UV coverage follow-up: the previous
crop-resize alpha still left alternating red/blue residual inside each stamp,
so the next probes tested the native resolved-material path more literally.
`0x142641C60` writes axis-AA fixed UV state at `ctx+0x208/+0x20c`,
`+0x210/+0x214`, `+0x218/+0x21c`, and `+0x220/+0x224`; the cached two-lane
helper `0x14263E970` then samples up to four material points, using byte lane
`1` for coverage and byte lane `0` for material mix. The accepted importer
change now caches the full left material lane (`169x449`), area-downsamples it
by the native resolved scale to `42x112`, then renders the final wide-material
stamp with the axis-AA footprint `(u,v)`, `(u+xstep,v)`, `(u,v+ystep/2)`,
`(u+xstep/2,v+ystep/2)` and floor fixed-UV lookup. Direct cropped-stamp
resizing is still kept for the ordinary material preview path.

Verification after the patch: `Vector_BrushTip_Material_Gap` improves from
`max=100`, `mean=0.289350`, `visible=1374` to `max=83`, `mean=0.200137`,
`visible=1313`. `Vector_BrushTip_Material` remains `mean=0.842944`.
`Vector_Baseline`, `Vector_OpacityPressure`, and `Vector_SizeRandom_50` remain
exact. `Vector_Texture_50` remains `mean=0.709181`, `visible=2106`; paused
`Vector_SizePressure` remains `mean=0.024732`, `visible=153`; and
`test_Filters_Vector_Text` remains `mean=0.568335`, `visible=48586`.

PSD-layer oracle after fixed-UV four-sample coverage: importer and PSD still
share bbox `[17,16,195,40]`, and median RGB stays aligned (`[191,93,97]` vs
`[192,93,97]`). Alpha abs residual improves from `39368` to `27496`; alpha
pixels are `1854` vs PSD `1727`; alpha sum is `170334` vs PSD `172314`, with
signed alpha sum only `-1980`. This removes most of the previous thinness and
narrowly bounds the remaining Gap residual to row/final-flush details rather
than spacing, colour, material dimensions, or single-sample UV coverage.

Post-acceptance rejection probes for material Gap: the fixed-UV four-sample
patch was followed by several no-edit probes to avoid over-reading the
remaining residual. Replacing the material stamp alpha-over with simple
`float_round_each`, `native16_floor_flush`, or `native16_round_flush`
accumulation/flush variants was exactly neutral (`mean=0.200137`,
`visible=1313`), so the remaining error is not explained by per-stamp 8-bit
alpha quantization for this fixture. A placement sweep found a small
metric-only improvement by effectively shifting stamps right (`mean=0.198150`),
but it does not line up cleanly with the recovered axis helper bbox rule
`center +/- nativeExtent/2 -> floor/ceil`; keep it rejected without native
row/bbox evidence. Per-pixel material mix probes also reject a tempting
half-native graft: using the coverage lane as byte0 mix regresses Gap to
`mean=0.772587..0.810700`, and using the apparent right lane as mix still
regresses to `0.430600..0.465813`. The accepted average PSD/native material
colour remains the safe preview boundary until the real cached byte0/byte1
plane mapping and BGR row writer are implemented together.

SizeTilt recheck after material work: `Vector_SizeTilt_50` still verifies at
`max=2`, `mean=0.001306`, `visible=14`. Focused no-edit probes over the
`SizeEffector=0x21` numeric path reject float/quantization explanations:
source `float32`, graph `float32`, all-`float32`, and one-ULP-lower source are
exactly neutral; 15-bit source quantization slightly worsens; 8-bit source
quantization regresses heavily (`visible=165`). A diagnostic layer-alpha-minus
one postprocess lowers final visible pixels to `2`, but it creates large
missing transparent-layer alpha (`alpha_abs=2835` vs current `133`), so it is
overfit and not native. The previous boundary still stands: this tiny residual
is a tail AA/coverage edge detail, not a supported `0x21` shrink or simple
numeric precision rule.
2026-06-04 texture native-helper reread: adjacent r2 fixed-range disassembly
was saved to `tmp_r2_csp/texture_helpers_pD_20260604.txt` for
`0x1426640E0`, `0x142664260`, `0x1426642D0`, `0x142664550`,
`0x142664760`, and `0x142664C00`, plus tail evidence in
`tmp_r2_csp/texture_descriptor_tail_pD_1426650fa_20260604.txt`. The reread
confirms the accepted texture map but narrows what should not be changed:
`0x142664C00` copies `TextureComposite` into texture context `+0x48`, writes
`+0x54` from `TextureFlag&0x100`, writes `+0x80` from `TextureFlag&0x200`,
and when remap is active computes the average byte through `0x1426642D0` and
stores the fixed baseline at `+0x84`. `0x142664260` updates `+0x4c` from the
plot texture-density scalar and `+0x50` from the row flow/fixed scalar. In
`0x142664760`, mode `0` multiplies incoming coverage by
`0x8000 + (ctx+0x84 if remap else 0x4000) - textureCoverage`, after the
optional density remap and threshold/range stage. Current `TextureFlag=0x201`
samples have zero `TextureBrightness/TextureContrast`, so `ctx+0x88` range
remap is off. A default-off importer probe tested whether texture strokes
should simply move from the current capsule/post-alpha bridge to native-dab
geometry with per-dab texture coverage. It was rejected: geometry-only mode
regressed `Vector_Texture/Vector_Texture_50` to `mean=0.734374/1.262050`,
native-sign mode to `0.733624/1.083781`, and positive-sign mode to
`1.056032/1.418662`, compared with the current `0.499736/0.709181`. Therefore
the next texture improvement should recover the true row-writer coordinate and
texture context path; do not retune global amplitude or force texture samples
through the no-pattern native dab branch from metrics alone.

2026-06-04 NoTexture terminal short-segment endpoint acceptance: after pausing
SizePressure, `Vector_NoTexture` was isolated because it shares the hard
no-pattern family (`StyleFlag=0x1c240`, `AntiAlias=0`, `AutoIntervalType=2`,
`Hardness=1.0`, no pattern/texture) without size or opacity dynamics. Residual
attribution showed `141` visible pixels split into `63` output-only hard-edge
pixels and `78` missing pixels; `77/78` missing pixels were nearest the final
emitted dab/tail region. The current feedback model emits `198` dabs; the last
regular dab is on segment `16` at `(698.9464744985142, 728.5918206068859)`,
radius `20.0`. Segment `17` is a tiny terminal span from
`(700.3332013762654, 729.8836251189384)` to
`(700.4085308558946, 729.9421460990609)` with length `0.4238767616`; the
carried walk entering it is `1.2901419548`, so the ordinary loop emits nothing.

The first apparent native explanation, the final-node branch at
`0x1422CC8AC`, remains rejected for this fixture because it checks feedback
state `+8` against `[-epsilon,+epsilon]`; the importer-equivalent final
residual is `0.8662651932`, not zero. The accepted native evidence is the
earlier overshot-segment branch in `0x1422CC1E0` at
`0x1422CC595..0x1422CC5B9`: when `walk >= segmentLength`, an endpoint-force
gate and node pointer gate can set the sample distance to the segment length
and emit one segment-end dab. This exactly matches the NoTexture terminal
short segment: adding one hard dab at the final endpoint with radius `20.0`
improves the metric from `max=226`, `mean=0.022792`, `visible=141` to
`max=226`, `mean=0.010992`, `visible=68`; a small grid confirmed the endpoint
is the best tested point (`t=1.0`, radius `20.0` beats nearby positions).

Importer change: both loader entrypoints now apply this only to a deliberately
narrow default no-pattern case: terminal segment, no ordinary dab emitted,
no size/opacity/flow dynamics, no native random opacity, overshot walk, and
endpoint flag `0x20` clear. Verification after the patch: exact guards remain
exact (`Vector_Baseline`, `Vector_OpacityPressure`, `Vector_FlowPressure_50`,
`Vector_Flow_50`, `Vector_OpacityRandom_50`, `Vector_SizeRandom_50`,
`Vector_OpacityVelocity_50`, `Vector_SizeVelocity_50`), `Vector_FlowVelocity_50`
remains effectively exact (`max=1`, `visible=0`), and paused
`Vector_SizePressure` remains `max=226`, `mean=0.024732`, `visible=153`.
AA family and texture samples are unchanged (`Vector_AA_None/Weak/Medium/Strong`
at `226/1759/1691/2070` visible; `Vector_Texture/Vector_Texture_50` at
`0.499736/0.709181` mean). Keep the hard row formula
`sqrt(radius^2-dy^2)-0.4`; the old hard-span shrink remains rejected.

2026-06-04 continuation after terminal endpoint acceptance: the terminal
short-segment endpoint rule was tested as a broader dynamic-path rule by
removing the no-dynamics guards in an in-memory importer variant. It is
metric-neutral for `Vector_SizeTilt_50`, `Vector_SizePressure`,
`Vector_NoTexture`, and the exact guards (`Vector_Baseline`,
`Vector_OpacityPressure`, `Vector_FlowPressure_50`, `Vector_Flow_50`,
`Vector_OpacityRandom_50`, `Vector_SizeRandom_50`,
`Vector_SizeVelocity_50`, `Vector_OpacityVelocity_50`). Therefore the accepted
NoTexture fix should not be over-read as the missing size/tilt/pressure
semantics.

`Vector_SizeTilt_50` was re-attributed against the PSD transparent layer and
saved as `tmp_vector_probe/sizetilt_dab_residual_attribution_codex_v1.json`.
Current layer-alpha residual is still `105` pixels, all importer-high by
`+1..+3`, with `25` alpha-visible pixels; final PNG remains
`max=2`, `mean=0.001306`, `visible=14`. Nearest-dab attribution puts most
visible residual on the last two AA dabs (`idx 173/174`) with centers
approximately `(174.718,166.753)` and `(175.685,167.382)`, radii
`5.870/5.858`, and `aa_width=2.5`. This confirms the remaining SizeTilt issue
is a tiny AA coverage/accumulation edge in the tail, not an endpoint branch,
paper composite, or `SizeEffector=0x21` numeric input problem.

2026-06-04 Hardness closure after the endpoint regression: `Vector_Hardness_50`
was temporarily reopened at `max=67`, `mean=0.075469`, `visible=209`; all
visible residual was importer-high alpha in the right tail. The cause was the
NoTexture terminal short-segment endpoint branch being applied to a soft/AA
brush. A no-edit variant that disabled endpoint emission for soft/AA styles
returned Hardness to invisible residual (`max=1`, `mean=0.004437`,
`visible=0`) while keeping NoTexture at `visible=68`; adding the native
fixed-point profile table made Hardness exact (`max=0`, `mean=0.0`,
`visible=0`). r2 evidence: `0x142664050` stores
`scale=round((0x400<<8)/radius)` plus center offsets, AA shifts profile center
by `-0.5px`, `0x14263AC30` indexes profile rows with
`abs((coord*scale-offset)>>8)`, and `0x142663B40` builds the same
`threshold=hardness*1.3-0.3` radial curve. Importer rule now accepted:
terminal endpoint sample is hard/no-AA only (`Hardness~=1.0`, `AntiAlias=0`);
soft no-pattern dabs use the `0x400` fixed-point hardness profile table.
Verification: `Vector_Hardness_50` exact; exact guards remain exact;
`Vector_NoTexture` stays `max=226`, `mean=0.010992`, `visible=68`;
`Vector_SizeTilt_50` stays `max=2`, `mean=0.001306`, `visible=14`; paused
`Vector_SizePressure` stays `max=226`, `mean=0.024732`, `visible=153`.

2026-06-04 AA compact `0x41` fallback refinement: after the Hardness closure,
the AA family was revisited with PSD layer oracles and WSL `pdg`/Windows r2
evidence. The second compact layer in each AA file is still
`header=(92,76,120,88)`, `flags=0x41`, `point_count=2`, `width=3.0`,
`brush_style=10`; `BrushStyle.AntiAlias` is exactly `0/1/2/3` across
None/Weak/Medium/Strong. The two compact point records expose double anchors
at `+0/+8`, integer side fields at `+16/+20/+24/+28`, and double curve tails
at `+88/+96` and `+104/+112`: point 0 has `+88/+96 == p0`, and point 1 has
`+104/+112 == p1`. WSL `pdg` on `CSVectorFileConv::LoadV2CtlPos @
0x124312d0`, `LoadV2Param @ 0x124314b0`, `LoadV2Scale @ 0x12431b90`, and
`ReadSplineChunk @ 0x12432110` shows the old V2 chunk path builds 0x38-byte
heads from control-position groups and later feeds float point/pressure lists
through `CSVectorSampling::CurveSampling -> DrawPenHeadCurve`; this is useful
renderer prior art, but it is not the same byte layout as the current
`VectorObjectList` 92/120-byte records, so it does not justify directly
replacing the compact parser. No-edit geometry probes confirm the current
degenerate cubic `p0,p0,out0,p1` remains much better than simple line,
quadratic-through-tail, or doubled-tail cubic interpretations; the previously
rejected standard cubic `p0,out0,in1,p1` remains closed. PSD layer comparison
then showed the remaining error is alpha-edge/scan-conversion shaped: AA1/AA2
benefit from per-AA radius/feather fallback tuning, while AA3 should remain at
the previous values. Both loader entrypoints now use a narrow compact dark
`0x41` edge map for AA levels 1..3 (`radius/feather`: `1 -> 0.72/0.50`,
`2 -> 0.62/1.10`, `3 -> 0.60/1.25`), leaving AA0 hard settings unchanged.
Verification: `Vector_AA_None` remains `max=226`, `mean=0.036532`,
`visible=226`; `Vector_AA_Weak` improves to `max=162`, `mean=0.036818`,
`visible=1163`; `Vector_AA_Medium` improves to `max=114`, `mean=0.016958`,
`visible=1560`; `Vector_AA_Strong` remains `max=128`, `mean=0.020742`,
`visible=2070`. Guards remain exact for `Vector_Baseline`, `Vector_Flow_50`,
`Vector_FlowPressure_50`, `Vector_OpacityPressure`,
`Vector_OpacityVelocity_50`, `Vector_OpacityRandom_50`, and
`Vector_SizeVelocity_50`; `Vector_SizeTilt_50` remains `visible=14`, paused
`Vector_SizePressure` remains `visible=153`, and `Vector_NoTexture` remains
`visible=68`. Treat this as a PSD-backed preview fallback refinement, not full
native curve/scan-conversion recovery.

2026-06-04 post-AA non-exact triage: after the compact `0x41` refinement, the
current isolated vector table was re-run to choose the next safest native
target. `Vector_FlowVelocity_50` and `Vector_OpacityTilt_50` are now practical
guards (`max=1`, `visible=0`). The remaining focused targets are still
`Vector_SizeTilt_50` (`max=2`, `visible=14`), `Vector_Thickness_50*`,
texture, material, AA compact, and larger composite vector/object fixtures.
Material_Gap probes reconfirmed the current fixed-UV four-sample coverage is
the best tested narrow preview: `corners`, center/single sampling,
floor/ceil placement, half-pixel shifts, and simple opacity changes all
regress or are neutral. A metric-only native-mip width `43` lowers
`Vector_BrushTip_Material_Gap` from `mean=0.200137` to about `0.190000`, but it
contradicts the saved Frida material context trace, where every
`materialLeave` record reports `w=42`, `h=112`, `fmt=17`; keep the accepted
`169x449 -> 42x112` resolved-material rule. The PSD layer comparison still
shows matching bbox and median RGB, but alpha remains slightly spread out:
importer `1854` alpha pixels / sum `170334` versus PSD `1727` / `172314`.
Therefore the remaining Material_Gap mismatch is retained row/cache/coverage
detail, not a justified dimension or opacity tweak.

2026-06-04 SizeTilt and thickness follow-up rejections: `Vector_SizeTilt_50`
was retested with no-edit probes over the circular AA branch and final native
alpha flush. Coverage round/ceil are worse; tiny coverage scales such as
`0.999` move visible pixels only from `14` to `13` while introducing
unsupported max-1 guard changes; center bias `0.49/0.51`, final flush
rounding, and AA cap nudges either regress SizeTilt or disturb exact guards.
Do not implement a `0x21`/AA micro-scale without new native evidence. For
`Vector_Thickness_50*`, r2/WSL re-read of `0x142640420` clarified that the real
stretched-AA helper solves rotated ellipse row roots, builds half-open outer
and inner x spans, writes a full `0x8000` center span, and ramps each row edge
linearly with constants `0.25`, `0.75`, `0.4999`, and `32768.0`. However, a
no-edit row-scan prototype is not yet a safe replacement for the current
inner/outer ellipse approximation: the straightforward port worsens 0/90
degree thickness samples badly (`mean ~=0.60/1.27`), and the best sin-flipped
45-degree variant only improves that one angle (`0.287650 -> 0.253625`) while
still leaving the other angles worse. The saved conclusion is to keep the
current approximation until the native clip iterator and x-interval details
are ported line-for-line.

2026-06-04 Thickness stretched-AA row solver acceptance: the previous
row-scan rejection is superseded. Re-reading `0x142640420` against the failed
prototype showed two concrete mistakes in the importer-side port: the solver
was missing the `0x14263F410` style `0x40` axis adjustment (`rotation + 90deg`
before the sine/cosine table lookup), and the row-root denominator had the
wrong rotated quadratic coefficient. The accepted implementation in both
`clip_studio_importer/clip_loader.py` and root `clip_loader.py` now keeps the
change narrow to no-pattern stretched AA dabs without a hardness profile. It
solves the outer rotated ellipse per row, uses the native half-open span shape
(`left + 0.5`, `right + 0.4999`), solves the inner ellipse after subtracting
AA width from both axes, clamps the inner span with the native quarter/three-
quarter guards, and fills left/full/right coverage using the row-edge ramp
denominators. Verification matches the in-memory probe: `Vector_Thickness_50`
improves to `max=12`, `mean=0.017019`, `visible=318`;
`Vector_Thickness_50_Angle_45` to `max=35`, `mean=0.050444`,
`visible=651`; and `Vector_Thickness_50_Angle_90` to `max=21`,
`mean=0.052687`, `visible=725`. Guard verification: `Vector_Baseline`,
`Vector_SizeVelocity_50`, `Vector_OpacityRandom_50`,
`Vector_Hardness_50`, `Vector_OpacityPressure`, `Vector_Flow_50`,
`Vector_FlowPressure_50`, and `Vector_SizeRandom_50` remain exact;
`Vector_SizeTilt_50` remains `max=2`, `visible=14`; paused
`Vector_SizePressure` remains `visible=153`; `Vector_NoTexture`,
`Vector_Texture(_50)`, `Vector_BrushTip_Material_Gap`, and
`Vector_AA_None/Weak/Medium/Strong` retain their previous metrics. This is now
native-backed thickness AA semantics, not a metric-only tweak.

2026-06-04 Secondary/auxiliary effector per-dab acceptance: the remaining
`Vector_SizeTilt_50` residual was not caused by circular-AA row coverage. A
no-edit port of `0x14263FC50`'s row-span/full-span/ramp structure was exactly
neutral for SizeTilt and exact guards. Re-reading native `0x142568040` pointed
to the real mismatch: `0x21` and `0x41` effectors are evaluated at each emitted
sample from the raw compact sample lanes (`+0x18` for secondary/tilt, `+0x20`
for auxiliary/velocity). The importer had already moved `0x31` pressure and
`0x81` random to per-emitted-sample evaluation, but `0x21`/`0x41` still
linearly interpolated endpoint effector outputs. Both loader entrypoints now
cache compact `+44` as `native_auxiliary_scalars`, interpolate `+40/+44` to the
current dab, and call `_brush_secondary_or_aux_effector_value()` inside the
native feedback loop for size and opacity. Verification: `Vector_SizeTilt_50`
goes from `max=2`, `mean=0.001306`, `visible=14` to exact;
`Vector_OpacityTilt_50` goes from `max=1`, `mean=0.000100`, `visible=0` to
exact; `Vector_SizeVelocity_50`, `Vector_OpacityVelocity_50`,
`Vector_Baseline`, `Vector_Flow_50`, `Vector_FlowPressure_50`,
`Vector_OpacityRandom_50`, `Vector_SizeRandom_50`, and
`Vector_OpacityPressure` stay exact. Paused `Vector_SizePressure` remains
`max=226`, `mean=0.024732`, `visible=153`, and NoTexture/texture/material/AA
compact metrics are unchanged. The earlier tempting `0x21 * 0.9999` shrink is
fully superseded by this native-backed sample-timing fix.

2026-06-04 material/texture follow-up rejections after Tilt exactness: after
the per-dab `0x21/0x41` fix, the next isolated residuals were rechecked before
editing. For material tips, forcing ordinary `Vector_BrushTip_Material` to use
the Gap path's full-lane `42x112` fixed-UV four-sample coverage regresses the
ordinary sample (`mean=0.842944 -> 1.005300`, `visible=3117 -> 3464`) while
leaving Gap unchanged. Combining fixed-UV with native spline float centers
lowers ordinary mean only slightly (`0.826394`) but increases visible pixels
(`3181`) and has no native support as a safe replacement. Native spline float
centers without fixed-UV, skipping `_vector_sample_point`, and step scales
`0.5/0.75/1.25/1.5` all regress ordinary material or badly regress Gap. Keep
the current split: Gap uses the native resolved full-lane/fixed-UV preview,
ordinary material keeps cropped-stamp bilinear preview until the real retained
row/color-plane writer is recovered.

Texture was also re-probed. Small integer texture offsets `[-4..4]` regress
both texture fixtures, so phase is not the missing narrow rule. Remap amplitude
`1.6` improves `Vector_Texture` (`mean=0.484157`) but worsens
`Vector_Texture_50` (`0.722200`), and `1.4` does the opposite weakly; treat
amplitude as diagnostic only. Rounding the texture average gives a tiny metric
improvement, but r2 disassembly of `0x1426642D0` shows the native average helper
accumulates bytes and uses integer division/truncation, not round-to-nearest.
Directly swapping the importer preview to literal `0x142664760` mode-0
fixed-point forms (`1.5 - tex`, `0.5 + tex`, or average-baseline raw/inverted
variants) regresses strongly. Current texture preview remains the best
native-compatible boundary until the full row-writer context/descriptor setup is
mapped.

2026-06-04 current NoTexture and texture row-writer audit: after the terminal
endpoint fix, `Vector_NoTexture` is now `max=226`, `mean=0.010992`,
`visible=68`. Fresh current-output attribution shows `66` importer-extra
hard-edge pixels and only `2` missing pixels, scattered across `199` submitted
dabs rather than clustered at the terminal segment. No-edit probes reject using
this as a global hard-row formula change: tightening the hard circular span
from `sqrt(radius^2-dy^2)-0.4` toward `-0.475` improves NoTexture to
`visible=25`, but immediately breaks exact `Vector_OpacityPressure`
(`visible=308`). Tiny global center shifts do not solve NoTexture and also
break exact guards; ignoring spline point flags leaves NoTexture at `68` and
breaks `Vector_OpacityRandom_50`. Keep `0x142640150` hard-row semantics as-is;
the remaining NoTexture pixels are a narrow sampler/projection or branch
interaction until native evidence says otherwise.

r2 rechecked the native texture row-writer boundary. `0x14260F8B0` copies the
writer-state texture descriptor and calls `0x142664C00` to build the texture
context. `0x142644180` stores the texture-enable flag at draw context
`+0x1e8`. Row writer `0x14263C060` then checks `+0x1e8` after profile/mask
coverage and calls `0x142664760(textureCtx, coverage, x, y)` before final
BGR/coverage accumulation. This confirms the structural mismatch in the
importer: `_apply_brush_texture_preview()` currently multiplies a completed
stroke alpha over the stroke bbox, while CSP applies texture as a per-pixel
coverage modifier inside the row writer. Further texture work should port that
row-stage modifier and sampler coordinates rather than tune post-alpha
constants or phase.
## 2026-06-04 - Texture Row-Stage Shortcut Rejections

Native r2 re-read keeps the texture helper placement closed: `0x14263C060`
calls `0x142664760(textureCtx, coverage, x, y)` after mask/profile coverage and
before final row accumulation. The call passes absolute row `x` in `r8d` and
current row `y` in `r9d`; `0x142664760` then applies texture context
`+0x68/+0x6c` offsets and the fixed-point `+0x58/+0x60` transform before
sampling the texture byte. `0x142664C00` maps descriptor `TextureFlag` to
context `+0x54=(flag&0x100)` and `+0x80=(flag&0x200)`, and maps
`TextureComposite` to context `+0x48`.

Two importer-side no-edit probes are rejected:

- Forcing `TexturePattern>0` strokes through the no-pattern native dab feedback
  path and applying row-stage texture coverage modulation was confirmed active
  (`texture_calls=198` for `Vector_Texture`, `78` for `Vector_Texture_50`), but
  the best tested variant was still worse than the current importer
  (`Vector_Texture` mean about `0.643692`, `Vector_Texture_50` mean about
  `1.094362`; current is `0.499736` / `0.709181`).
- Keeping the current polyline fallback geometry while moving texture
  multiplication from whole-stroke post-processing to per-segment row/delta
  writing also regressed overall. It can make one texture sample look better
  while breaking the other, so it is not a native-backed semantic change.

Conclusion: texture exactness needs the real retained row-writer geometry and
accumulation path. Do not fix the remaining texture residual with amplitude
constants, bbox-phase tweaks, post-alpha swaps, or a no-pattern dab graft.

## 2026-06-04 - Accepted Texture Density Remap Scalar

Reverse reread of `CLIPStudioPaint.exe` corrected the `TextureFlag=0x201`
amplitude rule. `0x142664260` is now identified as the per-dab texture scalar
updater: if `textureCtx+0x80` (`TextureFlag&0x200`) or `TextureComposite==9` is
active, it multiplies the incoming scalar by `10.0`, then always by `1024.0`,
truncates, and stores it in `textureCtx+0x4c`; the incoming `r8d` is stored in
`textureCtx+0x50`. `0x142664760` applies `+0x4c` before the composite switch by
remapping sampled texture coverage around the average baseline and clamping to
`0..0x8000`.

The caller chain matters. `0x14263F410` calls `0x142664260(ctx+0x38,
stackTextureScalar, ctx+0x1e4)` when texture is enabled, and byte-level call
scanning found its caller at `0x14260DED0`, which passes that stack scalar from
job field `[rbx+0x170]`. This supersedes the earlier conclusion that current
`0x201` texture should ignore `TextureDensityBase`; that old note only covered
the descriptor `0x10` density-effector gate, not the per-dab scalar consumed by
`0x142664260`.

Importer change: for the narrow supported `TextureFlag&0x200`,
`TextureComposite=0`, zero-transform texture preview, replace the fixed `1.5`
average-baseline amplitude with the native-shaped density scalar and clamp:
`remapped = clamp(avg + 10*TextureDensityBase*(tex-avg), 0, 1)`, then
`factor = 1 - avg + remapped`. The sign is intentionally in importer texture
orientation, because the decoded single-channel texture behaves opposite the
native sampled coverage byte in this preview layer. A follow-up fixed-point
probe accepted the native `0x1426642D0` average rule, using byte-sum integer
division instead of floating mean. Verification improves `Vector_Texture` from
`mean=0.499736`, `visible=11672` to `mean=0.222173`, `visible=8111`, and
`Vector_Texture_50` from `mean=0.709181`, `visible=2106` to `mean=0.359719`,
`visible=533`. `Vector_NoTexture` stays `mean=0.010992`,
`visible=68`; paused `Vector_SizePressure` stays `mean=0.024732`,
`visible=153`; exact guards remain exact; thickness, AA compact, and material
brush-tip metrics are unchanged. The remaining texture residual still belongs
to retained row-writer geometry/accumulation and sampler-coordinate fidelity,
not a broad post-alpha constant.

## 2026-06-04 - Retained Row Accumulator And Texture Order Recheck

Adjacent reverse r2 evidence tightened the retained row map. `0x14263AC30`
dispatches circular row spans by context flags. If a secondary color row exists,
the `0x14263AF7D` path calls `0x14263C060`; if no secondary row exists but
texture is enabled (`ctx+0x1e8 != 0`), `0x14263AF97` reaches `0x14263B251`,
which calls `0x14263C5C0`. Texture therefore remains ordinary circular dab
geometry plus retained row writing, not a post-stroke texture multiply.

`0x14263DDB0` is the final retained coverage/BGR accumulator. It computes
`pre = min(inputCoverage * opacityCap, 0x40000000)` and `candidate = pre >> 15`.
Direct/max mode (`ctx+0x1dc != 0`) replaces only when `candidate > oldCoverage`.
Build-up mode moves 16-bit coverage toward `opacityCap` with
`old + (((opacityCap - old) * inputCoverage) >> 15)`; BGR blending uses
`pre / newCoverage` as the new-color weight, `0x8000 - weight` as the old-color
weight, and byte rounding via `+0x4000` before `>> 15`.

The current texture fixtures use `StyleFlag=0x1c240`, which does not include
`0x1000`, so they are build-up, not direct/max. That invalidates the earlier
shortcut premise that post-stroke texture multiplication is equivalent under
direct/max. Still, no-edit importer probes reject simple order approximations:
per-segment texture-before-alpha-over regresses `Vector_Texture/Vector_Texture_50`
to `mean=0.845258/1.499181`, and a continuous build-up approximation
`1 - (1 - alpha)^factor` regresses them to `0.900940/1.547325`. A simple
retained-material replacement also regresses ordinary material
(`0.842944 -> 1.670581`) while leaving Gap unchanged. Keep the accepted `10x`
texture-density remap, but do not change texture/material accumulation without
the true 16-bit retained row model and sampler coordinates.

## 2026-06-04 - Plain Row And Flush Closure

Adjacent reverse Windows r2 evidence saved under `tmp_r2_csp/continue_*`
closes the ordinary no-texture row/flush path. `0x14263AC30` computes
`flowCoverage = (stackCoverage * ctx+0x1e4) >> 15`; when no retained
texture/material/profile complexity is active, the coverage-only accumulation
is inlined. The no-profile branch at `0x14263B195..0x14263B24C` is:

```c
for x in span:
    old = rowCoverage[x];
    if (ctx->auxRow)
        old = max(old, auxRow[x]);
    if (ctx->directMax)
        rowCoverage[x] = max(old, (ctx->opacityCap * flowCoverage) >> 15);
    else if (old < ctx->opacityCap)
        rowCoverage[x] = old + (((ctx->opacityCap - old) * flowCoverage) >> 15);
```

The profile branch at `0x14263B026..0x14263B180` uses the same formula after
hardness/profile coverage lookup. The final fast flush in `0x142653A40` writes
brush BGR and converts nonzero 16-bit coverage to 8-bit alpha as
`(coverage - 1) >> 7`; `0x14264BC90` is a related destination-alpha/color
mixing flush helper with fixed-point channel rounding. `0x14263D2F0` should be
classified as a fast coverage+BGR row writer, not a coverage-only texture
writer.

Importer verification after the reread is unchanged: `Vector_SizePressure`
`visible=153`, `Vector_NoTexture` `visible=68`,
`Vector_Texture/Vector_Texture_50` `mean=0.222173/0.359719`, and exact guards
such as `Vector_OpacityPressure`, `Vector_OpacityRandom_50`,
`Vector_SizeVelocity_50`, and `Vector_Hardness_50` remain exact. Therefore the
plain hard/no-texture native-dab row and alpha flush model is no longer the
likely cause of SizePressure. Keep searching there only if new evidence shows a
size-only radius/sample phase split; texture/material still require the
retained row writer and sampler-coordinate path.

## 2026-06-04 - Current SizePressure Trace Rebaseline

After the accepted 1px feedback-step floor, a current source-exec probe rebuilt
the SizePressure feedback trace instead of relying on the older 233-dab files.
The active importer now emits `213` hard no-AA dabs for
`Vector_SizePressure.clip`. Saved files:
`tmp_vector_probe/sizepressure_current_feedback_trace_codex_v2.json` and
`tmp_vector_probe/sizepressure_current_boundary_attribution_codex_v2.json`.

The current verifier remains `max=226`, `mean=0.024732`, `visible=153`.
Replaying the 213 trace exactly reproduces that result and splits the visible
residual into `152` importer-extra black hard-edge pixels plus one native-only
black pixel at `(530,462)`. Segment `26` contributes `53` extra pixels,
segment `9` contributes `23`, and the last-covering dab shrink needed to drop
extra pixels is small but not uniform (`median=0.0713px`,
`122/152 <= 0.25px`, `max=1.261px`).

Rejected no-edit shortcuts: changing the shared hard span from native
`sqrt(radius^2-dy^2)-0.4` to `-0.45` or `-0.5` improves SizePressure
(`visible=107/92`) and NoTexture, but breaks exact `Vector_OpacityPressure`
(`visible=212/409`). Current-trace adjacent-radius variants also do not solve
the residual: `avg_prev` reaches only `visible=140`, `avg_next` regresses to
`174`, and derivative corrections bottom out around `145`.

r2 rereads keep the native handoff closed. `0x14260F550` mechanically copies
prepared plot fields into the no-pattern queue context as 64-bit/32-bit fields,
so there is no hidden double-to-float radius truncation. `0x142568040` still
implements the multiplicative `0x31` effector model from sample `+0x10/+0x18`
with float constants converted to double; no native evidence supports changing
diagnostic `amount1=1.5` to `1.4`. Continue looking for a native-backed
size-path-specific footprint/sample-distribution nuance rather than generic
row, span, interval, or `0x31` changes.

A read-only external helper suggested rechecking a possible
`0x1422CC1E0` segment-start sample suppression. Current-trace replay rejects
the useful forms: skipping only the `residual_in == 0` first sample is exactly
neutral (`visible=153`), and skipping every segment's first sample only reaches
`visible=152` while creating `8` missing pixels. Do not pursue a simple
segment-first dab skip without new native evidence.

## 2026-06-04 - SizePressure Lane/Feedback/Quantisation Rejections

Probe `tmp_vector_probe/current_pressure_field_compare_codex_v1.json` compares
`Vector_SizePressure`, exact `Vector_OpacityPressure`, and the two
global-pressure fixtures. For the current SizePressure stroke, compact
`f32+52/+56/+60` are constant `1.0` and compact `f32+64..+76` are constant
`0.0`; only compact `+36/+40/+44` vary. This matches the native
`0x1422CC1E0 -> 0x142568040` lane map and leaves no hidden saved tail field or
global/device pressure curve that could selectively shrink SizePressure.

The current 213-dab trace was replayed directly through the hard-dab row writer
and exactly reproduced the verifier result (`visible=153`, `extra=152`,
`missing=1`). Diagnostic feedback-only source-exec variants are insufficient:
using primary-only feedback stays at `visible=153`, multiplying feedback by
`1.04` reaches only `148`, and a non-native raw-secondary feedback formula
reaches `139`. These do not explain the hard-edge superset and conflict with
the r2/IDA evidence that `0x1422D8550` writes one evaluated size into both plot
radius and feedback step.

Numeric handoff probes are also rejected. Converting center/radius to
`float32` before rasterisation is exactly neutral. Fixed-point-style radius
quantisation has the right direction only as a metric diagnostic:
`floor(radius*8)/8` reaches `visible=111` but creates `16` missing pixels;
finer grids such as `/32`, `/64`, `/128`, and `/256` stall well above zero.
Native `0x14260F550` copies plot/center fields as qwords, and
`0x142640150` uses `movsd/mulsd/sqrtsd/subsd/cvttsd2si` directly, so there is
no native support for a hidden 1/8 fixed-point radius.

Residual attribution remains local rather than global. Segment `26` still
dominates with `53` extra pixels and much larger required shrink windows
(`avg ~=0.3406`, median `0.2639`, max `1.261`) than segment `9`
(`avg ~=0.0489`). Local-only center-shift scans reduce the metric only
partially: moving segment `26+9` upward by about `0.25px` reaches
`visible=134` with missing pixels, and segment `26` tangent/normal shifts stay
around `141+`. Treat these as geometry diagnostics, not native semantics.

## 2026-06-04 - SizePressure Dynamic Rasterize And Endpoint Recheck

The GUI dynamic path still has not captured the real saved-vector Planeswalker
render path. CSP had `Vector_SizePressure.clip` open, but Frida hooks on the
known route emitted only `ready` during nudge and Layer -> Rasterize attempts.
The sibling RE workspace Stalker summary
`tmp_csp_stalker_sizepressure_rasterize_live_20260604.json` followed 63
threads and found hot targets dominated by infrastructure: `0x1438CA880` is
the stack-cookie checker, `0x1438CBBDA` is the CRT `malloc` import thunk,
`0x14205C6F0` wraps `HeapAlloc`, `0x14197DB60` is event-sync machinery around
`OpenEventA` / `ResetEvent`, and `0x142049870` is a small container/buffer
accessor. No hot target landed near `0x1422CC1E0`, `0x1422D8550`,
`0x14260F550`, `0x14263F410`, or `0x142640150`. Treat this rasterize/nudge
capture as cache/UI activity, not native dab evidence.

r2 re-reading of `0x1425A4100` also closes the endpoint-fallback suspicion for
current SizePressure. The active calls at `0x1425A4187` / `0x1425A41C9` pass
stack args as `a5=StyleFlag&2`, `a6=StyleFlag&0x20`, `a7=feedback_state`,
`a8=1`, `a9=0`, `a10=0`; current `StyleFlag=0x1c240` clears both `a5` and
`a6`. In `0x1422CC1E0`, the overshoot endpoint branch
`0x1422CC595..0x1422CC5B9` is gated by that `a6` stack slot, and the
`rdi==0` terminal fallback `0x1422CC8AC..0x1422CC9DE` only emits when the
carry is inside a tiny endpoint epsilon. Current SizePressure's final carry is
not in that window. Therefore the remaining `visible=153` is not a hidden
terminal/overshoot dab.

A no-edit direct-trace diagnostic replay reconfirmed that the current 213-dab
trace exactly reproduces the verifier (`visible=153`, `extra=152`,
`missing=1`). Perturbing segment `26` by sampling radius slightly forward in
local `t` can reduce extras but immediately creates missing pixels
(`t+0.12` gives about `visible=133`, `missing=12`); shifting only centers
backward can also improve diagnostically (`t-0.08` gives about `visible=131`,
`missing=7`), while shifting both center and radius together is basically
neutral. This argues against a simple spline-`t` phase error, and matches the
assembly evidence that `0x1422CC1E0` uses the same returned `t` for point and
lane interpolation.
2026-06-04 `Test_Vector` wide-`0x41` anti-alias follow-up: stroke-level PSD
splitting showed the remaining full-layer residual was not a missing vector
object. Rendering only/without each record confirmed the large two-point
`0x41` capsule accounts for most reference coverage, while the final residual
is distributed across the ordinary style-4 stroke, the one-point Koa_Lace1
stamp, and the capsule edge. Direction stats before the change showed the
capsule region was mostly output-alpha-heavy (`mean_abs ~= 67.5`). The wide
capsule's BrushStyle has `AntiAlias=3`, but the importer branch still used a
hard edge. A no-edit sweep isolated to that branch found `radius=0.99*width`
plus the existing native-AA level-3 feather (`1.75`) improved full
`Test_Vector` from `mean=0.923822` / `visible=25848` to `mean=0.638719` /
`visible=26167`; layer-vs-PSD mean improved to `0.888089`, and capsule
region mean_abs dropped to `42.5`. The accepted implementation is deliberately
narrow: only compact `92/76/120/88`, `flags=0x41`, `point_count=2`,
`width>16`, no-pattern/no-texture objects consume
`VECTOR_NATIVE_AA_FEATHER_BY_LEVEL[AntiAlias]` with
`VECTOR_FILLED_CURVE_WIDE_RADIUS_SCALE=0.99`. A corpus scan found that active
wide branch only in `Test_Vector`; width-3 AA compact samples stay on the dark
filled-curve path. Guards were unchanged: `Vector_OpacityRandom_50`,
`Vector_OpacityPressure`, `Vector_Hardness_50`, `Vector_Texture(_50)`,
`Vector_SizePressure`, `Vector_AA_None`, `Vector_AA_Medium`,
`Vector_NoTexture`, `test_Filters_Vector_Text`, `Test_Ballon`, and
`Test_Frames`.

2026-06-04 `Test_Vector` Koa_Lace1 placement rejection: the one-point compact
`0x41` pattern object stores primary point `(709,195)` and extra doubles near
`(707,215)`. A local no-edit sweep over pattern target width (`320..344`) and
paste offsets showed the current importer placement is already the best tested
choice: target width `328`, height `37`, `dx=0`, `dy=0` gives layer mean
`0.888089` and lace-region mean `8.104480`. The next-best variants already
regress noticeably (`target_w=332`, `dx=-2`, `dy=0` -> full mean `1.074435`;
`target_w=324`, `dx=2`, `dy=0` -> `1.098857`). Therefore do not use the extra
coordinate pair as a stamp anchor without new native evidence; remaining lace
residual is not a simple placement/scale offset.

2026-06-04 `Test_Vector` wide-`0x41` polygon bridge: r2 confirmed the historical
iswCoreTG V4 addresses still resolve (`CSVec4Draw::FillPolygon @ 0x12465250`
and `RenderStroke @ 0x1246a630`). `RenderStroke` reads `CSVStroke+0x58` as the
width/radius scalar and the V4 path builds circular pen-head envelope polygons
before `FillPolygon`; old IDA notes also showed simple vector AA uses a 4x
scratch/resolve. A no-edit prototype replacing only the wide `0x41` capsule
with a circular pen-head envelope polygon found the best native-shaped variant
at `radius=0.99*width`, `16` half-cap segments, and `4x` resolve (`layer mean
0.863977`, visible `27039`), better than the previous distance-field
`radius=0.99`/feather bridge (`layer mean 0.888089`, visible `28704`). The
importer now uses that polygon bridge only for wide compact `0x41`
two-point no-pattern/no-texture objects. Full `Test_Vector` improves again to
`max=187`, `mean=0.610175`, `visible=24477`; layer-vs-PSD is now `mean=0.861408`,
`visible=27004`. Guard samples remained unchanged: `Vector_OpacityRandom_50`,
`Vector_OpacityPressure`, `Vector_Hardness_50`, `Vector_Texture(_50)`,
`Vector_SizePressure`, `Vector_AA_None`, `Vector_AA_Medium`,
`Vector_NoTexture`, `test_Filters_Vector_Text`, `Test_Ballon`, and
`Test_Frames`. The active corpus scan still finds this wide branch only in
`Test_Vector`; compact width-3 AA `0x41` records remain on the separate dark
filled-curve path. The remaining capsule residual is output-heavy and should
not be chased by further scalar radius/endpoint fitting without native route
evidence for the exact runtime `CSVStroke`/pen-head representation.

2026-06-04 `Test_Vector` ordinary style-4 scalar-dab rejection: the remaining
ordinary stroke in `Test_Vector` is style `4`, `flags=0x2081`, `AA=1`,
`Hardness=0.95`, `AutoIntervalType=1`, `interval_scalar=0.135`, and uses the
default native no-pattern dab path. No-edit monkeypatch probes over only the
native dab function show tempting local improvements: `flow*0.90` reaches
`mean=0.600757` / `visible=25513`, `flow*0.95` reaches `0.601166` / `23069`,
`aa_width=1.25` reaches `0.588578` / `23134`, `hardness=1.0` reaches
`0.586660` / `22879`, and `radius*0.96` reaches `0.572845` / `22412`. These
are rejected as semantics. The corpus has this style in `Test_Vector`,
`Vector_Hardness_50`, and `Vector_Thickness_50`; a diagnostic global
`radius*0.96` wrapper breaks exact or near-exact guards:
`Vector_Hardness_50` exact -> `mean=0.407256`, `visible=1657`;
`Vector_Thickness_50` `mean=0.017019`, `visible=318` -> `mean=0.195738`,
`visible=764`; `Vector_OpacityPressure` exact -> `mean=0.132267`,
`visible=4849`; and `Vector_OpacityRandom_50` exact -> `mean=0.613800`,
`visible=1527`. Keep the result only as a diagnostic that the ordinary
soft-AA/profile footprint still has an unrecovered local phase/edge nuance.

2026-06-04 `CSVec4Draw::RenderCurve` V4 curve evidence: after rejecting more
ordinary-dab scalar fixes, the remaining AA compact `0x41` family was reopened
from the native V4 renderer side. Windows r2 confirmed
`CSVec4Draw::RenderCurve @ 0x12466E50` and its `FillPolygon` callees in
`iswCoreTG.dll` image base `0x12240000`. A read-only external helper used
IDA MCP on the `iswCoreTG.dll` database at port `13338` and agreed with the r2
disassembly. The key block is `0x12467A8C..0x12467B5B`: native calls
`CSBezier::CalcLengthFast`, divides by `*(draw+0x158)`, truncates to an integer
sample count, clamps it to at least `2`, and uses `1.0/count` as the `t` step.
The sample position is a three-point quadratic Bezier, not a cubic:
`(1-t)^2 * P0 + 2*(1-t)*t * P1 + t^2 * P2`; `0x12467B6F` then calls
`CSBezier::CalcTangent(t)`. For each sample, `0x12467CB9` calls
`CSVPenHead::CalcFarestPoint`, then the two pen-head side points are either
stored with `CSVec4Draw::Store` or appended to the draw hull buffer
(`draw+0x1A8/+0x1B8`, count `draw+0x1D4`). `FillPolygon` calls are visible at
`0x12467736`, `0x1246799F`, `0x12467D80`, and `0x1246826D`.

This changes the next AA compact target but does not justify an importer patch
yet. The current `.clip` `flags=0x41` header is a serialized compact object
family; the native `RenderCurve` branch condition involving `CSVCurve`
internal `flags&0x10` is a later runtime object flag and must not be equated
directly with the compact header. The current importer fallback uses a
cubic-like `p0,p0,compact+104/+112,p3` polyline plus per-AA radius/feather
maps. Native evidence says the renderer wants a quadratic `CSBezier` plus
pen-head hull polygons, but the still-missing piece is the
`CLIPStudioPaint.exe` compact-reader conversion from 120-byte point tails
(`+88/+96`, `+104/+112`) into native `CSVCurve` `P0/P1/P2` and any internal
flag bits. Next native work should inspect that compact-reader path before
changing AA constants or replacing the fallback with a guessed quadratic.

2026-06-04 compact `0x41` EXE reader follow-up: Windows r2 disassembly of
`CLIPStudioPaint.exe` resolved the 120-byte tail mapping. The stroke/list reader
`0x1422CF5F0` calls `0x142552AF0`, then `0x1422D0510` chooses a point-family
factory from the low byte of the compact flag. The dispatch is:
`flag&0x20 -> 0x142567730`, `flag&0x40 -> 0x1425677C0`, signed/high-bit
`-> 0x142567A00`, otherwise `0x142567850`. Thus compact `flags=0x41` is the
`0x40` extended curve family, not the signed `0x2081` spline family. The
`0x1425677C0` factory installs factory vtable `0x1444E93B0`, marks node size
`0x98`, and its allocation slot `+8 = 0x142567490` initializes nodes through
`0x142624430`; that initializer first runs the base point initializer
`0x1422C91A0`, then installs node vtable `0x1444EFB10` and clears
`node+0x78/+0x80/+0x88/+0x90`.

The shared point reader `0x1422CBAE0` still reads common fields first:
x/y doubles into `node+8/+0x10`, bbox integers into `+0x18..+0x24`, compact
flags into `+0x40`, floats into `+0x44..+0x6C`, and trailing dwords into
`+0x70/+0x74`. For the extended `0x41` node, its vtable tail-reader slot
`+0xD8 = 0x142625DE0` then reads four more doubles from the stream:
compact point `+88/+96` becomes `node+0x78/+0x80`, and compact point
`+104/+112` becomes `node+0x88/+0x90`. The point sampling slot
`+0x60 = 0x1426253E0` passes four points to `0x1431F9640`: current
`node+8`, first tail `node+0x78`, second tail `node+0x88`, and next-node
`+8`. In pseudocode:

```c
// compact flag low byte == 0x41
factory = CreateExtendedCurvePointFactory(); // node_size = 0x98
node = factory->alloc();                     // vtbl = 0x1444EFB10
ReadCommonPoint(node, stream);
node->tail0 = stream.read_double2();         // compact +88/+96
node->tail1 = stream.read_double2();         // compact +104/+112

sample(t, next) {
    return cubic_eval(node->xy, node->tail0, node->tail1, next->xy, t);
}
```

This corrects the native evidence: compact `.clip` `0x41` itself is a cubic
point family in the EXE reader. The earlier `iswCoreTG::RenderCurve` quadratic
evidence describes a later V4 `CSVCurve` renderer, not a direct replacement for
the saved compact record.

A scoped importer probe tested the obvious native-looking patch on the active
AA family: change the narrow dark `92/76/120/88 flags=0x41` fallback from the
current `p1=p0, p2=point+104/+112` to full reader cubic
`p1=point+88/+96, p2=point+104/+112`. It was rejected. Metrics with the probe:
`Vector_AA_None` unchanged at `0.036532/226`, `Vector_AA_Weak` worsened to
`0.044102/1207`, `Vector_AA_Medium` worsened to `0.027404/1794`, and
`Vector_AA_Strong` worsened to `0.025769/2142`, versus current
`0.036532/226`, `0.036818/1163`, `0.016958/1560`, and `0.020742/2070`.
`Test_Vector`, `Vector_SizePressure`, `Vector_OpacityPressure`, and
`Vector_Hardness_50` were unchanged by the probe. The direct patch was reverted.
Conclusion: tail0 is real native cubic state, but the remaining preview gap is
after reader mapping, likely in native curve flattening, endpoint/control
adjustment slots such as `0x142625E40`/`0x1426249B0`, or the brush/AA rasterizer
that consumes the sampled curve.

Native walker follow-up for compact `0x41`: the saved-vector walker
`0x1422CC1E0` does not consume only the simple point sample virtual. At
`0x1422CC54A` it calls the current node vtable `+0x70` and stores the returned
segment length in `xmm9`; for the compact `0x41` cubic node this slot is
`0x1426252B0`. That wrapper passes `(current.xy, current.tail0, current.tail1,
next.xy)` to `0x1431F87A0`, which in turn calls `0x1431F8330`. The helper first
evaluates the curve at quarter points, sums four chord lengths, then, when the
rough length implies more than four 4px buckets, resamples with
`min(int(rough / 4), 255)` segments.

The point walker then calls vtable `+0x68` at `0x1422CC5EF`; for compact `0x41`
this is `0x142625370 -> 0x1431F9AF0 -> 0x1431F9770`. Native pseudocode for the
active cubic branch is:

```c
double sample_cubic_by_distance(out vec2 *pt,
                                vec2 p0, vec2 p1, vec2 p2, vec2 p3,
                                double target_distance,
                                double segment_length) {
    int steps = clamp((int)(segment_length * 0.25), 4, 255);
    vec2 last = cubic(p0, p1, p2, p3, 0.0);
    double total = 0.0;
    for (int i = 1; i <= steps; i++) {
        double ti = (double)i / steps;
        vec2 cur = cubic(p0, p1, p2, p3, ti);
        double chord = hypot(cur.x - last.x, cur.y - last.y);
        if (total + chord >= target_distance && chord > tiny) {
            double t = ((i - 1) + (target_distance - total) / chord) / steps;
            *pt = cubic(p0, p1, p2, p3, t);
            return t;
        }
        total += chord;
        last = cur;
    }
    *pt = p3;
    return 1.0;
}
```

The auxiliary direction slot `+0x78 = 0x142625450` calls tangent helper
`0x1431F9F90`. It has endpoint-near branches: if `t` is tiny and
`node+0x40 & 0x400`, it uses the second stored control path; if `t` is near
one and `node+0x40 & 0x800`, it uses the first stored control path; otherwise
it computes and normalizes the cubic derivative. A direct parser check of the
current isolated `Vector_AA_*` compact `0x41` records found point flags
`[0x401, 0x0]` in all four files, so the first-point `0x400` endpoint tangent
special case is active for those AA strokes. This flag does not alter the
`+0x68` sampled position route.

A second scoped importer probe combined full native cubic controls
(`p1=point+88/+96`, `p2=point+104/+112`) with this native arc-length walk for
the narrow dark compact `0x41` fallback. It improved some metrics but failed as
a patch: `Vector_AA_Weak` became `0.034807/1143`, `Vector_AA_Medium`
`0.009327/1418`, while `Vector_AA_None` worsened to `0.040897/253` and
`Vector_AA_Strong` to `0.014118/2134`; `Test_Vector`, `Vector_SizePressure`,
`Vector_OpacityPressure`, and `Vector_Hardness_50` stayed unchanged. The probe
was reverted. Native cubic/arc-length is therefore part of the real path, but
the remaining AA mismatch sits in the native curve-to-brush consumer, endpoint
direction handling, or polygon/AA rasterizer, not in the compact reader alone.

2026-06-04 SizePressure writer/rasterizer continuation: a direct SQLite audit
fixed the style lookup mistake from the prior pass. The vector object tail
style field maps to `BrushStyle.MainId`, not `_PW_ID`. Current
`Vector_SizePressure` is a single `0x2081` stroke using tail `MainId=10`;
`BrushStyle.MainId=10` has `StyleFlag=0x1c240`, `AntiAlias=0`,
`PatternStyle=0`, and `TexturePattern=0`, with `StyleFlag&0x20` clear. Exact
`Vector_OpacityPressure` uses `MainId=11` with the same style flag bits, and
the isolated AA compact `0x41` samples also use `MainId=10` with
`StyleFlag&0x20` clear. Therefore sink vtable `0x1444E9110 + 0x20 =
0x14255C980` dispatches the ordinary `0x14255DFE0` branch for these fixtures,
not the alternate `0x142558A90` branch. The alternate branch remains a
material/retained-state style route for styles whose runtime `style+0x78`
has bit `0x20`.

The downstream ordinary path was reread in r2. `0x14255DFE0` still reaches
`0x14255E616 -> 0x14255E787 -> 0x14260F550 -> 0x14260DB90`. `0x14260F550`
is a mechanical prepared-plot/center/clip copy into the queue context, and
`0x14260DB90` dispatches the no-material case to `0x14263F410`. In
`0x14263F410`, the hard circular path is selected when AA/softness is off
and thickness ratio is approximately `1.0`; this calls `0x142640150`. The
`0x142640150` disassembly matches the current importer hard span formula:
center y is used as `cy - 0.5`, row span is
`sqrt(radius^2 - dy^2) - 0.4`, and x start/end are clamped before passing to
`0x14263AC30`. This re-closes generic hard-row coverage as the current
SizePressure cause.

No-edit runtime probes against the verifier-loaded importer module also
rejected sampled-point integerization as the current SizePressure gap.
Monkeypatching `_vector_sample_point()` to use float centers, floor, round, or
`int+0.5` leaves `Vector_SizePressure` unchanged at `max=226`,
`mean=0.024732`, `visible=153`, while exact `Vector_OpacityPressure`,
`Vector_Hardness_50`, and `Vector_SizeVelocity_50` remain exact. The float
center variant only gives a small diagnostic improvement for AA compact
`Vector_AA_Medium` (`mean=0.016958 -> 0.015083`, `visible=1560 -> 1506`).
Keep this as AA-specific diagnostic signal, not SizePressure semantics.

The same trace was re-evaluated through the current BrushEffectorGraphData.
Graph `5` is the identity line `(0,0)->(1,1)`. Graph `6` has five control
points ending in a zero tail, but the native-like graph evaluator still returns
only about `0.022..0.023` for the dominant segment `26` samples whose secondary
input is about `0.789..0.794`. Thus the `amount1=1.5` lane only multiplies
those tail radii by about `1.011`. Example current trace rows:
sample `206` has `primary=0.705463`, `secondary=0.791930`,
`graph6=0.022590`, `taper=0.713431`, radius `14.268628`; sample `209` has
`primary=0.475453`, `secondary=0.789320`, `graph6=0.023237`,
`taper=0.480977`, radius `9.619542`. This further demotes the old
`amount1=1.5 -> 1.4` improvement to a metric-only shrink diagnostic; the
largest residual cluster is not primarily a large secondary-curve boost.

A follow-up r2 pass closed the ordinary spline-control and scalar-sampling
suspects. `0x143204EA0` implements the same limiter as
`_native_spline_limit_control`: keep controls within squared distance `100.0`,
otherwise compare against `neighbor_distance_squared * 6.25` and clamp to
`max(10.0, sqrt(limit))`. `0x142626EE0` passes controls in the same order as
the importer (`limit(cur,next,prev)`, then `limit(prev,cur,prevprev)`, with the
same symmetric next-side calls) and gates them by node `+0x40 & 1`; current
SizePressure point flags are all zero. Re-reading `0x1422CC1E0` confirms that
native uses the same refined spline parameter returned by vtable `+0x68` for
both sampled center and compact scalar interpolation: node
`+0x44/+0x48/+0x4c/+0x54` becomes sample `+0x10/+0x18/+0x20/+0x30`, while
node `+0x58/+0x5c` becomes sample `+0x38/+0x40` unless the
`0x1000/0x2000` point flags force endpoint behavior. Current compact
`+52/+56/+60` are all `1.0`, so `0x142568040`'s optional `0x100` final-factor
branch through sample `+0x30` and the size/flow factor fields are value-neutral
for the remaining `Vector_SizePressure` `visible=153`.

No-edit diagnostics after this closure do not support tiny spline phase or
residual precision as a strong remaining lever. Globally perturbing
`_native_spline_t_at_distance()` by `+/-1e-6` or `+/-1e-5` is exactly neutral;
`+1e-4/+5e-4` improves only `153 -> 152`, while negative perturbations move to
`154`. Source-exec variants that quantise the cross-segment residual carry are
similarly weak: floor/round at `1e-3` or `1e-2` leaves the metric in
`152..154`, and even resetting residual to `0.0` every segment only reaches
`148`. An external helper proposed remaining suspects, but Codex audit
rejects its old `VECTOR_PRESSURE_SIZE_RADIUS_SCALE=1.05` route for the current
native no-pattern branch because the active code uses `native_radius_base =
width` before `_draw_native_dab_rgba`; the legacy constant was already
metric-neutral. The `0x31` composition-order idea is also low value here:
`0x142568040` already shows the multiplicative lane model and `amount0=1.0`.
The next high-value evidence is still a real native queue/eval trace hitting
`0x1422D8550` / `0x14260F550`, or another non-exact size-dynamic fixture that
shows a repeatable pattern.

Material colour fixed-point follow-up: after returning from the broad vector
triage, I re-read the current retained material row writer with r2 and ran a
current-state no-edit probe. `0x14263C3A0` calls `0x142637A70` or
`0x142637C70` to produce the row coverage and BGR components, then calls
`0x14263DDB0` to write/alpha-over retained row bytes. The cached two-lane path
still matches the earlier native conclusion: byte lane `1` gates/scales
coverage, byte lane `0` is expanded to a `0..0x8000` mix weight with
`(byte0 * 257 + 1) >> 1`, and BGR is blended fixed-point with
`+0x4000 >> 15`. `0x14263DDB0` then blends incoming colour against existing
row colour using fixed coverage; in the non-replace path it computes the new
retained alpha and blends each byte with `+0x4000 >> 15`.

The current no-edit probe monkeypatched only `_material_mix_preview_color()`,
leaving placement, coverage, and all other render code unchanged. Using the
native fixed-point blend with a rounded preview mix byte is exactly neutral:
`Vector_BrushTip_Material_Gap` stays `max=10`, `mean=0.023456`,
`visible=258`; ordinary `Vector_BrushTip_Material` stays `max=11`,
`mean=0.061113`, `visible=663`; baseline/opacity-pressure guards stay exact;
texture fixtures and paused SizePressure are unchanged. A floor mix byte
regresses the material pair (`Gap mean=0.024781`, ordinary `0.065875`). The
old diagnostic `+1R` still improves mean (`Gap 0.020594`, ordinary
`0.051981`) without improving ordinary visible pixels and without native
support, so it remains rejected. Save file:
`tmp_vector_probe/material_mix_fixed_probe_codex_20260604.json`. Conclusion:
the remaining material mismatch is not the global preview fixed-point colour
formula; it remains in per-pixel byte0/byte1 cached material mapping, UV phase,
retained row coverage, or final row flush/composite.

Material retained row-over no-edit follow-up: I then isolated the next closest
native-backed suspect, the alpha-over portion of `0x14263DDB0`. The current
importer `_draw_material_stamp_rgba()` does float straight-alpha over and
rounds back to 8-bit. The native writer keeps a retained 15-bit-ish alpha word:
for the normal non-replace path, it updates alpha as
`dst_alpha + ((0x8000 - dst_alpha) * src_alpha >> 15)`, then computes a
straight-RGB source ratio roughly `(src_alpha * 0x8000) / out_alpha` and blends
each row byte with `+0x4000 >> 15`. A no-edit monkeypatch replaced only
`_draw_material_stamp_rgba()` with this fixed-point row-over approximation,
leaving the current stamp placement, material alpha, preview mix colour, and
all non-material code unchanged.

The native-over variants are neutral in the current state. `round u8->15 +
round flush`, `(u8*257+1)>>1 + round flush`, and rounded opacity scaling all
leave `Vector_BrushTip_Material_Gap` at `max=10`, `mean=0.023456`,
`visible=258` and ordinary `Vector_BrushTip_Material` at `max=11`,
`mean=0.061113`, `visible=663`. `Vector_Baseline` and
`Vector_OpacityPressure` stay exact; `Vector_Texture`, `Vector_Texture_50`,
`Vector_SizePressure`, and `Vector_AA_Medium` are unchanged. A floor final
flush regresses material (`Gap mean=0.024538`, visible `265`; ordinary
`0.063637`, visible `758`). Save file:
`tmp_vector_probe/material_native_over_probe_codex_20260604.json`. Conclusion:
the current material residual is not explained by replacing float-over with
the native retained alpha-over rounding. The next meaningful material work
should recover real row-stage UV/cached byte0/byte1 mapping or the full
`0x142641C60 -> 0x14263B7F0 -> 0x14263E970` pipeline, not tune final
over/flush arithmetic.

Material UV footprint recheck: a final scoped no-edit probe varied only the
four sample offsets inside `_draw_material_wide_stamp_rgba()`. This reconfirms
the current half-step footprint recovered from `0x142641C60` and
`0x14263E970`. The current `(u,v)`, `(u+xstep/2,v)`, `(u,v+ystep/2)`,
`(u+xstep/2,v+ystep/2)` stays at Gap `0.023456/258` and ordinary
`0.061113/663`. The older full-x/half-y footprint regresses to Gap
`0.113162/775` and ordinary `0.259887/1334`; full-square, centered-quarter,
and same-lane/double-x variants regress further. Save file:
`tmp_vector_probe/material_uv_footprint_probe_codex_20260604.json`.
Conclusion: do not reopen material UV sample offsets. The remaining material
work is full cached-lane row mapping or deeper material submission state.

AA compact `0x41` consumer-shape rejection: after material was narrowed, I
returned to the isolated `Vector_AA_None/Weak/Medium/Strong` compact `0x41`
family. Existing native evidence says the compact reader's `+88/+96` and
`+104/+112` tails are real cubic state and that V4 curve rendering eventually
uses pen-head farthest points and `FillPolygon`, but direct cubic/arc-length
patches already regressed. A current no-edit probe therefore changed only the
consumer under `_draw_polyline_rgba()` while keeping importer parsing, current
sample points, radius/feather constants, and unrelated branches unchanged.
Replacing the distance-field polyline consumer with a one-shot envelope polygon,
per-segment hull rectangles, or capped envelope polygon all regressed the AA
family. One-shot envelope changes `Vector_AA_None` from `0.036532/226` to
`0.111214/688`, `Weak` from `0.036818/1163` to `0.083151/1342`, `Medium` from
`0.016958/1560` to `0.084422/1848`, and `Strong` from `0.020742/2070` to
`0.084952/2268`. Save file:
`tmp_vector_probe/aa41_consumer_shape_probe_codex_20260604.json`.

A companion probe replaced the current feather formula with supersampled hard
capsule scan conversion at 2x/4x/8x. That also regresses the AA branch: 4x
hard supersampling gives `Weak 0.080030/1439`, `Medium 0.076334/1857`, and
`Strong 0.079029/2260` versus current `0.036818/1163`, `0.016958/1560`, and
`0.020742/2070`. `Vector_AA_None` stays the same in the AA-only supersampling
variants because the no-AA path remains delegated to the current hard branch.
Save file:
`tmp_vector_probe/aa41_supersample_polyline_probe_codex_20260604.json`.
Conclusion: do not change AA compact `0x41` by only swapping in envelope,
segment-hull, or supersampled capsule rasterization. The native gap is deeper:
the real `CSVStroke`/`CSVCurve`/corner/pen-head state or Planeswalker compact
consumer must be recovered before replacing this fallback.

AA compact `0x41` branch-placement correction: I re-read the EXE compact
factory/vtable path with Windows r2 and dumped the evidence to
`tmp_r2_csp/aa41_compact_factory_consumer_pd_20260604_codex.txt` and
`tmp_r2_csp/aa41_extra_vslots_pd_20260604_codex.txt`. The vtable at
`0x1444EFB10` includes the previously accepted sample/length/tangent slots and
extra mutation/serialization slots: `+0x100=0x142625E40`,
`+0x108=0x142625260`, `+0x110=0x142625820`, `+0x118=0x1426244B0`. The
interesting `+0x118` helper builds/splits cubic controls via `0x1431FCB70` and
propagates endpoint flags `0x400/0x800`, but byte search for direct
`call [rax+0x118]` found only `0x14018E228` and `0x140193FDA`. Both contexts
look like editor/command coordinate manipulation with temporary object cleanup
and hash/formatting work, not the saved render path around `0x1422CC1E0`. So
`+0x118` remains real native structure, but not a live cause for the AA export
residual.

The importer-side branch boundary was also corrected. The narrow dark
`Vector_AA_*` compact `0x41` objects are rendered by the early filled-curve
branch in `clip_loader.py`, not by the later no-pattern native-dab branch. The
active importer condition is `92/76/120/88`, `flags=0x41`, dark RGB, and
`width<=16`; it uses raw/raw-or-rounded points, current `p1=p0`, current
`p2=point+104/+112`, and branch constants
`VECTOR_FILLED_CURVE_*_SAMPLE_SHIFT`, radius scale, and feather. The later
native-dab path only accepts `0x2011/0x2081`, so adding full cubic tail support
there is dead for these samples. A temporary runtime counter on
`Vector_AA_Weak` confirmed this: `_native_cubic_point_from_controls` was
called `0` times, while `_draw_native_dab_rgba` was called `220` times by other
stroke work. That temporary patch was reverted.

Fresh detail dump
`<project-root>\tmp_vector_probe\aa41_detail_codex_20260604.json`
confirms the four AA-family `0x41` records are `brush_style_id=10`,
`width=3.0`, two points, point flags `[0x401, 0x0]`. The native cubic tails are
not absent: p0 tail0 equals p0, p0 tail1 is the forward control; p1 tail0 is a
far return control, and p1 tail1 equals p1. The second point `f32+76` also
tracks the AA family numerically (`None=0`, `Weak=0.129946`,
`Medium=0.602505`, `Strong=0.820052`), but the old native field-map evidence
still maps it to internal `+0x6c` sampler persistence/carry, overwritten by
`0x1422CC1E0(a8=1)` for active rendering. Treat that AA correlation as a
diagnostic clue only, not renderer semantics.

A small AA-width/radius diagnostic on the actual native-dab path also rechecked
that this is not a generic `_native_aa_width` cap or all-native-dab radius
problem. Changing caps from current `{1:1.5,2:2.5,3:3.5}` to
`{1:1,2:2,3:3}`, `{1:1.25,2:2.25,3:3.25}`,
`{1:1.75,2:2.75,3:3.75}`, or `{1:2,2:3,3:4}` regresses AA and breaks exact
guards such as `Vector_Baseline`/`Vector_Hardness_50`; broad native-dab radius
scales `0.95`/`1.05` heavily regress the guard matrix. Keep the target on the
filled-curve dark branch itself: native cubic tail use, sample phase, and its
real Planeswalker/CSVec4 consumer, not the no-pattern dab path.

## 2026-06-05 Test_Vector V4 Wide 0x41 Scanline Boundary

After the retained balloon material route was bounded in the native workspace,
`Test_Vector.clip` was reselected as the next narrow target because the active
large wide-`0x41` V4 pen-head branch is isolated to this sample. Current metric
is `mean=0.610175`, `visible=24477`; current guard metrics are still
`Vector_OpacityPressure=0/0`, `Vector_OpacityVelocity_50=0/0`, and
`Vector_SizePressure=0.024732/153`.

The relevant saved r2 dumps are:

- `<reverse-workspace>\tmp_r2_iswcore\v4_renderstroke_curve_fill_store_pdf_20260605_codex.txt`
- `<reverse-workspace>\tmp_r2_iswcore\v4_initialize_pdf_20260605_codex.txt`

Native `CSVec4Draw::Initialize @ 0x124667C0` seeds
`draw+0x158 = max(1, int(offscreen_resolution * 0.15 / 25.4 * 16))` and
internal scalar fields `+0x15c=3.2`, `+0x160=8.8`, `+0x164=6.4`.
`CSVec4Draw::RenderStroke @ 0x1246A630` reads the stroke base radius from
`CSVStroke+0x58` and can apply pressure/min-radius logic before calling
`StorePenHeadPoint` and `FillPolygon`. The nearby `-1.5` constant is only used
for conservative bbox expansion, not as a pixel radius rule.

For the default circular pen head, `CSVec4Draw::StorePenHeadPoint @
0x1246BD30` computes a dynamic point count using radius and `draw+0x158`, then
clamps to `n=32` for large radii. That means the importer's current `16`
half-cap segments for the large `Test_Vector` capsule are native-backed, not
just a metric guess.

The remaining wide-capsule mismatch is therefore more likely
`CSVec4Draw::FillPolygon @ 0x12465250`: native 12-bit fixed-point edge
conversion, half-open scanline/row behavior, and byte alpha/composite branches.
Do not keep tuning `radius=width*0.99`, the 16 half-cap count, or bbox constants
as semantics. The next meaningful code experiment should be a narrow native
scanline/alpha probe for the wide-`0x41` polygon path, not another scalar
capsule sweep.

## 2026-06-14 Clipped Base Cache Ownership

Native evidence in the adjacent reverse workspace showed that a base layer and
its clipped siblings resolve through a base-owned cache, not through the
already-accumulated group buffer. The importer now routes every raster base
followed by clipped siblings through `_render_clipping_group`, not only
non-Normal bases. The base cache must use the base layer's masked alpha; using
raw base alpha caused false opaque linework in Terra's upper hair pixels.

Verification on `Ref_Terra404_Live2D.clip` improved the full-image
premultiplied max diff from 85 to 14 and reduced the old eye/highlight/lower
skirt probe points to 0-2/255. The remaining largest Terra residuals are small
bottom-edge semi-transparent green/cyan color differences. Guard samples
`Test_Mask`, `Test_ClippingEdge`, `Test_AddGlowMultiply`, and `Test_ToneCurve`
remained stable after the change.

Follow-up on the bottom-edge residual showed that it was not caused by the
Add/Add Glow formula itself. The affected pixels passed through only a small
number of Add Glow layers. The remaining error was in clipped Add/Add Glow
preserve strength: the importer used clip-attenuated `effective_strength`,
which weakened colour inside the base-owned clipping cache. Switching that
branch to source/mask strength reduced Terra's full-image premultiplied max
diff from 14 to 4 while keeping the same guard samples stable.

## 2026-06-15 Straight Uint8 Compositor Migration

The importer now uses straight `uint8` RGBA as the main compositor buffer. This
matches CSP's byte-domain caches better than premultiplied float because RGB
can remain meaningful when alpha is zero; fresh canvases and isolated clipping
caches are initialized as transparent white (`RGB=255`, `A=0`). The legacy
premultiplied-float compositor branch remains only for compatibility with
debug/selected-render callers that pass float buffers.

The Add/Add Glow path now writes the native byte blend result directly back into
the straight buffer, including the alpha channel from `_blend_add_u8()` or
`_blend_add_glow_u8()`. This removes the old premul-to-straight-to-premul
round-trip at the main compositor boundary, while still keeping existing
sample-backed blend formulas for modes whose real native call path is not fully
proven.

An audit suggested wiring recovered `RenderBlendModeCall` formulas for
Multiply, Overlay, Soft Light, Color Dodge, and Color Burn. Follow-up corrected
the Color Dodge mapping: case 260 is not Color Dodge; `LayerComposite=9`
Color Dodge is case 516 and uses the Photoshop-style dodge currently in the
importer. Do not replace Color Dodge with the case 260 formula
`255*(s+d-255)/s`. Alpha-weighted Soft Light/Multiply-style formulas still
regressed `Test_SoftLight` and `Test_AddGlowMultiply`, so keep those formulas
out of the importer until the exact internal case mapping and call-site
argument evidence are recovered for those samples.

Verification after the straight-buffer migration:

- `Test_Clipping`, `Test_Mask`, `Test_ClippingEdge`, `Test_ColorDodge`, and
  `Test_ColorBurn`: exact.
- `Test_AddGlow`: premultiplied max `1`, visible `0`.
- `Test_SoftLight`: premultiplied max `1`, visible `0`.
- `Test_AddGlowMultiplyClipping`: premultiplied max `1`, visible `0`.
- `Test_ToneCurve`: unchanged known residual, premultiplied max `17`.
- `Test_AddGlowMultiply`: premultiplied max `3`, visible `1016540`.
- `Ref_Kabi_Live2D`: premultiplied max `208`, visible `47889`. The prior
  staged loader has the same max pixel `(1372,782)` and value, so this is a
  pre-existing white-eye semantic gap rather than a straight-compositor
  regression; the straight compositor reduces Kabi visible premul-diff pixels
  from about `239469` to `47889`.

Follow-up reverse-analysis resolved the Color Dodge contradiction. Ordinary
layer Color Dodge does pass through `RenderBlendModeCall`, but it maps to case
516, not case 260. Case 516 computes the Photoshop-style dodge target
(`d*255/(255-s)`, clamped to `255` when `s+d>255`) and then blends toward it
with source alpha. This matches `Test_ColorDodge.clip`: at pixel `(282,64)`,
the upper layer is `s=[84,51,250], a=255`, the destination before the layer is
`d=[226,226,226], a=255`, and CSP's reference output is `[255,255,255]`.
The case 260 formula `255*(s+d-255)/s` gives `[166,110,225]`, proving only
that case 260 was misidentified as Color Dodge. Keep Color Dodge on the current
formula.

A paired experiment then changed the straight compositor only for
Multiply/Overlay/Soft Light to use alpha-weighted recovered helpers plus the
`LABEL_149`-style outer writeback `out_pm = blended * dst_a + src_pm *
(1-dst_a)`. This still regressed the guards and was reverted:

- `Test_SoftLight`: from premultiplied max `1`, visible `0` to max `8`,
  visible `24993`.
- `Test_AddGlowMultiplyClipping`: from premultiplied max `1`, visible `0` to
  premultiplied max `51`, visible `26544`.
- `Test_AddGlowMultiply`: from premultiplied max `3` to premultiplied max
  `30`.

The cleanest Soft Light counterexample is opaque, so the outer alpha writeback
cannot explain it away. In `Test_SoftLight.clip` at `(315,72)`, the source is
`[84,51,250,255]`, the destination before the Soft Light layer is also
`[84,51,250,255]`, and CSP's reference is `[65,27,252,255]`. The current
W3C-style Soft Light path produces `[65,27,252]`; the recovered pow helper
produces `[57,19,252]`. Ask the reverse workspace to re-check whether
`LayerComposite=15` maps to the claimed internal case, whether the pow
parameter/sign is reversed or quantized differently, and whether this helper is
for a different Soft Light variant.

After reverse follow-up confirmed case 770 is a non-raster Soft Light variant,
a narrower experiment left Soft Light on the current W3C formula and enabled
the recovered `LABEL_149`/alpha-weighted path only for Multiply and Overlay.
This also regressed and was reverted:

- `Test_AddGlowMultiplyClipping`: from premultiplied max `1`, visible `0` to
  premultiplied max `51`, visible `26544`.
- `Test_AddGlowMultiply`: from premultiplied max `3` to premultiplied max
  `30`.
- `Test_SoftLight` stayed stable, confirming this second regression comes from
  the Multiply/Overlay trial rather than Soft Light.

`Test_AddGlowMultiplyClipping` is the sharp guard here: its clipped Multiply
stack is within one level using the current preserve path, but breaks badly
when case 257 plus `LABEL_149` writeback is applied to that clipped-cache
context. Ask the reverse workspace to verify whether clipped siblings use the
same case 257 path, whether the destination passed to case 257 is the base cache
or a different below-cache view, and whether the preserve/cache resolve path
applies Multiply outside the ordinary `RenderRGB100_32bit` LABEL_149 writeback.

A further scoped attempt tried to use case 257 only for non-clipped
`MULTIPLY` layers with `LayerOpacity < 256`, leaving clipped Multiply and
opaque Multiply on the current W3C path. This preserved
`Test_AddGlowMultiply` and `Test_AddGlowMultiplyClipping`, but regressed
`Ref_Terra404_Live2D`: Terra contains three non-clipped semi-opacity Multiply
layers (`456` opacity `230`, `625` opacity `179`, `670` opacity `179`), and
the trial changed Terra from the current premultiplied max `4` to `9`
(`premul_visible_px=46877`). The trial was reverted. Therefore the safe
condition is not simply "non-clipped and opacity < 256"; those Terra layers are
real raster CSBitmapLayer cases and still prefer the current W3C-style path in
the importer. Ask the reverse workspace to determine what additional native
dispatch/context distinguishes case 257 users from these Terra layers.

Reverse follow-up then resolved the case 257 confusion: `CreateLayer` case
`0x101` constructs `CSToneLayer`, not an ordinary `CSBitmapLayer` Multiply
raster layer. Treat the recovered case 257 masked-alpha multiply as a tone/filter
layer path until native evidence proves otherwise. Do not apply it to ordinary
`LayerComposite=2` raster Multiply.

An `importer-audit-round2` spike also tried to replace the remaining
premultiplied helper semantics around THROUGH/filter opacity with more
straight-uint8-like behavior. Both attempts were rejected and reverted:

- Replacing `_blend_straight_toward` with direct straight RGBA interpolation did
  not improve the guard samples. On isolated Terra THROUGH+mask groups it
  produced very large old-vs-new differences (`max` up to `254`) because
  transparent-white cache RGB gets linearly mixed into masked edges. Without a
  native reference for that exact intermediate cache, this is too risky.
- Replacing the NORMAL path with an integer `EffectCacheResolve`-style
  alpha-over helper regressed guard samples: `Test_Saturation` went from
  `max=2` to `max=10`, and `Test_ToneCurve` went from `max=17` to `max=68`.

The current float premultiplied NORMAL and `_blend_straight_toward` paths should
therefore stay in place until the reverse workspace identifies the exact
`EffectCacheResolve` input buffers, rounding rules, and THROUGH mask/opacity
call context.

### Kabi Clipped Folder And Hidden Sibling Cache Fix

`Ref_Kabi_Live2D` exposed two clipping-cache bugs that were separate from the
blend-mode formula work.

First, the base eye-white layer is followed by a clipped sibling that is itself
a folder. The importer originally tried to decode that sibling as a direct
raster layer, got `None`, and skipped the folder's children. IDA supports the
fix: `CSGroupLayer::Render @ 0x122f1380` creates a same-size child offscreen,
initializes its transparent RGB to white (`[offscreen+0xAC] = 0xFFFFFF`), then
iterates child pointers and calls each child's virtual `Render` via
`vtable+912`. A clipped folder sibling is therefore a recursive render target,
not a direct raster mipmap.

Second, after the folder recursion fix, the remaining `premul_visible_px=37948`
was dominated by a hidden clipped sibling in the face chain. At `(1584,648)` the
importer wrote layer `289` into the clipped base cache even though its
`LayerVisibility` bit 0 was off; CSP's reference stayed on the visible face
base. Native render entry points support skipping it: `CSBitmapLayer::Render @
0x122d6b20` immediately returns when `this+123` is false, and group rendering
delegates to each child's own render method. `_render_clipping_group` now skips
invisible siblings, and `_render_chain` only creates an isolated clipping group
when the clipped run contains at least one visible clipped sibling.

Verification after the visibility fix:

- `Ref_Kabi_Live2D`: `premul_max 64 -> 47`, `premul_visible_px 37948 -> 1119`.
- `Test_ClippingEdge`: exact.
- `Test_AddGlowMultiplyClipping`: premultiplied max `1`, visible `0`.
- `Test_SoftLight`: premultiplied max `1`, visible `0`.
- `Test_ColorDodge`: exact.
- `Test_AddGlowMultiply`: unchanged known residual, premultiplied max `3`.

### HSL Filter Payload Scaling Follow-up

The reverse workspace evidence for `sub_123FC180 @ 0x123FC180` identifies CSP's
per-pixel HSL routine as a fixed-point HSV adjuster with luminosity/saturation
coupling: positive luminosity desaturates, positive saturation brightens by the
saturation increment, and negative saturation darkens at half rate. Applying
that routine's fixed-point divisors directly to the SQLite `FilterLayerInfo`
payload did not match `Test_HSL.clip`: the payload is `(-24, 35, 26)`, and
treating all three values as native fixed-point arguments left the image almost
unadjusted (`raw_mean=11.288223`, `raw_max=100` against the CSP PNG).

The sample-backed mapping for the SQLite payload is instead mixed: hue is stored
as UI degrees (`/360`), luminosity is stored as UI percent (`/100`), and
saturation remains on the native fixed-point scale (`/32768`) for this sample.
Keeping CSP's per-pixel lum/sat coupling with that payload mapping reduces
`Test_HSL` from the old all-pixel drift (`raw_mean=36.277059`, `raw_max=86`) to
`raw_mean=0.557394`, `raw_max=59`, with `premul_visible_px=47638`. The remaining
max is localized around a semi-transparent gray-over-base region such as
`(161,253)`, where the pre-filter composite is `[174,161,144]` but CSP is closer
to `[254,162,144]`; treat that as a separate compositing/filter-scope or alpha
quantization investigation, not as evidence to restore the old HSL formula.

2026-06-17 fixture triage rechecked the user's prioritized adjustment/filter
samples after the all-`img` comparison pass:

- `Test_HSL.clip` remains `raw_max=59`, `premul_max=59`. A temporary probe that
  changed the SQLite saturation payload scale from `/32768` to `/100` made the
  whole image worse (`raw_max=80`, `raw_mean=28.418291`, nearly all pixels
  visible), even though it appears attractive at the local max pixel. Layer 2's
  raw CHNKExta tile bytes at the max are genuinely gray (`alpha=174`,
  BGRA=`[137,137,137,0]`) and the HSL layer has only the 20-byte
  `FilterLayerInfo` blob, with no extra colorize/mixing field. Do not accept a
  saturation-scale change without new CSP-native evidence.
- `Test_ToneCurve.clip` remains `raw_max=17`, `premul_max=17`. At the max,
  pre-filter RGB is `[114,186,234]`, current output is `[157,249,255]`, and CSP
  is `[174,250,255]`. The red channel's per-channel LUT maps `114 -> 175`, but
  the master LUT maps that to `157`; simple permutations of curve block order,
  master-before-channel, master-after-channel, or omitting individual channels
  all lose badly at full-image scale. Keep the current `master(R/G/B(input))`
  path until the B-spline table generation or native rounding is recovered more
  precisely.
- `Test_Gradiation.clip` remains `raw_max=10`, `premul_max=10`. At the max,
  pre-filter RGB is `[255,77,79]`, current Gradient Map output is
  `[157,175,179]`, and CSP is `[147,173,180]`; the CSP color is close to the
  current LUT around index `126` while the current `0.3/0.59/0.11` luminance
  gives index `130`. Luminance-weight sweeps did not find a clean improvement:
  alternatives can reduce mean/visible drift but increase max, or keep max while
  providing no meaningful improvement. Keep the current weights unless a native
  luminosity-index formula is found.

2026-06-17 hue-only follow-up: public Photoshop documentation describes the
Hue/Saturation sliders semantically and documents `Colorize`, but does not
publish the ordinary Master Hue pixel formula. The commonly cited public
reverse-engineered Photoshop formula covers the `Colorize` checkbox path rather
than normal hue rotation, and general HSL/HSB definitions only establish that
hue is a 0..360-degree angle. The new `Test_HSL2.clip` fixture isolates hue:
the HSL payload is `[-26, 0, 0]`, and native strict GPU compares to CSP at
`raw_max=1`, `raw_visible_px=0`, `premul_max=1`, `premul_visible_px=0`.
Therefore the current ordinary Master Hue mapping (`payload_hue / 360.0`, same
rotation direction) is sample-backed. Do not replace hue with `/196608` in the
SQLite payload path; that divisor belongs to CSP's internal fixed-point routine
after caller-side normalization, not to this saved payload.

2026-06-17 HSL saturation/luminosity fixture follow-up: new isolated fixtures
supersede the earlier saturation-scale rejection, which only tested `/100`
without CSP's value-overshoot rescale and neutral-pixel guard. Payloads are
`Test_HSL3` `[0, 19, 0]`, `Test_HSL4` `[0, 0, 20]`, and `Test_HSL5`
`[-47, 37, -19]`. The saturation-only reference proves the saved SQLite
saturation payload is UI percent for these files: `/32768` leaves the output
almost unchanged (`Test_HSL3 raw_max=20`, `visible_px=118233`), while `/100`
with the native positive-saturation value coupling matches non-neutral colours.
Two native rules are required to make that general instead of a local fix:
ordinary saturation must not colorize neutral grayscale pixels (`S == 0`), and
when `V + V*inc` exceeds 1.0 the saturation increment is rescaled by the
available value headroom before clamping `V` to 1.0. With those rules, native
strict GPU compares at `Test_HSL3 raw_max=1`, `Test_HSL4 raw_max=1`,
`Test_HSL5 raw_max=1`, and the older combined `Test_HSL` improves from
`raw_max=59` / `premul_visible_px=47638` to `raw_max=3` /
`premul_visible_px=12696`.

2026-06-17 old-HSL rounding follow-up: the older `Test_HSL.clip` still had
`raw_max=3` after the isolated HSL2-5 formula fixes, while both Python verifier
and strict native GPU agreed on the same residual. A selected render of layers
`3,5` plus filter `13` reproduced the full-image residual exactly, and the HSL
filter layer mask is a constant `255`, so this is per-pixel HSL quantization
rather than filter mask/opacity blending. The `FilterLayerInfo`/PSD `hue2`
payload is `(-24, 35, 26)`.

Rejected probes: applying saturation before luminosity matches one old max
pixel but regresses the full old sample to `raw_max=23` and breaks
`Test_HSL5` (`raw_max=26`); removing the positive-luminosity desaturation is
much worse; small saturation/luminosity scale tweaks can lower the old max to
`2` but have no native evidence and are sample tuning; a direct integer rewrite
from the available fixed-point notes is incomplete and regresses combined HSL
guards. The accepted narrow rule is only the final HSV-to-RGB output
quantization: CSP's fixed-point path reaches channel bytes through right-shift
style truncation, not round-to-nearest. Changing `_hsv_to_rgb_u8` and the native
HSL filter shader to floor/truncate the final RGB values improves old
`Test_HSL` from `raw_mean=0.499208`, `premul_visible_px=12696` to
`raw_mean=0.014346`, `premul_visible_px=2668`, with `raw_max=3` unchanged.
Native guards stay stable: `Test_HSL2` is exact, while `Test_HSL3`,
`Test_HSL4`, and `Test_HSL5` remain `raw_max=1` / `visible_px=0`.

2026-06-17 Tone Curve isolated fixture follow-up: the user's new small
`Test_ToneCure*` fixtures split the compact Tone Curve payload by channel.
`Test_ToneCure2` contains only the master/RGB curve, `Test_ToneCure3` only the
R curve, `Test_ToneCure4` only the G curve, `Test_ToneCure5` only the B curve,
and `Test_ToneCure6` contains the combined master+R+G+B payload matching the
older `Test_ToneCurve` curve records. Both native strict GPU and the Python
verifier compare all five at `raw_max=1` with `visible_px=0`. Therefore
single-curve LUT generation, per-channel application, and the ordinary
master-after-channel ordering are sample-backed on the small isolated matrix.
`Test_ToneCurve_WithoutToneCurve.clip` removes the filter layer from the older
large sample and compares exact (`raw_max=0`), proving that sample's base-layer
composition is not the source of the residual. Re-exporting
`img/Test_ToneCurve.png` from CSP after the manual Difference check still
compares at `raw_max=17` / `premul_max=17` against native strict GPU output.
The separate `img/Test_ToneCurve.clip.png` file compares exact to the native
PNG, but it is not the CSP export oracle for this fixture. The manual CSP
Difference check with an imported native-output layer showing black therefore
needs a separate import/color-management/display-path explanation; it should not
be treated as proof that the raw PNG comparator is wrong. A follow-up profile
sanity check found CSP reports both embedded and working profiles as
`sRGB IEC61966-2.1`, perceptual intent, `IccLibrary`; tagging the native
diagnostic PNG with the CSP export's sRGB ICC profile still looked visually
black in CSP Difference mode, but eyedropper samples in the same area confirmed
distinct colours such as `#FFFED1` versus `#FFFFCA`. Therefore the manual
Difference display path is not sensitive enough for this low-amplitude residual;
use raw PNG byte comparison and sampled pixels for the oracle until native CSP
evidence explains the remaining Tone Curve/cache quantization gap.

2026-06-17 native Multiply edge quantization follow-up: a local isolated
Multiply fixture showed native strict GPU was slightly worse than the Python
reference compositor on semi-transparent Multiply edges. At `(476,158)`, the
destination was paper `[226,226,226,255]`, the Multiply source was
`[147,97,187,104]`, CSP exported `[186,168,200,255]`, Python produced
`[187,169,201,255]`, and the old native shader produced `[187,169,202,255]`.
Full-image native comparison was `raw_max=2`, `raw_visible_px=73`, while Python
was `max=1`, `visible_px=0`. Offline variants showed the best non-overfit rule
is to keep the W3C Multiply product unquantized before the alpha-over step while
leaving the final u8 output rounding in place; this matches Python and brings
native to `raw_max=1`, `raw_visible_px=0`. The change is scoped to Multiply in
both the standard shader and tile-silo shader; other standard blend modes keep
their existing pre-over u8 target quantization. Guards stayed stable:
`Test_AddGlowMultiply` remains `raw_max=5` / `premul_max=3`, `Test_SoftLight`
and `Test_ColorBurn` remain exact, `Test_ToneCurve` remains the known
`raw_max=17`, and `Test_Clipping` remains exact.

2026-06-17 Tone Curve IDA compact/runtime boundary follow-up: live IDA MCP was
connected to `iswCoreTG.dll` and `iswCmnTG.dll` to re-check the proposed
16-bit/33-sample Tone Curve fix. `CSLayerFilterData::CreateLookUpTableToneCurveStatic
@ 0x12304050` consumes already-expanded runtime `TONECURVEDATA`, creates a
256-entry `RCLookUpTable`, and calls imported `rtGetBsplineIntTable(points,
count, table, 256)`. The runtime points are byte-domain `tagPOINT` integers:
the function tests endpoints against `0`/`255`, clamps the final byte table, and
does not consume the SQLite compact `uint16 count + uint16 point pairs` payload
directly. `CSAdjustmentLayer_ReadSelf @ 0x122bee00` confirms the archive layout
for this runtime structure is `int32 count + tagPOINT[32]`, with 260-byte
blocks for master/R/G/B. `rtGetBsplineIntTable @ iswCmnTG:0x1216b9c0` really is
a quadratic B-spline helper with mirrored boundary controls, 33 samples
(`t=i/32`), span divided by the requested table size (`/256` here), line
segment fill, and `+0.5` rounding. However, replaying that runtime helper
directly on the SQLite compact payload after byte scaling is rejected by
samples: `Test_ToneCurve` worsens from `max=17`, `visible=6107` to
`max=67`, `visible=1012389`, and the isolated `Test_ToneCure2`/`6` guards lose
their `raw_max=1`, `visible_px=0` result. The current compact-payload importer
path remains the best sample-backed rule: compact coordinates are converted with
`ceil(value / 257)`, then the existing byte-domain B-spline table is used. The
IDA runtime helper is evidence for expanded archive/runtime data, not a direct
replacement for compact SQLite filter payload interpretation.

The old `Test_ToneCurve` residual is now narrowed further. At the max pixel
`(370,96)`, CSP without the Tone Curve layer is `[114,186,234,255]`, current
native/Python output is `[157,249,255,255]`, and CSP export is
`[174,250,255,255]`. The compact curves include near-vertical byte-domain
steps, for example master `(109,45)->(110,223)` and red `(114,97)->(115,255)`.
Current red evaluation at that pixel is `114 -> red LUT 175 -> master LUT 157`,
and no input value maps to `174` under the current full-strength LUT chain.
Because `Test_ToneCure6` uses the same compact curve records and still compares
at `raw_max=1` / `visible_px=0`, the remaining old-sample gap should be treated
as an unrecovered compact-expansion/runtime-context quantization edge, not as a
global Tone Curve order, 16-bit-domain, or IDA-runtime-B-spline replacement.

2026-06-17 Tone Curve compact `SAdjustmentCurves` closure: the previous
`rtGetBsplineIntTable` rejection remains valid for the runtime archive helper,
but it was not the SQLite compact payload path. Live IDA MCP was switched to
`CLIPStudioPaint.exe` because `FilterLayerInfo` and the Planeswalker filter
registration strings are not owned by `iswCoreTG.dll` or `iswCmnTG.dll`.
`FilterLayerInfo @ 0x1444e7660` is only registered by `sub_1400CA020`; the
Tone Curve compact payload path is Planeswalker `SAdjustmentCurves`.
`sub_1424C5930` case `3` initializes Tone Curve defaults via `sub_141A1D800`,
copies `4160` bytes (`32 * 0x82`), and installs the filter behavior through
`sub_1423EFFA0`. `sub_141A1D3A0` imports exactly 32 compact records of
`uint16 count + 32 * (uint16 x, uint16 y)`, and `sub_141A1DBC0` exports the
same raw u16 sequence.

The render-side compact path is `sub_1423F2570`. It builds the master table
from record `0` through `sub_141A1D640 -> sub_141AB20F0`, then builds RGB
channel tables from records `1..3` through `sub_141A1D460 -> sub_141AB20F0`.
`sub_141AB20F0` allocates a 65536-entry u16 `PWLookupTable`; its helper
`sub_141AB24B0` uses mirrored boundary controls, 33 samples per segment
(`t=i/32`), line-segment fill, gap fill, and `+0.5` rounding in 16-bit space.
`sub_141A80690` composes each channel table through the master table in the
same 16-bit domain, then builds the byte table by sampling `input * 257` and
taking the high byte. Therefore the old importer bug was the early
`ceil(value / 257)` byte-domain compact expansion, not an unknown packed
payload transform. Implementing this path in `clip_loader.py` and native
`clip_runtime::filter_lut` makes `Test_ToneCurve` and isolated
`Test_ToneCure2`..`Test_ToneCure6` exact in both Python verifier and strict
native GPU output. The old max pixel `(370,96)` now maps pre-filter
`[114,186,234]` to CSP's `[174,250,255]`. Guards stayed stable:
`Test_Gradiation` remains `raw_max=10` / `premul_max=10`, and
`Test_AddGlowMultiply` remains `raw_max=5` / `premul_max=3`.

2026-06-17 Color blend luminosity follow-up: `Test_Color.clip` showed a
shared Python/native residual (`raw_max=2`, `visible_px=12`) rather than a
strict-GPU parity bug. The max pixels are ordinary `LayerComposite=25` Color
blend over an opaque base, with a fixed source `[84,51,250,255]`; an example
destination `[206,201,229]` produced old output `[206,196,255]` while CSP
exported `[206,198,255]`. Formula probes rejected Rec.709 for this blend family
because it made `Test_Color` broadly worse (`visible_px=16576`). Changing only
the non-separable HSL blend luminosity coefficients from `0.3/0.59/0.11` to
`0.3/0.6/0.1` improves `Test_Color` to `raw_max=1` / `visible_px=0`, leaves
`Test_Hue` at `raw_max=1` / `visible_px=0`, and improves `Test_Saturation`
visible pixels from 31 to 9 while keeping the same `raw_max=2`. `Test_SoftLight`,
`Test_ColorBurn`, and `Test_ColorDodge` guards remain stable/exact. IDA constant
searches in `iswCoreTG.dll` found double constants for `0.3`, `0.6`, and `0.1`,
but not `0.59`, `0.11`, or Rec.709; xrefs did not close the raster-blend call
chain, so keep this as sample-backed plus constant-backed evidence rather than
a fully recovered native blend function. This correction applies only to the
HSL blend `lum()` helper, not `color_compare_lum` for Darker/Lighter Color and
not Gradient Map's grayscale index.

2026-06-17 Saturation blend high-clamp follow-up: after the HSL luminosity
weight fix, `Test_Saturation.clip` still had a small shared Python/native
residual (`raw_max=2`, `visible_px=9`). Replaying the stack shows all visible
pixels are ordinary opaque Saturation blend with the same source
`[84,51,250,255]`; the residual is not source sampling, alpha, masks, or layer
ordering. The failing bases are high-blue colours such as `[202,196,230]`,
`[220,218,227]`, `[156,140,238]`, and `[151,134,239]`. Current HSL Saturation
does `set_lum(set_sat(dst, sat(src)), lum(dst))`, but after the high-end
luminosity clamp the minimum output channel rounds one LSB too low, e.g.
`[202,196,230]` produced `[203,191,255]` while CSP exports `[203,193,255]`.
Ceiling the minimum channel after the high-end clamp improves that pixel to
`[203,192,255]`, which is within one LSB; applying the rule globally or to
near-neutral bases is rejected because it disturbs the Pin Light/Hue/Saturation
area in `IllustrationBlendModes2`. The accepted scope is therefore:
Saturation blend only, after high-end `set_lum` clamp only, and only when the
quantized base saturation span is greater than `4/255`. The existing
near-neutral tiny-span behaviour still covers spans at or below `4/255`
(`IllustrationBlendModes2` prefix `[224,223,226]` has span `3/255`). With the
scoped rule, `Test_Saturation` becomes `raw_max=1` / `visible_px=0`, while
`Test_Color` and `Test_Hue` remain `raw_max=1` / `visible_px=0`. A native A/B
probe on `IllustrationBlendModes2` showed the scoped rule is not the cause of
its current `raw_max=9`: disabling the new ceil branch entirely gave
`raw_mean=0.247104`, `visible_px=38187`, while the scoped rule gives
`raw_mean=0.246172`, `visible_px=38164`.

2026-06-17 IllustrationBlendModes Subtract boundary follow-up: the former
native max pixel `(266,244)` was not a Color Dodge or Color Burn formula error.
The stack reaches `Subtract before=[218,203,252,255]` with
`src=[197,182,252,253]`; only the blue channel has quantized `src == dst`.
Current Photoshop-style Color Dodge amplifies a pre-dodge blue value of `2` to
about `143`, while a pre-dodge blue value of `3` becomes about `214`, matching
the CSP export after the following Color Burn. Broad probes remained rejected:
global standard-pass `256-srcA` and Subtract-only `256-srcA` both introduce new
large residuals, and changing near-white Color Dodge thresholds would break
already-exact counterexamples. The accepted native rule is narrower: for
Subtract only, when the effective source alpha is partial (`0 < a < 255`) and
the quantized source and destination channel values are equal, keep a one-LSB
blend target (`1/255`) instead of zero for that channel. Fully opaque equal
channels still subtract to zero; source-greater-than-destination channels still
clip to zero. This changes `(266,244)` to
`Subtract after=[23,22,3,255]`, `ColorDodge after=[88,67,214,255]`, and final
`[42,4,214,255]`, matching CSP at that pixel. Full native comparison improves
`IllustrationBlendModes.png` from `raw_max=72` / `premul_max=72` to
`raw_max=7` / `premul_max=7`; blend guards `Test_ColorDodge`,
`Test_ColorBurn`, `Test_SoftLight`, `Test_Mask`, `Test_Clipping`, and
`Test_ToneCurve` remain exact, and `Test_AddGlowMultiply` remains
`raw_max=2` / `premul_max=2`.

2026-06-17 IllustrationBlendModes2 HSL/PinLight follow-up: the remaining
native max after the Subtract boundary fix is in the
`PinLight -> Hue -> Saturation` chain, not in support selection or layer
ordering. Focused traces showed two separate residual families:

- `(382,99)` passes through partial Hue:
  `Exclusion after=[103,64,15,255]`, Hue source `[84,51,250,77]`, old Hue
  after `[94,62,55,255]`, Saturation after `[174,31,0,255]`, final
  `[174,51,250,255]` versus CSP `[166,51,250,255]`.
- `(427,138)` remains the current max:
  `PinLight src=[84,51,250,6]` gives `[225,223,226,255]`, full Hue gives
  `[224,223,226,255]`, Saturation gives `[229,216,255,255]`, while CSP final
  is `[221,221,255,255]`.

The accepted change is deliberately narrow: for Hue blend only, when the source
alpha is partial, floor the final standard-pass writeback to the u8 grid instead
of rounding. This moves `(382,99)` to Hue after `[93,62,54,255]`, Saturation
after `[167,34,0,255]`, and final `[167,51,250,255]`, within one LSB of CSP at
that pixel. Guard samples stayed stable: `Test_Hue` remains `raw_max=1` /
`visible_px=0`, `Test_Color` remains `raw_max=1` / `visible_px=0`, and
`Test_Saturation` remains `raw_max=1` / `visible_px=0`. Full
`IllustrationBlendModes2.png` is still `raw_max=8`, now at `(427,138)`.

Rejected probes during this pass:

- Extending the floor writeback to all HSL blend modes removed the old
  `(382,99)` max but slightly worsened the full image
  (`raw_mean=0.247173`, `visible_px=38262`), so the accepted scope is Hue-only.
- Raising Saturation's tiny-span threshold from `2/255` to `3/255`, even with a
  high-key near-neutral guard, regressed `Test_Saturation` (`raw_max=9`,
  `visible_px=5` in the high-key probe).
- Rec.601 HSL luma (`0.299/0.587/0.114`) strongly improved
  `IllustrationBlendModes2` mean/max (`raw_max=7`, `raw_mean=0.139977`) and
  explains the blue residuals in the later Color region, but it regressed
  `Test_Color` and `Test_Saturation` to visible errors. Keep the existing
  sample-backed `0.3/0.6/0.1` HSL luminosity weights.
- Hue min-channel ceil for high-key near-neutral destinations regressed
  `IllustrationBlendModes2` itself (`raw_max=65`) by flipping other near-neutral
  Hue pixels into a wrong Saturation branch.
- A byte-domain PinLight target with `256`-style partial-alpha carry worsened
  `IllustrationBlendModes2` (`raw_max=9`) and broke the existing one-pixel
  PinLight fixture.
- A broad standard-pass partial-alpha decrement (`srcA - 1`) severely regressed
  `IllustrationBlendModes2` and `IllustrationBlendModes` (`raw_max=178` and
  `86` respectively), so it remains rejected even though it locally explains
  the `(427,138)` PinLight output.

2026-06-17 AddGlowMultiply transparent-target quantization follow-up:
after the clipped-cache parity work, `Test_AddGlowMultiply` still had a strict
native `raw_max=2` / `premul_max=2` residual. The hot spot
`(3594,2277)` traced through a Normal bottom pixel `[169,32,253,219]`,
an Add Glow clipping base `[135,253,252,95]`, and a clipped Multiply sibling
`[88,253,164,129]`. The clipped Multiply cache at that point is
`[90,252,207,95]`; resolving that cache through Add Glow with the old carry
floor plus nearest RGB divisions produced `[200,142,252,232]` versus CSP
`[198,142,252,232]`.

Accepted native rule: in Add Glow's byte-domain channel path, round the
transparent-destination carry term `dst_a * (255 - src_a) / 255`, but when the
first partial RGB mix resolves into a destination whose alpha is also partial,
floor that RGB division. Keep opaque destinations rounded to nearest and keep
the later final-tail RGB division rounded to nearest. This moves the hot spot
to `[199,142,252,232]` and reduces the full sample to `raw_max=1`,
`premul_max=1`, `raw_visible_px=0`, and `premul_visible_px=0`.

Rejected probes during this pass:

- Flooring both Add Glow partial RGB divisions locally reached CSP at the
  sample point, but badly worsened the full image (`raw_mean=0.134784`,
  `raw_visible_px=1026917`) and regressed the Add Glow one-pixel guard.
- Flooring the final-tail RGB division only for partial-alpha destinations
  also worsened `Test_AddGlowMultiply` back to `raw_max=2` with many visible
  pixels, while `Test_AddGlow` stayed exact. Keep the final-tail division
  rounded.

Verification after the accepted rule: `clip_gpu` unit tests pass, including the
new transparent-target Add Glow fixture; `Test_AddGlow` remains exact;
`Test_AddGlowMultiplyClipping` remains `raw_max=1` / visible `0`;
`Test_GlowDodge`, `Test_ColorDodge`, `Test_ColorBurn`, and `Test_SoftLight`
remain exact; `IllustrationBlendModes` remains `raw_max=7`, and
`IllustrationBlendModes2` remains `raw_max=8`.

2026-06-18 Brightness and Hue follow-up review: another agent landed three
strict GPU shader changes:

- `LayerComposite=26` Brightness/Luminosity now uses a separate Rec.601-ish
  `lum_rec601` / `set_lum_rec601` path (`0.299/0.587/0.114`) instead of the
  `0.3/0.6/0.1` Hue/Saturation/Color luminosity helper.
- `set_lum_saturation` now delegates through the shared `set_lum` helper and
  only applies its accepted minimum-channel ceil when the pre-clamp `set_lum`
  candidate would exceed `1.0`.
- Hue now ceils the minimum channel after `set_lum` when the quantized base
  saturation span is greater than `2/255`. This is distinct from the earlier
  rejected high-key near-neutral Hue ceil variant that pushed many Hue pixels
  into the wrong Saturation branch.

Local verification accepted these changes. `cargo fmt --all --check` and
`cargo test -q` pass. CSP PNG comparisons: `Test_Brightness` remains
`raw_max=1` / `visible_px=0`; `Test_Hue` remains `raw_max=1` /
`visible_px=0`; `Test_Color` remains `raw_max=1` / `visible_px=0`;
`Test_Saturation` remains `raw_max=1` / `visible_px=0`; `Test_HSL` remains
`raw_max=3`; `Test_AddGlowMultiply` remains `raw_max=1` / visible `0`; and
`IllustrationBlendModes` remains `raw_max=7`. Using the current
`IllustrationBlendModesB.clip/png` fixture, renamed from the old
`IllustrationBlendModes2` sample after layer-by-layer PNG export, the current
strict GPU output improves from the previous recorded `raw_max=8`,
`visible_px=38080` to `raw_max=7`, `visible_px=16653`.

Repository hygiene decision after Rizum's fixture update: the old
`IllustrationBlendModes2.clip` asset is removed, `IllustrationBlendModesB.*`
is the current full-sample reference, and the numbered
`IllustrationBlendModes*.png` plus `IllustrationBlendModesB*.png` exports are
intentional progressive layer reveal references for future blend-mode tracing.

2026-06-18 IllustrationBlendModesB Color low-clamp follow-up: after the Hue
and Saturation fixes, the current max moved into a Color blend region. At
`(308,239)`, the progressive reference before Color (`IllustrationBlendModesB13`)
is `[0,0,241,255]`, the Color source is `[84,51,250,255]`, and the full CSP
reference is `[28,0,168,255]`. The existing `0.3/0.6/0.1` Color formula produced
`[27,0,161,255]`; full Rec.601 Color produced this point but regressed
`Test_Color` to visible pixels. Scanning Color-only pixels where the Color source
is fully opaque and later Brightness/Lighten layers are transparent showed CSP's
low-luminosity blue outputs are stepped (`dst_b=223..231 -> [26,0,155]`,
`232..240 -> [27,0,162]`, `241..245 -> [28,0,168]`) rather than the continuous
current curve.

Accepted scope: Color blend only, and only when the ordinary Color luminosity
translation would enter the low-side ClipColor branch (`min < 0`). In that branch
the GPU path rounds source and target luminosity to the u8 grid using canonical
`0.3/0.59/0.11` weights before applying the low clamp. Ordinary/high-clamp Color
stays on the existing `0.3/0.6/0.1` path so `Test_Color` remains `raw_max=1` /
`visible_px=0`. A new `clip_gpu` one-pixel test locks `[84,51,250]` Color over
`[0,0,241]` to `[28,0,168]`.

Rejected probes during this pass:

- Color-only Rec.601 reduced one B hot spot but changed `Test_Color` to
  `raw_max=2` / `visible_px=24`.
- Disabling Hue's minimum-channel ceil improved one partial-Hue boundary but
  regressed the B max back to `(427,138)` at `raw_max=8`.
- Flooring the partial-Hue pure blend target moved the max back to `(382,99)`
  and slightly worsened the B full metrics, so keep only the accepted final Hue
  writeback floor.

Verification after the accepted Color branch: `cargo fmt --all --check` and
`cargo test -q` pass. PNG comparisons: `IllustrationBlendModesB` improves from
`raw_max=7` / `visible_px=16653` to `raw_max=5` / `visible_px=3194`;
`IllustrationBlendModes` remains `raw_max=7` / `visible_px=1008`;
`Test_Color`, `Test_Hue`, and `Test_Saturation` remain `raw_max=1` /
`visible_px=0`; `Test_AddGlowMultiply` remains `raw_max=1` / visible `0`.
The remaining B max is in the Pin Light/Hue/Saturation/Lighten chain; at
`(374,108)`, native Hue is `[93,62,56]` while the B12 reference is
`[93,61,56]`, and Saturation amplifies that boundary to the final red-channel
residual.

2026-06-18 IllustrationBlendModesB opaque-Hue luminosity follow-up: after the
Color low-clamp fix, the remaining visible B residuals were rechecked against
the progressive references `IllustrationBlendModesB11.png` (before Hue) and
`IllustrationBlendModesB12.png` (after Hue). Decoding the Hue source layer shows
the source RGB is constant `[84,51,250]` with varying alpha. A CPU diagnostic
scored Hue formulas over the full B11 -> B12 layer:

- Current Hue `0.3/0.6/0.1`: `max=7`, `visible=3766` on Hue-active pixels.
- Hue `0.3/0.59/0.11`: `max=3`, `visible=21` on Hue-active pixels.
- Hue Rec.601: `max=3`, `visible=35`.
- Low-clamp-only Hue `0.3/0.59/0.11`: still `visible=2623`, because ordinary
  full-alpha Hue pixels such as source `[84,51,250,255]` over `[112,83,16]`
  need `[85,69,165]`, while the old formula produced `[87,71,167]`.

The accepted scope is Hue-only and alpha-gated: fully opaque Hue uses the
canonical `0.3/0.59/0.11` luminosity path, while partially transparent Hue keeps
the existing `0.3/0.6/0.1` path plus final floor writeback. This keeps the
partial-Hue guards and avoids the max regression from applying `0.3/0.59/0.11`
to all Hue sources. A new `clip_gpu` one-pixel test locks the opaque Hue sample
`[84,51,250,255]` over `[112,83,16] -> [85,69,165]`.

Rejected probes during this pass:

- Broad Saturation `0.3/0.59/0.11` slightly reduced B visible pixels but
  regressed `Test_Saturation` to `raw_max=2` / `visible_px=9`.
- Saturation `0.3/0.59/0.11` only outside the high-clamp branch kept
  `Test_Saturation` stable but worsened B's `raw_mean`/`raw_diff_px`, so the
  gain was not general enough to keep.
- Keeping the partial-Hue blend target unquantized preserved `Test_Hue` but
  raised `IllustrationBlendModesB` to `raw_max=7`.
- Applying Hue `0.3/0.59/0.11` to both opaque and partial sources lowered B
  visible pixels but raised the max to `8` at `(374,108)`, where partial Hue
  moved to `[92,61,55]` before Saturation.

Verification after the accepted opaque-Hue branch: `cargo fmt --all --check`
and `cargo test -q` pass. PNG comparisons: `IllustrationBlendModesB` is now
`raw_max=5`, `raw_mean=0.076072`, `raw_diff_px=31570`, `visible_px=2718`;
`IllustrationBlendModes` remains `raw_max=7` / `visible_px=1008`; `Test_Hue`,
`Test_Color`, and `Test_Saturation` remain `raw_max=1` / `visible_px=0`;
`Test_AddGlowMultiply` remains `raw_max=1` / visible `0`.

2026-06-18 follow-up rejection pass: several plausible low-residual fixes were
tested after the opaque-Hue branch and should stay rejected until new native
evidence explains a broader rule.

- Saturation low-side `0.3/0.59/0.11` luminosity, gated only when the ordinary
  Saturation luminosity shift would enter the low clamp, kept `raw_max=5` but
  worsened B's aggregate error (`raw_mean=0.076236`,
  `raw_diff_px=31779`) despite a small visible-pixel decrease
  (`visible_px=2709`). It is not a general improvement.
- Hue partial-alpha colour mixing with `alpha_byte / 256` preserved
  `Test_Hue` (`raw_max=1` / `visible_px=0`) but regressed
  `IllustrationBlendModesB` to `raw_max=8` at `(362,130)`.
- Replacing the HSL filter shader with an integer/fixed-point HSV path matched
  the old `Test_HSL` max pixel locally but regressed whole images badly
  (`Test_HSL raw_max=136`, `Test_HSL2 raw_max=103`) and failed the existing
  streamed HSL unit test.
- Removing the HSL filter's saturation-positive overflow rescale also matched
  the old `Test_HSL` max pixel locally, but over-saturated broad regions
  (`Test_HSL raw_max=80`) and regressed the saturation-only guard
  (`Test_HSL3 raw_max=9`). Keep the current rescale path.

2026-06-18 IllustrationBlendModesB HSL follow-up audit: targeted IDA and
progressive-PNG diagnostics narrowed the remaining B residual further, but did
not produce a retainable general rule.

IDA facts:

- `RenderRGB100_8bit_Lkup @ 0x123f59e0` has code xrefs only from
  `CSAdjustmentLayer::RenderRGB100_8bitCaller` and
  `CSAdjustmentLayer::RenderXXXFor8Bit`; it is an adjustment/LUT writeback path,
  not proven ordinary raster HSL blend handling.
- `RenderRGB100_HSV_8bit @ 0x123f6920` also has a code xref only from the
  adjustment-layer caller and calls `sub_123FC180`, matching the HSL adjustment
  filter route rather than ordinary `LayerComposite=23/24/25`.
- `RenderRGB100_32bit @ 0x12401a60` still routes ordinary nonzero internal blend
  codes through `RenderBlendModeCall_0`, but the persisted `.clip`
  `LayerComposite` to internal `a9` mapping for HSL modes remains unrecovered.

Progressive reference facts:

- With CSP's `IllustrationBlendModesB12.png` as input, current Saturation alone
  differs from `IllustrationBlendModesB13.png` at `max=12`; broad
  `0.3/0.59/0.11` Saturation lowers that layer probe to `max=2`, but the same
  broad change regresses `Test_Saturation` to visible errors.
- The broad Saturation benefit clusters around high-clamp pixels where the base
  channel order is green-high/blue-low. `Test_Saturation`'s visible regressions
  are the opposite blue-high/green-low order. A channel-order Saturation spike
  using `0.3/0.59/0.11` except for blue-high/green-low high-clamp pixels kept
  `Test_Saturation` stable, but the full B comparison only moved
  `raw_visible_px=2718 -> 2693` while worsening aggregate error
  (`raw_mean=0.076072 -> 0.076137`, `raw_diff_px=31570 -> 31749`) and keeping
  `raw_max=5`. Reject it as a sample-shaped branch until native code explains
  the channel-order rule.
- The full-image `raw_max=5` hot pixel at `(374,108)` is primarily a partial Hue
  boundary: native B12 has `[93,62,56]` while CSP B12 has `[93,61,56]`. Feeding
  CSP's B12 into the current Saturation formula gives only a one-LSB local
  mismatch at that point. Therefore Saturation-only tweaks cannot fully solve
  the current max.
- For the `(374,108)` partial Hue pixel, CSP's alpha lookup
  `byte_12652F90[64*(255-81) + 58*81]` still returns `62`; the CSP value `61`
  cannot be explained by changing the final alpha lookup after a ceiled pure
  green value of `58`. The remaining cause is earlier in the partial-Hue pure
  blend/min-channel-ceil path.

Rejected probes during this audit:

- Partial-Hue low-clamp Rec.601/`0.298912/0.586611/0.114478` improved the
  B11->B12 Hue layer probe (`max 7 -> 3`) for low-blue partial-Hue pixels, but
  full `IllustrationBlendModesB` stayed at `raw_max=5` and slightly worsened
  aggregate error (`raw_mean=0.076086`, `raw_diff_px=31585`).
- The Saturation channel-order spike described above is rejected despite a small
  visible-pixel reduction because it does not reduce the full-image max and
  worsens aggregate error.
- Alpha-thresholded partial-Hue min-channel-ceil probes can locally flip
  `(374,108)` to `[93,61,56]`, but the useful threshold is magic
  (`alpha ~= 81`) and is not a faithful general rule without native evidence.

2026-06-18 Kabi/MXL reference residual audit:

`Ref_Kabi_Live2D` was rechecked after the saved `.clip` update and still
reports `raw_max=32`, `premul_max=32`, and `premul_visible_px=341`. Provider
GPU tracing localizes the max pixel `(1454,1104)` to the body folder's
`layer 232 [蝴蝶结]`, specifically `layer 263 [发光2]` Glow Dodge over
`layer 258 [黑蝴蝶结上1]`. The relevant source state is:

- before `layer 263`: `[0,0,0,101]`
- source `layer 263`: `[165,116,255,140]`
- current native bow cache after `layer 263`: `[57,40,88,186]`
- body/background before the bow group: `[252,247,247,255]`
- final native/reference at the max pixel: `[110,96,131,255]` vs
  `[85,64,124,255]`

IDA `RenderRGB100_32bit @ 0x12401a60` confirms nonzero internal blend modes
build a temporary RGBA source with effective alpha at `0x12402126` before
calling `RenderBlendModeCall_0`; the decompiler omitted that alpha write in
the local variable display. `RenderBlendModeCall_0` case `516` applied as an
`a3==0` Color Dodge-style path locally produces `[75,53,116,241]`, which would
resolve over `[252,247,247]` to `[85,64,123]`, matching the max pixel
almost exactly. However, applying that partial-destination rule broadly to
Glow Dodge is rejected:

- all `0 < dst_a < 255` routed through case 516 regressed Kabi to
  `premul_max=163` around `(1396,1141)`, where `layer 264 [发光1]` over
  near-opaque `[8,5,7,254]` exploded to `[198,17,23,255]`;
- a `dst_a >= 254` opaque guard still regressed Kabi to `premul_max=158`
  around the same region;
- a `dst_a >= src_a` guard regressed Kabi to `premul_max=245`, with a
  low-alpha black destination `[0,0,0,5]` becoming opaque black after
  `layer 264`.

Keep the current Glow Dodge shader until native evidence explains the missing
dispatch/resolve condition. The local case-516 match is useful evidence, but it
is not a retainable general algorithm by itself.

Follow-up after the 2026-06-18 saved `.clip` refresh kept that conclusion.
The fresh release scan of all matching `img/*.clip`/`.png` pairs ranked the
largest premultiplied visible residuals as:

- `Ref_Kabi_Live2D`: `premul_max=32`, `premul_visible_px=341`;
- `Test_Gradiation`: `premul_max=10`, `premul_visible_px=45528`;
- `IllustrationBlendModes`: `premul_max=7`, `premul_visible_px=1008`;
- `Ref_MXL_Idol1`: `premul_max=5`, `premul_visible_px=473627`;
- `IllustrationBlendModesB`: `premul_max=5`, `premul_visible_px=2718`.

The newly saved `Ref_Meimei_*_Live2D.clip` remains visually clean
(`premul_max=1`, `premul_visible_px=0`); its `raw_max=255` is transparent RGB.

A narrower Glow Dodge spike that applied a case-516-like additive-alpha path
only when `0 < dst_a < src_a` was also rejected. It preserved the exact
`Test_GlowDodge` fixture and left `Test_AddGlowMultiply`,
`IllustrationBlendModes`, `IllustrationBlendModesB`, `Ref_Terra404_Live2D`,
and `Ref_MXL_Idol1` unchanged, but it regressed the Kabi target itself from
`premul_max=32` to `premul_max=207` at `(1460,1157)`. This rules out the
tempting "source alpha stronger than destination alpha" gate as the missing
native condition.

`Ref_MXL_Idol1` was also rechecked and remains `raw_max=80` in transparent
RGB, `premul_max=5`, and `premul_visible_px=473627`. The premultiplied
threshold shape is low-value for further semantic work: `>2` has only `181`
pixels, `>4` has `3` pixels, and `>5` has `0`; the broad stocking-area residual
is predominantly 1-2/255. Treat this sample as visually low-level unless a new
reference exposes a structural hotspot.

Additional MXL instrumentation confirms that the broad stocking-area residual is
not caused by layer-mask sampling. At representative pixel `(2672,6177)`, the
`layer 306 [leg base]` subtree renders:

- `layer 309 [base]`: `[255,244,233,255]`;
- `layer 310 [black socks]` source window center: `[104,87,95,241]`, mask
  alpha `255`, output `[112,92,95,255]`;
- `layer 312 [leg shadow]` source window center: `[53,35,88,43]`, mask alpha
  `255`, output `[97,79,85,255]`;
- CSP reference at the final pixel is `[95,77,85,255]`.

Simple byte-domain probes over alpha denominators, alpha `+1..+7`, product
floor/round, output floor/round, and `255-a` versus `256-a` inverse-alpha
families either miss the blue channel or require channel-specific behaviour.
Do not retune ordinary `LayerComposite=2` Multiply from this local MXL point;
`Test_Multiply` remains a guard, and the current evidence points to a deeper
source/decode or native resolve detail rather than a safe generic Multiply
formula. The CLI diagnostic `--dump-layer-window` now prints a matching mask
alpha window when the layer has a mask, so future masked-layer probes can avoid
guessing whether mask sampling is involved.

Follow-up MXL probe: `clip_cli --dump-layer-rgba` was added as a developer-only
decoded-source dump and used to export layers `3`, `309`, `310`, and `312`.
`--gpu-trace-pixel 2672 6177` confirms the root stack starts with opaque
`layer 3 [Ref_MXL_Idol1]` at `[97,79,85,255]`; the later `layer 306 [leg base]`
subtree independently reaches the same value through `layer 310` and
`layer 312` Multiply, so the broad residual is not a top-layer visibility or
region-stitching failure. `LayerCompManager.AppliedLayerCompIndex` is `-1`, so
there is no obvious applied layer-comp override to explain the difference.

An offline formula sweep over the main stocking residual shows that adding about
`+4` to the effective alpha of `layer 312` collapses most of that local `+2`
delta, but applying the same treatment to both masked Multiply layers reverses
the signed error and worsens the region. The required effective alpha also
differs by colour channel when solved from CSP's target bytes. This remains a
rejected local fit, not a faithful compositor rule. The Blender worker path
itself is healthy for MXL: `Ref_MXL_Idol1.clip --blender-render-rgba/--json`
writes the expected `5877x8326` / `195727608`-byte payload and metadata without
the historical max-texture import failure.

Additional 2026-06-18 MXL follow-up rejects a broader masked-Multiply
rounding rule. A temporary shader spike that floored the final writeback for
all masked partial-alpha Multiply sources improved MXL from
`raw_visible_px=474022` / `premul_visible_px=473627` to
`raw_visible_px=225868` / `premul_visible_px=225607`, but it regressed the
Terra guard from `premul_visible_px=13501` to `18246`. A narrower low-alpha
gate (`src.a < 0.5`) still left Terra worse at `premul_visible_px=14475` while
only improving MXL to `premul_visible_px=269763`. `Test_Mask` remained exact
and `Test_AddGlowMultiply` stayed at visible `0`, so the spike is specifically
too broad for real masked Multiply stacks rather than a general shader
failure. Do not condition Multiply writeback on `has_mask` or simple source
alpha thresholds without native dispatch evidence.

The same follow-up also rules out nearby metadata explanations. MXL has
`LayerCompManager.AppliedLayerCompIndex = -1`; parsed `LayerComp.CompLayerInfo`
records mirror the visible layer state instead of overriding the `black socks`
or `leg shadow` layers. The canvas has no source/destination ICC profile or
colour-adjustment payload. `LayerMasking=32` appears on every folder/root entry,
so `leg base` is not a special masked group. Layers `309`, `310`, `312`, and
`313` have no palette/layer-colour/filter/offset payloads beyond ordinary
raster or masked-raster fields, and their `LayerRenderMipmap` values correctly
resolve through the `Mipmap.BaseMipmapInfo -> MipmapInfo.Offscreen` chain. These
checks leave the MXL stocking delta as an unresolved low-level CSP raster
resolve/detail, not a safe current renderer change.

Gradient Map fixed-point interpolation spike: replacing the current float
gradient color interpolation with the recovered `0x10000` fixed-point truncation
model lowered `Test_Gradiation` max only from `10` to `9`, but expanded visible
pixels from `45528` to `1028499` and raised mean from roughly `0.31` to
`0.77`. `Test_ToneCurve` stayed exact and `Test_HSL` stayed unchanged, so the
failure is local to Gradient Map. Rejected; keep the current rounded float LUT
interpolation until a native path explains how the fixed-point runtime model
maps back to the compact `.clip` payload without broad regression.

2026-06-18 Kabi Glow Dodge follow-up:

The remaining Kabi diff was rendered to raw RGBA and sorted by premultiplied
byte-domain error. Before this follow-up it had `premul_max=32`,
`premul_visible_px=341`, and every pixel above `2/255` was confined to the
small bow region around `layer 232 [蝴蝶结]`. The max pixel still traced to
`layer 263 [发光2]` over the child `layer 258 [黑蝴蝶结上1]` subtree:

- before `layer 263`: `[0,0,0,101]`;
- source `layer 263`: `[165,116,255,140]`;
- previous native output: `[57,40,88,186]`;
- CSP-compatible local target: `[75,53,116,241]`.

The retained fix is deliberately narrow: the Glow Dodge shader now applies the
case-516-style RGB denominator plus additive alpha only when the destination is
translucent black (`dst.a > 0` and `dst.rgb == 0`). A GPU unit test locks this
exact state as `[0,0,0,101] + [165,116,255,140] -> [75,53,116,241]`. This is
not a coordinate, layer-name, or file-name condition; it represents the offscreen
black-cache state that previous IDA and sample probes had isolated without
spreading the rule to coloured or near-opaque destinations.

The Kabi full-image result improves only modestly, from `premul_max=32` /
`premul_visible_px=341` to `premul_max=29` / `premul_visible_px=327`. Guard
checks stayed stable:

- `Test_GlowDodge`: exact;
- `Test_AddGlowMultiply`: `raw_max=1`, `premul_max=1`, visible `0`;
- `IllustrationBlendModes`: `raw_max=7`, `premul_max=7`,
  `premul_visible_px=1008`;
- `IllustrationBlendModesB`: `raw_max=5`, `premul_max=5`,
  `premul_visible_px=2718`;
- `Ref_Terra404_Live2D`: `premul_max=3`, `premul_visible_px=13501`;
- `Ref_MXL_Idol1`: `premul_max=5`, `premul_visible_px=473627`.

The remaining Kabi max moved to `(1463,1160)`, with native/reference
`[240,216,227,255]` versus `[255,245,245,255]`. Its trace is through
`layer 264 [发光1]` over a coloured partial destination:

- before `layer 263`: `[126,119,139,62]`;
- after `layer 263`: `[180,113,231,176]`;
- source `layer 264`: `[255,204,170,164]`;
- current after `layer 264`: `[239,213,225,227]`.

Simple formula spikes for this coloured-destination case remain rejected. A
red-source additive spike (`dst + src * alpha`) preserved `Test_GlowDodge` but
regressed Kabi to `premul_max=169` at `(1396,1141)`, the same neighbouring
region that earlier broad partial-destination experiments damaged. The layer
metadata for `232`, `258`, `261`, `262`, `263`, and `264` contains no hidden
palette, filter, offset, colour-mode, opacity, or visibility flag that explains
the coloured-destination difference; layers `263` and `264` are ordinary
`LayerComposite=10` rasters at opacity `256`. The accepted black-destination
fix should therefore remain, but further Kabi work needs native dispatch or
resolve evidence for the coloured Glow Dodge path rather than threshold tuning.

## Native render profile top-segment follow-up

`RIZUM_CLIP_RENDER_PROFILE=1` now reports the slowest render segments with
kind, source shape, fallback/barrier reason, source range, first layer id,
target rect, cost hints, and elapsed microseconds. The first useful large-sample
drilldown showed that the largest stable Terra/RealArt costs include planned
tile-local scope segments that fall back through the faithful path because
`plan_atlas_layout` cannot pack their raster source set under the current
8192-wide/tall atlas cap. Representative rejected fallbacks were Terra
ThroughGroups starting at first raster layers `61`, `407`, and `37`, and RealArt
first raster layer `365`.

A direct large-atlas trial was rejected. Requesting/using the adapter's larger
2D texture limit removed those `atlas_layout_unavailable` fallbacks, but it also
changed fidelity because the newly-enabled large ThroughScope tile-local path is
not yet a faithful replacement for the legacy path on those shapes. In the
trial, `Ref_Terra404_Live2D` stayed at `premul_max=3` but worsened visible
premultiplied pixels (`13643 -> 13908` in the measured run), and
`Test_RealArt` worsened from `premul_max=3` / `premul_visible_px=36` to
`premul_max=4` / `premul_visible_px=229`. The product path therefore remains on
the 8192 atlas cap and keeps the faithful fallback. Do not re-enable larger
scope atlases as a performance optimization until a focused legacy-vs-tile test
proves the affected large ThroughScope/container shapes faithful.

## Text decoration and synthetic italic follow-up

The first text decoration fix uses OpenType line metrics for decoration
thickness and supersampled synthetic oblique when the requested italic face is
not installed. Further probes on `Text_6` through `Text_12` narrowed the
remaining visible differences:

- `Text_7` underline and `Text_8` strikethrough are effectively aligned with the
  CSP exports. Their long dark decoration rows match the reference ranges
  (`Text_7`: y `85..88`, `Text_8`: y `58..61` native versus `57..60`
  reference).
- `Text_9` still differs because the italic HarmonyOS Sans decoration becomes
  wider and one pixel thicker in native (`x=13..173`, 5 rows) while CSP keeps the
  decoration close to the upright logical run (`x=15..169`, 4 rows).
- `Text_11` is dominated by synthetic italic/layout differences for
  `OldNewspaperTypes`; the font resolves correctly to the installed
  `OldNewspaperTypes.ttf`, so this is not a fallback-font problem.
- `Text_12` resolves correctly to `MiSans-ExtraLight.ttf`, but underline and
  strikethrough still differ in vertical placement and width.

Two tempting shortcuts were rejected:

- Disabling `fit_single_line_to_quad_width` badly regressed `Text_6`, `Text_9`,
  and `Text_11`, although it slightly improved `Text_12`. The quad-fit heuristic
  remains necessary for current italic glyph placement.
- Drawing decorations from pre-fit "logical" styles instead of fitted styles
  regressed `Text_9` and `Text_12`. CSP appears to couple decoration placement
  to fitted text layout, but with an additional italic-decoration rule that the
  current importer has not recovered.

The remaining safe next step is to find the native text decoration rule for
italic/synthetic italic runs, not to tune per-font offsets. In particular, avoid
changing the now-good upright underline/strikethrough path from `Text_7` and
`Text_8`.

IDA follow-up on `CLIPStudioPaint.exe.working.i64` found the relevant Skia text
setup path. `sub_14363D830` constructs an `SkFont`, calls `SkFont::setEmbolden`,
then calls `SkFont::setSkewX` before `SkTextBlobBuilder::allocRunTextPos`. The
skew constant passed for the italic path is `-0.25`; the non-italic path passes
`0.0`. `sub_14363C820` also sets `SkFont::setSubpixel(true)`,
`SkFont::setEdging(1)`, and `SkFont::setHinting(1)`. Related metrics functions
`sub_143639750` and `sub_14363DA10` call `SkFont::getMetrics`, but the observed
decompilation covers general font bounds aggregation rather than a recovered
underline/strike placement formula.

A direct importer probe that changed the current bitmap-shear approximation from
`0.23` to the native `0.25` skew constant was rejected. It regressed the focused
italic text samples (`Text_6` raw mean `3.732506 -> 4.300237`, `Text_9`
`8.256263 -> 8.701725`, `Text_11` `11.436844 -> 11.954869`, `Text_12`
`11.122837 -> 11.314950`). The likely reason is that Skia applies skew inside
the font/path rasterizer with its own transform origin, subpixel positioning,
edging, and hinting; changing only the importer shear scalar is not equivalent.
Keep the current `0.23` approximation until the renderer either reproduces the
full Skia coordinate model or switches to a Skia-backed text rasterizer.

Follow-up implementation: keep glyph layout/positioning on the existing
quad-fit path, but compute underline/strikethrough stroke thickness from the
logical pre-fit font size. This matches the native evidence better than scaling
decoration thickness by the importer-only quad-fit adjustment: Skia receives a
font/paint and then draws decoration-like strokes through the normal draw-list
paint path, while the importer's `fit_single_line_to_quad_width` is a local
layout heuristic rather than a recovered CSP font-size mutation. The focused
samples improved without regressing the already-good upright cases:

- `Text_7` unchanged at raw mean `0.947925`;
- `Text_8` unchanged at raw mean `1.477481`;
- `Text_9` improved from `8.256263` to `7.453594`;
- `Text_11` improved from `11.436844` to `10.896206`;
- `Text_12` improved from `11.122837` to `11.008219`;
- `Text_6` unchanged at raw mean `3.732506`.

This still does not solve the full Skia text parity problem. `Text_12` and
`Text_11` retain visible layout/position residuals, so further work should focus
on Skia's text blob positioning, hinting, and decoration draw-list coordinates
rather than changing the now-separated decoration thickness rule.

Additional rejected probes:

- Replacing the synthetic italic coverage drop with bilinear distribution after
  shear made edges smoother but regressed the focused samples (`Text_6`
  `3.732506 -> 4.585406`, `Text_9` `7.453594 -> 8.105588`, `Text_11`
  `10.896206 -> 11.674387`, `Text_12` `11.008219 -> 11.216925`). Skia's
  antialiasing difference is therefore not reproduced by simply distributing
  the importer supersamples across neighboring pixels.
- Treating the quad fit as an x-only text transform instead of scaling the local
  importer font size badly regressed the already-good upright samples
  (`Text_7` `0.947925 -> 16.884000`, `Text_8` `1.477481 -> 15.460725`) and the
  italic samples. The current quad-fit heuristic is not a faithful model, but it
  remains closer to the saved CSP samples than a naive x-only transform.
- Enabling `ab_glyph` pair kerning for same-font runs left HarmonyOS/MiSans
  samples unchanged and regressed `Text_11` (`10.896206 -> 12.837938`). CSP uses
  `SkShaper`/text-blob glyph positions, but substituting ab_glyph's legacy kern
  table is not equivalent to Skia shaping.
- Text-layer render mipmaps are not usable as a fidelity shortcut in the focused
  samples. For example, attempting to dump `Text_9` layer `5` follows the
  `render_mipmap` but fails on a missing external body
  `extrnlidEA23111439854ECFA48F5F99D2D39DD6`. The importer cannot rely on a
  saved CSP text raster cache for these files.

The practical remaining path is either a real Skia-compatible text rasterizer
or more native evidence for the draw-list command that emits text decoration
lines. Small local tweaks to the current ab_glyph rasterizer are now more likely
to trade one text sample against another than to recover a general rule.

One small decoration-position rule did hold up. The focused fonts expose these
OpenType metrics:

- HarmonyOS Sans Bold: strikeout position `0.300em`, strikeout size `0.050em`;
- OldNewspaperTypes: strikeout position `0.512em`, strikeout size `0.102em`;
- MiSans ExtraLight: strikeout position `0.265em`, strikeout size `0.050em`.

Using all OpenType underline/strike positions regressed the upright guards and
most italic samples (`Text_7` `0.947925 -> 5.295675`, `Text_8`
`1.477481 -> 2.970469`, `Text_9` `7.453594 -> 10.288913`, `Text_12`
`11.008219 -> 13.035262`). However, only honoring very high strikeout positions
(`> 0.45em`) improves the display-font case without moving the ordinary-font
guards. This matches `OldNewspaperTypes`, whose CSP strikethrough sits much
higher than the importer's legacy `0.52 * font_size` fallback:

- `Text_7` unchanged at raw mean `0.947925`;
- `Text_8` unchanged at raw mean `1.477481`;
- `Text_9` unchanged at raw mean `7.453594`;
- `Text_11` improves from `10.896206` to `10.201762`;
- `Text_12` unchanged at raw mean `11.008219`;
- `Text_6` unchanged at raw mean `3.732506`.

This is still not a font-name special case; it is a guarded use of an unusually
high OpenType strikeout metric. Ordinary strikeout metrics continue through the
legacy fallback because that remains closer to CSP on HarmonyOS Sans and MiSans.

Skia-backed text rasterizer feasibility probe: a standalone spike under
`native/spikes/skia_text_probe` verifies that `skia-safe 0.99` can build and run
on the maintainer Windows machine using a CPU raster surface and a font loaded
from local font bytes. The release probe executable is about `3.3 MiB`; the
linked Skia static library produced by the build is about `16 MiB`. This is a
real packaging cost but not an obvious blocker.

The probe renders `Text_6`-style HarmonyOS synthetic italic using Skia
`Font::set_subpixel(true)`, `set_edging(AntiAlias)`, `set_hinting(Normal)`, and
`set_skew_x(-0.25)`, matching the IDA-observed CSP text setup more closely than
the current ab_glyph shear. A coarse grid over size/x/baseline found a Skia
`draw_str` result with raw mean `2.650125` against the CSP `Text_6.png`, versus
the current native renderer's `3.732506`. This is only a glyph-body probe, not a
full `.clip` text integration, but it proves that a Skia-backed text path is
worth pursuing as a real milestone.

The spike was promoted into the native runtime without keeping an ab_glyph
product fallback. This is not a Skia compositor. `clip_runtime` now resolves
fonts through the existing system-font lookup, creates Skia `Typeface` values
from the matched font bytes, and uses Skia only to rasterize simple text layers
into source pixels. Those pixels still enter the existing Rust/wgpu native
compositor, so masks, blend modes, clipping, tile-event execution, barrier
segments, and final readback remain on the current GPU path. The
`native/spikes/skia_text_probe` package was deleted after promotion.

Two integration rules survived the first promotion:

- glyph baselines follow CSP's text layout more closely when the Skia draw
  baseline uses `origin_y + font_size` rather than `-SkFontMetrics::ascent`;
- the text entry quad's minimum y is a real local layout origin term
  (`line_origin_y = -quad_min_y / 100`), explaining why HarmonyOS samples and
  OldNewspaperTypes samples wanted opposite baseline shifts.

Current representative text compare means after the Skia promotion:

- `Text_1`: `0.203794`;
- `Text_2`: `0.670275`;
- `Text_3`: `0.336094`;
- `Text_6`: `2.780737`;
- `Text_7`: `3.399619`;
- `Text_8`: `2.294287`;
- `Text_9`: `7.361119`;
- `Text_10`: `0.789900`;
- `Text_11`: `4.595794`;
- `Text_12`: `12.209344`.

`Text_4` and `Text_5` remain large layout/quad/align cases rather than glyph
rasterization cases. Dependency trimming should wait until the Skia
source-raster path is closer to CSP's observed `SkShaper -> SkTextBlobBuilder`
path; feature shaving before then would optimize the wrong stage. The long-term
dependency goal is still a minimal Skia feature set for source rasterization
only, not replacing the Rust/wgpu compositor.

A direct `skia-safe` `Shaper::shape_text_blob` integration was tested and
rejected for the current product path. Enabling `skia-safe/textlayout` and
replacing per-character `draw_str` with `TextBlobBuilderRunHandler` initially
placed glyphs about `78-79px` too low because the run-handler offset is not the
same coordinate as the `draw_str` baseline. After compensating the y origin, the
blob path still produced narrower Latin advances and worse focused metrics than
the simpler Skia source-raster path:

- `Text_1`: `0.203794 -> 9.393450`;
- `Text_6`: `2.780737 -> 8.732306`;
- `Text_10`: `0.789900 -> 5.981494`;
- `Text_11`: `4.595794 -> 9.192375`;
- `Text_12`: `12.209344 -> 9.885000`.

Therefore the product path remains Skia `draw_str` source rasterization for now,
and the `textlayout` feature is not kept. A future shaper attempt needs the
exact CSP run-handler coordinate model and font-run setup, not just
`shape_text_blob` dropped into the current layout loop.

Vertical text follow-up: `Text_4` is not a whole-layer rotation; it serializes
text param `33 = 16`, while ordinary horizontal samples use `0` and italic
samples use `2`. Treating bit `0x10` as vertical writing mode and laying Latin
glyphs in right-to-left columns improves `Text_4` from raw mean `18.292500` to
`4.590094` without moving the horizontal text guards. The vertical text box uses
the parsed `box_size` plus the quad minimum; negative quad x coordinates are
anchored from the right edge of the text raster bbox, matching the Text_4
surface box `(57, 10, 165, 98)`.

Text_4 vertical refinement: replacing the fixed vertical row step with adjacent
glyph-advance spacing matches the observed per-column behavior: the `T/e`
column is looser than the `s/t` column. A small right-column inset adjustment
then moves the right column without disturbing the left column. Focused metric:
`Text_4` raw mean `4.590094 -> 0.943425`; `Text_1`, `Text_5`, and
`Test_AddGlowMultiply` guards stayed stable.

`Text_5` remains separate. It is a circular/arc text sample, not the same
vertical-writing mode. Its raw attributes differ with param `66 = 1`; params
`70` and `71` decode as big-endian doubles in the useful range (`195.0`,
`165.0` for Text_5), and param `72` stores `(171, 171)`. A basic path-text
branch now treats mode `66 = 1` plus a path center as circular arc text: glyph
advances distribute the string along the upper arc, while the baseline radius is
kept inside the bounding circle. This fixes the previous failure where Text_5's
ordinary horizontal baseline landed below the canvas. Focused metric:
`Text_5` raw mean `12.114187 -> 1.291969`; `Text_1`, `Text_4`, `Text_6`, and
`Test_AddGlowMultiply` guards stayed stable.

New Chinese text samples `Text_13` through `Text_15` split the remaining text
scope into horizontal CJK, upright vertical CJK, and mixed vertical CJK/Latin.
`Text_13` follows the existing horizontal Skia path and compares at raw mean
`1.048331`. `Text_14` and `Text_15` showed that CJK vertical writing is not the
same as the earlier Latin vertical mode: CJK glyphs remain upright, columns are
anchored near the text raster center line, explicit newlines create new
right-to-left columns, and short ASCII runs such as `hu` are drawn as a
horizontal tate-chu-yoko-style item instead of individual rotated letters. The
focused CJK vertical path now compares at `Text_14` raw mean `3.375075` and
`Text_15` raw mean `5.222719`; the full `Text_1..Text_15` guard matrix stayed
stable.

## Text residual reverse-analysis consolidation

A read-only r2/PE import pass over the local CSP executable confirms that the
remaining focused text differences should be treated as text-run pipeline
differences, not as per-sample offsets. IDA MCP was not active for this pass, so
the evidence came from static import thunks and focused disassembly in
`CLIPStudioPaint.exe`.

The relevant native Skia imports resolve as follows:

- `0x1439e457c -> SkShaper::MakeShapeThenWrap`;
- `0x1439e4582 -> SkShaper::MakeFontMgrRunIterator`;
- `0x1439e4588 -> SkShaper::MakeBiDiRunIterator`;
- `0x1439e458e -> SkShaper::MakeScriptRunIterator`;
- `0x1439e4462 -> SkTextBlobBuilder::allocRunTextPos`;
- `0x1439e4528 -> SkCanvas::drawTextBlob`;
- `0x1439e44fe -> SkCanvas::drawLine`;
- `0x1439e43b4/426/42c/432/438/43e -> SkFont::setSize`,
  `setSubpixel`, `setEmbolden`, `setEdging`, `setHinting`, and `setSkewX`.

Focused disassembly confirms this is an active text path, not just dead imports:

- `0x14363c883` calls `SkShaper::MakeShapeThenWrap`, and the same function later
  calls the BiDi, script, and font-manager run iterators.
- `0x14363d94b` calls `SkFont::setSkewX`; the italic branch loads
  `0xbe800000` (`-0.25f`) from `0x144551e18`, while the non-italic branch passes
  zero.
- `0x14363d968` calls `SkTextBlobBuilder::allocRunTextPos` and then copies
  glyph/run data into the run buffer.
- `0x14363eb1e` calls `SkCanvas::drawTextBlob`.
- `0x14363e9f8` calls `SkCanvas::drawLine` after a helper fills paint/line
  state, matching the observed underline/strikethrough residual family.

This supersedes the older tempting "text path equals `SkTextUtils::GetPath ->
drawPath`" interpretation for the focused simple text samples. `drawPath` is
still present in nearby general canvas code, but the observed text-raster route
builds shaped text blobs and draws decoration strokes separately.

The current runtime already uses Skia CPU surfaces and native-like font flags,
but it still lays out most text as one `canvas.draw_str` call per character and
computes decorations in importer code. That explains the current residual
clusters:

- `Text_7`, `Text_9`, `Text_11`, and `Text_12` are dominated by long horizontal
  underline/strike rows and italic text-run width differences. Their common
  cause is decoration draw-list coordinates plus shaped run positioning, not a
  font-specific y-offset.
- `Text_14` and `Text_15` are CJK vertical-writing/tate-chu-yoko layout cases.
  They should not be tuned from the Latin vertical constants used for `Text_4`.
- `Text_5` remains a path-text case. The basic circular arc branch is useful,
  but CSP likely feeds shaped glyph runs through a path-text transform rather
  than placing independent per-character `draw_str` calls.
- `Text_1` through `Text_3`, `Text_10`, and `Text_13` are comparatively small
  horizontal glyph-position/raster differences.

The next general implementation target is therefore a CSP-shaped text source
rasterizer: construct line/run data through Skia shaping or an equivalent
`allocRunTextPos` model, draw with `drawTextBlob`, and derive decoration line
geometry from that shaped run data. A naive `TextBlobBuilderRunHandler`
replacement was already rejected because its coordinate model was wrong; the
next attempt must first recover the run-handler origin, font-run grouping, and
decoration draw-line state. Avoid more per-sample constants unless this native
run model is already in place and a specific parameter is independently
verified.

Follow-up probes refine that conclusion:

- Replacing horizontal `draw_str` calls with `TextBlob::from_pos_text` and
  `draw_text_blob` while keeping the current per-character positions produced
  byte-identical compare metrics for `Text_1..Text_15`. Therefore the remaining
  mismatch is not in Skia's final text-blob raster operation; it is upstream of
  the glyph positions.
- Re-enabling `skia-safe/textlayout` and routing single-style horizontal lines
  through `Shaper::new_shape_then_wrap`, font/BiDi/script/language iterators,
  and `TextBlobBuilderRunHandler` still regressed the guard matrix when used
  naively. Default-offset raw means became `Text_1=14.291756`,
  `Text_6=24.399956`, `Text_11=20.518744`, and `Text_12=14.962537` versus the
  current `0.203794`, `2.780737`, `4.595794`, and `12.209344`. A small y-offset
  sweep only made `Text_12` slightly better at one offset (`11.402606`) while
  `Text_1` and `Text_6` remained far worse, so the failure is not a single
  baseline offset.
- `0x14363f5b0`, the helper called before both `drawLine` and `drawTextBlob`,
  configures SkPaint style, color, blend mode, antialias/subpixel state, stroke
  width, color/mask/image filters, stroke cap, stroke join, and alpha. That
  confirms decoration lines are normal draw-list stroke commands with runtime
  paint state, but the compact text metadata does not expose a simple final
  stroke-width/cap value that can be copied directly.

The safe next reverse target is the CSP text-run builder around
`0x14363c820..0x14363d9c0`: recover which script value, font-manager iterator
request, run-handler offset, width, and later `drawTextBlob` origin are paired
with the saved text entry. Do not turn `textlayout` back on in product code
until that coordinate model is recovered and the focused text guard matrix
improves broadly.

Additional read-only disassembly around that builder narrows the shape:

- CSP creates `SkShaper::MakeShapeThenWrap` at `0x14363c883`, but the
  surrounding code does not simply hand the resulting handler blob to the
  renderer.
- Its `MakeBiDiRunIterator` call passes `0xfe` as the BiDi level byte. Using the
  same value in a direct `TextBlobBuilderRunHandler` probe still regressed the
  samples, so the BiDi level alone is not the missing rule.
- The script iterator tag comes from helper `0x143639610`, not from ICU's
  default iterator alone. That helper maps classified text to `Jpan`, `Hans`,
  `Hant`, `Hang`, `Latn`, or fallback `Zyyy`.
- Width is effectively unlimited (`0x7f7fffff`) unless a finite positive layout
  width is stored on the text entry.
- After shaping, CSP calls a follow-up routine around `0x14363d320` that walks
  UTF-8 codepoints and copies glyph IDs, cluster maps, and position arrays into
  its own text entry.
- Later, CSP calls `SkTextBlobBuilder::allocRunTextPos` at `0x14363d968`,
  copies the saved UTF-8/run payload into that buffer, and only then stores the
  blob/run structure.
- The draw path resolves this saved blob through `0x14363c140` and calls
  `SkCanvas::drawTextBlob` at `0x14363eb1e` with the blob's internal positions
  plus a separate external `(x, y)` origin loaded from the draw command.

This explains the failed shaper probes: the recoverable rule is not "enable
`skia-safe/textlayout` and draw the handler output". It is "match CSP's saved
glyph-run buffer". Future text fidelity work should therefore target the
`0x14363d320` codepoint/cluster/position transfer and the later
`allocRunTextPos` payload layout before changing product rendering again.
