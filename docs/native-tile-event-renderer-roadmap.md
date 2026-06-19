# Native Tile-Event Renderer Roadmap

Last updated: 2026-06-19

## Purpose

This document records the next architecture direction for native renderer
performance. The goal is not another local pass optimization. The goal is to
make tile-local event execution the main native renderer model, with explicit
barriers only for `.clip` semantics that do not yet have a faithful tile-local
model.

The forward-looking main-execution plan lives in
`docs/native-tile-event-main-execution-plan.md`, and the implementation
strategy lives in `docs/native-tile-event-architecture-strategy.md`. This
roadmap remains the durable phase record for what has been implemented,
verified, and deliberately left as an explicit barrier.

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

Second form: segment manifests now carry a compact tile work-list surface.
`clip_gpu::stream_program_inspect` reports the raster and mask resources used
by each segment. `clip_runtime` joins those resource refs against the existing
compressed source-tile manifest and stores:

- segment resource refs
- tile work-list source count
- tile work-list compressed tile count
- tile work-list signature
- compact source tile refs for the segment

The reload planner now also emits `dirty_segments` for patch plans. A changed
compressed source tile is matched to the segment work-list tile refs, while
raster semantic/resource metadata changes mark segments through their resource
refs. Product rendering still uses the existing dirty-rectangle patch path;
`dirty_segments` is a planning diagnostic and the data seam future segment
rerender/cache invalidation will consume.

This second form still does not reuse atlas slots across reloads and does not
yet rerun only affected segment/tile-event ranges. It makes the affected
segment set explicit so the cache allocator and segment executor can be wired
without re-deriving source/resource/tile ownership at the worker layer.

Third form: tile work-list refs now carry event ranges. Reload manifest ABI `4`
adds `event_start` / `event_end` to each compact segment tile ref, and patch
plans include `dirty_event_ranges` for every dirty segment.

The current range model is exact for the simple source-backed raster tile
events in `RasterRun` and `RasterClippingRun` segments. For segment shapes where
the manifest does not yet expose per-event ownership, such as scope, filter,
or mask-driven invalidation, the range is deliberately conservative and covers
the whole segment. This keeps the Interface executable for a future segment
executor without claiming narrower invalidation than the current planner can
prove.

Product rendering still uses dirty rectangles. The added event ranges are the
planner surface for the next implementation step: a segment executor that can
rerun only affected event ranges after an atlas slot update when the segment
graph is unchanged.

Fourth form: `clip_runtime::gpu_provider::atlas_cache` now provides the first
session-level sparse atlas allocator Interface. It is keyed by logical
source-tile identity plus compressed tile fingerprints, so an unchanged tile
reuses its atlas slot, a changed compressed payload updates the same slot, and
unused tiles are reclaimed by generation. The planner entrypoint consumes the
reload manifest's segment tile work-list when available, falling back to source
tiles only for older or synthetic manifests. `RuntimeGpuRenderer` owns this
allocator as session state, and the persistent Blender worker writes
`sparse_atlas_cache` diagnostics with cached tile count, inserted/reused/
changed/evicted tile counts, atlas count, resident bytes, and upload bytes.

This fourth form still does not bind those atlas slots to GPU textures or use
them for drawing. It is the planner/cache state seam required before the render
executor can update atlas regions in place and rerun only dirty segment/event
ranges.

Fifth form: `clip_runtime::gpu_provider::atlas_rerun` now maps sparse atlas
updates plus `ReloadDiffPlan.dirty_segments` into rerunnable segment work. For
patch reloads, the planner emits each dirty segment's event ranges and the
non-reused atlas slots that intersect those event ranges. Resource-only
invalidations can still produce a rerunnable segment with no atlas upload, while
no-change and full-render plans intentionally produce no rerunnable segment
work. The persistent Blender worker includes these rerunnable segment entries
inside `sparse_atlas_cache.rerunnable_segments` diagnostics.

This fifth form still does not execute the segment rerun or read back
segment/tile output. It is the executor Interface for the next step: updating
changed atlas regions in place and using the mapped event ranges to rerun only
the affected tile-local segment work when the segment graph is unchanged.

