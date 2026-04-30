# Blender Clip Studio Importer

Blender add-on for reading Clip Studio Paint `.clip` files directly as flattened image textures. The add-on decodes the `.clip`, writes a sidecar PNG cache next to the source file, and loads that PNG as a regular Blender image.

## Status

The MVP is delivered and the project is in multi-layer fidelity work. Package version: `0.8.22`.

Implemented:
- Full-color raster tile decode from `.clip` external chunks.
- Paper/background layers, raster-like masked layers, folders, masks, opacity, visibility bit flags, clipping layers, and offscreen group compositing.
- Observed CSP blend modes used by the supplied samples.
- Blender import operator, manual reload operator, and non-blocking auto-reload.

Known fidelity gaps:
- `Ref_Terra404_Live2D` still has an opaque-content leak where the CSP export is transparent.
- `Test_AddGlowMultiply` shows unresolved Add Glow base plus clipped Multiply/Normal group semantics.
- Non-zero layer offsets are detected but not implemented until a supplied sample requires them.
- Vector, text, 3D, grayscale, monochrome, animation timelines, and write-back are out of scope.

## Install

1. In Blender, open `Edit > Preferences > Add-ons`.
2. Choose `Install...`.
3. Select `clip_studio_importer.zip` from this project root.
4. Enable `Clip Studio Paint (.clip) Importer`.

## Use

1. In Blender, choose `File > Import > Clip Studio (.clip)`.
2. Select a `.clip` file.
3. Blender loads the decoded sidecar PNG at `<source>.clip.png`.
4. Save the `.clip` again in Clip Studio Paint to trigger auto-reload, or use `Reload from .clip` in the Image Editor N-panel.

## Project Layout

- `clip_studio_importer/` - Blender add-on package.
- `clip_studio_importer.zip` - installable add-on zip.
- `clip_loader.py` - project-root development copy of the decoder/compositor.
- `analysis.md` - technical findings and sample-specific investigations.
- `design.md` - Blender UX and user-facing behavior.
- `plan.md` - current directions and tracked implementation work.

## Verification Samples

The repository root contains `.clip` files and CSP-exported `.png` files used as ground truth. Important current samples include:

- `Illustration.clip/png` - single-layer MVP decode.
- `Illustration4K.clip/png` - large multi-layer alpha-over baseline.
- `Test_RealArt.clip/png` - real artwork smoke test for masks, groups, clipping, and folder semantics.
- `Ref_Wuwu_Live2D.clip/png` - visibility bit-flag regression sample.
- `Test_ClippingEdge.clip/png` and `Test_ClippingEdge4K.clip/png` - root-level clipped edge alpha samples.
- `Ref_Emuri_Live2D_2024.clip/png` - clipped Add Glow sample.
- `Test_AddGlowMultiply.clip/png` - unresolved clipping group structure sample.
- `Ref_Terra404_Live2D.clip/png` - unresolved transparency leak sample.
