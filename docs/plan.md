# Plan

Last reconciled: 2026-06-18

## Purpose

This file records durable project directions. Do not update it for every probe, rejected hypothesis, address trace, or metric change.

- Current compact state: `docs/AI_MEMORY.md`
- Full evidence and rejected hypotheses: `docs/analysis.md`
- User-facing design context: `docs/design.md`
- Native rewrite direction: `docs/native-direct-load-rewrite.md`
- Native performance investigation: `docs/native-performance-investigation.md`
- Native tile-event renderer roadmap: `docs/native-tile-event-renderer-roadmap.md`
- Native tile-event main execution plan: `docs/native-tile-event-main-execution-plan.md`
- Native tile-event architecture strategy: `docs/native-tile-event-architecture-strategy.md`
- Domain vocabulary for architecture work: `CONTEXT.md`

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

- `Test_Gradiation`: highest public-fidelity target. Current release compare is
  `raw_max=10`, `premul_max=10`, and `premul_visible_px=45528`, much larger
  than the remaining HSL/blend-mode residuals. Treat this as the next clean
  public sample to investigate before more blend-mode quantization nibbling.
- `Ref_Kabi_Live2D`: strongest local-reference outlier by visible magnitude.
  The former white-eye structural error is fixed, but the current release
  compare still has `premul_max=29` at a small local Glow Dodge residual.
  The accepted translucent-black destination fix reduced the earlier
  `premul_max=32` hot spot; remaining work is the coloured-destination
  `layer 264 [发光1]` path, not another broad partial-alpha threshold. This is
  more suspicious than local references whose raw max is dominated by
  transparent RGB and whose premultiplied max is only 2-5.
- `Test_HSL`: current release compare is `raw_max=3`, `premul_max=3`, and
  `premul_visible_px=2668`. This remains worth tracking, but it should come
  after Gradient Map because the guard samples `Test_HSL2` through `Test_HSL5`
  are exact/one-LSB and prior fixed-point HSL shader probes regressed them.
- `IllustrationBlendModesB` and `IllustrationBlendModes`: remaining public
  blend-mode quantization targets (`raw_max=5` / visible `2718` and
  `raw_max=7` / visible `1008`). Continue only with native mapping/evidence for
  ordinary raster HSL/internal blend codes; avoid alpha-threshold or
  channel-order branches inferred only from these samples.
- Local large references such as MXL/Terra/Aya/Emuri should be treated as
  smoke guards for structural regressions. Their broad low-level differences
  are less actionable than the targets above unless a hotspot shows a coherent
  visible artifact rather than transparent-RGB or edge quantization noise.
- `Ref_Meimei_*_Live2D` is currently a clean smoke guard rather than a repair
  target: release compare reports `premul_max=1` and `premul_visible_px=0`,
  with raw max dominated by transparent RGB.
- Non-zero render or mask offsets should stay sample-driven and guarded.

## Direction 2: Blender Add-on Workflow

Goal: keep the Blender importer usable on the native renderer path while the remaining native rewrite work closes.

Current focus:

- Keep initial import, manual reload, and auto-reload behavior non-blocking.
- Keep the native renderer bridge as the add-on's only import/reload path:
  native imports render through the packaged out-of-process native worker,
  create generated Blender images, store source-tracking properties, and update
  through the add-on's reload/watch path without writing sidecar PNGs.
- Keep persistence explicit and cheap during reload: successful renders mark the
  image as needing pack, `Pack` packs immediately on user request, and
  Blender `save_pre` packs dirty native images before saving the `.blend`.
- Keep reload updates manifest-driven where possible: the worker should compare
  the prior reload manifest with the current `.clip` graph/source chunks, return
  no-change or dirty-rect patch payloads for same-graph tile edits, and fall
  back to full image updates for structural or broad changes. The packaged
  worker process should stay alive across Blender requests so reloads reuse the
  native process and wgpu renderer instead of paying startup cost each time.
- Keep native render state visible in Blender: image metadata and the Image
  Editor panel should report ready, refreshing, stale, missing-source,
  render-error states, and elapsed/last render timing.
