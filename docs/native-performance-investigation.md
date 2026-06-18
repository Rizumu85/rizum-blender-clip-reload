# Native Performance Investigation

Last updated: 2026-06-18

## Purpose

This file records high-leverage native renderer performance directions. It is
not a micro-optimization log. Keep correctness constraints from
`docs/AI_MEMORY.md` and `docs/native-code-architecture.md`: no CPU compositor
fallback, no post-processing, and no heuristic pruning that changes CSP
semantics.

The forward-looking architecture roadmap now lives in
`docs/native-tile-event-renderer-roadmap.md`. This file remains the evidence and
measurement record behind that roadmap.

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

At that milestone, eligibility was conservative and faithful: only unmasked
raster sources with nonzero opacity and supported Normal/standard blend modes
were collapsed. Masks, clipping-run nodes, filters, THROUGH groups, containers
as source nodes, and byte-domain special blends (`AddGlow`, `ColorDodge`,
`ColorBurn`, `GlowDodge`) remained explicit barriers and used the existing pass
path.

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

This was initially scoped to the same conservative raster-run eligibility:
unmasked ordinary raster sources with supported Normal/standard blend modes.
Masked Normal sources are now covered by the follow-up milestone below.
Clipping-run nodes, filters, THROUGH groups, containers as source nodes, masked
standard blends, and byte-domain special blends remain semantic barriers.

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

The next large lever is no longer ordinary raster-run source upload or mask
upload. It is making more source types tile-local: clipping-base and clipped
relationships, and eventually larger faithful raster/clipping segments.
Filters, THROUGH groups, isolated containers, and byte-domain special blends
should stay barriers until their tile-local semantics are explicitly modelled.

## Mask Atlas Tile Events

The mask-aware tile-silo path now uses shader-side R8 mask atlas events instead
of CPU pre-applying masks into RGBA chunks. This keeps the event model closer to
Silicate's tile-silo shape and allows masked standard raster blends to stay in
provider-backed atlas runs when their blend mode is otherwise eligible.

- `GpuNormalStackResourceProvider::raster_run_atlas_supports_masks()` exposes
  whether provider-backed atlas runs can carry mask events. The default is
  false, so alternate/test providers keep masked sources as barriers unless
  they explicitly opt in.
- `GpuRasterAtlasTileChunk` carries optional `mask_atlas_x/y` coordinates, and
  `GpuRasterAtlasTilePixels` carries R8 `GpuMaskAtlasTileChunk` uploads. The
  runtime provider samples layer masks at canvas-global coordinates and emits
  one mask chunk matching each decoded compressed RGBA atlas chunk.
- The tile-silo shader binds a second atlas texture for masks. Event words keep
  the existing RGBA atlas coordinates, source size, canvas offset, opacity, and
  blend kind, then add optional mask atlas coordinates. `u32::MAX` marks
  unmasked events.
- Normal and standard blends intentionally keep their existing arithmetic
  domains: Normal applies the mask through the existing integer
  `alpha * mask / 255` rule before the established tile-silo opacity step,
  while standard blends multiply the float source alpha by the mask after
  opacity, matching the per-source standard shader.
- The encoder still refuses the old per-source texture-copy atlas fallback for
  masked runs, because that fallback has no mask events. If the provider cannot
  return masked tile chunks, the renderer falls back to the existing faithful
  per-source path.
- The runtime atlas-run code lives in
  `clip_runtime/src/gpu_provider/atlas_run.rs`, and tile-silo buffer creation is
  split into `clip_gpu/src/stream_tile_silo_buffers.rs`, keeping the runtime
  provider and GPU stream modules below the production-file size budget.

Verification after this milestone:

- Rust: `cargo fmt --all --check`, `cargo check -q`, and `cargo test -q`.
- GPU unit coverage locks provider-backed masked Normal atlas collapse and adds
  a masked Multiply comparison against the existing per-source shader path,
  verifying that masked standard blends can use atlas tile events without
  changing pixels.
