# Native Performance Investigation

Last reconciled: 2026-06-20

This file is the compressed evidence log for performance work. It keeps the
useful lessons and rejected detours without preserving every implementation
milestone.

## Current Performance Model

The main renderer is a GPU tile-event renderer with explicit barrier segments.
The high-level goal is not to shave one API call at a time; it is to keep CSP
semantics faithful while moving safe work into tile-local execution and reusing
session resources during reload.

Already accepted:

- Metadata-first source selection.
- Recursive provider streaming from an owned `ClipContainer`.
- Selected CHNKExta tile reads.
- Sparse decode/upload of non-empty compressed tiles.
- Atlas-backed raster runs.
- Typed tile events for supported raster/filter/scope/clipping cases.
- Persistent worker reloads with `full`, `patch`, and `no_change` modes.
- Reload manifests based on visible graph and compressed raster/mask tile
  fingerprints.
- Render/decode profiling behind environment variables.

## Silicate Takeaway

Silicate's useful lesson was not "use WebGPU" by itself. The transferable model
was:

- Keep tiled sources in shared atlases.
- Build per-tile work lists.
- Execute many layer events inside one or a few tile-local passes.
- Treat unsupported semantics as explicit barriers.

This repo has adopted that direction through the tile-event renderer. Future
semantic coverage must still pass the scoreboard gate in
`docs/native-tile-event-renderer.md`.

## Decode Investigation

Environment:

- `RIZUM_CLIP_DECODE_PROFILE=1`
- `RIZUM_CLIP_PARALLEL_TILE_DECODE=1`

Measured fields included selected raster/mask tiles, skipped empty tiles,
compressed bytes, zlib inflate time, RGBA swizzle time, mask decode/crop time,
atlas chunk build time, sparse atlas update time, region patch render time, and
worker total time.

Result:

- Independent zlib tile decode can be parallelized deterministically.
- Adaptive policy kept tiny work sequential.
- Output stayed identical on guard samples.
- Release medians did not improve worker-total time meaningfully on the large
  samples that mattered.

Conclusion:

- Decode/zlib/atlas chunk construction is not the current dominant bottleneck.
- Keep parallel decode disabled unless future profiles show decode dominates.
- Do not pursue JPEG-style speculative Huffman decoding; `.clip` CHNKExta/zlib
  blocks do not provide JPEG entropy self-synchronization.

## Render Profiling

Environment:

- `RIZUM_CLIP_RENDER_PROFILE=1`

Useful fields:

- Source selection.
- Render-program planning.
- Event/payload build.
- GPU pass encode.
- Queue submit/poll and CPU wait-proxy execution time.
- Readback copy and patch extraction.
- Checkpoint reconstruction.
- Sparse atlas update.
- Legacy barrier and tile-local segment counts/times.
- Top slow segments.

Findings:

- Process startup and `wgpu` device initialization dominate small CLI samples.
- Large samples still spend meaningful time in tile-local segment execution and
  legacy barrier segments.
- Readback and source selection are not the main full-render bottlenecks.
- Blender upload/pack has separate user-facing cost, but native worker time is
  the main renderer-side target.

## Reload Cache Investigation

Persistent reload benchmark results showed:

- The persistent worker is useful because it avoids process/device startup.
- Reload manifests correctly separate `full`, `patch`, and `no_change`.
- Current fixed reload fixtures often fall back to `patch_renderer=region`.
- Sparse atlas and checkpoint diagnostics are useful for explaining why a patch
  did or did not become cheaper.

Important no-go:

- A resident sparse atlas RasterRun prototype inside `patch_renderer=region`
  produced byte-identical patch payloads but had zero useful resident hits in
  the fixed fixture. It should not become default from current evidence.

## External Architecture Takeaways

libvips:

- Demand-driven regions are useful when a barrier is accidentally rendering a
  larger area than requested.
- The first coarse region-demand prototype was not applicable because top reload
  segments were already rendering dirty rects, not full canvas.

OpenImageIO ImageCache:

- Tile identity, reuse, eviction, and dirty counts are useful diagnostics.
- The project should keep sparse tile caches keyed by file/source/tile
  fingerprints.
- Do not introduce full-canvas per-layer caches.

Chromium TileManager/task graph:

- Task graph diagnostics help explain decode/upload/run/readback/fallback
  ordering.
- Keep diagnostics lightweight; do not add a broad task graph scheduler without
  a measured waste pattern.

Rendering Elimination:

- Output tile signatures may later prove rendered-but-unchanged patch tiles.
- Current work only measures signatures; it does not skip tiles from signatures.

Krita:

- SIMD or byte-domain changes must be correctness-first and sample-guarded.
- Do not trade visible fidelity for speed.

Vello:

- Compute-centric renderers are a useful long-term reference, but this project
  should not rewrite the renderer around compute without a focused prototype.

## Rejected Or Deferred Detours

- Large scope atlas expansion above the current cap worsened fidelity on large
  samples and remains rejected until focused legacy-vs-tile guards prove it.
- Queue upload micro-optimizations are not the next high-leverage target unless
  profiling changes.
- Broad libvips-style region pipeline work is deferred.
- Broad Chromium-style task graph scheduling is deferred.
- Decode parallelism is not enabled by default.
- Semantic lowering for speed is paused unless the scoreboard gate selects one
  measured barrier reason.

## How To Use This File

Before starting performance work:

1. Run the relevant profile or benchmark.
2. Identify the dominant phase or barrier.
3. Choose one prototype with byte-for-byte or compare-png validation.
4. Record only the durable result here.
