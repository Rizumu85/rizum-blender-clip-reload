# Native Tile-Event Architecture Strategy

Last updated: 2026-06-19

## Purpose

This document turns the next native renderer performance direction into a
concrete architecture strategy. The renderer should make tile-local event
execution the main execution model, and treat pass-heavy rendering as an
explicit barrier seam for `.clip` semantics that have not yet been lowered
faithfully.

This is not a plan to tune one more pass, copy, texture view, or shader branch.
The goal is to deepen the render-program planner, typed tile-event executor,
and session resource-cache Modules so large `.clip` files can render and
reload by executing only the tile-local work that actually matters.

Correctness remains the controlling constraint:

- no approximate `.clip` semantics for speed
- no CPU compositor fallback
- no post-processing
- no global full-canvas layer texture cache
- no vector, text, bubble/frame, animation, or write-back scope in this repo

## Architecture Decision

The old mental model was:

```text
per-source pass renderer is the main path
tile-silo is an optimization when an eligible run appears
```

The new mental model is:

```text
tile event renderer is the main path
barrier passes are temporary seams for semantics not yet lowered
```

This changes the shape of performance work. A new semantic case should first
ask whether it can lower to tile events. Only the cases without a faithful
tile-local model should become barriers.

The target execution shape is:

```text
strict CSP source selection
  -> render-program planner
  -> tile-local event executor
  -> explicit barrier executor
```

The render-program planner Interface should answer:

- which source ranges lower to tile-local segments
- which source ranges stay as barriers
- why each barrier still exists
- how many tile events, passes, atlas slots, and intermediate targets are
  planned
- which segments and event ranges can be invalidated on reload

The executor Interface should consume a planned tile program, not recover CSP
semantics by walking source trees opportunistically.

## Current Baseline

The renderer already has the first useful forms:

- `clip_gpu::stream_program` plans strict sources into render segments.
- `--performance-plan-json` reports tile-local segment counts, barrier counts,
  barrier reasons, compressed tile occupancy, estimated atlas bytes, estimated
  tile events, and tile-event ABI version.
- The typed tile-event ABI currently models raster events, special-blend
  rasters, pointwise filters, container scopes, THROUGH scopes, scope masks,
  and raster-only clipping runs.
- The sparse atlas cache and sparse atlas texture pool can reuse raster and
  mask source tiles across persistent-worker reloads.
- Product reload can execute selected sparse affected-window segments over the
  initial accumulator or over a cached/reconstructed segment-before checkpoint.
- Sparse affected-window lowering already supports direct raster simple scopes,
  scope masks including multi-slot masks, point-filter children, nested simple
  scopes, and raster-only clipping-run children in several nested positions.

The remaining performance cost is concentrated at semantic seams:

- clipped container or folder siblings
- complex containers and THROUGH groups beyond the current scope-depth and
  event-count limits
- provider-unavailable or unknown masked filters and scopes
- future non-local filters without a tile-local model
- cases where dirty reload lacks a reusable segment-before checkpoint

Ordinary raster upload is no longer the primary architecture lever.

## Module Deepening Targets

### Render-Program Planner

The planner is the main seam between strict CSP semantics and GPU execution. It
should hide more lowering logic behind a small Interface:

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

The deletion test for this Module is clear: if the planner disappears,
eligibility rules, barrier reasons, event counts, dirty segment fingerprints,
and cost hints reappear across stream traversal, CLI diagnostics, reload
manifests, and tests. Keeping that knowledge behind one Interface gives
Leverage and Locality.

Eligibility should become first-class lowering decisions:

```rust
enum LoweringDecision {
    TileLocal { reason: &'static str, cost: CostHint },
    Barrier { reason: BarrierReason },
}
```

Barrier reasons must be explicit and counted:

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

Example diagnostics:

```json
{
  "tile_local_segments": 42,
  "barrier_segments": 17,
  "barriers": {
    "ThroughGroupNotLowered": 3,
    "ByteDomainBlendNotLowered": 8,
    "IsolatedContainerRequiresIntermediate": 6
  }
}
```

### Typed Tile-Event Executor

The event Interface should stay versioned and typed. A fixed raster event
layout cannot be the long-term renderer Interface.

Target event shape:

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

Storage should remain split by payload type:

- event headers
- raster payloads
- clipping payloads when clipping grows beyond raster-only events
- filter payloads
- scope payloads
- tile spans and work lists

This keeps the shader and executor extensible without reshaping every raster
payload when a new semantic event is added.

### Session Sparse Atlas Allocator