- `Test_Mask.clip --compare-png Test_Mask.png` is exact.
- `Test_AddGlowMultiply.clip --compare-png Test_AddGlowMultiply.png` remains at
  the existing one-LSB invisible residual (`raw_max=1`, `premul_max=1`,
  visible `0`).
- `Ref_Terra404_Live2D.clip --compare-png Ref_Terra404_Live2D.png` remains
  stable (`premul_max=3`, `premul_visible_px=13501`), with raw differences still
  dominated by transparent RGB. A baseline worktree at `aed8840` produced the
  same Terra metrics, so this residual is not from shader-side mask atlas
  events.

Performance note: this is a structural stepping stone, not the full Silicate
clipping model. RealArt/Terra direct timings remain in the same broad range on
this machine because their largest remaining barriers are clipping/container
relationships and byte-domain special blends. The next implemented step below
starts collapsing the raster-clipped-sibling part of clipping caches, but the
base/clipping relationship itself is still not fully tile-local.

## Clipped Raster Sibling Atlas Events

The streaming provider path now has a conservative clipping-cache tile-silo
subset:

- When a clipping run has already rendered its base into the owned clipping
  cache, consecutive clipped siblings that are plain eligible raster sources can
  be collapsed into one tile-silo pass instead of one preserve-alpha pass per
  sibling.
- The tile-silo shader has an explicit preserve-alpha mode. It samples the
  current clipping-cache destination, skips pixels where the base alpha is zero,
  applies the clipped source opacity/mask, blends Normal or standard RGB, and
  writes the original destination alpha back. This mirrors the existing
  clipped-raster preserve-alpha path; it is not a fallback compositor.
- The same provider-backed compressed-tile atlas path is used. Runtime atlas
  chunks that do not intersect the cropped clipping cache are filtered out when
  event records are prepared, so source tiles outside the base cache do not
  become errors or shader work.
- Byte-domain special blends (`AddGlow`, `ColorDodge`, `ColorBurn`,
  `GlowDodge`), clipped container/folder siblings, filters, THROUGH groups, and
  the base-cache creation/resolution relationship remain semantic barriers on
  the existing faithful path.
- The large tile-silo WGSL moved to `clip_gpu/src/shaders/tile_silo.wgsl`; the
  Rust module is now include wiring, keeping production Rust files below the
  project file-size budget.

Verification after this milestone:

- Rust: `cargo fmt --all --check`, `cargo check -q`, and `cargo test -q`.
- New GPU unit coverage locks clipped raster sibling atlas collapse and the
  cropped-cache edge case where the provider returns chunks outside the
  clipping base bounds.
- Release guard comparisons remain stable: `Test_Clipping` exact,
  `Test_ClippingEdge` exact, `Test_FolderNested` exact,
  `Test_AddGlowMultiply` `raw_max=1` / `premul_max=1` / visible `0`,
  `Test_RealArt` `raw_max=5` / `premul_max=1`, and
  `Ref_Terra404_Live2D` `premul_max=3` / `premul_visible_px=13501`.

The remaining order-of-magnitude native renderer target is still broader
tile-local clipping/base modelling: represent the base relationship and larger
faithful raster/clipping stretches as tile events rather than allocating and
resolving many cropped intermediate caches. This milestone only removes a
subset of per-sibling preserve passes after the clipping cache already exists.

## Raster Clipping-Run Tile-Local Events

The streaming provider path now has a direct raster-only clipping-run tile-silo
subset:

- `clip_gpu::stream_sequence` detects a `GpuNormalStackSource::ClippingRun`
  whose base and clipped siblings are all plain raster sources and whose whole
  `[base, clipped...]` sequence is eligible for the same tile-silo event model.
- `clip_gpu::stream_clipping_tile_silo` plans one atlas/work-list from the base
  plus clipped sources, keeps event order deterministic, and encodes the pass
  directly into the parent target instead of allocating the owned clipping
  cache for that subset.
- The tile-silo shader has an explicit clipping-run mode. For each output
  pixel, it composites base events into a tile-local transparent clip
  destination, applies clipped events with preserve-alpha semantics against
  that local destination, then resolves the local result into the parent
  destination through the base blend mode.
