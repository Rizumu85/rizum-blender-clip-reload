# Native Performance Investigation

Last updated: 2026-06-16

## Purpose

This file records high-leverage native renderer performance directions. It is
not a micro-optimization log. Keep correctness constraints from
`docs/AI_MEMORY.md` and `docs/native-code-architecture.md`: no CPU compositor
fallback, no post-processing, and no heuristic pruning that changes CSP
semantics.

## Silicate Reference

Reference project: <https://github.com/Avarel/silicate>

Silicate is useful because it is a Rust/WebGPU compositor that renders tiled
artwork quickly, but it should not be copied literally. Procreate's format and
CSP `.clip` semantics differ, and Blender integration still requires final
RGBA readback/upload.

Observed source-level architecture:

- `libs/silica-gpu/src/types/file.rs` opens the Procreate archive, parses
  `Document.archive`, computes a chunk atlas size from `chunk_count`,
  `tile_size`, and `wgpu::Limits`, then creates one `Rgba8Unorm` atlas texture
  array.
- `libs/silica-gpu/src/types/layer.rs` loads layer chunks in parallel on native
  builds, decompresses each chunk, and writes it directly into the atlas with
  `Queue::write_texture`. Mask chunks are expanded into RGBA atlas entries.
- `src/app/compositor.rs` flattens the layer tree into two dense arrays:
  `CompositeLayer` records and `ChunkTile` records. Chunk records are sorted by
  `(col, row)`.
- `libs/compositor/src/buffer.rs` builds a per-canvas-tile `silos` table:
  each tile stores the start/end range of chunk records that affect that tile.
- `libs/compositor/src/lib.rs` renders by uploading layer/chunk/silo storage
  buffers, binding the atlas texture, then issuing one indexed instanced draw
  over canvas tiles.
- `libs/compositor/src/shader.wgsl` uses the current tile's silo to loop only
  the chunk/layer records that affect that tile, samples atlas/mask/clipping
  entries, and composites them in order in the fragment shader.

The important pattern is not "wgpu" by itself. The pattern is: upload tiled
sources into a shared atlas, build compact per-tile work lists, then collapse
many layer passes into one or a few tile-local shader passes.

Follow-up source review on 2026-06-16 reinforced the same conclusion:

- Silicate keeps a long-lived app/compositor/pipeline state and renders updates
  through a compositor thread or tick. The exact thread model is not directly
  portable to the Blender worker path because the current add-on intentionally
  launches an out-of-process worker per render, but the resource lifetime lesson
  still applies: avoid rebuilding GPU setup that is not used by the current
  file.
- Silicate's large-file speed comes from chunk-local work, not from a magic
  blend formula. It avoids full-canvas layer passes by drawing canvas tiles as
  instances and letting each tile's `silo` range drive the fragment loop.
- The `.clip` equivalent should therefore prioritize atlas/tile-silo execution
  before more local pass tuning. Current streaming optimizations make the
  existing pass-heavy path tolerable, but RealArt/Terra-class files still encode
  hundreds of raster passes.

## Implications for `.clip`

The current renderer is faithful but pass-heavy: many sources, containers,
clipping runs, THROUGH groups, and filters become many ping-pong passes and
intermediate textures. Existing dirty-bounds and cropped-cache work reduces the
damage, but RealArt-class files are still dominated by visible tile
inflate/upload plus GPU pass/intermediate overhead.

The next likely order-of-magnitude investigation is a `.clip` atlas/tile-silo
renderer:

1. Build a render-session atlas for visible raster and mask tiles. The existing
   selected-tile decoder should fill atlas slots directly or through a reusable
   staging buffer, avoiding per-source texture creation and repeated texture-view
   churn.
2. Build canvas-tile work lists from the strict render plan. Each tile should
   contain ordered source events that intersect that tile: raster chunk atlas
   slot, optional mask atlas slot/fill, clipping-base relation, opacity, blend
   mode, and layer/container boundaries needed by the shader.
3. Collapse raster-only and clipping-only stretches into one tile-silo pass.
   Treat semantic barriers explicitly: filters, THROUGH groups, and isolated
   containers may require segment boundaries or intermediate caches until a
   faithful tile-local model exists.
4. Measure before rewriting broadly: source count, planned pass count, decoded
   tile count, upload bytes, atlas occupancy, CPU decode time, GPU encode time,
   GPU execution time, readback time, and Blender upload time.

