# Blender `.clip` Loader UX

## Project Goal

Let an artist use raster-focused Clip Studio Paint `.clip` files in Blender as flattened full-resolution image textures that refresh without manual PNG or PSD export.

## Current User Workflow

1. Install the Blender add-on from `clip_studio_importer.zip`.
2. Use `File > Import > Clip Studio (.clip)` to choose a `.clip` file.
3. By default, the add-on calls the packaged `clip_capi` native renderer,
   uploads the returned RGBA pixels into a generated Blender image, and packs
   the rendered pixels into the `.blend`.
4. When the source `.clip` is saved again, auto-reload watches the file timestamp and refreshes the Blender image after the background render finishes.
5. If auto-reload is disabled or the user wants an immediate refresh, the Image Editor N-panel exposes `Reload from .clip`.
6. The `Native renderer library` preference is only needed as an override for
   the packaged library.
7. The Image Editor N-panel shows native render status, native support summary,
   missing-source state, and the latest native render error for the selected
   `.clip` image.

## Later Native Workflow

The accepted stock Blender native workflow is an image-datablock bridge, not a
sidecar PNG cache and not a Python compositor:

1. Install the native renderer plus the Blender add-on.
2. Use `File > Import > Clip Studio (.clip)` to choose a `.clip` file.
3. The add-on calls the native Rust/wgpu renderer and uploads the returned RGBA
   pixels into a generated Blender `Image`.
4. The add-on packs the rendered pixels into the `.blend` by default and records
   `.clip` source metadata on the image.
5. When the `.blend` is reopened, Blender immediately shows the packed last
   render. The add-on then checks the source `.clip`; if it changed, the add-on
   re-renders through the native renderer and updates the image datablock.
6. If the source `.clip` is missing, the packed pixels remain visible and the UI
   reports the missing source.

This stock Blender bridge does not make `.clip` a true file-backed Blender image
format. `Image.reload()` and generic external image auto-reload add-ons do not
own this path; reload is controlled by this add-on. If Blender later gains an
explicit ImBuf/source bridge for `.clip`, that can provide PSD-like
`bpy.data.images.load(".clip")` behavior.

## Blender UI Surface

- Import menu entry: `File > Import > Clip Studio (.clip)`.
- Image Editor N-panel: `Image > Clip Studio`.
- Add-on preferences:
  - `Auto-reload on .clip change`
  - `Poll interval (seconds)`
  - `Debug log`
  - `Native renderer library`

## Interaction Principles

- Keep the Blender UI responsive while reloading large `.clip` files.
- In the native bridge, pack rendered pixels into the `.blend` by default so
  reopening a project never shows an empty texture while waiting for source
  reload.
- Make failures visible through Blender reports for direct actions and through
  image-level status/error metadata for background work.
- Avoid adding CSP-editing concepts to Blender. The add-on is read-only and only presents the flattened canvas.
- Prefer ordinary Blender Image semantics when possible. If native OIIO loading works later, `.clip` should behave like PSD/PNG from the artist's point of view.
- Keep vector, bubble/frame, and text content outside the user-facing promise unless the project scope is explicitly reopened.

## Current UX Gaps

- Background render progress is only shown as a small `Rendering in background` label when the image panel is visible.
- Unsupported layer features are summarized at image level with counts and the
  first unsupported layer reason. A full layer-by-layer diagnostic view is not
  implemented yet.
- Fidelity failures are only visible through rendered image differences; Blender does not yet summarize unsupported layer kinds or skipped semantics in the UI.
- Native generated-image loading exists, including packed-pixel persistence,
  manual reload, background watcher refresh, and `load_post` freshness checks.
  The remaining native-path UX work is a richer layer-by-layer support view
  beyond the current image-level support summary.
