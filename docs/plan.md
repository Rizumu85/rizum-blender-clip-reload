# Plan

Last reconciled: 2026-06-20

## Purpose

This is the durable direction for `Rizum Clip Reload`. Keep implementation logs
out of this file. Code and focused tests should carry most detail.

Read with:

- `docs/AI_MEMORY.md` for compact current state.
- `docs/native-code-architecture.md` for module boundaries.
- `docs/native-direct-load-rewrite.md` for the accepted Blender runtime path.
- `docs/native-tile-event-renderer.md` for renderer convergence rules.
- `docs/native-performance-investigation.md` for compressed optimization
  evidence and rejected detours.
- `docs/design.md` for user-facing Blender behavior.
- `docs/analysis.md` for append-only historical evidence.

## Direction 1: Product Scope

Keep the project focused on flattened `.clip` textures in Blender.

In scope:

- Raster layers and paper/background rendering.
- Folders, masks, clipping, opacity, blend modes, and THROUGH/container behavior.
- Adjustment/filter layers supported by the native strict path.
- Generated Blender image import, reload, packing, and diagnostics.
- Native CLI verification against CSP-exported PNG references.

Out of scope:

- Editable Blender layer import.
- Vector, text, bubble/frame, 3D, animation, and `.clip` write-back.
- CPU compositor fallback, sidecar PNG workflows, post-processing fixes, and
  global full-canvas per-layer caches.

## Direction 2: Fidelity

Treat CSP visual fidelity as the highest-level correctness constraint.

Rules:

- Fix fidelity only from targeted samples, native evidence, or clear runtime
  invariants.
- Keep guard samples stable when changing formulas or renderer semantics.
- Prefer explicit unsupported behavior over approximate rendering.
- Do not tune broad formulas from one local residual.

Current known residuals:

- `Test_Gradiation` still has a Gradient Map residual.
- Some private/reference files keep low-level edge or quantization residuals.
- Fidelity work is now lower priority unless a visible user-facing issue appears
  or a focused sample proves a general algorithmic bug.

## Direction 3: Blender Add-on

Keep the add-on simple for artists:

- Initial import and manual reload should not block Blender's UI.
- Initial import should not create confusing placeholder images.
- Reload should mark `Needs Pack`; save should pack automatically.
- Missing sources should keep packed pixels visible.
- Normal UI should hide native internals; Developer Mode can expose timing and
  diagnostics.
- Keep source locators for issue reports, not layer navigation.

Packaging:

- Windows x64 is the tested platform.
- Linux x64, macOS x64, and macOS arm64 package support exists but remains
  maintainer-untested until real devices verify it.

## Direction 4: Native Architecture

Keep Rust native code split by responsibility:

- `clip_model`: shared data types.
- `clip_file`: `.clip` container and SQLite/resource reads.
- `clip_graph`: semantic render graph construction.
- `clip_gpu`: GPU execution, shaders, tile events, barriers, and stream encoder.
- `clip_runtime`: session orchestration, manifests, persistent renderer, and
  Blender worker JSON.
- `clip_capi`: C ABI surface only.
- `clip_cli`: command-line and worker entry points.

Root files are wiring only. Split files before adding behavior if they approach
the project threshold in `AGENTS.md`.

## Direction 5: Tile-Event Convergence

The tile-event renderer is the main performance model. Barrier segments are
allowed when a CSP semantic is not safely tile-local.

Before any new semantic lowering:

1. Update `docs/native-tile-event-scoreboard.md`.
2. Pick the largest measured barrier reason.
3. Add focused planner/runtime/shader tests.
4. Prove `planned_passes` or `barrier_segments` drops on a fixed sample.
5. Keep Terra regression guards stable.
6. Run `scripts/verify_native_convergence.ps1`.

Do not continue old implementation logs as a backlog. Add only the next measured
semantic task when it passes the gate.

## Direction 6: Performance

Prefer measured, structural changes:

- Persistent worker and reload manifests.
- Sparse tile/resource reuse.
- Tile-local event execution where semantics are proven faithful.
- Checkpoint and patch diagnostics that explain why a reload is full, region,
  patch, or no-change.

Avoid:

- Micro-optimizing upload details before profiles point there.
- Parallel decode work unless future profiles show decode dominates.
- Broad task graph architecture without a benchmarked waste pattern.
- Semantic lowering just for speed.

## Direction 7: Documentation Hygiene

Keep docs short and current:

- `docs/AI_MEMORY.md` is the first-stop current state.
- `docs/analysis.md` is the long append-only evidence archive.
- Optimization detours should be compressed into
  `docs/native-performance-investigation.md`.
- Old milestone logs should not remain as parallel roadmaps once code and tests
  carry the behavior.
