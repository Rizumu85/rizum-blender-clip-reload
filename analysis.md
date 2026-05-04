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

- The worst pixel after group compositing was under `LayerType=0` folders with `LayerComposite=30`. This value appears to be CSP's folder pass-through mode, but it was not the root cause of the pixel error.
- At the worst point, the CSP export matched masked raster layer 204's raw pixel `[255, 246, 242, 255]`, while the loader's decoded mask value was `0`, causing the layer to be hidden and a darker lower layer to show through.
- The mask is enabled in CSP. The actual bug was that `_parse_exta()` filled omitted `exist=0` mask chunks with black (`0`). The mask offscreen attribute contains `InitColor = 0xffffffff`, so omitted mask chunks must default to white (`255`, fully visible).
- The loader now reads the mask offscreen `InitColor` and uses it as the fill byte for omitted single-channel mask chunks. `Test_Mask` remains bit-perfect.
- On `Test_RealArt.clip/png`, this lowers premultiplied mean diff from `0.0012807` to `0.0000324`, and exact RGBA pixels improve from `10445084 / 24400000` to `10520542 / 24400000`. Median, 90th, and 99th percentile per-pixel premultiplied max diff remain `0.0`.

Follow-up on the next worst real-art pixel:

- The next worst point was `(2210, 1506)`: reference `[190, 198, 217, 255]`, loader `[4, 3, 3, 255]`.
- Raw layer inspection showed the reference color exists on layer `375` (`耳奥髪９`) and inside group `376` (`数珠ｋ`) under folder `363` (`インナー`). Layer `375` has `LayerClip=1`.
- The immediate lower siblings before layer `375` are `LayerComposite=30` / `THROUGH` folders whose effective alpha at that pixel is `0`. Treating those through folders as clipping bases incorrectly hides layer `375`.
- The loader now renders `THROUGH` folders directly into the parent buffer but clears the clipping target after such a folder. This matches the observed CSP output for the pixel and improves `Test_RealArt` to premultiplied mean diff `0.00000786`, max diff `0.5333`, and exact RGBA pixels `10524499 / 24400000`.

Follow-up on consecutive clipping:

- The next worst point was `(1970, 977)`: reference `[255, 255, 255, 255]`, loader `[187, 119, 119, 255]`.
- Raw layer inspection showed the reference color exists on layer `509` (`歯`), with `LayerClip=1`. The previous sibling `508` (`舌`) is also clipped and has alpha `0` at that pixel; both should clip to the same non-clipped base layer `507` (`効果`).
- The loader now keeps a separate clipping base alpha. Non-clipped layers update the base; clipped layers use that base but do not replace it. `THROUGH` folders still clear the base at their boundary.
- On `Test_RealArt.clip/png`, this lowers the premultiplied max diff from `0.5333` to `0.1490`, mean diff from `0.00000786` to `0.00000344`, and exact RGBA pixels improve from `10524499 / 24400000` to `10524997 / 24400000`.

Follow-up on `LayerType=0` folder blend modes:

- The next worst point was `(1768, 1314)`: reference `[200, 115, 110, 255]`, loader `[218, 151, 148, 255]`.
- Raw layer inspection showed folder `314` (`前髪ｋ`) has `LayerType=0` and `LayerComposite=2` (`MULTIPLY`). The previous implementation treated every non-group folder as direct traversal unless it was `THROUGH`, so this folder's Multiply mode was ignored.
- The loader now treats `LayerType=0` folders as pass-through only when their blend mode is `THROUGH`. Other folder modes are rendered to an offscreen buffer, then composited through the folder row's own blend mode, opacity, mask, and clipping flag.
- On `Test_RealArt.clip/png`, this aligns the point exactly and lowers the premultiplied max diff from `0.1490` to `0.1117`, mean diff from `0.00000344` to `0.00000282`, and exact RGBA pixels improve from `10524997 / 24400000` to `10525646 / 24400000`.
- The next largest remaining difference is a semi-transparent clipping edge at `(1842, 1013)`: reference `[203, 157, 149, 245]`, loader `[218, 176, 173, 253]`. The involved layers are `451` (`輪郭ベース`) and clipped layer `452` (`輪郭効果`). Table fields do not yet expose an obvious "base hidden" or special alpha flag, so this should be investigated separately before changing clipping edge semantics.

Follow-up on clipped edge alpha inside offscreen folders:

