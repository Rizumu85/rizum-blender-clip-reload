# plan.md — Blender `.clip` Native Loader

## Working Agreement

- Rizum Guidelines are active for this project/thread until the user says otherwise.
- Karpathy Guidelines are active for this project/thread until the user says otherwise.

## Project Goal (one sentence)

Build a Blender add-on that reads Clip Studio Paint `.clip` files directly as image textures, reproducing the canvas at full resolution without requiring manual PNG/PSD export.

## MVP — Verifiable Success Criterion

A `.clip` file containing **a single Normal-blend raster layer at canvas size** can be opened in Blender and the resulting image, pixel-compared against the same file's CSP-exported PNG, matches within a small tolerance (e.g. >99% of pixels identical, allowing for color-space rounding).

Anything beyond this — multiple layers, blend modes, groups, masks, vector layers — is post-MVP. We do not start them until MVP passes.

---

## Direction 1: Survey Prior Art

Goal: Find and catalogue what's already publicly known about the `.clip` format so we don't redo solved work.

- [ ] Search public repos and writeups on `.clip` reverse engineering (GitHub, blog posts, forum threads).
- [ ] Record findings in `analysis.md` under a "Prior Art" section with links and a one-line summary of what each source covers.
- [ ] Identify which parts of the format are already documented vs. still unknown.

## Direction 2: Map the SQLite Schema on a Real Sample

Goal: Understand the table structure of an actual user `.clip` file end-to-end.

- [ ] Open the sample `.clip` with the `sqlite3` CLI.
- [ ] Dump `.schema` and a list of all tables with row counts.
- [ ] Identify the tables that hold: canvas metadata, layer hierarchy, layer properties, layer pixel blobs, masks, color profile.
- [ ] Write findings into `analysis.md` under "Schema Map".

## Direction 3: Decode the Tile / Chunk Format for One Raster Layer

Goal: Take ONE raster layer's binary blob and produce a correct full-resolution RGBA bitmap.

- [ ] Locate the chunked pixel data for one layer.
- [ ] Identify the tile grid size, per-tile compression, and byte ordering.
- [ ] Write a Python decoder that turns the blob into a NumPy RGBA array.
- [ ] Verify by writing the array to PNG and pixel-diffing against CSP's PNG export of that single layer.

## Direction 4: MVP — Multi-Layer Loader (delivered)

Goal: Tie the verified compositor into a Blender add-on.

- [x] Package the decoder + compositor as a Blender add-on with `File → Import → Clip Studio (.clip)`.
- [x] On import, create a Blender Image data-block from the decoded RGBA (vertical flip handled).
- [x] Add a "Reload from .clip" operator surfaced in the Image Editor N-panel.
- [ ] **User-side verification**: install the add-on, import `Illustration4K.clip`, confirm result matches `Illustration4K.png`.

## Direction 5 (Post-MVP): Multi-Layer Compositing

Goal: Produce a correct composite of multiple raster layers with Normal blending.

- [ ] Read full layer hierarchy (order, parent groups, opacity, visibility).
- [ ] Composite layers bottom-up with Normal blend + opacity + visibility.
- [ ] Pixel-diff against CSP's flattened PNG export.

## Direction 6 (Post-MVP): Blend Modes, Masks, Groups

Goal: Expand fidelity to cover what real-world `.clip` files actually use.

- [x] Map observed `LayerComposite` integers for Multiply, Screen, Overlay, Hard Light, Soft Light, Add, Subtract, Difference, Color Dodge, and Color Burn.
- [x] Map observed `LayerComposite` integers for Darken, Linear Burn, Glow Dodge, Add (Glow), Vivid Light, Linear Light, Pin Light, Hard Mix, Exclusion, Darker Color, Lighter Color, Divide, Hue, Saturation, Color, and Brightness.
- [x] Stop warning for mapped non-Normal blend modes; warn only for unknown composite integers.
- [x] Treat `LayerType=1584` paper layers as an opaque background color so colored paper affects flattened output.
- [x] Confirm and map Lighten.
- [x] Verify masked raster layers stored as `LayerType=3`.
- [x] Verify layer opacity within rounding tolerance.
- [x] Verify hidden layer / empty folder visibility behavior.
- [x] Verify recursive traversal for non-empty folders.
- [x] Add first-pass clipping layer support with alpha clipping.
- [x] Implement offscreen group compositing for `LayerType=2` grouped layers.
- [x] Investigate remaining localized `Test_RealArt` differences after Multiply group support.

## Direction 7 (Post-MVP): Live-Reload UX

Goal: Make iteration fast for the user — change `.clip` in CSP, see it update in Blender with one click or automatically.

Steps to be detailed once Direction 5 is in.

---

## Out of Scope (Explicit Non-Goals)

- Writing `.clip` files. Read-only.
- Vector layers, 3D layers, frame animation timelines, brush metadata.
- Round-tripping CSP-specific effects.
- Supporting CSP versions we have no sample from.

## Risks

- **Tile decode is the hard part.** If the per-tile compression turns out to be more than zlib (e.g. has a CSP-specific predictor or a custom packing scheme), Direction 3 expands significantly.
- **CSP version drift.** We only verify against versions the user provides samples from.
- **Color management.** CSP authoring color space vs. Blender scene linear may produce visible diffs even when the raw decode is correct.
