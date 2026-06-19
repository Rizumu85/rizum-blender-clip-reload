# Native Tile-Event Renderer Roadmap

Last updated: 2026-06-19

## Purpose

This document records the next architecture direction for native renderer
performance. The goal is not another local pass optimization. The goal is to
make tile-local event execution the main native renderer model, with explicit
barriers only for `.clip` semantics that do not yet have a faithful tile-local
model.

The current renderer already proved the useful shape through sparse tile
decoding, atlas raster-run collapse, direct compressed tile events, mask atlas
events, clipped raster sibling events, and raster-only clipping-run events. The
next work should deepen that shape into a planner/executor architecture:

```text
strict CSP source selection
  -> render program planner
  -> tile-local event executor
  -> explicit barrier executor
```

## Mental Model Shift

Old model:

```text
per-source pass renderer is the main path
tile-silo is an optimization when an eligible run appears
```

New model:

```text
tile event renderer is the main path
barrier passes are temporary seams for semantics not yet lowered
```

This must not reduce fidelity. Every barrier must be explicit, countable, and
explainable. If a structure cannot be lowered faithfully, it remains a barrier
until the semantic model is recovered and guarded by tests.

## Architecture Targets

### Render Program Planner

`clip_gpu::stream_program` is the first seam. It should grow into the Module
that owns lowering decisions, barrier reasons, segment cost hints, and planner
statistics.

Current state:

- `TileLocal(RasterRun)`
- `TileLocal(RasterClippingRun)`
- `TileLocal(RasterFilterRun)`
- `TileLocal(PointFilterRun)`
- `TileLocal(SimpleContainerScope)` for the first narrow isolated-container
  subset
- `TileLocal(SimpleThroughScope)` for the first narrow THROUGH-group subset
- `Barrier(LegacySource)`

Target state:

