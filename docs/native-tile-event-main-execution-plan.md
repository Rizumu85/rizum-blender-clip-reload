# Native Tile-Event Main Execution Plan

Last updated: 2026-06-19

## Purpose

This document defines the next architecture target for native renderer
performance. The renderer should stop treating tile-silo execution as an
opportunistic optimization and instead make tile-local event execution the main
model. Barrier passes remain only for `.clip` semantics that do not yet have a
faithful tile-local model.

The product goal is order-of-magnitude native render and reload improvement
without approximate compositing, CPU compositor fallback, post-processing, or
global full-canvas layer caches.

## Current Bottleneck

The remaining cost is no longer ordinary raster upload alone. The renderer
already has sparse tile decode, atlas raster-run collapse, direct compressed
tile events, mask atlas events, clipped raster sibling events, raster-only
clipping-run events, pointwise filter events, simple container scopes, simple
THROUGH scopes, session sparse atlas cache state, GPU sparse atlas textures,
and the first suffix patch executor seam.

Large files still spend work at semantic seams:

- complex containers and THROUGH groups that remain barriers
- clipped container or folder siblings
- provider-unavailable or unknown masked filters and scopes
- scope-depth and event-count limits
- dirty reloads that lack a valid segment-before checkpoint

The next leverage is to deepen the render-program planner and tile-event
executor Modules, not to keep tuning individual pass creation, texture views,
or upload calls.

## Architecture Target

Target execution shape:

```text
strict CSP source selection
  -> semantic render program
  -> tile-local event executor
  -> explicit barrier executor
```

The core seam is the render program. Its Interface should answer what lowered
to tile-local execution, what stayed as a barrier, why, and what cost is
expected.

```rust
struct RenderProgram {
    canvas: CanvasSize,
    segments: Vec<RenderSegment>,
    resources: ResourcePlan,
    stats: RenderProgramStats,
}

struct RenderSegment {
    id: SegmentId,
    bounds: CanvasRect,
    kind: SegmentKind,
    depends_on: Vec<SegmentId>,
    cost_hint: CostHint,
}

enum SegmentKind {
    TileLocal(TileProgram),
    Barrier(BarrierProgram),
}
```

The stream executor should eventually become thin:

```rust
for segment in render_program.segments {
    match segment.kind {
        SegmentKind::TileLocal(program) => tile_vm.encode(program),
        SegmentKind::Barrier(program) => barriers.encode(program),
    }
}
```

## Tile Events as the Default

The old mental model was:

```text
per-source pass renderer is the main path
tile-silo is an optimization for eligible runs
```

The new mental model is:

```text
tile event renderer is the main path
barrier passes are explicit temporary seams
```

Default all renderable semantics toward tile events. Only structures without a
faithful tile-local model should become barriers.

Every barrier must be explainable:

```rust
enum BarrierReason {
    UnsupportedTileLocalFilter(FilterKind),
    ThroughGroupNotLowered,
    IsolatedContainerRequiresIntermediate,
    ScopeMaskNotLowered,
    ByteDomainBlendNotLowered(GpuRasterBlendMode),
    ClippedContainerSiblingNotLowered,
    ScopeDepthLimitExceeded,
    TileEventLimitExceeded,
}
```

Performance diagnostics should expose barrier counts and examples, so slow
files explain whether the cost is dominated by THROUGH, containers, filters,
special blends, clipping shapes, tile occupancy, or Blender upload.

## Typed Event VM

The event buffer should stay versioned and typed. A fixed raster-event word
layout cannot remain the main renderer Interface.

Target shape:

```rust
#[repr(u32)]
enum TileEventKind {
    Raster = 1,
    BeginClipBase = 2,
    ClippedRaster = 3,
    ResolveClipBase = 4,
    BeginContainer = 5,
    EndContainer = 6,
    PointFilter = 7,
    SpecialBlendRaster = 8,
}

struct TileEventHeader {
    kind: u32,
    flags: u32,
    payload_offset: u32,
    payload_len: u32,
}
```

Use separate payload buffers for raster, clipping, filter, and scope data. The
tile program ABI version must be part of CLI diagnostics, reload manifests, and
persistent worker compatibility checks.

## Session Sparse Atlas Cache

The long-lived atlas cache should be sparse and tile-based, not a full-canvas
layer cache.

```rust
struct AtlasAllocator {
    rgba_atlases: Vec<TextureAtlas>,
    mask_atlases: Vec<TextureAtlas>,
    lut_atlases: Vec<TextureAtlas>,
    entries: HashMap<ResourceFingerprint, AtlasSlot>,
    budget_bytes: u64,
    generation: u64,
}

struct ResourceFingerprint {
    clip_file_id: ClipFileId,
    layer_id: LayerId,
    mipmap_id: MipmapId,
    source_rect: Rect,
    compressed_tile_fingerprint: TileFingerprint,
    mask_fingerprint: Option<TileFingerprint>,
}
```

Reload behaviour:

- unchanged graph plus unchanged compressed tile reuses the atlas slot
- changed compressed tile updates only that atlas region
- unused tiles are reclaimed through generation or LRU policy

This cache must be fed by the render program and reload manifest, not rebuilt
from ad hoc worker logic.

## Independent Mask Resources

Masks should be first-class R8 atlas resources:

- raster compressed tile -> RGBA atlas slot
- mask compressed tile -> R8 atlas slot
- event references raster slot plus mask slot or fill
- shader samples mask in canvas/global coordinates
- multiple raster events can reference the same mask tile