This is the high-leverage path to test before more local tuning. If it works,
the gain comes from eliminating hundreds of small passes/intermediate textures
and from reusing atlas resources across tile-local work. If CSP semantics force
too many barriers, the same instrumentation will show where smaller optimizers
are still justified.

## Validation Diagnostic

`clip_cli --tile-silo-estimate [--tile-size <px>]` is the current
metadata/block-level validator for this direction. It does not initialize wgpu
or decode pixel payloads. It reuses the strict native render selection, walks
the recursive GPU source tree, and inspects CHNKExta tile block headers to
separate compressed tiles from empty tiles.

The first validation run showed that large `.clip` samples often have
canvas-sized raster metadata even when most source tiles are empty. This means
metadata-only visible bounds are not enough to judge atlas value; the actual
CHNKExta compressed/empty block split is the stronger signal.

Sample estimates with `tile_size=256`:

- `Test_RealArt.clip`: raster metadata slots `131712`, raster compressed tiles
  `2220`, compressed raster tile events `2220` across `237` active canvas
  tiles (`max=60`, `mean=9.37`), raster empty tiles `129492`, mask compressed
  tiles `223`, semantic barriers `67`, collapsible raster/clipping segments
  `75`.
- `Ref_Terra404_Live2D.clip`: raster metadata slots `326040`, raster compressed
  tiles `4826`, compressed raster tile events `4826` across `361` active
  canvas tiles (`max=123`, `mean=13.37`), raster empty tiles `321214`, mask
  compressed tiles `2052`, semantic barriers `220`, collapsible
  raster/clipping segments `178`.
- `Ref_绫音Aya_Live2D.clip`: raster metadata slots `55575`, raster compressed
  tiles `924`, raster empty tiles `54651`, mask compressed tiles `2`, semantic
  barriers `47`, collapsible raster/clipping segments `42`.

Conclusion: the first high-leverage implementation should use compressed tile
occupancy as the runtime source bounds before attempting a broader tile-silo
renderer. The pass-collapse side still matters, but the block-level evidence
says empty-tile skipping is the first order-of-magnitude piece to validate in
code.

## First Implementation Milestone

The first sparse-tile milestone is implemented in the native render path:

- `clip_file` region readers stream only non-empty CHNKExta tile blocks into
  prefilled raster/mask region writers. Empty blocks are treated as transparent
  or mask-fill records and are no longer inflated or copied.
- `clip_runtime` precomputes each planned raster/mask source's compressed tile
  bounds, decodes/uploads only that cropped region, and exposes the cropped
  offset/size to streaming bounds. All-empty raster sources become known empty
  before decode/upload.
- Raster source shaders distinguish real raster textures from generated/cache
  textures. Samples outside cropped raster uploads return transparent black,
  preserving `.clip` raster empty-tile semantics, while generated/cache
  textures keep transparent white.
- Streaming cache allocation stays sparse-resource-bounded. A temporary
  full-metadata bounds experiment preserved `Test_AddGlowMultiply` but made
  `Ref_Terra404_Live2D` hit wgpu OOM; the accepted shape is cropped upload plus
  cropped cache/pass bounds with the raster-source transparent-black sampling
  rule.

Verification after the milestone:

- `Test_Clipping`, `Test_ClippingEdge`, and `Test_FolderNested`: exact.
- `Test_Mask`: `raw_max=1`, `premul_max=1`.
- `Test_AddGlowMultiply`: `raw_max=5`, `premul_max=3`.
- `Test_ToneCurve`: `raw_max=17`, `premul_max=17`.
- `Test_Gradiation`: `raw_max=10`, `premul_max=10`.
- `Test_RealArt.clip --compare-png Test_RealArt.png` release: `raw_max=5`,
  `premul_max=2`, 3.466s on this machine, down from the prior roughly 48s.
- `Ref_Terra404_Live2D.clip --compare-png Ref_Terra404_Live2D.png` release:
  no wgpu OOM, `premul_max=5`, 4.756s on this machine.

Next quantity-level work is to extend atlas-backed execution beyond ordinary
raster runs: direct compressed-tile atlas uploads, mask/clipping events, and
larger faithful tile-local segments. Filters, THROUGH groups, and isolated
containers remain semantic barriers until faithful tile-local models exist.

## Atlas Raster Run Collapse Milestone

The second Silicate-shaped milestone is implemented in the streaming provider
path. It is deliberately narrower than a complete long-lived atlas renderer:

- `clip_gpu::stream_sequence` scans normal source sequences and attempts to
  collapse consecutive eligible raster sources before falling back to the
  existing per-source encoder.
