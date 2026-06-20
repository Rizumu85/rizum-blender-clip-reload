# Native Tile-Event Renderer

Last reconciled: 2026-06-20

## Purpose

The tile-event renderer is the native renderer's main performance model. It
compiles safe `.clip` source sequences into tile-local event programs and keeps
unsafe or unmodelled CSP semantics as explicit barrier segments.

This document replaces the old implementation roadmap/log set. It describes the
current model and the gate for future semantic coverage.

## Current Model

Render planning produces segments:

- `TileLocal` segments execute typed tile events.
- `Barrier` segments use the faithful legacy path for semantics that are not
  safely tile-local.

Common barrier reasons include:

- `ThroughGroupNotLowered`
- `IsolatedContainerRequiresIntermediate`
- `ClippingRunNotLowered`
- `ScopeDepthLimitExceeded`
- `RasterBoundsOrResourceNotLowered`
- `SolidColorNotLowered`

Future work should reduce measured barriers only when tests prove the tile-local
result matches the faithful path.

## Module Boundaries

`stream_program*` owns planning:

- inspect `GpuNormalStackSource` sequences
- decide tile-local vs barrier segments
- assign `RenderProgramBarrierReason`
- produce cost hints and planner statistics

Keep lowering rules in `stream_program_lowering.rs`,
`stream_program_barriers.rs`, and focused scope-plan modules. Do not add new
lowering decisions to `stream_sequence.rs`.

`stream_sequence` owns dispatch:

- call the render-program planner
- dispatch planned tile-local segments
- dispatch explicit barriers through the faithful legacy path
- handle provider fallback
- maintain ping-pong texture indices and dirty bounds

`stream_tile_event*` owns ABI and payload layout:

- typed event headers and payloads
- tile span/work buffers
- payload tests
- shader-visible ABI versioning

`tile_silo.wgsl` executes planned events only. It must not infer source graph
structure that the planner did not encode.

## ABI Rule

`TILE_EVENT_ABI_VERSION` is currently tracked in the native code and scoreboard.
Every shader-visible payload layout change must:

1. Bump `TILE_EVENT_ABI_VERSION`.
2. Keep or add a reload-manifest compatibility test proving old manifests
   promote to full render.
3. Update `docs/native-tile-event-scoreboard.md`.

Existing guard:

- `clip_runtime::reload_diff::reload_diff_tests::tile_event_abi_change_promotes_to_full`

## Convergence Gate

Before adding semantic coverage:

1. Update `docs/native-tile-event-scoreboard.md`.
2. Pick the largest measured barrier reason from fixed samples.
3. Add focused planner/runtime/shader tests.
4. Keep known regression guards stable, especially the Terra nested THROUGH
   container shape.
5. Prove `planned_passes` or `barrier_segments` decreases on a fixed sample.
6. Run `scripts/verify_native_convergence.ps1`.

Do not continue the old implementation log as a backlog. Choose exactly one
semantic task at a time from measured scoreboard evidence.

## Current Safe Posture

- Tile-local event execution is preferred for proven semantics.
- Barrier segments are acceptable and should remain explicit.
- Broad THROUGH/container guard relaxation is not allowed without a focused
  legacy-vs-tile test.
- Product rendering should not fall back to a Python or CPU compositor.
- Debug/test paths may keep faithful legacy rendering as an oracle.
