# Blender Clip Studio Importer

Blender add-on for loading Clip Studio Paint `.clip` files as flattened raster image textures.

The importer calls the Rust C ABI, uploads RGBA pixels into a generated Blender image datablock, and packs the latest render into the `.blend` without writing a sidecar PNG. The installable add-on no longer includes the older Python compositor or sidecar PNG path.

## Status

Package version: `0.8.32`.

Implemented:
- Full-color raster tile decode from `.clip` external chunks.
- Paper/background layers, masks, opacity, `LayerVisibility` bit flags, clipping layers, folders, and offscreen group compositing.
- Observed CSP blend modes, plus current adjustment/filter-layer support used by the supplied samples.
- Blender import, manual reload, non-blocking auto-reload, and packed-image freshness checks after opening a `.blend`.
- Native renderer bridge for Blender generated images through packaged `clip_capi`.
- Image-panel diagnostics for native render status, missing sources, render errors, native support summaries, support resource statistics, expandable unsupported-layer details, and copyable support reports.
- Native CLI support diagnostics in both readable text and machine-readable JSON.

Known fidelity gaps:
- Remaining native GPU differences are low-level formula/quantization cases on complex blend/filter samples.
- Vector, bubble/frame, text, 3D, animation timelines, and write-back are out of scope.

## Install

1. In Blender, open `Edit > Preferences > Add-ons`.
2. Choose `Install...`.
3. Select `clip_studio_importer.zip` from this project root.
4. Enable `Clip Studio Paint (.clip) Importer`.

## Use

1. In Blender, choose `File > Import > Clip Studio (.clip)`.
2. Select a `.clip` file.
3. Blender creates or updates a generated image from the packaged native renderer and packs the rendered pixels into the `.blend`.
4. Save the `.clip` again in Clip Studio Paint to trigger auto-reload, or use `Reload from .clip` in the Image Editor N-panel.

The installable zip includes the locally built release `clip_capi` library under `clip_studio_importer/native/`, so `Native renderer library` is only needed as an override.

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

The repository root contains `.clip` files and CSP-exported `.png` files used as ground truth. Important current samples include:

- `Illustration.clip/png` - single-layer MVP decode.
- `Illustration4K.clip/png` - large multi-layer alpha-over baseline.
- `Test_RealArt.clip/png` - real artwork smoke test for masks, groups, clipping, and folder semantics.
- `Test_ClippingEdge.clip/png` and `Test_ClippingEdge4K.clip/png` - root-level clipped edge alpha samples.
- `Ref_Emuri_Live2D_2024.clip/png` - clipped Add Glow sample.
- `Test_AddGlowMultiply.clip/png` - unresolved clipping group structure sample.
- `Test_ToneCurve.clip/png` - adjustment/filter-layer regression sample.
- `Ref_Terra404_Live2D.clip/png` - real artwork follow-up sample for mask and THROUGH group semantics.