- The same compressed-tile atlas and optional R8 mask atlas event paths are
  used, so empty compressed tiles are not uploaded or sampled.
- Clipped container/folder siblings, filters, THROUGH groups, isolated
  containers, and byte-domain special blends (`AddGlow`, `ColorDodge`,
  `ColorBurn`, `GlowDodge`) remain barriers on the existing faithful path. This
  milestone is a raster-only subset, not a CPU fallback or a semantic
  approximation.

Verification after this milestone:

- Rust: `cargo fmt --all --check`, `cargo check -q`, and `cargo test -q`.
- GPU unit coverage locks direct base-plus-clipped raster event collapse,
  base-blend resolve through the parent stack, and the earlier clipped-sibling
  cache-collapse cases.
- Release guard comparisons remain stable: `Test_Clipping` exact,
  `Test_ClippingEdge` exact, `Test_FolderNested` exact,
  `Test_AddGlowMultiply` `raw_max=1` / `premul_max=1` / visible `0`,
  `Test_RealArt` `raw_max=5` / `premul_max=1`, and
  `Ref_Terra404_Live2D` `premul_max=3` / `premul_visible_px=13619`.

The remaining order-of-magnitude native renderer target is broader tile-local
semantic modelling across the barriers above. The raster-only base/clipped
relationship is now represented as events; the hard cases are container,
filter/THROUGH, and clipped container/folder sibling semantics.

## Render Program Planner Seam

The native streaming renderer now has a first explicit render-program IR seam:

- `clip_gpu::stream_program` plans a strict `GpuNormalStackSource` sequence into
  ordered render segments before GPU encoding.
- Current tile-local segment kinds cover atlas-backed raster runs and
  raster-only clipping runs. Current barrier segments use the existing faithful
  source encoder.
- `clip_gpu::stream_sequence` is now a segment executor. It invokes tile-silo
  encoders for tile-local segments and falls back to legacy source execution if
  a provider cannot fulfill a planned tile-local segment.
- The render-program stats record segment count, tile-local/barrier count,
  planned tile events, and planned passes. These are the first stable planning
  counters for future CLI/runtime diagnostics.

This does not make containers, THROUGH groups, filters, or clipped container
siblings tile-local yet. Its purpose is to stop growing opportunistic traversal
branches and give future work a single planner Module to deepen.

## Byte-Domain Special Blend Tile Events

The tile-event renderer now lowers the four current byte-domain raster blends
instead of treating them as `ByteDomainBlendNotLowered` barriers:

- Add Glow
- Color Burn
- Color Dodge
- Glow Dodge

Implementation shape:

- `stream_tile_event.rs` bumps `TILE_EVENT_ABI_VERSION` to `2` and marks these
  raster payloads as `TileEventKind::SpecialBlendRaster`.
- The shader still reads the same raster payload storage; the semantic event
  kind is carried in the header so later event/payload splits do not need to
  recover it from blend-mode values.
- `tile_silo.wgsl` now contains the existing verified byte-domain formulas from
  the pass shaders for normal compositing, clipped preserve-alpha compositing,
  and raster-only clipping-run resolve through a special-blend base.
- The planner no longer counts these modes under
  `ByteDomainBlendNotLowered`. For example, `IllustrationBlendModesB.clip`
  plans one raster-run tile-local segment plus the Paper/SolidColor barrier.

Verification after this milestone:

- Rust: `cargo check -q` and `cargo test -q`.
- GPU unit coverage locks tile-silo output for Add Glow, Color Burn, Color
  Dodge, and Glow Dodge against the established pass-shader fixture values.
- `.clip` guard comparisons: `Test_AddGlow`, `Test_ColorBurn`,
  `Test_ColorDodge`, and `Test_GlowDodge` exact; `Test_Clipping` and
  `Test_ClippingEdge` exact; `Test_AddGlowMultiply` remains at the existing
  invisible one-LSB residual (`raw_max=1`, `premul_max=1`, visible `0`);
  `Test_AddGlowMultiplyClipping` also remains at a one-LSB invisible residual;
  `IllustrationBlendModesB` stays at the known fidelity residual
  (`raw_max=5`, `premul_max=5`).