```rust
struct RenderProgram {
    canvas: CanvasSize,
    segments: Vec<RenderSegment>,
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

The planner Interface should answer:

- Which source ranges became tile-local segments?
- Which source ranges became barriers?
- Why is each barrier still required?
- How many tile events, passes, atlas chunks, and intermediate targets are
  planned?

### Explainable Barrier Reasons

Replace scattered eligibility booleans with lowering decisions.

```rust
enum LoweringDecision {
    TileLocal { reason: &'static str, cost: CostHint },
    Barrier { reason: BarrierReason },
}

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

The same reasons should appear in performance-plan diagnostics. A slow file
should explain whether the remaining cost is dominated by containers, THROUGH
groups, pointwise filters, byte-domain blends, clipped container siblings, tile
occupancy, or Blender upload.

### Typed Tile Events

The current tile-silo event layout is a fixed ten-word raster payload. That is
correct for the current raster-run subset but too narrow for the main renderer
model. Move to versioned typed events before lowering more semantics.

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

Use separate typed payload storage:

- `event_headers`
- `raster_payloads`
- `clip_payloads`
- `filter_payloads`
- `tile_spans`

The tile program should carry an ABI version so CLI diagnostics, persistent
workers, and future reload manifests can identify incompatible event layouts.

### Session-Level Sparse Atlas Cache

Run-local atlases are a useful stepping stone, but the target renderer should
own session-level sparse atlas resources. This is not a global full-canvas
layer texture cache. It is a sparse tile atlas cache keyed by source identity
and compressed tile fingerprints.

Target shape:

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

Reload behavior:

- unchanged graph plus unchanged compressed tile -> reuse atlas slot
- changed tile -> update only that atlas region
- unused tile -> generation/LRU reclamation

Do this after the event model is stable. Adding long-lived caches too early
will cache incorrect semantics and make debugging harder.

### Independent Mask Tile Resources

Current mask atlas events are cleaner than CPU pre-applied masks, but runtime
still creates matching mask chunks per raster chunk. The target model should
make masks independent tile resources:

- raster compressed tile -> RGBA atlas slot
- mask compressed tile -> R8 atlas slot
- event references raster slot plus mask slot/fill
- shader samples mask by canvas/global coordinates
- multiple raster events can reference the same mask atlas tile

This removes duplicate CPU mask-pixel generation and better matches Live2D
files with large masks and sparse raster chunks.

### Pointwise Filters as Tile Events

Supported adjustment/filter layers should not remain barriers when their
semantics are pointwise:

```text
out(x, y) = f(in(x, y), filter_payload, mask(x, y), opacity)
```

Initial candidates:

- Brightness/Contrast
- Level Correction
- Tone Curve
- HSL
- Color Balance
- Invert/Reverse Gradient
- Posterization
- Threshold
- Gradient Map

Non-local filters should remain barriers until faithfully modelled.

### Tile-Local Scope Stack

Containers and THROUGH groups need tile-local scopes rather than direct
one-shot lowering. The planner should express scope operations and refuse
lowering when limits are exceeded.

Target concepts:

```rust
enum ScopeOp {
    PushTransparentWhite,
    PushCopyParent,
    PopResolve { opacity, blend, mask },
    BeginClipBase,
    ResolveClipBase,
}
```

The shader can maintain a bounded set of per-pixel accumulators. The planner
decides whether a container or THROUGH group is tile-local by checking:

- finite bounds
- event count limit
- scope depth limit
- absence of non-local filters
- absence of not-yet-lowered special blends

Unsupported scope shapes remain explicit barriers with reasons.

### Frame Arena

When tile events become the main execution path, per-segment buffer creation
will become a fixed tax. Introduce a render-frame arena after the typed event
model exists:

```rust
struct GpuFrameArena {
    event_u32_buffer: RingStorageBuffer<u32>,
    work_u32_buffer: RingStorageBuffer<u32>,
    span_u32_buffer: RingStorageBuffer<u32>,
    params_buffer: RingUniformBuffer<Params>,
    bind_group_cache: BindGroupCache,
}
```

Small buffers should append into ring buffers, segments should refer to
offset/length ranges, and the arena should reset after a render.

### Dirty Segment Reload

Reload should eventually move from final dirty rectangles to dirty graph,
resource, segment, and tile-event state:

- graph node fingerprint
- resource tile fingerprint
- segment fingerprint
- tile work-list fingerprint

Reload examples:

- source tile changed -> update atlas slot, dirty affected tile events,
  rerender affected segments, read back affected output tiles
- layer opacity changed -> reuse atlas, update event payload, rerender affected
  segments
- container structure changed -> rebuild segment graph, possibly promote to a
  full or subtree render

This should come after the typed event VM and session atlas cache are stable.

## Implementation Phases

### Phase 0: Performance Plan JSON

Done in first form. The diagnostic command is:

```powershell
clip_cli <file.clip> --performance-plan-json
```

Expected output shape:

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
    "ScopeMaskNotLowered": 6,
    "ScopeDepthLimitExceeded": 2,
    "TileEventLimitExceeded": 1,
    "ThroughGroupNotLowered": 8,
    "FilterNotLowered": 15
  },
  "compressed_raster_tiles": 4826,
  "mask_tiles": 2052,
  "atlas_upload_bytes": 123456789,
  "estimated_tile_events": 18000
}
```

This makes performance work measurable instead of sample-by-sample intuition.
The current implementation combines actual `clip_gpu::stream_program`
planning stats with metadata/block-level compressed tile occupancy. The
`atlas_upload_bytes` value is an estimate based on sparse compressed tile slots,
not a measured GPU upload counter.

### Phase 1: Render Segment IR

Done in first form: `clip_gpu::stream_program` now plans raster-run,
raster-only clipping-run, and legacy-source barrier segments, while
`stream_sequence.rs` executes segments.

Second form: `stream_program_lowering.rs` now owns the first-class
`LoweringDecision` interface for the next source range. The planner consumes
tile-local or barrier decisions with source span and cost hints instead of
directly branching on tile-silo eligibility helpers.

Next deepening:

- keep expanding explicit `BarrierReason` coverage as new semantics are lowered
- continue moving boolean eligibility helpers behind lowering-decision modules
  as new segment kinds are added
- attach cost hints and barrier reasons to future segment kinds, not only the
  current legacy-source barrier

### Phase 2: Typed Tile Event VM

Move existing raster/mask/clipping event support from the fixed ten-word raster
layout to a versioned typed event backend. Do not add new semantics in this
phase. The success criterion is stable pixels and no performance regression for
existing tile-local paths.

Started in first form: `stream_tile_event.rs` now defines
`TILE_EVENT_ABI_VERSION`, `TileEventKind`, typed event headers, and raster event
payloads. Existing raster tile-silo paths still upload the legacy 10-word shader
buffer, but that buffer is now generated through the typed raster event adapter.
`--performance-plan-json` reports the tile event ABI version so future workers
and reload manifests can detect incompatible tile programs.

Second form: the tile-silo shader now consumes separate event-header and
raster-payload storage buffers. That first typed event VM supported only
`TileEventKind::Raster`, preserving existing raster-run, clipped-raster, and
raster-only clipping-run semantics while removing direct shader dependence on
the fixed event-index-to-10-word layout.

Third form: byte-domain special blend rasters now use
`TileEventKind::SpecialBlendRaster` headers with the existing raster payload
storage. `TILE_EVENT_ABI_VERSION` is `2`; the tile VM still executes the same
ordered per-tile raster payload stream, but the header now carries enough
semantic information for performance-plan/debug output and future payload
splits.

Fourth form: `TileEventKind::PointFilter` now has a separate
`filter_payloads` storage buffer and LUT atlas binding. `TILE_EVENT_ABI_VERSION`
is `3`. Existing raster-only paths bind an empty filter payload buffer and a
dummy LUT texture, while raster/filter tile-local segments bind real filter
payloads and LUT rows.

Fifth form: `TileEventKind::BeginContainer` and
`TileEventKind::EndContainer` now have a separate `scope_payloads` storage
buffer. `TILE_EVENT_ABI_VERSION` is `4`. The tile VM can maintain a bounded
transparent-white local scope stack and resolve it back to the parent
accumulator for the simple unmasked isolated-container subset, including
positive container opacity and modeled resolve blend modes.

Sixth form: `TileEventKind::BeginThrough` and `TileEventKind::EndThrough` use
the same `scope_payloads` storage buffer. `TILE_EVENT_ABI_VERSION` is `5`. The
tile VM can capture the parent accumulator as a local THROUGH `before`, render
eligible child events into a local THROUGH `after`, and resolve `before` /
`after` through the existing premultiplied THROUGH opacity formula.

Seventh form: scope payload words 6/7 now carry an optional R8 mask atlas
origin for container and THROUGH scope resolves. `TILE_EVENT_ABI_VERSION` is
`6`. The tile VM samples that mask over the scope's local bounds, multiplies
container resolve source alpha by the mask, and multiplies THROUGH resolve
strength by the mask, matching the existing faithful pass shaders.

Eighth form: `TileEventKind::BeginClipBase`,
`TileEventKind::ClippedRaster`, and `TileEventKind::ResolveClipBase` now allow
the VM to express a raster-only clipping run inside a scope-local event stream.
`TILE_EVENT_ABI_VERSION` is `8`. The implemented planner subset uses these
events for clipping runs inside simple container scopes and direct clipping-run
children inside simple THROUGH scopes when the clipping base and clipped
siblings are atlas-eligible rasters. This includes raster masks, layer opacity,
and non-Normal clipping-base blend modes because the VM's Normal raster alpha
path now follows the same byte-domain opacity and mask arithmetic as the
faithful legacy pass. Clipped container/folder siblings and clipping runs
nested through another scope inside THROUGH remain barriers until their
cache-writeback semantics are modelled and guarded.

Ninth form: eligible pure raster sources lower to `TileEventKind::Raster` even
when there is only one source in the segment. This removes the old
`RasterRunTooShort` optimisation gate from the product render plan for
atlas-eligible rasters and makes tile events the default execution model for
ordinary rasters, not only for multi-source raster runs.

Remaining Phase 2 work:

- add explicit typed event readers for additional clipping/THROUGH payloads
  only when those semantics are ready to lower
- keep guard samples stable as new event kinds are added

### Phase 3: Byte-Domain Special Blend Events

Status: implemented for the currently supported raster byte-domain blends.

Lowered from explicit `ByteDomainBlendNotLowered` barriers into tile-local
events:

- Add Glow
- Color Burn
- Color Dodge
- Glow Dodge

The tile VM now reuses the existing verified byte-domain formulas for:

- normal raster compositing
- clipped preserve-alpha compositing
- raster-only clipping-run resolve through a special-blend base

At the end of this phase, `--performance-plan-json` reported
`tile_event_abi_version: 2`. Files whose only previous barriers were these
special blends should no longer report `ByteDomainBlendNotLowered`; for
example `IllustrationBlendModesB.clip` planned one raster-run tile-local
segment plus the Paper/SolidColor barrier.

### Phase 4: Pointwise Filter Events

Lower supported pointwise adjustment/filter layers into tile events. Keep
non-local filters as explicit barriers.

Status: implemented for raster/filter mixed runs and filter-only runs whose
filter mask is absent, proven fully opaque from `.clip` mask metadata and
compressed tile inspection, or available through provider-backed R8 mask atlas
chunks.

Implemented shape:

- `TileProgramKind::RasterFilterRun` lowers source ranges such as
  `raster, filter, raster` and `filter, raster` into one tile-local segment
  instead of raster segment plus filter barrier/pass.
- `TileProgramKind::PointFilterRun` lowers consecutive filter-only source
  ranges into one tile-local segment, applying them to the current dirty
  parent accumulator without requiring a raster event in the same segment.
- `TileEventKind::PointFilter` carries LUT row, opacity, filter mode, HSL
  parameters, local dirty bounds, and optional R8 mask atlas coordinates in
  `filter_payloads`.
- The tile VM applies Tone Curve, HSL, Threshold, and Gradient Map filter modes
  to the per-pixel accumulator in event order, using the same formulas as the
  existing LUT filter pass.
- Leading filters in a mixed run operate on the parent accumulator over the
  current target bounds before later raster events in the same segment.
- Runtime providers expose `mask_is_fully_opaque()` and provider-backed R8 mask
  atlas chunks. A filter with a default all-opaque mask can lower without
  sampling a mask; a real non-opaque filter mask lowers by sampling the shared
  tile mask atlas. Unknown or provider-unavailable filter masks remain
  `FilterNotLowered`.

Current diagnostics:

- `Test_ToneCurve.clip --performance-plan-json` now reports one
  `raster_filter_run_segments` segment, no filter barrier, and one planned pass.
- `Test_Gradiation.clip --performance-plan-json` also reports one
  `raster_filter_run_segments` segment and no filter barrier.
- `Test_HSL2.clip` lowers the raster plus HSL filter, while its Paper source
  remains a `SolidColorNotLowered` barrier.

Verification:

- `Test_ToneCurve` exact.
- `Test_HSL2` exact.
- `Test_HSL3`, `Test_HSL4`, and `Test_HSL5` keep the existing one-LSB
  non-visible residual shape.
- `Test_Gradiation` keeps the known `raw_max=10` / `premul_max=10` residual.
- GPU unit coverage compares a leading filter followed by a raster against the
  existing legacy source path.
- GPU unit coverage also locks a filter-only tile segment after a legacy
  source, proving a standalone pointwise filter run can consume the previous
  segment's dirty accumulator.
- GPU unit coverage compares provider-backed masked filter-only and
  masked-filter-inside-container tile events against the existing legacy source
  path.
- `Test_AddGlowMultiply` and `Test_ClippingEdge` guards remain stable.

Remaining Phase 4 work:

- non-local or future unsupported filters whose faithful tile-local model is
  not defined

### Phase 5: Container and THROUGH Scope Stack

Status: started in first form for simple isolated containers and simple THROUGH
groups.

Implemented subset:

- `TileProgramKind::SimpleContainerScope` lowers a `Container` source only when
  container opacity is positive, the container mask is absent, proven fully
  opaque, or available through provider-backed R8 mask atlas chunks, the
  resolve blend mode is modeled by the tile VM, bounds are known and intersect
  the current target, and children are limited to eligible raster events,
  simple container scopes with the same scope-mask support up to
  `SIMPLE_CONTAINER_SCOPE_DEPTH_LIMIT`, plus pointwise filters whose masks are
  absent, proven fully opaque, or available through provider-backed R8 mask
  atlas chunks, plus simple THROUGH scope children within the remaining scope
  depth budget, plus raster-only clipping-run child subsets whose base and
  clipped siblings are atlas-eligible rasters.
- The shader handles `BeginContainer` / `EndContainer` events by rendering
  child events into a transparent-white local accumulator, then resolving that
  local result into the parent accumulator through the same Normal,
  byte-domain special-blend, or standard blend helpers used by existing raster
  events.
- Nested simple containers use a fixed four-level transparent-white local
  accumulator stack. Each inner container resolves into its parent accumulator
  before the outer container resolves to its parent. Container depth beyond the
  fixed limit remains a barrier.
- A simple THROUGH child inside a simple container stack can lower by capturing
  the current active container accumulator as the THROUGH `before`/`after`
  pair, rendering THROUGH children into `after`, then resolving back into that
  same container accumulator. THROUGH children inherit the current remaining
  container scope-depth budget instead of resetting it.
- A simple raster-only clipping run child lowers as `BeginClipBase`,
  clip-base raster events, clipped-raster preserve events, and
  `ResolveClipBase`, resolving back into the active container accumulator.
  Masked rasters, layer opacity, and non-Normal clipping-base blend modes can
  lower when the provider can supply the needed atlas/mask tiles.
- Clipped container/folder siblings, solid colors, unavailable container masks,
  and provider-unavailable or unknown filter masks still remain explicit
  legacy barriers.

Implemented THROUGH subset:

- `TileProgramKind::SimpleThroughScope` lowers a `ThroughGroup` source only
  when group opacity is positive, the THROUGH mask is absent, proven fully
  opaque, or available through provider-backed R8 mask atlas chunks, bounds are
  known and intersect the current target, and children are limited to eligible
  raster events, simple container scopes with the same scope-mask support, plus
  pointwise filters whose masks are absent, proven fully opaque, or available
  through provider-backed R8 mask atlas chunks.
- The shader handles `BeginThrough` / `EndThrough` events by copying the current
  parent accumulator into a local `before` and `after`, rendering child events
  into `after`, then resolving `before` and `after` with the same premultiplied
  opacity interpolation used by the existing THROUGH pass.
- Simple containers inside a THROUGH scope can lower as nested
  `BeginContainer` / `EndContainer` events up to the same fixed depth limit,
  and the outermost container resolves into the THROUGH `after` accumulator.
- Nested THROUGH groups can lower one level deeper when the nested THROUGH has
  positive opacity, has the same supported scope-mask shape, has known
  intersecting bounds, and its children fit the same raster/container/
  pointwise-filter subset. The shader keeps a bounded two-level THROUGH
  `before`/`after` stack, resolves the inner THROUGH into the outer `after`
  accumulator, and floor-quantizes the inner resolve to match the intermediate
  RGBA8 writeback of the existing pass-heavy path.
- A direct raster-only clipping-run child can lower inside a simple THROUGH
  scope as `BeginClipBase`, clip-base raster events, `ClippedRaster` preserve
  events, and `ResolveClipBase`, resolving the completed clip-base cache into
  the THROUGH `after` accumulator through the base blend mode.
- Deeper nested THROUGH groups, clipping runs nested inside containers or
  nested THROUGH groups, clipped container/folder siblings, solid colors,
  unavailable THROUGH masks, container depth beyond the fixed limit, and
  provider-unavailable or unknown filter masks still remain explicit legacy
  barriers.

Verification:

- A GPU unit test distinguishes isolated-container semantics from direct
  through compositing by placing a Multiply raster inside a Normal folder over
  an opaque gray background. The expected isolated result is the source colour;
  the direct-through result would be darker.
- GPU unit tests compare the tile-scope path against the existing legacy
  source path for Normal container opacity, Multiply container resolve, and
  Multiply container resolve with non-1 opacity.
- GPU unit tests compare direct, three-deep, and four-deep nested containers
  against the existing legacy source path and assert five-deep nesting remains
  a barrier.
- GPU unit tests compare a direct THROUGH child inside a container scope
  against the existing legacy source path, proving THROUGH resolves back into
  the container accumulator rather than the parent canvas.
- GPU unit tests compare a THROUGH child inside a nested container scope
  against the existing legacy source path. Planner tests lock a THROUGH child
  at the fixed fourth container-scope depth as tile-local, while deeper stacks
  remain `ScopeDepthLimitExceeded`.
- GPU unit tests compare a raster-only clipping run inside a supported Normal
  container scope against the existing legacy source path.
- GPU unit tests compare simple THROUGH tile-scope execution against the
  existing legacy source path for THROUGH opacity and child blend execution.
- GPU unit tests compare a simple container inside a THROUGH scope against the
  existing legacy source path, proving nested container resolve goes to the
  THROUGH `after` accumulator.
- GPU unit tests compare a fractional-opacity nested THROUGH scope against the
  existing legacy source path and assert nesting beyond the fixed limit remains
  a barrier.
- GPU unit tests compare a direct raster-only clipping run inside a simple
  THROUGH scope against the existing legacy source path, while planner tests
  keep clipping runs nested inside a container inside THROUGH as
  `ThroughGroupNotLowered`.
- Planner and GPU unit tests prove masks that are explicitly known to be fully
  opaque do not block simple container/THROUGH lowering, and GPU unit tests now
  compare non-opaque masked container and THROUGH scope resolves against the
  existing legacy source path. Unknown or provider-unavailable scope masks
  remain `ScopeMaskNotLowered` barriers.
- Planner and GPU unit tests prove provider-backed non-opaque pointwise filter
  masks can lower inside scope stacks, while provider-unavailable filter masks
  remain `FilterNotLowered`.
- Planner unit tests classify scope stacks beyond the fixed accumulator depth
  as `ScopeDepthLimitExceeded`, and scope programs whose event count exceeds
  `MAX_SILO_EVENTS` as `TileEventLimitExceeded`.
- `Test_FolderNested.clip --performance-plan-json` reports
  `simple_through_scope_segments: 1` and `tile_event_abi_version: 8`.
- `Test_Clipping`, `Test_ClippingEdge`, `Test_FolderNested`, `Test_ToneCurve`,
  and `Test_AddGlowMultiply` remain stable.

Current `Ref_Terra404_Live2D.clip --performance-plan-json` after the four-level
container scope stack:

- `planned_passes=481`
- `tile_local_segments=436`
- `barrier_segments=45`
- `tile_event_abi_version=8`
- remaining barriers:
  - `ThroughGroupNotLowered=36`
  - `IsolatedContainerRequiresIntermediate=4`
  - `ScopeDepthLimitExceeded=3`
  - `ClippingRunNotLowered=2`

The four-level stack extends the faithful tile-local scope model and removes
the previous synthetic fourth-container depth barrier, but it does not change
Terra's current plan. Terra's remaining cost is still dominated by unsupported
THROUGH shapes rather than ordinary four-deep isolated containers.

Guard comparisons remain stable after the planner/executor change:
`Test_ClippingEdge` exact, `Test_ToneCurve` exact, `Test_AddGlowMultiply`
`raw_max=1` / `premul_max=1` / visible `0`, `Test_RealArt` unchanged at
`premul_visible_px=28`, and `Ref_Terra404_Live2D` unchanged at
`premul_visible_px=13501`.

Next scope-stack work:

- nested/complex container and THROUGH subtrees that still hit depth or shape
  barriers
- broader clipping-run shapes and clipped container/folder siblings inside
  scope stacks

Then extend the same scope-stack model to each remaining scope shape once it
has focused parity tests.

### Phase 6: Session Atlas Cache and Dirty Segment Reload

Add session-level sparse atlas allocation and segment/tile-event dirty reload
after the typed event model is stable.

First form: reload manifests now carry render-program identity, not only graph
and source-tile identity. `clip_gpu::stream_program_inspect` exposes a stable
inspection Interface with planner stats plus a preorder list of render segment
records. `clip_runtime` reload manifest ABI `2` stores:

- `tile_event_abi_version`
- segment ordinal and nesting depth
- source-range span
- tile-local or barrier kind
- optional barrier reason
- expected pass, tile-event, and legacy-source cost hints
- a segment signature that includes the tile-event ABI

The reload planner currently promotes to full render when the tile-event ABI or
segment plan changes. This is deliberate: it prevents patch reload from reusing
old manifests after shader event layout or lowering-plan changes. It also gives
future dirty-segment reload a durable data shape to refine, instead of bolting
segment invalidation onto final dirty rectangles later.

This first form is not yet the session-level sparse atlas allocator. It does
not reuse atlas slots across reloads, and it does not yet compute affected
tile-event ranges from segment/resource differences.

Next Phase 6 work:

- add tile work-list fingerprints to the segment manifest
- map changed source tiles to affected segment/tile-event ranges
- introduce a session-level sparse atlas allocator keyed by compressed tile
  fingerprints
- update changed atlas regions in place and rerun only affected segments when
  the segment graph is unchanged

## Correctness Policy

Do not introduce approximate semantics for speed. If a structure cannot be
lowered faithfully, keep it as a barrier.

Do not maintain two production renderers. The existing pass-heavy GPU path can
remain as a test oracle and debug compare backend while the product path moves
toward tile-event execution.

Do not use a global full-canvas layer texture cache. Sparse compressed-tile
atlas caching is allowed; full-canvas layer caching is not.

Do not prioritize Blender upload tuning over native tile-local execution.
Blender-side upload and pack policy still matter, but the order-of-magnitude
native path is reducing semantic barriers and pass/intermediate churn.

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

`stream_sequence.rs` should stay thin:

```rust
for segment in render_program.segments {
    match segment.kind {
        SegmentKind::TileLocal(program) => tile_vm.encode(program),
        SegmentKind::Barrier(program) => barriers.encode(program),
    }
}
```

That is the architectural goal: tile-local event execution as the default
model, explicit barriers for not-yet-lowered faithful semantics, and planner
diagnostics that explain remaining performance limits.
