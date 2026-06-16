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
  `2220`, raster empty tiles `129492`, mask compressed tiles `223`, semantic
  barriers `67`, collapsible raster/clipping segments `75`.
- `Ref_Terra404_Live2D.clip`: raster metadata slots `326040`, raster compressed
  tiles `4826`, raster empty tiles `321214`, mask compressed tiles `2052`,
  semantic barriers `220`, collapsible raster/clipping segments `178`.
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

Next quantity-level work is tile-local work lists and pass collapse for
raster/clipping stretches. Filters, THROUGH groups, and isolated containers
remain semantic barriers until faithful tile-local models exist.

## Non-Goals

- Do not add a CPU compositor fallback.
- Do not hide residuals with post-processing.
- Do not keep a global all-layer full-canvas texture cache for large files.
- Do not optimize vector, text, bubble/frame, or future unknown filters in this
  repo unless scope is explicitly reopened.
