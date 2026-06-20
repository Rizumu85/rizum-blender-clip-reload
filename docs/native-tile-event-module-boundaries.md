# Native Tile-Event Module Boundaries

Last updated: 2026-06-19

This document freezes the current tile-event module seams. Use it when changing
the renderer after the convergence pause.

## `stream_program*`

Owns render-program planning.

Allowed responsibilities:

- inspect `GpuNormalStackSource` sequences
- decide tile-local vs barrier segments
- assign `RenderProgramBarrierReason`
- produce cost hints and planner statistics
- expose inspection data for manifests and diagnostics

Keep lowering rules in:

- `stream_program_lowering.rs`
- `stream_program_barriers.rs`
- dedicated scope-plan modules such as `stream_tile_scope_silo_plan.rs`

Do not add lowering decisions to `stream_sequence.rs`.

## `stream_sequence`

Owns segment dispatch.

Allowed responsibilities:

- call `plan_render_program`
- dispatch each segment to the matching tile-local encoder
- use the faithful legacy encoder for explicit barriers
- use the faithful legacy encoder when a provider cannot fulfill a planned
  tile-local segment
- maintain ping-pong texture indices and dirty bounds while dispatching

Disallowed responsibilities:

- scanning ahead for new semantic lowering opportunities
- adding source-shape eligibility rules
- adding barrier classification
- mutating the tile-event ABI

## `stream_tile_event`

Owns typed event ABI and payload layout.

Rules:

- every shader-visible payload layout change must bump
  `TILE_EVENT_ABI_VERSION`
- every ABI bump must keep or add a reload-manifest compatibility test proving
  old manifests promote to full render
- payload words must be documented by their payload type and covered by unit
  tests
- event kinds should be semantic enough to avoid overloading raster payloads
  for unrelated behaviours

Existing guard:

- `clip_runtime::reload_diff::reload_diff_tests::tile_event_abi_change_promotes_to_full`

## `tile_silo.wgsl`

Owns tile-event execution, not planning.

Allowed responsibilities:

- decode typed event headers and payloads
- execute the modelled tile-local semantics
- keep unsupported semantics out of shader code until a planner segment and
  tests exist

Disallowed responsibilities:

- inferring source graph structure that the planner did not encode
- silently approximating unsupported CSP semantics

## Scoreboard Gate

Before adding semantic coverage:

1. Update `docs/native-tile-event-scoreboard.md`.
2. Pick the largest measured barrier reason.
3. Add a focused planner/runtime/shader test.
4. Prove `planned_passes` or `barrier_segments` decreases on a fixed sample.
5. Run `scripts/verify_native_convergence.ps1`.
