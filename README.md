# Blender Clip Studio Importer

Blender add-on for loading Clip Studio Paint `.clip` files as flattened raster image textures.

The current path decodes `.clip` in Python, writes a sidecar PNG cache next to the source file, and loads that PNG as a normal Blender image.

## Status

Package version: `0.8.22`.

Implemented:
- Full-color raster tile decode from `.clip` external chunks.
- Paper/background layers, masks, opacity, `LayerVisibility` bit flags, clipping layers, folders, and offscreen group compositing.
- Observed CSP blend modes, plus current adjustment/filter-layer support used by the supplied samples.
- Blender import, manual reload, and non-blocking auto-reload.

Known fidelity gaps:
- `Ref_Terra404_Live2D` still has localized differences around complex highlight, clipping, and group stacks.
- `Test_AddGlowMultiply` still isolates Add Glow base plus clipped Multiply/Normal group behavior.
- Vector, bubble/frame, text, 3D, animation timelines, and write-back are out of scope.

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
- `docs/AI_MEMORY.md` - current compact agent-readable state.
- `docs/analysis.md` - technical findings and sample-specific investigations.
- `docs/design.md` - Blender UX and user-facing behavior.
- `docs/plan.md` - durable directions and open work.

## Roadmap

- Improve Python raster fidelity first: masks, clipping, folder/group behavior, blend modes, and adjustment/filter layers.
- Keep the sidecar PNG workflow stable while raster semantics are still changing.
- Later, port the verified raster decoder/compositor core to C++ or Rust if performance requires it.
- Later, evaluate an OpenImageIO `ImageInput` plugin so Blender can load `.clip` through the normal image path. If accepted, this replaces the Python compositor/loader and sidecar PNG workflow instead of keeping compatibility or fallback paths.

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