The cache target is sparse tile reuse, not full-canvas layer reuse.

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

Reload rules:

- unchanged graph plus unchanged compressed tile reuses the atlas slot
- changed compressed tile updates only the affected atlas region
- unused tiles are reclaimed by generation or LRU policy

This Module should be fed by the render program and reload manifest. It should
not be rebuilt from worker-side ad hoc logic.

### Independent Mask Tile Resources

Masks should be independent R8 atlas resources:

- raster compressed tile maps to an RGBA atlas slot
- mask compressed tile maps to an R8 atlas slot
- tile events reference raster slots plus mask slots or mask fill
- shaders sample masks in canvas coordinates
- multiple raster events can reuse one uploaded mask tile

This avoids generating matching CPU mask chunks for every raster chunk and
keeps masked Live2D-style files closer to the sparse tile model.

### Pointwise Filter Events

Pointwise filters should remain tile events:

```text
out(x, y) = f(in(x, y), filter_payload, mask(x, y), opacity)
```

Eligible filters include:

- Brightness/Contrast
- Level Correction
- Tone Curve
- HSL
- Color Balance
- Invert/Reverse Gradient
- Posterization
- Threshold
- Gradient Map

Non-local filters should remain barriers until their faithful tile-local model
exists.

### Tile-Local Scope Stack

Containers and THROUGH groups should lower through a scope-stack model instead
of scattered one-off cases.

```rust
enum ScopeOp {
    PushTransparentWhite,
    PushCopyParent,
    PopResolve { opacity, blend, mask },
    BeginClipBase,
    ResolveClipBase,
}
```

The planner decides whether a scope can lower by checking:

- finite bounds
- scope depth limit
- event count limit
- mask availability
- nested container and THROUGH shape
- filter locality
- clipping relationship shape
- whether special blends are modeled

Unsupported scope shapes stay barriers. This preserves fidelity while making
the remaining performance cost explainable.

### Render-Frame Arena

After tile events dominate execution, per-segment buffer creation becomes fixed
tax. Add a frame-arena Module:

```rust
struct GpuFrameArena {
    event_u32_buffer: RingStorageBuffer<u32>,
    work_u32_buffer: RingStorageBuffer<u32>,
    span_u32_buffer: RingStorageBuffer<u32>,
    params_buffer: RingUniformBuffer<Params>,
    bind_group_cache: BindGroupCache,
}
```

The intended Interface:

- append small event/work/span payloads into ring buffers
- pass offset/length ranges to each segment
- reuse compatible bind groups and atlas views
- reset the arena after each render

Do this after the event model is stable. Doing it earlier risks optimizing
temporary Interfaces.

## Dirty Reload Strategy

Reload should move from final dirty rectangles toward dirty graph, dirty
segment, and dirty event state.

Manifest data should include:

- graph node fingerprints
- resource tile fingerprints
- segment fingerprints
- tile work-list fingerprints
- event-range fingerprints

Reload rules:

- source tile changed: update the atlas slot, dirty affected tile events, rerun
  affected segments, and read back affected output tiles
- layer opacity changed: reuse atlas slots, update event payloads, and rerun
  affected segments
- container structure changed: rebuild the segment graph and promote to full or
  subtree render when needed

This is the long-term path for local edits such as adding one raster layer or
changing one tile. It should not become a global layer texture cache.

## Implementation Phases

### Phase 0: Keep Performance-Plan JSON as the Scoreboard

No renderer behavior change is needed in this phase. The diagnostic must
continue to report the planner coverage that matters:

```json
{
  "canvas": [4096, 4096],
  "sources": 715,
  "planned_passes": 312,
  "tile_local_segments": 28,
  "barrier_segments": 94,
  "barrier_reasons": {
    "ByteDomainBlendNotLowered": 31,
    "IsolatedContainerRequiresIntermediate": 40,
    "ThroughGroupNotLowered": 8,
    "FilterNotLowered": 15
  },
  "compressed_raster_tiles": 4826,
  "mask_tiles": 2052,
  "atlas_upload_bytes": 123456789,
  "estimated_tile_events": 18000
}
```

Performance changes should be judged by this coverage plus actual timings, not
by intuition from one file.

### Phase 1: Make RenderSegment and TileProgram the Executor Interface

The planner should emit segment programs. The stream executor should only route
segments:

```rust
for segment in render_program.segments {
    match segment.kind {
        SegmentKind::TileLocal(program) => tile_vm.encode(program),
        SegmentKind::Barrier(program) => barriers.encode(program),
    }
}
```

