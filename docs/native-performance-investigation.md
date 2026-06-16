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

## Non-Goals

- Do not add a CPU compositor fallback.
- Do not hide residuals with post-processing.
- Do not keep a global all-layer full-canvas texture cache for large files.
- Do not optimize vector, text, bubble/frame, or future unknown filters in this
  repo unless scope is explicitly reopened.