## Pointwise Filter Tile Events

The tile-event renderer now lowers the first pointwise filter subset into the
same tile-local execution model:

- `clip_gpu::stream_program` can plan a `RasterFilterRun` segment when a source
  range contains eligible raster events and supported pointwise LUT filters,
  including filter-first mixed runs such as `filter, raster`.
- It can also plan a `PointFilterRun` segment for consecutive filter-only
  ranges, applying those filters to the current dirty accumulator from previous
  segments.
- `stream_tile_event.rs` adds `TileEventKind::PointFilter` plus a separate
  `filter_payloads` storage buffer; the current tile event ABI is `7`.
- `tile_silo.wgsl` applies Tone Curve, HSL, Threshold, and Gradient Map filter
  modes to the local accumulator in event order, using the same math as the
  existing LUT filter pass.
- Leading filters in a mixed run now apply to the parent accumulator over the
  current target bounds before later raster events execute in the same tile
  program.
- Filter masks are bypassed when the provider can prove they are fully opaque,
  and real non-opaque filter masks lower when the provider can emit R8 mask
  atlas chunks. The runtime provider proves opaque masks from mask
  `empty_fill=255` plus zero compressed mask tiles. Unknown or
  provider-unavailable filter masks remain an explicit `FilterNotLowered`
  barrier.

Performance-plan evidence:

- `Test_ToneCurve.clip --performance-plan-json` now reports
  `raster_filter_run_segments: 1`, `barrier_segments: 0`, and `planned_passes:
  1`.
- `Test_Gradiation.clip --performance-plan-json` reports the same filter
  segment shape.
- `Test_HSL2.clip --performance-plan-json` lowers the raster plus HSL filter;
  the remaining barrier is the Paper/SolidColor source.

Verification after this milestone:

- Rust: `cargo check -q` and `cargo test -q`.
- GPU unit coverage compares a leading filter followed by a raster against the
  existing legacy source path.
- GPU unit coverage also checks a filter-only segment after a legacy source.
- GPU unit coverage compares provider-backed masked filter-only and
  masked-filter-inside-container tile events against the existing legacy source
  path.
- `Test_ToneCurve` exact.
- `Test_HSL2` exact.
- `Test_HSL3`, `Test_HSL4`, and `Test_HSL5` keep the existing one-LSB
  non-visible residual shape.
- `Test_Gradiation` remains at the known `raw_max=10` / `premul_max=10`
  Gradient Map residual.
- `Test_AddGlowMultiply` remains at the existing one-LSB invisible residual,
  and `Test_ClippingEdge` remains exact.

This is a real semantic-barrier reduction. Masked pointwise filters now use the
same provider-backed R8 mask atlas model as raster and scope masks; future
unknown or non-local filters still need explicit faithful tile-local models
before they can lower.

## Simple Container Scope Tile Events

The tile-event renderer now lowers a simple isolated-container subset,
including provider-backed non-opaque scope masks:

- `clip_gpu::stream_program` can plan a `SimpleContainerScope` segment for a
  folder with positive opacity, no container mask, a proven fully opaque
  container mask, or a provider-backed non-opaque R8 scope mask, a resolve
  blend mode modeled by the tile VM, known finite bounds, and children limited
  to eligible raster events, one direct simple container scope with the same
  scope-mask support, plus pointwise filters whose masks are absent, proven
  fully opaque, or available through provider-backed R8 mask atlas chunks, plus
  one direct simple THROUGH scope child.
- `stream_tile_event.rs` now uses tile event ABI `7`; `BeginContainer` /
  `EndContainer` scope payloads carry optional R8 mask atlas coordinates in
  payload words 6/7, and `PointFilter` payloads carry optional filter mask
  atlas coordinates in payload words 10/11.