- Surface only actionable native support failures in the normal Blender UI.
  Compact unsupported layer/node/kind locators may be copied for issue reports,
  while renderer version, resource statistics, full-support summaries, and other
  debug metadata stay in copied/opened diagnostics instead of the panel. The
  Blender add-on must not grow layer-navigation or CSP layer-management UI.
- Rebuild `clip_studio_importer.zip` whenever package code changes.

## Direction 3: Native Image Loading Rewrite

Goal: replace the Python compositor/sidecar PNG workflow with a native GPU renderer and Blender image-datablock integration.

Current policy:

- Rust plus `wgpu` is the chosen renderer direction, with a thin C++ OpenImageIO plugin boundary and a stock Blender image-datablock bridge.
- External OpenImageIO plugin loading alone is not enough for stock Blender `bpy.data.images.load(".clip")`; true file-backed support requires a Blender ImBuf/source bridge or upstream source patch. The C ABI has a byte-buffer session entry point (`clip_renderer_session_open_memory`), and the C++ OIIO adapter now supports `IOProxy` memory opens by reading `oiio:ioproxy` bytes into Rust memory sessions without temp files. This is host-integration plumbing for OIIO/ImBuf callers, not a replacement for the accepted generated-image Blender add-on path.
- Current native milestone: the stock Blender image-datablock bridge is the
  add-on runtime path. The add-on calls the packaged `clip_cli` worker so native
  GPU rendering runs outside Blender's UI process, creates generated Blender
  images, records source metadata (mtime, size, SHA-256), and updates those
  images through initial import, manual reload, the background watcher, and
  Blender `load_post` freshness scans without writing sidecar PNGs. The watcher
  uses lightweight mtime/size checks, while `load_post` can also compare
  SHA-256. Successful renders do not pack immediately; they mark the image as
  needing pack, with `Pack` and a `save_pre` handler providing persistence.
  Render failures are stored as image metadata and shown in the Image Editor
  panel, while successful renders clear old error metadata. The add-on records
  elapsed/last render timing for manual and background renders. Reloads store a
  native manifest on the generated image and pass it back to the packaged
  worker; the default Python bridge uses `clip_cli --blender-render-server` so
  repeated reloads keep one native process and one reusable wgpu renderer alive.
  Same-graph raster/mask compressed-tile edits can return dirty-rect patch
  payloads or no-change metadata instead of forcing a full Blender `foreach_set`,
  while canvas/root/node-order/container/filter/paper semantic changes still use
  conservative full image updates. Persistent worker reloads reuse raster/mask
  GPU textures across requests and render patch payloads through a dirty-region
  GPU stack path instead of rendering the full canvas and slicing it afterward.
  The remaining next-level performance target is a fuller tile-local
  mask/clipping DAG cache, not a global full-canvas all-layer texture cache. The C ABI exposes
  metadata-only native support summaries, and the add-on stores native support
  metadata but displays only unsupported counts, expandable unsupported
  layer/node detail lines, compact unsupported layer/node/kind issue locators,
  and copyable/searchable diagnostics when failures exist. `tools/build_blender_addon.py`
  builds the installable zip with `__init__.py`, `native_bridge.py`, and the
  locally built release `clip_cli` worker plus `clip_capi` library under
  `clip_studio_importer/native/`;
  it no longer packages the Python compositor/loader. Preferences expose reload
  timing, debug logging, and Developer Mode; they only report packaged native
  worker status when the worker is missing. The project-root `clip_loader.py` remains slow reference tooling for
  verification while native fidelity gaps close.
