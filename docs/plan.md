# Plan

Last reconciled: 2026-06-16

## Purpose

This file records durable project directions. Do not update it for every probe, rejected hypothesis, address trace, or metric change.

- Current compact state: `docs/AI_MEMORY.md`
- Full evidence and rejected hypotheses: `docs/analysis.md`
- User-facing design context: `docs/design.md`
- Native rewrite direction: `docs/native-direct-load-rewrite.md`
- Native performance investigation: `docs/native-performance-investigation.md`

## Direction 1: Raster Fidelity First

Goal: improve flattened `.clip` output for raster artwork without reopening vector, bubble/frame, or text rendering.

Current focus:

- Keep the project-root `clip_loader.py` available as slow reference tooling for
  verification scripts while native fidelity gaps close.
- Keep the main compositor on straight `uint8` RGBA buffers so transparent cache RGB, Add/Add Glow byte alpha, and future native byte-domain blend work are expressible.
- Continue raster layer semantics: folders, masks, clipping, THROUGH groups, blend modes, adjustment/filter layers, and visibility bit flags.
- Keep `LayerVisibility` as a bit flag: values `0` and `2` are hidden; values `1` and `3` are visible.
- Use targeted sample improvements with guard samples before accepting compositor changes.
- Treat vector, bubble/frame, and text layers as unsupported/skipped content in this repo.

Open raster targets:

- `Ref_Terra404_Live2D`: complex clipped/grouped highlight stacks and bottom-edge clipped blend residuals are now close; remaining differences are low-level rounding-scale pixels.
- `Test_AddGlowMultiply`: Add Glow base plus clipped standard-preserve siblings now routes through the native GPU path; remaining CSP residual is low-level (`raw_max=5`, `premul_max=3`) and matches the Python verifier baseline.
- `Ref_Kabi_Live2D`: the former large white-eye residual is fixed; the native clipped-folder sibling path now renders recursively and the remaining known max is a small low-level residual around `(1454,1104)`.
- Non-zero render or mask offsets should stay sample-driven and guarded.

## Direction 2: Blender Add-on Workflow

Goal: keep the Blender importer usable on the native renderer path while the remaining native rewrite work closes.

Current focus:

- Keep manual reload and non-blocking auto-reload behavior stable.
- Keep the native renderer bridge as the add-on's only import/reload path:
  native imports render through the packaged out-of-process native worker,
  create generated/packed Blender images, store source-tracking properties, and
  update through the add-on's reload/watch path without writing sidecar PNGs.
- Keep native render state visible in Blender: image metadata and the Image
  Editor panel should report ready, refreshing, stale, missing-source,
  render-error states, and elapsed/last render timing.
- Surface native support summaries in Blender from the metadata-only C ABI
  support check, including resource statistics, expandable unsupported
  layer/node details, copyable diagnostic text, and searchable Text Editor
  reports. Compact unsupported layer/node/kind locators may be copied for issue
  reports, but the Blender add-on must not grow layer-navigation or CSP
  layer-management UI.
- Rebuild `clip_studio_importer.zip` whenever package code changes.

## Direction 3: Native Image Loading Rewrite

Goal: replace the Python compositor/sidecar PNG workflow with a native GPU renderer and Blender image-datablock integration.

Current policy:

