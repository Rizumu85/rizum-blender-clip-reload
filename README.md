# Blender Clip Studio Importer

Blender add-on for loading Clip Studio Paint `.clip` files as flattened raster image textures.

The default path decodes `.clip` in Python, writes a sidecar PNG cache next to the source file, and loads that PNG as a normal Blender image. An optional native renderer mode calls the Rust C ABI, uploads RGBA pixels into a generated Blender image datablock, and packs the latest render into the `.blend` without writing a sidecar PNG.

## Status

Package version: `0.8.23`.

Implemented:
- Full-color raster tile decode from `.clip` external chunks.
- Paper/background layers, masks, opacity, `LayerVisibility` bit flags, clipping layers, folders, and offscreen group compositing.
- Observed CSP blend modes, plus current adjustment/filter-layer support used by the supplied samples.
- Blender import, manual reload, and non-blocking auto-reload.
- Optional native renderer bridge for Blender generated images through `clip_capi`.

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
3. In the default mode, Blender loads the decoded sidecar PNG at `<source>.clip.png`.
4. Save the `.clip` again in Clip Studio Paint to trigger auto-reload, or use `Reload from .clip` in the Image Editor N-panel.

To test the native path, enable `Use native renderer` in the add-on preferences and set `Native renderer library` to the built `clip_capi` library, for example `native/rust/target/release/clip_capi.dll` on Windows. In this mode the add-on creates or updates a generated Blender image, stores `.clip` source metadata on it, and repacks the rendered pixels after import/reload.

## Project Layout

- `clip_studio_importer/` - Blender add-on package.
- `clip_studio_importer.zip` - installable add-on zip.
- `clip_loader.py` - project-root development copy of the decoder/compositor.
- `docs/AI_MEMORY.md` - current compact agent-readable state.
- `docs/analysis.md` - technical findings and sample-specific investigations.
- `docs/design.md` - Blender UX and user-facing behavior.
- `docs/plan.md` - durable directions and open work.

## Roadmap

- Keep the sidecar PNG workflow stable only as the current Python implementation.
- Continue the native direct-load rewrite toward making the Rust renderer path the accepted Blender import path.
- Replace and then remove the Python compositor/loader and sidecar PNG workflow once the native path owns import, reload, source tracking, and packaging end to end.

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