- Strict GPU coverage status: ordinary raster blend modes `LayerComposite=1..26` plus `36` are enabled, isolated containers can resolve with supported non-NORMAL blend modes, clipping runs support non-NORMAL raster bases, container/folder clipping bases, and clipped sibling stacks whose members may be rasters or recursively rendered containers/folders. THROUGH groups clear the clip base for following clipped layers, and adjustment/filter layers now route through a dedicated GPU pass: Brightness/Contrast (`FilterLayerInfo` type `1`), Level Correction (`2`), Tone Curve (`3`), HSL (`4`, native HSV-adjust shader mode), Color Balance (`5`), Invert/Reverse Gradient (`6`), Posterization (`7`), Threshold (`8`), and Gradient Map (`9`). Unknown future filter types remain explicit unsupported filter work until faithful native models exist. A metadata-only scan of the current public/local `img/*.clip` fixtures reports `unsupported=0`, including `Ref_绫音Aya_Live2D.clip --gpu-support-json`, `Ref_Kabi_Live2D.clip --gpu-support-json`, and `Test_RealArt.clip --gpu-support-check`. These samples are fully routed but still have residual formula/quantization or performance work; distinguish native/Python parity from shared Python/CSP residuals before changing shaders, and improve correctness only with source-backed native evidence and guard samples.
- HSL filter payload mapping is now sample-backed separately from the native
  per-pixel routine: the SQLite payload uses UI-degree hue plus UI-percent
  saturation/luminosity for the current `Test_HSL*` fixtures, while the GPU
  shader implements CSP's luminosity/saturation coupling from the native
  routine.