- Rust plus `wgpu` is the chosen renderer direction, with a thin C++ OpenImageIO plugin boundary and a stock Blender image-datablock bridge.
- External OpenImageIO plugin loading alone is not enough for stock Blender `bpy.data.images.load(".clip")`; true file-backed support requires a Blender ImBuf/source bridge or upstream source patch. The C ABI has a byte-buffer session entry point (`clip_renderer_session_open_memory`), and the C++ OIIO adapter now supports `IOProxy` memory opens by reading `oiio:ioproxy` bytes into Rust memory sessions without temp files. This is host-integration plumbing for OIIO/ImBuf callers, not a replacement for the accepted generated-image Blender add-on path.
- Current native milestone: the stock Blender image-datablock bridge is the
  add-on runtime path. The add-on calls the packaged `clip_cli` worker so native
  GPU rendering runs outside Blender's UI process, creates generated/packed
  Blender images, records source metadata (mtime, size, SHA-256), and updates
  those images through manual reload, the background watcher, and Blender
  `load_post` freshness scans without writing sidecar PNGs. The watcher uses
  lightweight mtime/size checks, while `load_post` can also compare SHA-256.
  Render failures are stored as image metadata and shown in the Image Editor
  panel, while successful renders clear old error metadata. The add-on records
  elapsed/last render timing for manual and background renders. The C ABI
  exposes metadata-only native support summaries,
  and the add-on stores/displays source count, unsupported count, raster/mask
  resource statistics, expandable unsupported layer/node detail lines, compact
  unsupported layer/node/kind issue locators, and copyable/searchable support
  diagnostics. `tools/build_blender_addon.py`
  builds the installable zip with `__init__.py`, `native_bridge.py`, and the
  locally built release `clip_cli` worker plus `clip_capi` library under
  `clip_studio_importer/native/`;
  it no longer packages the Python compositor/loader. Preferences report the
  packaged native worker found/missing state instead of exposing a renderer
  override. The project-root `clip_loader.py` remains slow reference tooling for
  verification while native fidelity gaps close.
