# Rizum Clip Reload

Blender add-on for loading Clip Studio Paint `.clip` files as flattened raster image textures.

The importer calls the Rust C ABI, uploads RGBA pixels into a generated Blender image datablock, and packs the latest render into the `.blend` without writing a sidecar PNG. The installable add-on no longer includes the older Python compositor or sidecar PNG path.

## Status

Package version: `0.8.66`.

Implemented:
- Full-color raster tile decode from `.clip` external chunks.
- Paper/background layers, masks, opacity, `LayerVisibility` bit flags, clipping layers, folders, and offscreen group compositing.
- Observed CSP blend modes, plus current adjustment/filter-layer support used by the supplied samples.
- Blender import, manual reload, non-blocking auto-reload, and packed-image freshness checks after opening a `.blend`.
- Native renderer bridge for Blender generated images through packaged `clip_capi`.
- Image-panel status for native render state, missing sources, render errors, pack state, Developer Mode timing, and copyable English diagnostics.
- Localized Blender UI strings for Simplified Chinese, Japanese, and Spanish while keeping the add-on name and diagnostic output in English.
- Native CLI support diagnostics in both readable text and machine-readable JSON.

Known fidelity gaps:
- Remaining native GPU differences are low-level formula/quantization cases on complex blend/filter samples.
- Vector, bubble/frame, text, 3D, animation timelines, and write-back are out of scope.

## Install

1. In Blender, open `Edit > Preferences > Add-ons`.
2. Choose `Install...`.
3. Select `clip_studio_importer.zip` from this project root.
4. Enable `Rizum Clip Reload`.

## Use

1. In Blender, choose `File > Import > Clip Studio (.clip)`.
2. Select a `.clip` file.
3. Blender creates or updates a generated image from the packaged native renderer and packs the rendered pixels into the `.blend`.
4. Save the `.clip` again in Clip Studio Paint to trigger auto-reload, or use `Manual Reload` in the Image Editor N-panel.

The installable zip includes the locally built release native worker and C ABI library under `clip_studio_importer/native/`.

## Build Package

Build the native C ABI first, then rebuild the installable add-on zip:

```powershell
cd native\rust
cargo build --release -q -p clip_capi
cd ..\..
python tools\build_blender_addon.py
```

The package script writes `clip_studio_importer.zip` and, by default, requires and includes the release native renderer library. Use `--no-native` only for package-structure tests or native-library packaging probes.

## Project Layout

- `clip_studio_importer/` - Blender add-on package.
- `clip_studio_importer.zip` - installable add-on zip.
- `clip_loader.py` - project-root reference decoder/compositor used by verification tools.
- `docs/AI_MEMORY.md` - current compact agent-readable state.
- `docs/analysis.md` - technical findings and sample-specific investigations.
- `docs/design.md` - Blender UX and user-facing behavior.
- `docs/plan.md` - durable directions and open work.

## Roadmap

- Keep the installable add-on on the native renderer path.
- Continue native fidelity, diagnostics, and eventual true Blender file-backed `.clip` integration work.
- Keep the project-root Python loader only as slow verification/reference tooling while native fidelity gaps close.

## Verification Samples

The `img/` directory contains public `.clip` fixtures and CSP-exported `.png`
ground truth. Larger client/reference files such as `Ref_*` and
`Test_RealArt*` are local-only ignored fixtures.

Important current tracked samples include:

- `Illustration4K.clip/png` - large multi-layer alpha-over baseline.
- `IllustrationBlendModes.clip/png` and `IllustrationBlendModesB.clip/png` - blend-mode regression samples, with numbered PNG layer-reveal references.
- `Test_ClippingEdge.clip/png` and `Test_ClippingEdge4K.clip/png` - root-level clipped edge alpha samples.
- `Test_AddGlowMultiply.clip/png` - clipped Add Glow plus Multiply regression sample.
- `Test_ToneCurve.clip/png` and `Test_Gradiation.clip/png` - adjustment/filter-layer regression samples.