- Large-stack performance has passed the sparse-tile milestone, atlas-backed pass-collapse, direct compressed raster tile chunks, masked Normal atlas-run support, shader-side R8 mask atlas events for masked standard raster runs, byte-domain special blend events, pointwise filter events, simple container/THROUGH scope events, conservative clipped-raster-sibling tile-silo inside already-created clipping caches, raster-only clipping-run tile events where base/clipped relationships are encoded tile-locally, and clipped container/folder sibling tile events whose children fit the current simple child stream, including raster children followed by pointwise filters and nested simple container children. The host-facing normal render path uses a recursive streaming GPU source provider: the main selector builds a metadata-only GPU source tree/resource plan, then raster/mask payloads are decoded and uploaded at point of use inside containers, clipping runs, THROUGH groups, and filters. CHNKExta tile-blob parsing is split into `clip_file::external`; compressed tile blocks inflate directly into the expected output buffer, selected-region reads stream matching external blocks through reusable scratch storage, and region raster/mask reads skip empty tile blocks even when the requested source rectangle covers the full metadata extent. Runtime providers precompute compressed-present tile bounds, expose the cropped offset/size to streaming bounds, and mark all-empty raster sources as known empty before decode/upload. Provider chunks outside cropped cache/target bounds are filtered before event emission. The older per-source texture-copy path remains for providers that do not expose tile chunks. Raster source shaders sample outside cropped uploads as transparent black while generated/cache textures keep transparent white, preserving `.clip` empty-raster semantics without expanding cache bounds back to canvas-sized metadata. Remaining barriers are complex containers, complex THROUGH groups, provider-unavailable or unknown masked scopes/filters, clipping runs nested through another scope inside THROUGH, child clipping runs and THROUGH children inside clipped container/folder siblings, clipped container/folder sibling subtrees beyond the current simple child stream, scope-depth limits, and event-count limits. The next native performance step is broader tile-local semantic event support plus general dirty-segment reload checkpoints; do not introduce CPU compositor fallback, post-processing, or a global all-layer full-canvas texture cache.
- The native renderer now has the first render-program seam in `clip_gpu::stream_program`: strict source selection still emits `GpuNormalStackSource`, then the render-program planner groups that sequence into ordered tile-local or barrier segments before encoding. The current segment kinds cover atlas raster runs, raster-only clipping runs, and faithful legacy source barriers; the executor invokes existing tile-silo encoders for tile-local segments and uses legacy source encoding only for barrier segments or provider fallback. Future performance work should deepen this planner/executor split rather than adding more ad hoc source traversal branches.
- The durable performance roadmap is `docs/native-tile-event-renderer-roadmap.md`, the compact target model is `docs/native-tile-event-main-execution-plan.md`, and the implementation strategy is `docs/native-tile-event-architecture-strategy.md`: tile-local event execution is the main renderer model, while barrier passes are explicit, counted, and explained as not-yet-lowered faithful semantics. Future work should add render segment kinds and typed event executor support rather than more opportunistic traversal branches.
- Selected-tile external decoding should stop once the last requested tile block has been found, so unrelated trailing CHNKExta blocks are not scanned or inflated.
- Streaming intermediate caches should stay cropped whenever metadata or compressed-tile bounds prove finite canvas bounds: clipping runs use the base sparse/visible bounds, isolated containers use the union of child bounds, and THROUGH groups use a bounded after-cache seeded from the parent-before texture. Source/cache resolve uniforms carry both source and target origins, and wgpu scissors are translated from global dirty bounds into local attachment coordinates. Unknown stacks, solid layers, and leading LUT filters keep full-canvas intermediate allocation; masked or unmasked LUT filters after bounded draws keep the prior dirty bounds by sampling masks at target-origin-adjusted canvas coordinates. Runtime masks upload as cropped R8 textures with canvas origin and fill metadata, and shaders sample outside cropped bounds as fill. Bounded nested THROUGH groups inside containers carry the parent target origin and keep cropped cache bounds. The remaining large-stack performance blocker is GPU pass/intermediate throughput plus leading filter and unknown intermediate full-canvas cases.
- Streaming scheduling should skip sources that provably cannot affect output before decode/upload or cache allocation: zero-opacity rasters, containers, THROUGH groups, clipping bases/siblings, LUT filters, and transparent solid colors. Clipping runs with no provably effective clipped siblings may render their base directly instead of allocating a clipping cache. Keep this limited to faithful no-op cases; do not generalize it into heuristic pruning or post-processing.
- Metadata-only mask planning may elide constant off-canvas masks before provider decode/upload: fill `0` folds the source to zero opacity, fill `255` drops the mask resource as fully opaque, and partial fill values stay on the real mask-resource path. Keep this as a faithful resource-planning optimization only.
- High-leverage native performance work has moved from broad investigation to a
  validated sparse-tile implementation plus atlas raster-run collapse, direct
  compressed raster tile chunks, shader-side mask atlas events, a clipped
  raster sibling tile-silo subset inside existing clipping caches, and a
  raster-only clipping-run tile-silo subset.
  `clip_cli --tile-silo-estimate` showed RealArt/Terra/Aya-class samples have
  canvas-sized raster metadata but very low compressed CHNKExta tile occupancy;
  the first implementation now uses those compressed-present bounds for sparse
  decode/upload and empty-tile skipping. The diagnostic also projects exact
  compressed source tiles into canvas tile events, and the streaming provider
  now uses the same tile-local execution shape for eligible raster-run collapse,
  with runtime raster runs emitting compressed tile chunks directly as atlas
  events instead of first uploading per-source textures. Masked Normal and
  masked standard rasters can join those provider-backed atlas runs through
  optional R8 mask atlas chunks and shader-side mask event coordinates;
  consecutive eligible raster clipped siblings can also collapse after the
  clipping base cache exists. The base/cache relationship, clipped containers,
  filters, THROUGH groups, and isolated containers remain barriers.
  A follow-up source review of
  Avarel/silicate confirms that the remaining order-of-magnitude milestone is
  richer tile-local semantic events beyond the current raster-only
  clipping-run subset while keeping filters, THROUGH groups, and isolated
  containers as explicit semantic barriers until faithful tile-local models
  exist. Short-lived
  Blender worker setup now avoids
  duplicate support selection/full-canvas region copying and lazily creates only
  the normal-stack pipelines used by the current file; this is a useful fixed
  cost reduction, not a replacement for tile-silo rendering. Follow-up research
  against wgpu, Bevy, and Blender public API docs adds two concrete next tracks:
  extend tile-local semantic events on the native side, and use Blender
  phase timing before changing upload or pack policy. A first explicit
  staging-buffer atlas tile upload attempt was measured and rejected because it
  slowed RealArt on this machine; keep `Queue::write_texture` until profiling
  proves a no-extra-copy upload design is worthwhile. The likely Blender
  product lever of deferred packing is now implemented in the Blender add-on:
  reload avoids immediate `image.pack()`, while manual `Pack` and save-time
  packing preserve `.blend` persistence. See `docs/native-performance-investigation.md`.