Sixth form: `clip_gpu::sparse_atlas` now provides the first real GPU sparse
atlas texture pool. The pool is keyed by atlas format plus atlas id, creates
resident atlas textures on demand, validates chunk payload sizes and atlas
bounds, and applies in-place region updates through `Queue::write_texture`.
`GpuSparseAtlasTexturePoolStats` reports created atlas count, updated chunk
count, upload bytes, resident atlas count, and resident bytes. Runtime
rerunnable slot diagnostics now carry the atlas format and atlas texture size
needed to feed this pool without re-deriving executor details from source kind.

This sixth form still does not decode changed `.clip` chunks into pool update
chunks, bind pooled atlas textures in the tile-silo pass, rerun the mapped
segment event ranges, or read back only those updated output tiles. It is the
GPU resource layer required by that executor step.

Seventh form: `clip_runtime::gpu_provider::atlas_upload` now decodes
non-reused sparse atlas cache updates into `GpuSparseAtlasTexturePoolUpdate`
values and can feed them into the GPU sparse atlas texture pool through
`RuntimeGpuRenderer::prepare_sparse_atlas_textures()`. Raster updates decode the
visible source rect into RGBA8 chunks; mask updates decode the visible source
rect into R8 chunks. The source rect is derived from the reload manifest's
canvas-space tile rect plus the source offset, not only from tile index, so
off-canvas tiles update the correct source sub-rectangle. This path is covered
by tests for full-cache warmup, no-change reuse, patch update upload, and
off-canvas source rect recovery.

This seventh form is intentionally not called by the Blender worker product
path yet. It proves the decoded update stream can populate the resident GPU
pool, but the renderer still needs a tile-local executor path that binds pooled
atlas textures and reruns mapped event ranges before the product reload path can
use it without extra unused work.

Eighth form: rerunnable segment planning now exposes the full resident sparse
atlas slot surface for each dirty event range, not only the non-reused slots
that need upload. `resident_slots` includes reused, changed, and inserted
raster/mask slots intersecting the segment's dirty event ranges; `updated_slots`
remains the upload-only subset. This is the executor Interface needed to build
tile-event payloads from long-lived atlas slots while updating only changed
regions in the GPU pool. The Blender worker sparse-atlas diagnostics are
formatted through a small `blender_worker_sparse` Module and now include
`resident_slots` beside `updated_slots`.

Ninth form: `clip_gpu::sparse_atlas_executor` now provides the first real
pooled-atlas tile-event executor seam. `GpuRenderer` can bind a resident
`GpuSparseAtlasTexturePool` RGBA atlas, optionally bind a resident R8 mask atlas
or a dummy opaque mask atlas, build typed raster tile-event payloads from
`GpuSparseAtlasRasterEvent`, and run the existing `tile_silo.wgsl` shader to
produce an RGBA output. The current executor slice deliberately supports only
one raster atlas key and one optional mask atlas key per pass, because the
current tile-silo bind layout exposes one RGBA atlas texture and one R8 mask
atlas texture. Missing atlas textures, wrong atlas formats, and mixed atlas keys
fail explicitly instead of falling back to the legacy renderer.

Tenth form: runtime rerunnable slots now carry the executor coordinates needed
to build actual tile events: `event_start`, `event_end`, `canvas_x`, and
`canvas_y`, in addition to source rect, atlas rect, atlas format, and atlas
texture size. `clip_runtime::gpu_provider::atlas_events` lowers dirty
`RasterRun` resident slots plus the current selected `GpuNormalStackSource`
metadata into `GpuSparseAtlasRasterEvent` values, preserving source opacity and
blend mode. Masked raster events lower only when a matching resident R8 mask
slot covers the same canvas rect as the raster slot; otherwise the segment is
reported as an explicit skipped event-plan reason instead of silently dropping
mask semantics. `RuntimeGpuRenderer::prepare_sparse_atlas_raster_event_plan()`
performs one cache generation, decodes/uploads non-reused atlas chunks into the
resident GPU pool, and returns the typed raster event plan. The companion
`draw_sparse_atlas_raster_event_segment_to_rgba8()` adapter can execute one
prepared raster event segment against the resident pool for the next rerun
step.

