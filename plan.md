# plan.md - Blender `.clip` Native Loader

## Working Agreement

- Rizum Guidelines are active for this project/thread until the user says otherwise.
- Karpathy Guidelines are active for this project/thread until the user says otherwise.

## Project Goal

Build a Blender add-on that reads Clip Studio Paint `.clip` files directly as image textures, reproducing the canvas at full resolution without requiring manual PNG/PSD export.

## MVP Success Criterion

A `.clip` file containing a single Normal-blend raster layer at canvas size can be opened in Blender and the resulting image, pixel-compared against the same file's CSP-exported PNG, matches within a small tolerance.

Status: delivered. The implementation has moved beyond MVP into multi-layer fidelity and live-reload behavior.

## Direction 1: Format Research

Goal: Find and catalogue what is already publicly known about the `.clip` format so we do not redo solved work.

- [x] Search public repos and writeups on `.clip` reverse engineering.
- [x] Record findings in `analysis.md` under prior art and reusable-code sections.
- [x] Identify the solved pieces: chunk container, SQLite schema, tile decode, and full-color raster layers.

## Direction 2: Raster Decode

Goal: Decode raster layer tile data into a correct full-resolution RGBA bitmap.

- [x] Map the relevant SQLite tables for canvas, layers, mipmaps, offscreens, thumbnails, and previews.
- [x] Locate external chunk IDs for layer and mask pixel data.
- [x] Decode zlib-compressed 256x256 tiles into RGBA arrays.
- [x] Verify `Illustration.clip` against CSP-exported PNG with alpha-aware exact matching.

## Direction 3: Blender Add-on Integration

Goal: Tie the verified decoder and compositor into a Blender add-on.

- [x] Package the decoder and compositor as a Blender add-on with `File > Import > Clip Studio (.clip)`.
- [x] On import, create a file-backed Blender Image from a decoded sidecar PNG.
- [x] Add a `Reload from .clip` operator in the Image Editor N-panel.
- [x] Add non-blocking auto-reload with a background decode worker and main-thread image refresh.
- [x] User-side verification: install the add-on, import a sample `.clip`, edit/save in CSP, and confirm Blender refreshes.

## Direction 4: Multi-Layer Fidelity

Goal: Match CSP flattened PNG exports for real-world raster artwork as closely as possible.

- [x] Composite visible raster layers bottom-up with opacity.
- [x] Support paper layers as opaque background color.
- [x] Support mapped CSP blend modes observed in supplied samples.
- [x] Support masks, masked raster layers, folder traversal, layer visibility bit flags, clipping groups, and offscreen group compositing.
- [x] Validate against `Illustration4K`, isolated blend/mask/folder samples, `Test_RealArt`, and `Ref_Wuwu_Live2D`.
- [x] Fix root-level clipped-layer edge alpha behavior using `Test_ClippingEdge` and `Test_ClippingEdge4K`.
- [x] Fix clipped Add Glow color update using `Ref_Emuri_Live2D_2024`.
- [ ] Investigate the opaque-content transparency leak in `Ref_Terra404_Live2D`.
- [ ] Resolve clipping group semantics for Add Glow + Multiply stacks using `Test_AddGlowMultiply`.
- [ ] Add support for non-zero layer offsets when a sample requires it.
- [ ] Decide how unsupported vector, text, 3D, monochrome, and grayscale layers should be surfaced to Blender users.

## Direction 5: Packaging and Handoff

Goal: Make the current add-on easy to test and iterate on in Blender.

- [x] Build `clip_studio_importer.zip` from the add-on package.
- [x] Refresh package after the root-level clipping edge fix.
- [x] Write a short install/test handoff for Blender in `README.md`.
- [ ] Decide whether to keep project-root `clip_loader.py` as a development copy or remove duplication after confirming package layout.

## Direction 6: Future Enhancements

Goal: Improve the add-on after current raster fidelity and reload behavior are stable.

- [ ] Improve background decode status in Blender's UI.
- [ ] Add a cache-location preference if sidecar PNG files next to `.clip` become undesirable.
- [ ] Explore lower-resolution preview mode using mipmap chains for faster iteration.
- [ ] Evaluate color-management behavior between CSP exports and Blender texture display.

## Out of Scope

- Writing `.clip` files.
- Vector layers, 3D layers, text layers, frame animation timelines, and brush metadata.
- Round-tripping CSP-specific effects.
- Supporting CSP versions we have no sample from.

## Risks

- **CSP version drift.** We only verify against versions the user provides samples from.
- **Color management.** CSP authoring color space vs. Blender scene linear may produce visible diffs even when the raw decode is correct.
- **Unsupported layer kinds.** Vector, text, 3D, grayscale, monochrome, and timeline-specific data are intentionally out of scope until a real sample needs them.
- **Behavioral edge cases.** The remaining fidelity work is likely dominated by localized CSP group/clipping semantics rather than the basic tile or blend-mode decode.
