# Blender `.clip` Loader UX

## Project Goal

Let an artist use raster-focused Clip Studio Paint `.clip` files in Blender as flattened full-resolution image textures that refresh without manual PNG or PSD export.

## Current User Workflow

1. Install the Blender add-on from `clip_studio_importer.zip`.
2. Use `File > Import > Clip Studio (.clip)` to choose a `.clip` file.
3. By default, the add-on starts the packaged out-of-process `clip_cli` worker
   in the background without creating a temporary placeholder image. When the
   worker returns RGBA pixels, the add-on creates the generated Blender image,
   uploads the pixels, switches any already-open Image Editor or UV Editor
   areas to that completed image, and then runs the initial pack as a deferred
   main-thread timer. The worker keeps native GPU rendering out of Blender's UI
   process; it is not a Python compositor or sidecar PNG cache.
4. When the source `.clip` is saved again, auto-reload watches lightweight file
   freshness metadata and refreshes the Blender image after the background
   worker finishes. Reload passes the previous native reload manifest back to a
   persistent native worker when available; unchanged renders only update
   metadata, matching graph/source tile changes update dirty image rects, and
   structural changes fall back to a full image update. The persistent worker
   reuses unchanged raster/mask GPU textures and renders matching patch reloads
   as dirty-region GPU outputs rather than full-canvas renders. Reload does not
   pack immediately.
5. If auto-reload is disabled or the user wants an immediate refresh, the Image Editor N-panel exposes `Manual Reload`.
6. Add-on preferences expose reload timing, debug logging, and Developer Mode;
   users do not choose a renderer path.
7. The Image Editor N-panel shows source file, non-ready render status, pack
   status with adjacent `Pack`, missing-source state, and the latest render or pack error
   for the selected `.clip` image. It does not show native renderer mode,
   full-support summaries, renderer version, resource counts, largest-resource
   metadata, successful packaged-worker status, or render timing in the normal
   UI. Developer Mode reveals last-render timing, phase timing, and an open
   diagnostics action. When unsupported native nodes exist, the panel shows
   compact unsupported layer/node/kind/name issue locators. These locators are
   for bug reports and source-file follow-up, not Blender layer navigation.

## Later Native Workflow

The accepted stock Blender native workflow is an image-datablock bridge, not a
sidecar PNG cache and not a Python compositor:

1. Install the native renderer plus the Blender add-on.
2. Use `File > Import > Clip Studio (.clip)` to choose a `.clip` file.
3. The add-on calls the packaged native Rust/wgpu worker in the background.
   Initial import creates the generated Blender `Image` only after the worker
   returns real canvas pixels, shows it in open image editors, and then packs
   the first render. Reload updates the existing generated image on the main
   thread without packing immediately. Native reload manifests are stored on the
   image so an active session, and reopened `.blend` files when the property is
   available, can request tile-diff worker output instead of always uploading a
   full canvas.
4. The add-on records `.clip` source metadata on the image. Initial imports
   auto-pack after the completed image is shown; reloads mark images as needing
   pack. Dirty reloads are packed either by the `Pack` button or
   automatically from Blender's `save_pre` handler before saving the `.blend`.
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
  Editor areas already open on the current screen. It does not create a
  placeholder image, create new editors, or change the user's workspace layout.
- Image Editor N-panel: `Image > Clip Studio`.
  - `Manual Reload`
  - short pack status (`Packed` / `Needs Pack`) with adjacent `Pack`
  - non-ready render status, pack status, errors, lower `Copy Diagnostic`, and
    unsupported-node locators only when unsupported nodes exist
  - Developer Mode-only timing and open-diagnostics controls
- Add-on preferences:
  - reload box: `Autoreload .Clip`, `Check Timer Frequency (s)`
  - developer box: `Debug log`, `Developer Mode`
  - Packaged native renderer missing status only

## Interaction Principles

- Keep the Blender UI responsive while reloading large `.clip` files.
- Keep generated images source-tracked once real pixels exist. Initial import
  packs after the completed image is shown; reload defers persistence cost by
  marking images as needing pack, `Pack` packs the current pixels on demand,
  and `save_pre` packs dirty native images before the `.blend` is saved.
- Prefer manifest-driven reload diffs over timestamp-only reload behaviour.
  Same-graph raster/mask compressed-tile changes may update only dirty rects;
  canvas, root, node-order, container/filter/paper semantic, or large dirty-area
  changes conservatively use full image updates. Keep the packaged worker
  process alive across requests so reload avoids repeated process startup and
  wgpu device initialization, reuses unchanged GPU input textures, and produces
  patch payloads from dirty-region GPU renders.
- Make failures visible through Blender reports for direct actions and through
  image-level status/error metadata for background work.
- Avoid adding CSP-editing concepts to Blender. The add-on is read-only and only presents the flattened canvas.
- Prefer ordinary Blender Image semantics when possible. If native OIIO loading works later, `.clip` should behave like PSD/PNG from the artist's point of view.
- Keep vector, bubble/frame, and text content outside the user-facing promise unless the project scope is explicitly reopened.
- Do not add layer navigation or CSP layer-management UI. Blender owns the
  flattened texture reload workflow only.

## Current UX Gaps

- Background render progress is elapsed-time only, and detailed render timing is
  hidden behind Developer Mode; there is no per-layer or percentage progress
  indicator yet.
- Unsupported layer features are summarized in the panel only when unsupported
  native nodes exist, using compact layer/node/kind issue locators with layer
  names when available plus a short preview of unsupported details. Full native
  support summaries, renderer version, source/resource counts, largest
  raster/mask metadata, successful packaged-worker status, and normal timing
  details are kept out of the normal UI. Copied/opened diagnostics focus on
  issue/debug fields: source path, source size/hash, status, timing, canvas
  size, renderer version, unsupported node count/locations/details, and
  render/pack errors.
- Fidelity failures are only visible through rendered image differences; Blender does not yet summarize supported-but-imperfect formula or quantization residuals in the UI.
- Native generated-image loading exists, including manual reload, background
  watcher refresh, `load_post` freshness checks, explicit pack status, manual
  `Pack`, and save-time packing for dirty native images. The remaining
  native-path UX work is limited to clearer progress reporting for the
  flattened texture workflow.