This tenth form still does not compose rerun segment output back into the
previous full image or read back only dirty output tiles for Blender patch
reload. It turns the Phase 6 data seam from diagnostics into executable event
payloads, but product reload still uses the existing dirty-rectangle path.

Eleventh form: prepared raster event segments now split into ordered
executor-compatible batches through `clip_gpu::sparse_atlas_batches`. A batch
may contain only one resident RGBA atlas key and at most one resident R8 mask
atlas key, while unmasked events can share a batch with masked events because
their payloads do not sample the mask binding. The splitter preserves event
order by producing contiguous batches; it does not group non-contiguous events
by key. `GpuRenderer::draw_sparse_atlas_raster_event_batches_to_rgba8()` can
execute those batches in one command encoder with two output textures used as a
ping-pong pair. Multi-batch execution uses full-canvas passes so each batch
copies the previous batch's pixels through `dest_texture` even outside the
current batch's source bounds, avoiding loss of earlier atlas-key batches.

Twelfth form: `GpuRenderer::draw_sparse_atlas_raster_event_batches_over_rgba8()`
can now execute prepared sparse-atlas raster batches over a supplied RGBA8 base
accumulator. The base is the segment-before checkpoint for the segment being
rerun, not the old final image; using the old final image would double-compose
the changed segment. The executor validates the base buffer size, writes it into
the first ping-pong texture, clears the alternate target, and then runs every
batch over full-canvas bounds so unchanged pixels from the base survive outside
the dirty event bounds. Empty batch input returns the base pixels unchanged.
`RuntimeGpuRenderer::draw_sparse_atlas_raster_event_segment_over_rgba8()` exposes
the same seam to the runtime adapter.

This twelfth form still does not persist segment-before checkpoints or read
back only dirty output tiles. It provides the faithful executor primitive that
the product patch path needs before it can rerun dirty raster segments without
falling back to full output recomposition.

Thirteenth form: `clip_gpu::readback` now owns RGBA8 texture readback layout
and exposes region readback for internal GPU paths. The sparse-atlas executor
uses that seam through
`GpuRenderer::draw_sparse_atlas_raster_event_batch_patches_over_rgba8()`, which
draws prepared sparse-atlas raster batches over a supplied segment-before RGBA8
base accumulator and copies only the requested dirty rects back to CPU memory.
Empty batch input returns the same dirty rect payload cut from the supplied
base accumulator, preserving patch protocol semantics without launching a GPU
pass. `RuntimeGpuRenderer::draw_sparse_atlas_raster_event_segment_patches_over_rgba8()`
exposes the same dirty-payload adapter to runtime callers.

This thirteenth form still does not decide or persist the valid
segment-before checkpoint. It only proves the executor/readback half of dirty
segment reload: once a checkpoint exists, a dirty raster segment can produce the
same patch payload shape the Blender worker already consumes without reading
back the full canvas.

Fourteenth form: `clip_runtime::gpu_provider::atlas_rerun` can now build a
suffix rerun window from the earliest dirty segment to the end of the render
program. This is the correct checkpoint shape: the base accumulator must be the
state before the first segment in that suffix, and the suffix must include
unchanged later segments so their compositing still applies after the changed
segment. `clip_runtime::gpu_provider::atlas_events` exposes
`sparse_atlas_raster_suffix_event_plan()`, and
`RuntimeGpuRenderer::prepare_sparse_atlas_raster_suffix_patch_plan()` prepares
the resident sparse-atlas textures plus executable raster-event batches for
that suffix. The current executable subset is intentionally strict: suffix
segments lower only when every segment in the window is a `RasterRun`; later
container/scope/filter/barrier segments appear as explicit skipped segments
instead of being silently ignored.

This fourteenth form still does not provide the base accumulator. If the suffix
starts at source index 0, the eventual product route can use the transparent
initial accumulator. Otherwise the renderer still needs to persist or
reconstruct the segment-before checkpoint before calling the suffix patch
executor.