- Visual inspection showed the remaining `(1842, 1013)` difference was a very small color/alpha mismatch on the face outline edge rather than a structural layer-order problem.
- At that point, folder `450` (`輪郭`) is rendered to an isolated offscreen buffer. Its base layer `451` has raw `[198, 198, 198, 191]`, and clipped layer `452` has raw `[231, 177, 175, 255]`. The old generic clipping path composited both as normal layers, producing offscreen `[224, 181, 180, 239]` and final `[218, 176, 173, 253]`, while CSP's export is `[203, 157, 149, 245]`.
- CSP is better approximated there by letting the clipped layer recolor the clipping base while preserving the base edge alpha. Applying that rule globally is wrong: a lower-body point `(1966, 3577)` under `足衣装` has a base alpha of only `6` over already opaque artwork, and global alpha preservation incorrectly recolors it to the clipped layer color.
- The loader now uses a hybrid clipped-layer path only inside isolated folder/group buffers: if the current destination alpha is effectively the clipping base alpha at that pixel, preserve alpha and repaint color; otherwise use the previous product-alpha clipping path. This keeps opaque-underpaint clipping cases stable.
- On `Test_RealArt.clip/png`, this changes `(1842, 1013)` to `[203, 156, 148, 246]` (within 1/255 of the CSP export), keeps the previously fixed `(1768, 1314)`, `(1970, 977)`, and `(2210, 1506)` points exact, and lowers the full-image premultiplied max diff from `0.1117` to `0.0980`. Mean diff is `0.00000585`; exact RGBA pixels are `10525908 / 24400000`.

Follow-up on layer visibility bit flags:

- `Ref_Wuwu_Live2D.clip/png` exposed large extra visible regions in the loader output: a pink necklace and dark Multiply patches on both wings.
- Layer tracing showed these came from `LayerType=2` Multiply groups: `15` (`袖奥`), `26` (`袖左`), and `311` (`数珠ｋ`). The problematic groups/layers had `LayerVisibility` values `2` or were downstream of layers with value `2`.
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
- Layer tracing at `(2217, 268)` showed the stack under `*髪飾り > バイオリン`: normal base layer `220`, clipped `ADD_GLOW` layer `222`, clipped `MULTIPLY` layer `223`, then clipped `ADD_GLOW` layer `224`.
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
- Pixel tracing showed the opaque pixel came from layer `489` (`落影`), a `LayerType=3` layer with `LayerLayerMaskMipmap=515` but `LayerMasking=0`.
- Decoding that mask mipmap directly showed mask value `0` at `(1845, 1990)`. The loader previously ignored the mask because it gated mask decoding on `LayerMasking`; `LayerType=3` can carry an active mask mipmap even when that flag is `0`.
- The loader now applies a layer mask whenever `LayerLayerMaskMipmap` is present. `Test_Mask.clip/png` remains bit-perfect, and the Terra sampled point now decodes as transparent `[0, 0, 0, 0]`.
- Full-image follow-up then exposed `(2162, 1449)`: reference `[41, 4, 9, 255]` vs loader `[234, 204, 218, 255]`.
- Pixel tracing showed the loader produced the correct dark value after folder `540` (`線`), then overwrote it through `LayerType=2` group `545` (`影`) with `LayerComposite=30` (`THROUGH`). Rendering `LayerType=2` THROUGH children directly into the parent buffer makes the dark-line points exact, but also bypasses the group mask and leaks opaque pixels outside the masked region.
- The loader now renders THROUGH children into the parent, then blends the before/after contribution back through the THROUGH group's own mask and opacity. Sampled Terra points now match both sides: mask-inside dark-line points `(2162, 1449)` and `(2636, 1449)` decode as `[41, 4, 9, 255]`, while mask-outside points `(2162, 1453)`, `(2165, 1458)`, and `(2587, 1597)` remain transparent.
- A later follow-up exposed clipped Add Glow over-brightening around `(2397, 559)`: reference `[71, 68, 80, 255]` vs loader `[144, 170, 177, 255]`. The involved clipped `ADD_GLOW` layer had high raw alpha but an effective layer alpha near zero after mask/clip application.
- The clipped preserve path now keeps raw source alpha for Normal/other blend recoloring, but uses effective layer alpha for `ADD` / `ADD_GLOW` strength. This keeps `Test_ClippingEdge.clip/png` at max premultiplied diff `0.003906`, `Test_ClippingEdge4K.clip/png` at max `0.003568`, preserves exact `Ref_Emuri_Live2D_2024` sampled Add Glow points, and reduces the Terra sampled point `(2397, 559)` to `[75, 114, 124, 255]`.
- Full-image Terra follow-up after the mask, THROUGH, and clipped Add Glow fixes: max premultiplied diff `0.349019617`, mean `0.000395088`, exact pixels `7054122 / 29280000`. Previously fixed samples remain exact: `(1845, 1990)`, `(2162, 1449)`, `(2636, 1449)`, and `(2162, 1453)`. The next worst point is `(2190, 1319)`, reference `[223, 164, 201, 255]` vs loader `[154, 75, 137, 255]`.
- Single-pixel tracing at `(2190, 1319)` shows `Group 584` is not leaking its Multiply layer: the group mask is `0` at this pixel. The visible darkening happens later in `Group 605` / folder `610` (`線`). There, two low-alpha non-clipped dark layers (`611` alpha `3`, then `612` alpha `100`) establish a clipping base, and clipped color layers `613`-`616` follow.
- The old clipped preserve threshold was `clip_base + 1.5/255`. Because layer `611` raises the destination alpha from `100` to `102`, the clipped color layers missed the preserve path by half a quantization step and used regular compositing, raising the local folder alpha to about `199` and darkening the bright parent color to `[154, 75, 137]`.
- The preserve threshold was first widened to `clip_base + 2.0/255`. A full-image Terra follow-up confirmed `(2190, 1319)` moved to near-exact: reference `[223, 164, 201, 255]` vs loader `[223, 163, 202, 255]`. Overall Terra summary became max premultiplied diff `0.345098078`, mean `0.000394923`, exact pixels `7054141 / 29280000`.
- The next Terra worst point was `(2287, 1311)`, reference `[203, 139, 186, 255]` vs loader `[137, 51, 125, 255]`. Single-pixel tracing showed the same folder `610` mechanism at a slightly higher alpha: layer `611` alpha `4`, layer `612` alpha `117`, then clipped layers `613`-`616`. The `2.0/255` threshold still missed preserve; `2.25/255` is enough for a targeted scalar replay to move the point toward `[214, 139, 200]`.
- The preserve threshold is now `clip_base + 2.25/255`. Regression samples remain stable: `Test_Mask` is bit-perfect, `Test_ClippingEdge` remains max `0.003906`, `Test_ClippingEdge4K` remains max `0.003568`, and the known `Ref_Emuri_Live2D_2024` Add Glow points remain exact. The next full-image Terra run should confirm the new worst point after the `2.25/255` adjustment.