- Native raster extraction now applies render offscreen placement through `LayerRenderOffscrOffsetX/Y`, matching the existing mask placement model and the known `Ref_Terra404_Live2D` negative-X render sources. This removes a structural decode gap before further large-reference GPU work.
- Native raster extraction now decodes full-color, grayscale, and monochrome raster tile streams. `Test_ Grayscale.clip` and `Test_Monochrome.clip` route through the strict GPU path and compare exactly against CSP PNGs.
- Native support diagnostics use a metadata-only strict selector. `clip_cli --gpu-support-check` validates graph, raster source, mask source, and LUT-filter support without tile decode, GPU initialization, or rendering, and labels resource/unsupported layer ids with layer names when available. `clip_cli --gpu-support-json` emits the same support/resource/unsupported-node data as pure JSON for automation and issue capture, also carrying layer names when available. Other CLI plan, resource, stack, unsupported, and trace diagnostics should use the same layer-label helper so layer ids are easy to map back to Blender support reports. These commands must remain diagnostics only, not fallback renderers.
- `clip_cli --gpu-trace-pixel <x> <y>` is available for native GPU prefix tracing and now includes per-source before/after/input pixels. `IllustrationBlendModes.clip` keeps a one-LSB Subtract blend target for partially transparent equal source/destination channels before Color Dodge/Color Burn, reducing that sample to `raw_max=7`. `IllustrationBlendModesB.clip` now floors partial Hue writeback, ceils Hue's minimum channel after set_lum when the quantized base saturation span is greater than `2/255`, uses fully opaque Hue `0.3/0.59/0.11` luminosity while preserving the old partial-Hue path, and uses Color's rounded byte-domain `0.3/0.59/0.11` luminosity only for the low-side ClipColor branch, improving the full sample to `raw_max=5` / visible `2718`. Rejected broad fixes such as global `256-srcA`, HSL tiny-span `3/255`, Rec.601 HSL luma for Hue/Saturation/Color, full Color Rec.601, broad Saturation `0.3/0.59/0.11`, full Color/Hue Rec.601, partial Hue target-floor or unquantized-target quantization, disabling Hue min-channel ceil, the earlier high-key near-neutral Hue ceil variant, PinLight byte-domain/256-alpha writeback, and standard-pass partial-alpha decrement remain evidence only.
- Do not reintroduce the Python compositor/loader or sidecar PNG implementation into the installable add-on.
- This direction is about flattened raster loading only; it does not restore vector, bubble/frame, or text renderer compatibility.

## Direction 4: Architecture Deepening

Goal: deepen shallow modules without changing renderer semantics, user-facing
scope, or adding compatibility shims.

Current policy:

- Architecture work should improve Locality and Leverage at existing seams.
  Avoid moving code only to reduce line counts; the Interface must become
  smaller or more explicit.
- Keep root files as wiring. Crate/package roots and command entry points should
  delegate to named modules with clear Interfaces.
- Refactors must preserve current raster fidelity and Blender workflow behavior.
  Run targeted guards for the touched seam rather than broad sample sweeps by
  default.
- Use `CONTEXT.md` vocabulary for named domain Modules and keep new durable
  terms there.

Deepening milestones:

1. Blender image state and worker protocol.
   - Deepen the generated-image state Module so pack status, reload status,
     source freshness, timing, support diagnostics, and IDProperty keys are not
     spread across `clip_studio_importer/__init__.py` and
     `clip_studio_importer/native_bridge.py`.
     Current progress: `clip_studio_importer/image_state.py` now owns the
     generated-image IDProperty keys, reload/source freshness checks, pack
     status writes, support locator parsing, render-result property writes, and
     timing property writes. `native_bridge.py` still exposes imported state
     names for existing internal tests, but its implementation no longer owns
     those writes.
   - Deepen the native worker protocol Module so one-shot files, persistent
     server messages, reload manifests, dirty-rect patches, and timing fields
     have one explicit Interface shared by the Python Adapter and Rust worker
     implementation.
     Current progress: `clip_studio_importer/worker_protocol.py` now owns
     one-shot command construction, persistent worker request JSON, temporary
     render-file layout, worker output reading, reload manifest compaction,
     dirty-rect patch parsing, support JSON parsing, and typed native render
     result records. `native_bridge.py` is now the Adapter that times worker
     calls, translates protocol errors to bridge errors, and uploads pixels into
     Blender.
   - Expected benefit: Blender UI/reload changes become localized, and protocol
     tests can verify JSON examples without exercising full Blender operators.