Fifteenth form: `RuntimeGpuRenderer::draw_sparse_atlas_initial_suffix_patches()`
now exposes the first product-safe sparse suffix patch route. It only attempts
execution for patch reloads whose earliest dirty segment starts at source index
0 and whose suffix manifest is entirely `RasterRun` segments. That lets the
renderer use CSP's initial transparent-white accumulator as the valid
segment-before checkpoint, upload only non-reused sparse atlas chunks, execute
the suffix event batches against the resident sparse atlas pool, and read back
only the dirty rect payload expected by the Blender worker. The method returns
the sparse-atlas diagnostics from the same cache generation it executed, so the
worker does not pre-plan the cache and accidentally convert required uploads
into apparent reuse before the GPU pool is populated.

The Blender worker patch path now tries this safe sparse suffix route first.
If it is ineligible, it falls back to the existing region patch renderer and
records `reload_diff.patch_renderer` as either
`sparse_atlas_initial_suffix`, `region`, or `full_render_patch_extract`.
General dirty segment reload still requires a persisted or reconstructed
segment-before checkpoint; the initial-accumulator route is intentionally only
the safe suffix subset.

Sixteenth form: `RuntimeGpuRenderer::draw_sparse_atlas_reconstructed_suffix_patches()`
now adds the first reconstructed checkpoint route. For patch reloads whose
dirty suffix is entirely `RasterRun` segments but starts after source index
0, the runtime renders the unchanged source prefix through the existing
faithful GPU provider to reconstruct the segment-before RGBA8 accumulator,
then executes the resident sparse-atlas suffix event batches over that
checkpoint and reads back only the requested dirty rect payload. The Blender
worker tries this after the initial-accumulator route and records
`reload_diff.patch_renderer` as `sparse_atlas_reconstructed_suffix` when it is
used.

This is a correctness seam, not the final performance shape: reconstructing
the prefix is still full-canvas prefix work. The next milestone is to persist
or cheaply reconstruct selected segment-before checkpoints across reloads, so
the same suffix executor can avoid rerendering unchanged prefixes.

Seventeenth form: `clip_runtime::gpu_api::checkpoint` now owns segment-before
checkpoint reconstruction and caching. `RuntimeGpuRenderer` keeps a session
checkpoint cache keyed by the current reload manifest's canvas, root layer,
tile-event ABI, checkpoint `source_start`, all node signatures, and the prefix
segment signatures up to that source boundary. A changed suffix tile work-list
does not invalidate the prefix checkpoint key, while a changed prefix segment
does. The reconstructed sparse suffix route now asks this module for the
checkpoint; on a key hit it reuses the cached RGBA8 accumulator instead of
rendering the unchanged prefix again. This is deliberately a selected
segment-before accumulator cache, not a global per-layer full-canvas texture
cache.

Eighteenth form: the checkpoint cache is now a small budgeted LRU set instead
of a single slot. The default cache keeps up to two RGBA8 checkpoints within a
512 MiB budget, skips checkpoints larger than the budget, updates recency on
hits, and evicts least-recently-used entries by count or byte budget. This
lets repeated edits around different suffix boundaries reuse more than one
valid segment-before accumulator without turning the renderer into an
unbounded full-canvas cache.

This eighteenth form is still CPU RGBA checkpoint storage. The next
improvement is to make checkpoint selection explicit in the render program,
then move toward GPU-resident checkpoints where that is safe and measurable.

Nineteenth form: checkpoint selection is now explicit in render-program
inspection and reload manifests. `RenderProgramSegmentInfo` carries
`checkpoint_before`, and the planner marks only raster-only suffix boundaries
inside each inspected stack. `ReloadDiffSegment` persists that flag as
`checkpoint_before` with backward-compatible JSON defaults. The product sparse
suffix route now requires the earliest dirty segment to be a depth-0 explicit
checkpoint candidate before using its `source_start` as a top-level
segment-before boundary. This prevents nested stack segment indexes from being
misinterpreted as top-level source indexes and gives future checkpoint
selection a durable data surface.

This nineteenth form still uses a simple candidate rule: a segment is a
checkpoint candidate when the suffix from that segment to the end of its local
stack consists only of `RasterRun` segments.