## Known Bugs in Reference Code

- `csp_tool.py._get_layer_thumbnail` matches `MainId` against the user-supplied `layer_id` but should match `LayerId`. Coincidence in single-layer files masks this. Patched locally for verification; another reason to write our own minimal decoder rather than vendor csp_tool.

## Open Questions

1. **Terra localized color follow-up.** The original opaque-content worst point is fixed by honoring `LayerLayerMaskMipmap` on `LayerType=3`; the dark-line overwrite is fixed by masked THROUGH group rendering; the sampled clipped Add Glow over-brightening is reduced by using effective alpha for Add Glow strength; and the `(2190, 1319)` / `(2287, 1311)` darkening is improved by a slightly wider clipped preserve threshold. `Ref_Terra404_Live2D` still needs another full-image pass to confirm the new worst point after the `2.25/255` threshold.
2. **Clipping group structure.** `Test_AddGlowMultiply` shows that CSP likely treats a base layer plus clipped siblings as a more isolated clipping group before applying the base layer's blend mode to the parent stack. A naive grouping prototype improves the sample but oversaturates blue, so clipped Multiply / Normal strength still needs a tighter formula before implementation.
3. **Layer offsets.** The loader warns on non-zero `LayerOffsetX / LayerOffsetY`; no supplied sample has required offset support yet.
4. **Grayscale / monochrome layers.** csp_tool says unsupported. Still out of scope until a real sample requires it.
5. **Vector / 3D / text layers.** Still out of scope; decide later whether to skip silently, warn in Blender UI, or use fallback preview data.
6. **Color management.** CSP authoring color space vs Blender scene linear may produce display differences even when raw decode is correct.

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