This phase is a refactor phase. The success criterion is stable output and more
explicit planner diagnostics, not immediate speed.

### Phase 2: Keep Existing Semantics on the Typed Event Backend

Existing raster, mask, clipping, special-blend, point-filter, container, and
THROUGH event support should continue using the typed event backend. Avoid
adding a second event path for sparse patch reload.

Success criteria:

- existing guard samples remain stable
- tile-event ABI version changes only when the Interface changes
- reload manifests reject incompatible ABI versions instead of misusing cached
  patches

### Phase 3: Lower Special Blends as Events

The already modeled byte-domain special blends should remain tile events:

- Add Glow
- Color Burn
- Color Dodge
- Glow Dodge

Future special blends should only move from barrier to tile event when their
normal, preserve-alpha, and clipping-run resolve semantics are sample-backed.

### Phase 4: Lower Pointwise Filters

Pointwise filters should stay inside the event stream whenever their masks are
absent, proven opaque, or fully covered by resident R8 mask slots. Unsupported,
non-local, malformed, or provider-unavailable filters should remain explicit
barriers.

### Phase 5: Expand the Scope Stack

Add tile-local scope coverage in this order:

1. clipped container or folder siblings beyond the current filtered
   simple-child-stream subset
2. deeper simple container nesting when the scope-depth limit allows it
3. nested THROUGH cases with explicit before/after accumulator semantics
4. scope masks that span resident R8 mask slots
5. mixed scope children containing filters and raster-only clipping runs

The current clipped container/folder support reuses the simple-scope child
stream for raster children followed by pointwise filters, and keeps unsupported
or over-depth child subtrees as explicit barriers. The remaining work is
broader clipped container/folder subtrees: more nested scope positions,
raster-only clipping runs in more positions, THROUGH children, and other
non-direct-raster children. Do not solve those by only relaxing eligibility
checks.

### Phase 6: Add Session Atlas and Dirty Segment Reuse After Semantics Stabilize

The sparse atlas cache already exists in first form. Continue evolving it only
after the relevant event semantics are stable:

- update changed atlas regions in place
- rerun only dirty event ranges when the segment-before checkpoint is valid
- keep checkpoint storage selected and budgeted
- prefer GPU-resident or cropped checkpoints only when profiling proves the
  current CPU RGBA8 checkpoint path is the limiting factor

### Phase 7: Add Frame Arena and Buffer Reuse

Add ring-buffer and bind-group reuse when tile-event segments dominate render
execution. This is a fixed-cost optimization after the renderer has fewer
barriers; it is not the first lever.

## Test and Verification Policy

Keep the pass-heavy GPU renderer as a correctness oracle and debug backend, not
as a product fallback. New tile-local lowering should be verified against:

- the existing faithful GPU source execution path
- targeted CSP PNG guard samples when the semantic path has known fixtures
- `--performance-plan-json` barrier and segment-count deltas
- reload-manifest ABI and dirty-segment compatibility checks when reload state
  changes

Expected test shape:

- unit tests for planner lowering decisions and barrier reasons
- GPU executor tests comparing tile-event output to the legacy source path
- runtime provider tests for atlas slot reuse, mask coverage, and sparse
  affected-window lowering
- targeted CLI compares for samples affected by the semantic change

## Avoided Work

Do not continue optimizing the old "one layer or a few layers per pass"
renderer as the product architecture.

Do not hide unsupported tile-local semantics behind production fallback. The
product path should expose barriers so coverage can improve deliberately.

Do not tune Blender upload first when native worker timing shows semantic
barriers and intermediate work still dominate. Blender upload and pack timing
matter, but they are not the order-of-magnitude native-render lever.

Do not introduce full-canvas layer caches. Sparse tile atlas caches and selected
segment-before checkpoints are allowed because they are budgeted and keyed by
the reload manifest, not by unbounded layer textures.

## Target Code Shape

Long-term target:

```text
clip_runtime
  render_ir/
    semantic.rs
    segment.rs
    lower.rs
    stats.rs
  gpu_provider/
    atlas_cache.rs
    resource_fingerprint.rs
    tile_chunks.rs

clip_gpu
  tile_vm/
    event.rs
    buffers.rs
    shader.wgsl
    encoder.rs
  barriers/
    filter_pass.rs
    container_pass.rs
    through_pass.rs
  stream/
    frame_arena.rs
    pipeline_cache.rs
```

`stream_sequence.rs` should stay thin and should not become the place where
each new semantic optimization adds another opportunistic branch.
