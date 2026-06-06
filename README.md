# Blender Clip Studio Importer

Blender add-on for reading Clip Studio Paint `.clip` files as flattened image textures. The current implementation decodes the `.clip`, writes a sidecar PNG cache next to the source file, and loads that PNG as a regular Blender image.

## Status

The MVP is delivered and the project is in multi-layer fidelity work. Package version: `0.8.22`.

Implemented:
- Full-color raster tile decode from `.clip` external chunks.
- Paper/background layers, raster-like masked layers, folders, masks, opacity, visibility bit flags, clipping layers, and offscreen group compositing.
- Observed CSP blend modes used by the supplied samples.
- Blender import operator, manual reload operator, and non-blocking auto-reload.

Known fidelity gaps:
- `Ref_Terra404_Live2D` still has localized color differences after the sampled `LayerType=3` mask, masked THROUGH group, and clipped Add Glow effective-alpha fixes.
- `Test_AddGlowMultiply` shows unresolved Add Glow base plus clipped Multiply/Normal group semantics.
- Non-zero layer offsets are detected but not implemented until a supplied sample requires them.
- Vector and text rendering have partial native-backed preview support. The current vector blocker is `Vector_SizePressure`, which is close but not pixel-exact.
- 3D, grayscale, monochrome, animation timelines, and write-back are out of scope.

## Roadmap

Current track:
- Finish `.clip` semantics in Python first: blend modes, masks, clipping, folder/group behavior, and real-art fidelity.
- Keep the Blender add-on stable through the sidecar PNG path while the format rules are still changing.

Native track, later:
- Move the verified decoder/compositor core to C++ or Rust for speed and lower memory churn.
- Revisit an OpenImageIO `ImageInput` plugin so Blender can potentially load `.clip` through the normal image path.
- If that succeeds, use `.clip` as the actual Blender Image filepath and keep sidecar PNG only as fallback/debug output.
- Let generic image auto-reload plugins handle `.clip` reloads when they watch `Image.filepath` and call `image.reload()`.

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

## Native Loader Notes

An initial Blender 5.0.1 spike found:
- Blender can load a valid PNG even when renamed to `.clip`.
- Blender's bundled OpenImageIO exposes a configurable `plugin_searchpath`.
- An external OIIO plugin may eventually make `.clip` load like PSD/PNG without a visible sidecar.

Blocker before the next native spike:
- Get Blender-matched OIIO headers/libs.
- Use a compatible MSVC toolchain.
- Build a fake `ImageInput` plugin that returns a known test image before porting the real `.clip` decoder.

## Project Layout

- `clip_studio_importer/` - Blender add-on package.
- `clip_studio_importer.zip` - installable add-on zip.
- `clip_loader.py` - project-root development copy of the decoder/compositor.
- `docs/AI_MEMORY.md` - current agent-readable state and next reverse-engineering target.
- `docs/analysis.md` - technical findings and sample-specific investigations.
- `docs/design.md` - Blender UX and user-facing behavior.
- `docs/plan.md` - current directions and tracked implementation work.

## Verification Samples

The repository root contains `.clip` files and CSP-exported `.png` files used as ground truth. Important current samples include:

- `Illustration.clip/png` - single-layer MVP decode.
- `Illustration4K.clip/png` - large multi-layer alpha-over baseline.
- `Test_RealArt.clip/png` - real artwork smoke test for masks, groups, clipping, and folder semantics.
- `Ref_Wuwu_Live2D.clip/png` - visibility bit-flag regression sample.
- `Test_ClippingEdge.clip/png` and `Test_ClippingEdge4K.clip/png` - root-level clipped edge alpha samples.
- `Ref_Emuri_Live2D_2024.clip/png` - clipped Add Glow sample.
- `Test_AddGlowMultiply.clip/png` - unresolved clipping group structure sample.
- `Ref_Terra404_Live2D.clip/png` - real artwork follow-up sample for mask and THROUGH group semantics.