Avoid generating per-raster matching mask chunks on CPU when one mask atlas
tile can be uploaded once and reused.

## Pointwise Filters

Pointwise filters should lower to tile events:

```text
out(x, y) = f(in(x, y), payload, mask(x, y), opacity)
```

Eligible filters include Brightness/Contrast, Level Correction, Tone Curve,
HSL, Color Balance, Invert, Posterization, Threshold, and Gradient Map.
Non-local filters remain barriers until their faithful tile-local model exists.

## Scope Stack

Containers and THROUGH groups need a tile-local scope model instead of
case-by-case pass collapse.

```rust
enum ScopeOp {
    PushTransparentWhite,
    PushCopyParent,
    PopResolve { opacity, blend, mask },
    BeginClipBase,
    ResolveClipBase,
}
```

The planner decides whether a scope can lower by checking bounds, event count,
scope depth, mask availability, nested THROUGH/container shape, filters, and
clipping relationships. Unsupported shapes stay barriers with explicit reasons.

## Frame Arena

Once tile events are the main path, per-segment buffer creation becomes fixed
overhead. Add a render-frame arena after the event model stabilizes:

```rust
struct GpuFrameArena {
    event_u32_buffer: RingStorageBuffer<u32>,
    work_u32_buffer: RingStorageBuffer<u32>,
    span_u32_buffer: RingStorageBuffer<u32>,
    params_buffer: RingUniformBuffer<Params>,
    bind_group_cache: BindGroupCache,
}
```

Small buffers should append into ring buffers, segments should pass offset and
length ranges, and the arena should reset after a render.

## Dirty Segment Reload

Patch reload should move beyond final dirty rectangles:

- graph node fingerprint
- resource tile fingerprint
- segment fingerprint
- tile work-list fingerprint
- event-range fingerprint

Reload rules:

- source tile changed -> update atlas slot, dirty affected events, rerun
  affected segments, read back affected output tiles
- layer opacity changed -> reuse atlas, update event payload, rerun affected
  segments
- container structure changed -> rebuild segment graph and promote to full or
  subtree render when needed

Current safe product subsets:

- if the first dirty segment starts at source index `0`, the Blender worker may
  rerun the affected raster segment window over the initial transparent-white
  accumulator and return dirty-rect patch payloads
- if the first dirty segment starts after source index `0`, the runtime may
  reconstruct or reuse the segment-before checkpoint, then execute only the
  later `RasterRun` segments whose tile work-list intersects the dirty patch
  rectangles
- later segments whose tile work-list does not intersect the dirty rectangles
  can be skipped; later overlapping non-raster or unknown-work segments are
  explicit skipped segments and force the region-render fallback
- affected raster windows report coalesced event ranges from only the
  work-list tiles whose canvas bounds intersect the dirty rectangles; unknown
  work-list tiles keep a full segment range and force fallback

The reconstructed-prefix path is a correctness seam, not the final performance
shape when there is no cache hit. The runtime now has a small session
budgeted LRU segment-before checkpoint cache keyed by the current reload
manifest prefix, so repeated dirty segment reloads can reuse selected RGBA8
checkpoints when their prefixes are unchanged. Checkpoint selection is now
explicit in render-program inspection and reload manifests through a
`checkpoint_before` flag; product reruns only use depth-0 explicit candidates
as top-level source boundaries. Candidate ranking now exists as
`checkpoint_priority` in render inspection and reload manifests; the session
checkpoint cache uses that priority when choosing which cached checkpoint to
evict under count or memory budget pressure, with LRU retained as the
equal-priority tie-breaker. Affected-window execution now also lowers the
raster-only `RasterClippingRun` form into sparse atlas executor batches by
running the existing tile-silo clipping-run shader mode over resident atlas
slots. It also lowers unmasked `PointFilterRun` segments into filter-only
sparse atlas batches with uploaded LUT rows and the existing typed point-filter
shader path. The first provider-backed masked `PointFilterRun` subset is also
executable when the dirty filter bounds are fully covered by one resident R8
mask slot; the filter payload points into that slot with the dirty-rect offset.
The next target is expanding affected-window execution beyond single-slot
masked point filters by lowering multi-slot filter masks or simple scopes into
executable sparse atlas events.

## Implementation Order

1. Keep `--performance-plan-json` as the coverage scoreboard.
2. Keep expanding `RenderSegment` and `LoweringDecision`; do not add more
   opportunistic traversal branches.
3. Move all existing raster, mask, clipping, special-blend, filter, container,
   and THROUGH event support behind typed tile-event executor Modules.
4. Lower more special blends and pointwise filters only with sample-backed
   parity tests.
5. Expand the scope stack for containers, THROUGH, clipping runs, and clipped
   container/folder siblings.
6. Add frame arena and bind-group/buffer reuse once tile events dominate the
   path.
7. Expand affected-window execution beyond single-slot masked `PointFilterRun`
   by lowering multi-slot filter masks or simple scopes into executable sparse
   atlas events.
8. Promote useful segment-before checkpoint storage toward GPU-resident or
   cropped forms only when profiling proves the CPU RGBA8 checkpoint is the
   limiting factor.
9. Keep the pass-heavy renderer as a test oracle and debug backend, not as a
   product fallback.

## Non-Goals

- No CPU compositor fallback.
- No post-processing or approximate fixes for speed.
- No global full-canvas layer texture cache.
- No vector, text, bubble/frame, or animation renderer scope in this repo.
- No dual production renderer path that hides tile-event coverage gaps.
