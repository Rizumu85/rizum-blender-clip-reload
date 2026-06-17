# Blender `.clip` Loader UX

## Project Goal

Let an artist use raster-focused Clip Studio Paint `.clip` files in Blender as flattened full-resolution image textures that refresh without manual PNG or PSD export.

## Current User Workflow

1. Install the Blender add-on from `clip_studio_importer.zip`.
2. Use `File > Import > Clip Studio (.clip)` to choose a `.clip` file.
3. By default, the add-on creates a generated Blender image immediately and
   starts the packaged out-of-process `clip_cli` worker in the background. When
   the worker returns RGBA pixels, the add-on uploads them into that image and
   marks the image as needing pack. The worker keeps native GPU rendering out of
   Blender's UI process; it is not a Python compositor or sidecar PNG cache.
   If the current Blender screen already has Image Editor or UV Editor areas
   open, those editors switch to the newly imported image.
4. When the source `.clip` is saved again, auto-reload watches lightweight file
   freshness metadata and refreshes the Blender image after the background
   render finishes. Reload does not pack immediately.
5. If auto-reload is disabled or the user wants an immediate refresh, the Image Editor N-panel exposes `Reload from .clip`.
6. Add-on preferences report whether the packaged native renderer worker is
   present; users do not choose a renderer path.
7. The Image Editor N-panel shows native render status, elapsed and last render
   timing, pack status, `Pack Now`, renderer version, native support summary,
   support resource statistics, missing-source state, and the latest native
   render or pack error for the selected `.clip` image.
   The copied/searchable diagnostics also include source size and SHA-256. The
   panel shows compact unsupported layer/node/kind/name issue locators and can
   copy either those locations or the full support report to the clipboard.
   These locators are for bug reports and source-file follow-up, not Blender
   layer navigation.

## Later Native Workflow

The accepted stock Blender native workflow is an image-datablock bridge, not a
sidecar PNG cache and not a Python compositor:

1. Install the native renderer plus the Blender add-on.
2. Use `File > Import > Clip Studio (.clip)` to choose a `.clip` file.
3. The add-on creates a generated Blender `Image`, calls the packaged native
   Rust/wgpu worker in the background, and uploads the returned RGBA pixels into
   that image on the main thread.
4. The add-on records `.clip` source metadata on the image and marks successful
   renders as needing pack. Dirty images are packed either by the `Pack Now`
   button or automatically from Blender's `save_pre` handler before saving the
   `.blend`.
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
- Import completion shows the new generated image in any Image Editor or UV
  Editor areas already open on the current screen. It does not create new
  editors or change the user's workspace layout.
- Image Editor N-panel: `Image > Clip Studio`.
  - `Reload from .clip`
  - `Pack Now`
  - render status, pack status, and timing diagnostics
- Add-on preferences:
  - `Auto-reload on .clip change`
  - `Poll interval (seconds)`
  - `Debug log`
  - Packaged native renderer found/missing status

## Interaction Principles

- Keep the Blender UI responsive while reloading large `.clip` files.
- Keep generated images source-tracked immediately, but defer persistence cost:
  reload marks images as needing pack, `Pack Now` packs the current pixels on
  demand, and `save_pre` packs dirty native images before the `.blend` is saved.
- Make failures visible through Blender reports for direct actions and through
  image-level status/error metadata for background work.
- Avoid adding CSP-editing concepts to Blender. The add-on is read-only and only presents the flattened canvas.
- Prefer ordinary Blender Image semantics when possible. If native OIIO loading works later, `.clip` should behave like PSD/PNG from the artist's point of view.
- Keep vector, bubble/frame, and text content outside the user-facing promise unless the project scope is explicitly reopened.
- Do not add layer navigation or CSP layer-management UI. Blender owns the
  flattened texture reload workflow only.

## Current UX Gaps

- Background render progress is elapsed-time only; there is no per-layer or
  percentage progress indicator yet.
- Unsupported layer features are summarized at image level with counts,
  resource statistics, compact unsupported layer/node/kind issue locators with
  layer names when available, and unsupported layer/node details. The panel
  previews the first few entries, can expand to show the full support-detail
  list stored on the image, can copy only the locator list or the full support
  report to the clipboard, and can open the report in Blender's Text Editor for
  searching.
- Fidelity failures are only visible through rendered image differences; Blender does not yet summarize supported-but-imperfect formula or quantization residuals in the UI.
- Native generated-image loading exists, including manual reload, background
  watcher refresh, `load_post` freshness checks, explicit pack status, manual
  `Pack Now`, and save-time packing for dirty native images. The remaining
  native-path UX work is limited to clearer progress reporting for the
  flattened texture workflow.