- Strict GPU coverage status: ordinary raster blend modes `LayerComposite=1..26` plus `36` are enabled, isolated containers can resolve with supported non-NORMAL blend modes, clipping runs support non-NORMAL raster bases, container/folder clipping bases, and clipped sibling stacks whose members may be rasters or recursively rendered containers/folders. THROUGH groups clear the clip base for following clipped layers, and adjustment/filter layers now route through a dedicated GPU pass: Brightness/Contrast (`FilterLayerInfo` type `1`), Level Correction (`2`), Tone Curve (`3`), HSL (`4`, native HSV-adjust shader mode), Color Balance (`5`), Invert/Reverse Gradient (`6`), Posterization (`7`), Threshold (`8`), and Gradient Map (`9`). Unknown future filter types remain explicit unsupported filter work until faithful native models exist. A metadata-only scan of all 35 current `img/*.clip` fixtures reports `unsupported=0`, including `Ref_绫音Aya_Live2D.clip --gpu-support-json`, `Ref_Kabi_Live2D.clip --gpu-support-json`, and `Test_RealArt.clip --gpu-support-check`. These samples are fully routed but still have residual formula/quantization or performance work; distinguish native/Python parity from shared Python/CSP residuals before changing shaders, and improve correctness only with source-backed native evidence and guard samples.
- Large-stack performance is now a throughput and scheduling issue rather than an OOM blocker. Strict GPU raster uploads use source-sized offscreen textures with shader-side canvas offsets, and raster/mask resource uploads now use `Queue::write_texture` with source row strides instead of explicit staging-buffer copy submissions. CHNKExta tile-blob parsing is split into `clip_file::external`, compressed tile blocks inflate directly into the expected output buffer instead of allocating a temporary decoded `Vec<u8>` per zlib block, and region raster reads can request only the tile blocks intersecting the visible source `Rect` so non-visible raster tile blocks are skipped before zlib decode. Full-tile coverage still uses the contiguous blob path to avoid per-tile allocation overhead. Selected-region reads now use a compact tile-rectangle selection and stream matching external blocks through a reusable scratch buffer directly into `TileRegionWriter`, avoiding the former per-region tile-index vector, owned block list, and block-ref vector. Tile plane decoders now use row spans so alpha rows copy as slices while full-colour/grayscale/monochrome rows avoid per-pixel edge checks. The full-colour, grayscale, and monochrome tile decoders can now also swizzle a validated source `Rect` directly into the smaller RGBA output used for upload. The host-facing normal render path uses a recursive streaming GPU source provider: the main selector builds a metadata-only GPU source tree/resource plan, then raster/mask tile payloads are decoded and uploaded at point of use inside containers, clipping runs, THROUGH groups, and filters. Clear-only passes for the main accumulation texture plus container/clipping caches are encoded with the first real pass instead of forcing their own queue submit/poll, and streaming ping-pong texture initialization clears both paired attachments in one render pass. Streaming state and provider resource binding helpers are split out of `stream.rs`; each batch retains its raster, mask, LUT, and intermediate cache resources until submission, then submits after either forty-eight encoded passes or about 256 MiB of retained GPU resources instead of flushing every source pass. Active-batch retained-resource lookup reuses duplicate raster/mask GPU caches until the next flush, avoiding repeated provider decode/upload for duplicate keys without pinning one-off large-stack resources for the whole render. Intermediate streaming flushes do not poll/wait; final readback polling waits for the ordered queue. Unmasked streaming passes reuse existing texture views for shader bindings that are not sampled, THROUGH groups reuse the caller's parent texture view instead of creating a duplicate before view, and the main accumulator, clipping cache, container cache, and THROUGH after-cache retain their paired texture views for each ping-pong cache lifetime instead of recreating views per source/child pass. Streaming source/cache passes maintain dirty canvas bounds and use scissored `LoadOp::Load` rendering for raster, clipping, container, and THROUGH passes; this reduces fragment work on sparse stacks while preserving ping-pong correctness by repainting the previous dirty region plus the current source/cache bounds. The runtime provider now answers raster source pixel sizes from render-plan metadata before decode/upload, so fully off-canvas raster sources can be skipped before tile decode/upload while fallback providers still derive bounds after upload. It also computes the canvas-visible source region before decode, asks `clip_file` to decode only that region, and returns the effective source offset to the streaming renderer, reducing CPU swizzle/copy output and GPU upload footprint without changing shader-space placement. Runtime masks now decode only the canvas-visible source region, upload true cropped R8 textures, and carry mask origin/fill metadata into normal, container, THROUGH, and filter shader sampling; non-zero `InitColor` is preserved by sampling fill outside cropped mask bounds instead of allocating a full-canvas mask. `ClipContainer` indexes external ids for constant-time CHNKExta body lookup, `clip_file` uses the `flate2` `zlib-rs` backend for tile inflation, and runtime provider/resource-plan code has been split from `clip_runtime/src/lib.rs` into `clip_runtime/src/gpu_provider.rs`. Known-empty container, clipping, and THROUGH subtrees now short-circuit before allocating full-canvas intermediate ping-pong caches. `ClipSession` holds the opened `.clip` container and batches render-plan raster/mask/filter source metadata, so support checks and the render provider reuse resolved sources instead of reopening the file or rerunning source queries per layer. `Test_RealArt.clip` now full-renders and compares against `Test_RealArt.png` without wgpu OOM (`raw_max=5`, `premul_max=2`) in roughly 48s on this machine after submission batching, submit-only intermediate flushes, dirty-bounds scissoring, metadata-bound raster skipping, selected-tile visible-source decode/upload, true cropped mask textures, known-empty cache short-circuiting, paired clear-pass initialization, view reuse, bounded masked filter passes, and active-batch duplicate resource reuse. The next native performance step is reducing non-empty full-canvas intermediate cache size/lifetime, remaining visible-tile inflate/upload and GPU pass overhead, and leading/unknown intermediate full-canvas cases without introducing CPU compositor fallback, post-processing, or a global all-layer texture cache.
- Selected-tile external decoding should stop once the last requested tile block has been found, so unrelated trailing CHNKExta blocks are not scanned or inflated.
- Streaming intermediate caches should stay cropped whenever metadata proves finite canvas bounds: clipping runs use the base visible bounds, isolated containers use the union of child bounds, and THROUGH groups use a bounded after-cache seeded from the parent-before texture. Source/cache resolve uniforms carry both source and target origins, and wgpu scissors are translated from global dirty bounds into local attachment coordinates. Unknown stacks, solid layers, and leading LUT filters keep full-canvas intermediate allocation; masked or unmasked LUT filters after bounded draws keep the prior dirty bounds by sampling masks at target-origin-adjusted canvas coordinates. Runtime masks upload as cropped R8 textures with canvas origin and fill metadata, and shaders sample outside cropped bounds as fill. Bounded nested THROUGH groups inside containers carry the parent target origin and keep cropped cache bounds. The remaining large-stack performance blocker is visible-tile inflate/upload and GPU pass throughput plus leading filter and unknown intermediate full-canvas cases.
- Streaming scheduling should skip sources that provably cannot affect output before decode/upload or cache allocation: zero-opacity rasters, containers, THROUGH groups, clipping bases/siblings, LUT filters, and transparent solid colors. Clipping runs with no provably effective clipped siblings may render their base directly instead of allocating a clipping cache. Keep this limited to faithful no-op cases; do not generalize it into heuristic pruning or post-processing.
- Metadata-only mask planning may elide constant off-canvas masks before provider decode/upload: fill `0` folds the source to zero opacity, fill `255` drops the mask resource as fully opaque, and partial fill values stay on the real mask-resource path. Keep this as a faithful resource-planning optimization only.
- High-leverage native performance work has moved from broad investigation to a
  validated atlas/tile-silo direction. `clip_cli --tile-silo-estimate` shows
  RealArt/Terra/Aya-class samples have canvas-sized raster metadata but very low
  compressed CHNKExta tile occupancy, so the next performance milestone should
  build an external tile-block atlas that uploads compressed-present raster/mask
  tiles and treats empty tiles as fill/transparent records. After that, build
  per-canvas-tile ordered work lists from the strict render plan and collapse
  raster/clipping stretches into one or a few tile-local shader passes while
  keeping filters, THROUGH groups, and isolated containers as explicit semantic
  barriers until faithful tile-local models exist. See
  `docs/native-performance-investigation.md`.