Twentieth form: checkpoint candidates now carry a ranked retention signal.
`RenderProgramSegmentInfo` computes `checkpoint_priority` for each explicit
candidate from estimated prefix reconstruction cost, suffix reuse signal, and
full-canvas checkpoint memory cost. `ReloadDiffSegment` persists
`checkpoint_priority` with backward-compatible JSON defaults, and
`checkpoint_selection` owns suffix candidate lookup separate from sparse-atlas
execution. The reconstructed suffix route passes that priority into the session
checkpoint cache. The cache remains LRU for equal-priority entries, but budget
or entry-count pressure now evicts the lowest-priority checkpoint first and
refuses to let a low-priority incoming checkpoint displace higher-priority
cached checkpoints.

This twentieth form still stores CPU RGBA8 full-canvas segment-before
checkpoints. It does not pre-render all candidates and does not add global
per-layer full-canvas caches. The next improvement is to use the dirty
segment/event-range data to rerun only affected segments when a valid
checkpoint and executable sparse-atlas plan are available.

Twenty-first form: product sparse patch reload now uses an affected-segment
window instead of requiring the entire suffix after the first dirty segment to
be raster-only. `clip_runtime::gpu_provider::atlas_rerun` builds the window
from the first dirty segment, keeps only later segments whose tile work-list
intersects the dirty patch rectangles, and treats empty or unknown work-lists
with real tile/legacy work as affecting the patch so they fail closed.
`clip_runtime::gpu_provider::atlas_events` lowers that affected window through
`sparse_atlas_raster_affected_event_plan()`. The Blender worker now records
`reload_diff.patch_renderer` as `sparse_atlas_initial_segments` or
`sparse_atlas_reconstructed_segments` when this product path succeeds.

This is still a strict raster-event product route. Overlapping later
`RasterRun` segments are rerun so their compositing still applies to the dirty
rect; non-overlapping later barriers can be skipped; overlapping non-raster,
scope, filter, or otherwise unsupported affected segments are explicit skipped
segments and force the worker back to the region renderer. This moves dirty
reload from "raster-only suffix" toward "only segments that can affect the
patch" without introducing approximate compositing.

Twenty-second form: affected raster windows now carry narrowed event ranges.
When all work-list tiles for an affected segment are known, `atlas_rerun`
coalesces the `event_start` / `event_end` ranges only from tiles whose
canvas-space bounds intersect the dirty patch rectangles. Unknown work-list
tiles still fail closed: they mark the segment as affecting the patch, keep a
full segment event range, produce no resident slots, and force the event
lowerer to skip the segment so the worker falls back to the region renderer.
This keeps diagnostics and future executor scheduling aligned with the actual
affected tile events without claiming a narrower range for unknown work.

Twenty-third form: sparse affected-window execution now supports
`RasterClippingRun` segments when the clipping run is the planner's
raster-only direct sibling form. `GpuSparseAtlasRasterEventBatch` now carries
an explicit batch kind, so the sparse atlas executor can reuse the existing
tile-silo clipping-run shader mode with the batch's base-event count and base
resolve blend. Runtime lowering builds clipping batches from resident atlas
slots in CSP source order: base raster tile events first, then clipped raster
tile events. Mixed RGBA atlas keys, mixed mask atlas keys, missing mask slots,
or unexpected clipped container siblings fail closed as skipped segments and
preserve the existing region-render fallback. This moves affected-window
reload beyond plain `RasterRun` without creating a second compositor path or
approximating clipping semantics.

Twenty-fourth form: sparse affected-window execution now supports unmasked
`PointFilterRun` segments. `GpuSparseAtlasRasterEventBatch` can carry
filter-only payloads and LUT rows in addition to raster events; the sparse
atlas executor binds a dummy RGBA atlas when a batch has no raster events,
uploads the batch LUT rows, and runs the same typed `PointFilter` shader path
used by the main tile-silo renderer. Runtime lowering accepts only faithful
point-filter segments: positive opacity, a 256-entry RGBA LUT, no filter mask,
and a dirty-rect-bounded local filter area. Masked point filters fail closed
with `FilterMaskNotLowered`, and malformed filter payloads fail closed with
`InvalidPointFilter`. This lets dirty raster windows continue through
unmasked adjustment/filter layers without falling back to the region renderer.