- `clip_gpu::stream_tile_silo_plan` builds a per-run atlas layout plus
  attachment-local 256px tile work lists. It supports non-zero target origins,
  so cropped container caches can participate instead of only the top-level
  canvas.
- `clip_gpu::stream_tile_silo` copies the already-cropped raster textures into a
  per-run atlas and issues one shader pass. The shader loads only the current
  tile's ordered event list and applies Normal or standard blend modes in
  sequence, quantizing after each event to preserve the existing multi-pass
  byte behaviour.
- `clip_gpu::stream_tile_silo_pipeline` keeps the tile-silo bind group layout
  and render pipeline cached per streaming render through `StreamingEncoder`,
  avoiding a WGSL/pipeline rebuild for every collapsed run.

Current eligibility is conservative and faithful: only unmasked raster sources
with nonzero opacity and supported Normal/standard blend modes are collapsed.
Masks, clipping-run nodes, filters, THROUGH groups, containers as source nodes,
and byte-domain special blends (`AddGlow`, `ColorDodge`, `ColorBurn`,
`GlowDodge`) remain explicit barriers and use the existing pass path.

Verification after this milestone:

- Rust: `cargo fmt --all --check`, `cargo check -q`, and `cargo test -q`.
- New GPU unit tests lock Normal raster-run collapse, standard Multiply order,
  non-zero target-origin container collapse, and planner barriers for masks and
  byte-domain blends.
- Guard samples remain stable: `Test_Clipping` exact, `Test_ClippingEdge`
  exact, `Test_FolderNested` exact, `Test_Mask` `raw_max=1` /
  `premul_max=1`, `Test_ToneCurve` `raw_max=17` / `premul_max=17`, and
  `Test_AddGlowMultiply` `raw_max=5` / `premul_max=3`.
- Direct release executable timings on this machine:
  `Test_RealArt.clip --compare-png Test_RealArt.png` is `raw_max=5` /
  `premul_max=2` in about 2.559s; `Ref_Terra404_Live2D.clip --compare-png
  Ref_Terra404_Live2D.png` is `premul_max=5` in about 3.943s.

The remaining order-of-magnitude path is still a fuller atlas architecture:
represent masks and clipping relationships as tile events and grow the faithful
tile-local segments beyond ordinary raster runs. This milestone proves the
pass-collapse execution shape inside the accepted streaming renderer without
adding fallback compositing or post-processing.

## Direct Compressed Tile Chunk Events

The third Silicate-shaped milestone removes the per-source texture copy from
eligible raster-run collapse on the runtime path:

- `clip_file::atlas_chunks` decodes selected non-empty CHNKExta tile blocks
  directly into RGBA atlas tile chunks. It does not build a full CPU atlas and
  does not emit chunks for empty tile blocks.
- `clip_gpu::GpuNormalStackResourceProvider` now has a tile-atlas hook:
  providers may return ordered atlas chunks plus per-source resource info for a
  planned raster run. A full-atlas pixels hook remains for tests or alternate
  providers, but the runtime provider uses tile chunks.
- `clip_runtime::RuntimeGpuResourceProvider` fills the hook from the existing
  sparse decode region and compressed-present tile selection. Each compressed
  source tile chunk becomes its own tile-silo event with the same opacity/blend
  semantics as the source raster.
- `clip_gpu::stream_tile_silo` tries provider-backed tile chunks before falling
  back to full-atlas pixels or the previous per-source texture-copy path. The
  shader already uses `textureLoad`, so chunk events can safely skip atlas holes
  instead of clearing or uploading transparent empty regions.
- `clip_gpu::stream_tile_silo_upload` owns atlas texture creation and both
  upload forms, keeping `stream_tile_silo.rs` focused on planning/encoding and
  below the production file-size guideline.

This is still scoped to the same conservative raster-run eligibility: unmasked
ordinary raster sources with supported Normal/standard blend modes. Masks,
clipping-run nodes, filters, THROUGH groups, containers as source nodes, and
byte-domain special blends remain semantic barriers.

Verification after this milestone:

- Rust: `cargo fmt --all --check`, `cargo check -q`, and `cargo test -q`.
- Guard release comparisons remain stable: `Test_Clipping`,
  `Test_ClippingEdge`, and `Test_FolderNested` exact; `Test_Mask`
  `raw_max=1` / `premul_max=1`; `Test_ToneCurve` `raw_max=17` /
  `premul_max=17`; `Test_AddGlowMultiply` `raw_max=5` / `premul_max=3`.
