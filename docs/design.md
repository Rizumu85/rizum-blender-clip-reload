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
7. The Image Editor N-panel shows native render status, elapsed and last render
   timing, renderer version, native support summary, support resource
   statistics, missing-source state, and the latest native render error for the
   selected `.clip` image.
   The panel shows compact unsupported layer/node/kind locators, can copy either
   those locations or the full support report to the clipboard, and can open the
   full report as a searchable Blender Text datablock.

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

- Background render progress is elapsed-time only; there is no per-layer or
  percentage progress indicator yet.
- Unsupported layer features are summarized at image level with counts,
  resource statistics, compact unsupported layer/node/kind locators, and
  unsupported layer/node details. The panel previews the first few entries, can
  expand to show the full support-detail list stored on the image, can copy only
  the locator list or the full support report to the clipboard, and can open the
  report in Blender's Text Editor for searching.
- Fidelity failures are only visible through rendered image differences; Blender does not yet summarize supported-but-imperfect formula or quantization residuals in the UI.
- Native generated-image loading exists, including packed-pixel persistence,
  manual reload, background watcher refresh, and `load_post` freshness checks.
  The remaining native-path UX work is richer layer navigation beyond copied
  locators and the searchable support report, such as locating layers in a
  dedicated browser.