This twenty-fourth form still does not execute masked point filters or simple
container/THROUGH scopes from sparse affected windows. The next improvement is
to lower provider-backed R8 filter masks or start the first simple scope form,
using the existing tile-silo shader semantics.

Twenty-fifth form: sparse affected-window execution now supports the first
provider-backed masked `PointFilterRun` subset. `atlas_events.rs` was split
before adding the behavior: event result/skip types live in
`atlas_events_types.rs`, point-filter lowering lives in
`atlas_events_filter.rs`, and the main `atlas_events.rs` module now stays
focused on orchestration plus raster/clipping lowering. Masked point filters
lower when the dirty filter bounds are fully covered by one resident R8 mask
slot; the filter payload's mask atlas origin is offset into that resident slot
so the existing typed `PointFilter` shader samples mask alpha at the correct
dirty-rect coordinates. Missing mask slots, cross-tile mask coverage, or
malformed filter inputs still fail closed and preserve the region-render
fallback.

This twenty-fifth form still does not execute multi-slot masked point filters
or simple container/THROUGH scopes from sparse affected windows. The next
improvement is either to make filter mask payloads span multiple resident mask
slots faithfully, or to start the first sparse affected-window simple scope
form.

Twenty-sixth form: sparse affected-window execution now supports multi-slot
provider-backed masked `PointFilterRun` lowering. Masked filter dirty bounds
are represented by one or more non-overlapping `PointFilter` events, each
covering the intersection between the dirty filter bounds and a resident R8
mask slot. The lowering verifies that resident mask-slot intersections fully
cover the dirty filter bounds before emitting events; gaps, missing tiles, or
provider-unavailable masks still fail closed and keep the region-render
fallback. The GPU sparse atlas executor already accepted multiple filter
events with different mask atlas origins, and now has coverage for split mask
events in `sparse_atlas_batch_tests`.

This twenty-sixth form still does not execute simple container/THROUGH scopes
from sparse affected windows. The next improvement is to start the first
sparse affected-window simple scope form, reusing the typed scope events that
the main tile-silo renderer already executes.

Twenty-seventh form: sparse affected-window execution now supports the first
simple scope subset. `SimpleContainerScope` and `SimpleThroughScope` segments
whose direct children are raster sources lower into executable sparse atlas
batches with typed `BeginContainer`/`EndContainer` or
`BeginThrough`/`EndThrough` events wrapped around the resident raster events.
The executor reuses the existing tile-silo shader path and scope payload
buffer; it does not introduce a second scope compositor. Child raster masks
continue to use the existing R8 atlas event coordinates. Scope-level masks,
nested child scopes, filter children, clipping-run children, and clipped
container/folder children still fail closed and keep the region-render
fallback.

This twenty-seventh form is intentionally narrow: it proves sparse affected
windows can execute scope events from resident atlas slots, while leaving the
broader simple-scope subset to be migrated piece by piece from the main
tile-silo renderer.

Twenty-eighth form: sparse affected-window direct-raster simple scopes now
support the first scope-level mask subset. When a `SimpleContainerScope` or
`SimpleThroughScope` segment has a scope mask and the current dirty scope bounds
are fully covered by one resident R8 mask slot, lowering passes that atlas
coordinate through the typed scope payload. The sparse executor already uses
the same scope payload words as the main tile-silo renderer, so this change does
not add a second mask compositor. Missing mask slots or scope mask coverage that
spans multiple resident slots still fail closed and keep the region-render
fallback.

Twenty-ninth form: sparse affected-window simple scopes now use ordered child
tile events and support unmasked point-filter children after raster content.
`GpuSparseAtlasRasterEventBatch` can carry an ordered child event stream for
scope batches, currently `Raster` and `PointFilter`, while still exposing the
derived raster/filter lists needed by existing executor compatibility checks.
Runtime lowering tracks the current scope bounds as raster child events are
emitted, then gives point filters the accumulated local bounds so filter order
matches the main tile-silo scope model. Filter masks inside sparse scopes,
nested child scopes, raster-only clipping-run children, and clipped
container/folder children still fail closed and keep the region-render fallback.