2. CLI command runner.
   - Split `clip_cli/src/main.rs` into command parsing, command execution, and
     output formatting modules.
     Current progress: `clip_cli/src/options.rs` owns command-option parsing and
     returns `Result<CliOptions, String>` instead of exiting the process inside
     the parser. `main.rs` now prints parser errors and exits with code 2.
   - Existing diagnostics such as support text/json, pixel trace text,
     layer-window dumps, PNG compare, tile-silo estimates, and Blender worker
     commands should become command implementations behind a small runner
     Interface.
     Current progress: `clip_cli/src/compare_png.rs` owns PNG reference loading,
     full-image render readback for compare, raw/premultiplied diff statistics,
     and compare report formatting.
     Current progress: `clip_cli/src/runner.rs` owns command execution for
     file-based CLI commands and returns process exit codes to the entry point.
     `clip_cli/src/reload_manifest.rs` owns shared reload-manifest reads for the
     runner and persistent Blender server. `main.rs` is now 37 lines of entry
     wiring for usage, server mode, option parsing, and runner dispatch.
     Remaining work: move any future non-PNG diagnostic/report formatting into
     named formatter modules instead of adding it to `runner.rs`.
   - Expected benefit: adding diagnostics no longer expands the entry point, and
     command behavior can be tested without going through process-level `main`.

3. `clip_file` metadata readers.
   - Deepen `clip_file/src/metadata.rs` into schema/sqlite access, layer graph
     reading, raster/render source reading, mask source reading, filter source
     reading, and paper colour reading.
     Current progress: `metadata.rs` is now wiring/re-exports only. Typed
     records live in `metadata/records.rs`; shared SQLite/schema helpers live
     in `metadata/schema.rs`; and focused reader implementations live in
     `metadata/summary.rs`, `metadata/layer_graph.rs`,
     `metadata/raster_source.rs`, `metadata/mask_source.rs`,
     `metadata/filter_source.rs`, and `metadata/paper_color.rs`.
   - Keep storage-specific SQLite details inside `clip_file`; runtime callers
     should continue to ask for typed `.clip` domain records.
   - Expected benefit: schema variation and source-resolution bugs gain
     Locality, while new diagnostics avoid touching unrelated metadata readers.

4. Streaming execution context.
   - Deepen `clip_gpu` streaming execution so full render, region render,
     split-region stitching, encoder lifecycle, ping-pong texture selection,
     dirty bounds, flush policy, and provider resource retention sit behind one
     execution-context Interface.
     Current progress: `clip_gpu/src/stream_context.rs` owns the internal
     `StreamingExecutionContext` Interface for renderer/provider/encoder,
     output size, pipeline set, texture-pair creation, initial clears, and
     final flush/drawn-resource extraction. Sequence/source/group/clipping,
     THROUGH, and tile-silo streaming modules now take that context instead of
     threading renderer, provider, encoder, output size, and pipelines through
     each call. `stream_state.rs` remains the lower-level encoder/resource
     retention implementation behind the context. `clip_gpu/src/stream_program.rs`
     now owns the render-program planner Interface that classifies a strict
     source sequence into tile-local segments or legacy barriers before
     `stream_sequence.rs` executes them.
   - Keep tile-silo, mask/clipping events, filters, THROUGH groups, and
     byte-domain special blends as explicit semantic barriers until faithful
     models exist.
   - Expected benefit: future mask/clipping tile-event performance work changes
     scheduling in one place instead of threading invariants through multiple
     streaming modules.

## Direction 5: Documentation Hygiene

Goal: make new conversations start from the right state quickly.

Current policy:

- Keep `docs/AI_MEMORY.md` as the short current-state memory.
- Keep `docs/analysis.md` as the append-only historical evidence log.
- Keep this file as durable direction, not a running checklist.
- Keep `CONTEXT.md` aligned with durable domain vocabulary used by
  architecture work.