- `tile_silo.wgsl` keeps local transparent-white scope accumulators. Raster
  and pointwise-filter events inside the scope modify the active accumulator,
  then `EndContainer` resolves it into the parent accumulator through the
  existing Normal alpha-over, byte-domain special-blend, or standard blend
  helper, multiplying the resolve source alpha by the optional scope mask.
- Nested simple containers can lower as nested `BeginContainer` /
  `EndContainer` events up to `SIMPLE_CONTAINER_SCOPE_DEPTH_LIMIT == 3`. Inner
  accumulators resolve into their parent accumulator before the outer container
  resolves to its parent.
- A direct THROUGH child inside a simple container can lower as
  `BeginThrough` / `EndThrough` events. The tile VM captures the active
  container accumulator as the THROUGH `before`/`after` pair, renders the
  THROUGH children into `after`, then resolves back into the same container
  accumulator.
- Unsupported scope shapes remain barriers: container depth beyond the fixed
  limit, nested/indirect THROUGH-in-container shapes, clipping runs, solid
  colors, unavailable masked containers, and provider-unavailable or unknown
  filter masks.

Verification after this milestone:

- Rust: `cargo fmt --all --check`, `cargo check -q`, and
  `cargo test -q -p clip_gpu`.
- New GPU unit coverage renders a Multiply raster inside a Normal folder over
  an opaque gray background. The tile scope path produces the isolated source
  colour; a direct-through Multiply implementation would darken it.
- New GPU unit coverage also compares tile-scope execution against the existing
  legacy source path for Normal container opacity, Multiply container resolve,
  and Multiply container resolve with non-1 opacity.
- New GPU unit coverage compares direct and three-deep nested containers
  against the existing legacy source path and locks container depth beyond the
  fixed limit as a barrier.
- New GPU unit coverage compares a direct THROUGH child inside a simple
  container scope against the existing legacy source path.
- New GPU unit coverage compares non-opaque masked container scope resolve
  against the existing legacy source path.
- New GPU unit coverage compares a provider-backed non-opaque masked filter
  inside a container scope against the existing legacy source path.
- Guard comparisons remain stable: `Test_Clipping` exact,
  `Test_ClippingEdge` exact, `Test_FolderNested` exact, `Test_ToneCurve` exact,
  and `Test_AddGlowMultiply` remains at the existing one-LSB invisible
  residual (`raw_max=1`, `premul_max=1`, visible `0`).

Most existing public fixtures either have their structural root container
elided or express folder semantics as THROUGH groups, so they may still report
`simple_container_scope_segments: 0`. This milestone is still useful because it
turns the scope-stack model into an executed tile event path and gives future
container/THROUGH lowering a tested shader seam.

## Simple THROUGH Scope Tile Events

The tile-event renderer now lowers the first narrow THROUGH subset:

- `clip_gpu::stream_program` can plan a `SimpleThroughScope` segment for a
  THROUGH group with positive opacity, no THROUGH mask, a proven fully opaque
  THROUGH mask, or a provider-backed non-opaque R8 scope mask, known finite
  bounds, and children limited to eligible raster events, simple container
  scopes with the same scope-mask support, plus pointwise filters whose masks
  are absent, proven fully opaque, or available through provider-backed R8 mask
  atlas chunks.
- `stream_tile_event.rs` uses tile event ABI `7`, with scope payload words 6/7
  as optional R8 mask atlas coordinates for container and THROUGH scope
  resolves, and point-filter payload words 10/11 as optional R8 mask atlas
  coordinates for masked pointwise filters.
- `tile_silo.wgsl` stores the current parent accumulator as THROUGH `before`,
  renders child events into THROUGH `after`, and resolves `before`/`after`
  through the same premultiplied opacity interpolation as the existing THROUGH
  pass, multiplying resolve strength by the optional scope mask.
- Simple containers inside the THROUGH scope lower as nested `BeginContainer` /
  `EndContainer` events up to the same fixed depth limit and resolve into the
  THROUGH `after` accumulator.