- Direct release executable comparisons on this machine:
  `Test_RealArt.clip` stays `raw_max=5` / `premul_max=2` in about 2.560s;
  `Ref_Terra404_Live2D.clip` stays `premul_max=5` in about 3.834s.
- Worker-mode timings on this machine:
  `Test_RealArt.clip --blender-render-rgba --blender-render-json` is about
  2.180s; `Ref_Terra404_Live2D.clip --blender-render-rgba
  --blender-render-json` is about 3.322s.

The next large lever is no longer ordinary raster-run source upload. It is
making more source types tile-local: mask atlas events, clipping-base/clipped
relationships, and eventually larger faithful raster/clipping segments. Filters,
THROUGH groups, isolated containers, and byte-domain special blends should stay
barriers until their tile-local semantics are explicitly modelled.

## Compressed Occupancy Planner

The tile-silo diagnostic now has the first Silicate-shaped planner input:
`clip_file::external` can enumerate compressed CHNKExta tile coordinates
without inflating pixel payloads, and `clip_runtime` projects those source tile
coordinates through raster offsets into canvas tile event counts. The old
metadata rectangle count is kept for comparison; the new compressed event count
is the exact sparse work-list signal for an atlas renderer.

This does not change the main renderer yet. Its purpose is to make the next
milestone measurable: atlas-backed raster/mask tile storage plus per-canvas-tile
ordered source events for raster/clipping stretches. RealArt and Terra both show
two orders of magnitude fewer compressed raster tile events than metadata raster
tile events, so this remains a quantity-level optimization direction rather
than local pass tuning.

## Worker Setup Optimization

The Blender worker path now avoids a duplicate support-check selection and a
second full-canvas host-region copy. `clip_cli` worker mode calls
`ClipSession::draw_normal_raster_stack_via_gpu()` directly, writes the returned
RGBA buffer, and builds support JSON from the render result's resource stats.
The normal-stack GPU pipeline set is also lazy: the streaming renderer creates
only the blend/filter pipelines used by the current file instead of eagerly
constructing every normal-stack pipeline for each short-lived worker process.

Release worker timings on this machine:

- `Test_RealArt.clip --blender-render-rgba --blender-render-json` improved from
  about 2.511s after sparse uploads to about 2.180s with direct raster tile
  chunks active.
- `Ref_Terra404_Live2D.clip --blender-render-rgba --blender-render-json`
  improved from about 3.610s to about 3.322s with direct raster tile chunks
  active.

This is a real fixed-cost reduction, but it is not the next order-of-magnitude
lever. The same runs still draw hundreds of raster resources (`343` for
RealArt, `715` for Terra). The next large step remains a Silicate-style
tile-silo renderer that makes masks and clipping relationships tile-local.

## External wgpu and Blender API Follow-Up

Reference material checked on 2026-06-16:

- wgpu `Queue` docs:
  <https://docs.rs/wgpu/latest/wgpu/struct.Queue.html>
- wgpu `PipelineCache` docs:
  <https://docs.rs/wgpu/latest/wgpu/struct.PipelineCache.html>
- Learn Wgpu texture upload and instancing tutorials:
  <https://sotrh.github.io/learn-wgpu/beginner/tutorial5-textures/>
  and <https://sotrh.github.io/learn-wgpu/beginner/tutorial7-instancing/>
- Bevy texture atlas and render rework references:
  <https://docs.rs/bevy/latest/bevy/prelude/struct.TextureAtlas.html>
  and <https://github.com/bevyengine/bevy/discussions/2265>
- Blender Python API/Image and Blender foreach_set source-example references:
  <https://docs.blender.org/api/current/bpy.types.Image.html>
  and
  <https://github.com/blender/blender/blob/main/doc/python_api/examples/bpy.types.bpy_prop_collection.foreach_set.py>

wgpu conclusions:

- `Queue::write_texture` is good for simple texture uploads and avoids the old
  explicit intermediate-buffer shape in the single-texture tutorial case, but
  the current wgpu docs say it has the same native-platform performance
  consideration as `write_buffer`: staging memory is a short-lived allocation
  released after the next submit. Our direct compressed raster tile path can
  call `write_texture` once per non-empty source tile chunk, so RealArt/Terra
  scale into thousands of short-lived upload calls. This is evidence for
  measuring upload strategy, not evidence to replace `write_texture` blindly:
  a first explicit mapped-staging-buffer attempt batched atlas chunks into
  `copy_buffer_to_texture` commands but regressed RealArt worker timing on this
  machine, likely because it added a second CPU copy before wgpu's backend copy.
  Do not restore that shape without a profile showing upload calls dominate and
  a design that avoids duplicated CPU copying.
