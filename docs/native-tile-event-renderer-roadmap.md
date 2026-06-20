# Native Tile-Event Renderer Roadmap

Last updated: 2026-06-19

## Scope Freeze

Pause new tile-event semantic coverage until this convergence work is done.
The current milestone is not "lower every CSP semantic into one shader." The
current milestone is:

- keep tile-local event execution as the main performance architecture
- keep unsupported semantics as explicit, counted barriers
- keep the product renderer faithful, with no CPU compositor fallback and no
  post-processing
- make progress measurable through a small scoreboard before adding another
  semantic lowering task

Detailed implementation history moved to
`docs/native-tile-event-renderer-changelog.md`.

## Architecture Target

The renderer architecture remains:

```text
strict CSP source selection
  -> render program planner
  -> tile-local event executor
  -> explicit barrier executor
```

Module responsibilities:

- `stream_program*` owns lowering decisions, barrier reasons, segment cost
  hints, and render-program stats.
- `stream_sequence` dispatches already-planned segments and calls either the
  tile-local encoder for that segment kind or the legacy faithful encoder for a
  barrier/provider fallback.
- `stream_tile_event` owns the typed tile-event ABI and payload layout.
- `tile_silo.wgsl` consumes only versioned typed events; shader payload changes
  require a tile-event ABI bump and a reload-manifest compatibility test.

## Current Status

Implemented tile-local segment families:

- raster runs
- raster-only clipping runs
- raster plus pointwise filter runs
- filter-only pointwise runs
- byte-domain special blend raster events
- simple container scopes
- simple THROUGH scopes
- scope masks backed by resident R8 atlas tiles
- clipped container/folder sibling child streams when the child stream fits the
  current simple model: raster, SolidColor, pointwise filter, nested simple
  container, direct raster-only child clipping run, simple THROUGH child,
  pointwise filter or direct raster-only clipping run inside simple THROUGH,
  and one nested simple THROUGH level

Current explicit barriers:

- top-level Paper/SolidColor outside tile-local scopes
- complex containers and complex THROUGH groups
- THROUGH nesting beyond the two-level tile VM limit
- provider-unavailable or unknown masked scopes/filters
- clipped container/folder sibling subtrees beyond the current simple child
  stream
- scope-depth and tile-event-count limits
- unknown future filter/vector/text/bubble/frame scope, which remains out of
  scope for this repository

Current diagnostics:

- `clip_cli --performance-plan-json` reports planned passes, tile-local
  segments, barrier segments, legacy segment count, top barrier reasons, tile
  event ABI, tile occupancy, and coverage/fallback metadata.
- `docs/native-tile-event-scoreboard.md` records fixed sample baselines.

## Next 3 Tasks

1. **Scoreboard and verification gate**
   Keep `docs/native-tile-event-scoreboard.md` current for fixed samples and
   run `scripts/verify_native_convergence.ps1` before renderer commits.

2. **Planner/executor seam audit**
   Keep lowering rules in `stream_program_lowering` and scope-rule modules.
   Keep `stream_sequence` as segment dispatch plus provider fallback only.
   Do not add opportunistic source-walking semantic rules to
   `stream_sequence`.

3. **Next semantic task only from scoreboard**
   After the convergence gate is stable, choose exactly one next semantic
   coverage task from the largest measured barrier reason in the scoreboard.
   The task is accepted only if `planned_passes` or `barrier_segments` drops on
   a fixed sample without fidelity regression.

## Completion Rule

Do not add a new tile-event semantic subset unless all of these are true:

- the scoreboard identifies it as the largest useful barrier
- the change has a focused planner/runtime/shader test
- `--performance-plan-json` proves a planned-pass or barrier-count reduction
- guard compares stay stable
- the tile-event ABI is bumped if shader payload layout changes
- reload-manifest compatibility tests prove old manifests promote to full
  render after ABI changes