- One level of nested THROUGH can lower when the nested THROUGH has positive
  opacity, has the same supported scope-mask shape, has known intersecting
  bounds, and its children fit the same raster/container/pointwise-filter
  subset. The tile VM keeps two local THROUGH `before`/`after` accumulators,
  resolves the inner THROUGH into the outer THROUGH `after` accumulator, and
  floor-quantizes the inner resolve to match the intermediate RGBA8 writeback
  of the existing pass-heavy path.
- Unsupported THROUGH shapes remain barriers: deeper nested THROUGH groups,
  clipping runs, solid colors, unavailable masked THROUGH groups, container
  depth beyond the fixed limit, and provider-unavailable or unknown filter
  masks.

Verification after this milestone:

- Rust: `cargo fmt --all --check`, `cargo check -q`, and `cargo test -q`.
- GPU unit coverage compares the tile-scope path against the existing legacy
  source path for THROUGH opacity and for child Multiply blending inside a
  THROUGH group.
- GPU unit coverage also compares a simple container inside a THROUGH scope
  against the existing legacy source path.
- GPU unit coverage compares a fractional-opacity nested THROUGH scope against
  the existing legacy source path and keeps deeper nested THROUGH groups as
  planner barriers.
- Planner and GPU unit coverage prove explicitly fully opaque masks do not
  block simple container/THROUGH tile-local lowering, and GPU unit coverage now
  compares non-opaque masked container and THROUGH scope resolves against the
  existing legacy source path. Unknown or provider-unavailable masks still
  report the explicit `ScopeMaskNotLowered` barrier reason.
- Planner and GPU unit coverage prove provider-backed non-opaque pointwise
  filter masks can lower inside scope stacks, while provider-unavailable
  filter masks remain explicit `FilterNotLowered` barriers.
- Planner unit coverage reports scope stacks beyond the fixed accumulator limit
  as `ScopeDepthLimitExceeded`, and simple scope programs above
  `MAX_SILO_EVENTS` as `TileEventLimitExceeded`, so large/complex scope
  barriers are measurable instead of collapsing into generic container or
  THROUGH barriers.
- `Test_FolderNested.clip --performance-plan-json` reports
  `simple_through_scope_segments: 1` and `tile_event_abi_version: 7`.
- Guard comparisons remain stable: `Test_Clipping` exact,
  `Test_ClippingEdge` exact, `Test_FolderNested` exact, `Test_ToneCurve` exact,
  and `Test_AddGlowMultiply` remains at the existing one-LSB invisible
  residual (`raw_max=1`, `premul_max=1`, visible `0`).

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
tile-silo renderer that expands beyond the current raster-run, mask-event, and
raster-only clipping-run subsets.

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
  small passes. The current tile-silo path follows this for eligible raster
  runs, mask-bearing raster runs, clipped-raster sibling runs, and raster-only
  clipping runs. The next large native milestone should extend the event model
  across remaining semantic barriers, not spend time on more local pass tuning.
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
  renders as needing pack, and persists them through explicit `Pack` or a
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
  image updates. The worker now also has a persistent JSON-lines server mode
  that keeps one native process and reusable `RuntimeGpuRenderer` alive across
  Blender requests, avoiding repeated process startup and wgpu device
  initialization on reload. Persistent server reloads also keep a raster/mask
  GPU texture cache keyed by source metadata plus CHNKExta compressed-tile
  fingerprints, so unchanged input textures are reused across requests. Patch
  reloads now render the stack through dirty-region GPU targets and read back
  only those patch targets instead of rendering the full canvas and slicing it
  afterward. A fuller per-subtree tile-DAG cache is still a later native
  renderer milestone, but the Blender image-update protocol no longer requires
  full-canvas native rendering for same-graph patch reloads.

## Non-Goals

- Do not add a CPU compositor fallback.
- Do not hide residuals with post-processing.
- Do not keep a global all-layer full-canvas texture cache for large files.
- Do not optimize vector, text, bubble/frame, or future unknown filters in this
  repo unless scope is explicitly reopened.