- The separate instancing/atlas references point in the same direction as
  Silicate: group per-source data into buffers/atlases and draw many logical
  items with one pass/draw instead of repeatedly updating uniforms or issuing
  small passes. The current raster-run tile-silo pass already follows this for
  ordinary unmasked raster runs. The next large native milestone should extend
  the event model to masks and clipping relationships, not spend time on more
  local pass tuning.
- wgpu pipeline caches can reduce pipeline creation cost between program
  executions, but the current docs and feature support make this a secondary
  worker-startup probe, not the main Windows/Blender speed lever. The renderer
  already lazily creates only pipelines used by the current file; if startup
  timing later proves pipeline compilation dominates, persistable
  `PipelineCache` can be benchmarked per backend.
- Native-only binding arrays/non-uniform indexing exist in wgpu/Bevy feature
  documentation, but they are not the first portability-preserving route here.
  A single atlas plus storage-buffer event lists fits WebGPU's common subset
  better and matches the existing tile-silo direction.

Blender conclusions:

- The accepted add-on path currently has a hard Blender-side floor after native
  rendering: read the worker RGBA8 bytes, convert/flip them into a float
  sequence for `Image.pixels.foreach_set`, call `image.update()`, then pack the
  generated image. Blender's public Python Image API exposes pixels through
  this float property path; the source example for `foreach_set` also requires
  a one-dimensional sequence of basic values. There is no documented public
  Python API that bulk-installs raw RGBA8 bytes directly into an Image datablock
  or hands an external wgpu texture to a persistent Blender Image.
- The first Blender-side task should be phase timing, not speculative tuning:
  record native worker time, RGBA temp read time, uint8-to-float/row-flip time,
  `foreach_set`, `image.update()`, and `image.pack()`. User-observed 6-7s
  imports with 2-3s native worker times are consistent with this post-render
  bridge being a large share of the remaining delay, but the exact phase split
  should be measured in the add-on UI before changing persistence semantics.
- The main product-level speed lever is packing policy. The add-on now keeps
  generated images visible and source-tracked immediately, marks successful
  renders as needing pack, and persists them through explicit `Pack Now` or a
  Blender `save_pre` handler. This is not a renderer fallback and does not
  change pixel semantics; it moves `image.pack()` cost out of the reload path.
- Smaller Blender bridge optimizations are still worth measuring after phase
  timing: avoid extra NumPy temporaries by writing directly into one
  bottom-row-first `float32` output array, optionally have the worker emit
  bottom-up RGBA8 to remove the row-flip copy, and use memory mapping or a
  persistent temp buffer to reduce duplicate 100MB+ byte copies. These are
  likely incremental; they should not distract from pack policy and native
  mask/clipping tile-silo work unless timings prove otherwise.
- Loading a temporary PNG/TGA/BMP through Blender's C image loader might avoid
  Python float conversion, but it risks recreating a sidecar-like workflow and
  adds encode/decode costs. Keep it as a measured experiment only if phase
  timing proves `foreach_set` dominates and the implementation can remain
  temporary/internal, with no persistent sidecar artifact and no fallback
  compositor.

Implemented diff-reload seam:

- The add-on now stores a native reload manifest on the generated Blender
  Image. The packaged worker can compare that manifest with the current `.clip`
  render graph and raster/mask CHNKExta compressed-tile fingerprints, then
  return `full`, `patch`, or `no_change` metadata. The Blender bridge applies
  patch payload rows directly to dirty `Image.pixels` rects using bottom-row
  Blender coordinates, avoiding full `foreach_set` for same-graph tile edits.
- This milestone is intentionally conservative. Canvas/root changes, visible
  node-order changes, and non-raster semantic changes still promote to full
  image updates. The current short-lived worker still renders through the
  existing full native path before slicing patch bytes, so native GPU/DAG cache
  reuse remains future work. The important accepted seam is now present:
  future persistent worker or tile-DAG cache invalidation can replace patch
  production without changing the Blender image-update protocol.

## Non-Goals

- Do not add a CPU compositor fallback.
- Do not hide residuals with post-processing.
- Do not keep a global all-layer full-canvas texture cache for large files.
- Do not optimize vector, text, bubble/frame, or future unknown filters in this
  repo unless scope is explicitly reopened.