Thirtieth form: sparse affected-window simple scopes now support raster-only
clipping-run children. The ordered child event stream can express
`BeginClipBase`, clip-base raster events, clipped raster events, and
`ResolveClipBase`; GPU preparation maps those sparse events onto the existing
typed tile-silo clip-base payloads. Runtime lowering accepts `ClippingRun`
children when the base and every clipped sibling are raster sources with
resident atlas slots, preserving the base resolve blend mode and output bounds
used by the main scope tile-silo renderer. Clipped container/folder siblings,
clipping runs nested through another child scope, multi-slot scope masks, and
nested simple scopes still fail closed and
keep the region-render fallback.

Thirty-first form: sparse affected-window simple scopes now support
provider-backed point-filter masks. The runtime lowering reuses the same
coverage-checked R8 mask-slot lowering used by top-level `PointFilterRun`
segments, so a masked filter child can emit one or more disjoint
`PointFilter` tile events over the accumulated scope bounds. Missing mask
coverage still fails closed with `FilterMaskNotLowered`, and the sparse
executor continues to use the existing typed filter payload and mask-atlas
sampling path rather than a separate scope-filter compositor.

Thirty-second form: sparse affected-window simple scopes now support nested
simple container and THROUGH children. Sparse scope batches gained inner
`BeginScope` / `EndScope` tile events, and GPU preparation maps those markers
onto the same typed `BeginContainer` / `EndContainer` or `BeginThrough` /
`EndThrough` payloads used by the main tile-silo renderer. Runtime lowering
recursively lowers nested simple scope children within the bounded scope stack,
while keeping nested clipping runs as explicit fail-closed cases until their
cache/writeback semantics are modelled in that position.

Thirty-third form: sparse affected-window simple scopes now support raster-only
clipping runs inside nested container children. The runtime lowering now uses
the same clipping-run policy as the main scope tile-silo planner: container
scopes can carry clipping runs through nested containers, while THROUGH scopes
still only allow direct clipping-run children. Nested clipping runs under a
THROUGH child remain explicit fail-closed cases.

Thirty-fourth form: sparse affected-window simple scopes now support
multi-slot masks on the outer scope. When a `SimpleContainerScope` or
`SimpleThroughScope` has a scope mask whose dirty bounds are covered by
multiple resident R8 mask slots, runtime lowering splits the scope into one
executor batch per mask slot and clips the child tile events to that slot's
canvas bounds before resolving the scope. This keeps child raster/filter/clip
events from leaking into the parent accumulator outside the masked sub-rect.

Thirty-fifth form: nested simple scope masks now use the same multi-slot R8
mask lowering. When a nested `BeginScope` / `EndScope` child has a mask whose
child bounds span multiple resident R8 slots, runtime lowering emits one nested
scope pair per mask slot and clips that nested scope's child events to the
slot's canvas bounds. The parent scope still sees the full nested child bounds
for accumulated scope bounds, while pixels outside each mask slot sub-rect
cannot leak into the parent accumulator. Missing nested mask coverage still
fails closed to the region renderer.

Thirty-sixth form: nested THROUGH child scopes now support direct raster-only
clipping runs. The main render-program planner, main tile-silo scope program,
and sparse affected-window scope lowering now all allow a `ClippingRun`
directly inside a nested `ThroughGroup`, emitting the existing
`BeginClipBase` / `ClipBaseRaster` / `ClippedRaster` / `ResolveClipBase`
events inside the nested THROUGH scope. Clipping runs inside a container nested
under a THROUGH scope still fail closed, preserving the narrower faithful
subset until that extra scope relationship is modelled.

Thirty-seventh form: container scopes nested under THROUGH now carry
raster-only clipping runs as tile events. The same `DirectOnly` clipping-run
policy now propagates through nested containers while inside a THROUGH scope,
so `ThroughGroup -> Container -> ClippingRun` lowers in the render-program
planner, the main tile-silo scope program, the sparse affected-window runtime
lowering, and the sparse atlas executor. This still accepts only clipping runs
whose base and clipped siblings are atlas-eligible rasters; clipped
container/folder siblings remain a barrier.

Next Phase 6 work:

- expand sparse affected-window simple scopes beyond direct raster children:
  clipped container/folder siblings

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