- Native raster extraction now applies render offscreen placement through `LayerRenderOffscrOffsetX/Y`, matching the existing mask placement model and the known `Ref_Terra404_Live2D` negative-X render sources. This removes a structural decode gap before further large-reference GPU work.
- Native raster extraction now decodes full-color, grayscale, and monochrome raster tile streams. `Test_ Grayscale.clip` and `Test_Monochrome.clip` route through the strict GPU path and compare exactly against CSP PNGs.
- Native support diagnostics use a metadata-only strict selector. `clip_cli --gpu-support-check` validates graph, raster source, mask source, and LUT-filter support without tile decode, GPU initialization, or rendering, and labels resource/unsupported layer ids with layer names when available. `clip_cli --gpu-support-json` emits the same support/resource/unsupported-node data as pure JSON for automation and issue capture, also carrying layer names when available. Other CLI plan, resource, stack, unsupported, and trace diagnostics should use the same layer-label helper so layer ids are easy to map back to Blender support reports. These commands must remain diagnostics only, not fallback renderers.
- `clip_cli --gpu-trace-pixel <x> <y>` is available for native GPU prefix tracing and now includes per-source before/after/input pixels. Current open traces point to a Subtract alpha/rounding boundary feeding Color Dodge/Color Burn in `IllustrationBlendModes.clip`, plus a Pin Light/Hue/Saturation residual in `IllustrationBlendModes2.clip`; rejected broad fixes should remain in evidence, not shader code.
- Do not reintroduce the Python compositor/loader or sidecar PNG implementation into the installable add-on.
- This direction is about flattened raster loading only; it does not restore vector, bubble/frame, or text renderer compatibility.

## Direction 4: Documentation Hygiene

Goal: make new conversations start from the right state quickly.

Current policy:

- Keep `docs/AI_MEMORY.md` as the short current-state memory.
- Keep `docs/analysis.md` as the append-only historical evidence log.
- Keep this file as durable direction, not a running checklist.
