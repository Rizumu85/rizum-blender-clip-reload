# Plan

Last reconciled: 2026-06-05

## Purpose

This file records durable project directions only. Do not update it for every probe, rejected hypothesis, address trace, or metric change.

- Current compact state: `docs/AI_MEMORY.md`
- Full evidence and rejected hypotheses: `docs/analysis.md`
- User-facing design context: `docs/design.md`
- Native-analysis workflow: `E:\Documents\Claude\Projects\rizum-clip-studio-paint\R2_COMMANDLINE_WORKFLOW.md`
- Windows/WSL decompiler split: `E:\Documents\Claude\Projects\rizum-clip-studio-paint\R2_HYBRID_DECOMP_WORKFLOW.md`

## Direction 1: Native Vector Fidelity

Goal: Match CSP's native no-pattern vector rendering for the isolated vector samples without sample-specific overfits.

Current focus:

- Improve isolated vector fixtures first; `Vector_SizePressure.clip` remains
  paused while other non-exact semantics are harvested.
- For rough balloon/frame/material objects, keep the current native-backed
  point-family and interval-dab previews narrow; exactness needs the native
  retained material quad/row writer, not broader fallback outline tuning.
- Current native target: `Test_Vector` wide-`0x41` V4 pen-head rendering. The
  16 half-cap point bridge is native-backed for the large capsule; the next
  useful boundary is native `FillPolygon` fixed-point scan conversion and
  alpha/composite behavior.
- Preserve the exact native guards already solved for baseline, flow, opacity random, and opacity pressure.

Update this direction only when the active blocker changes or a major native-rendering milestone is reached.

## Direction 2: General `.clip` Fidelity

Goal: Keep improving flattened `.clip` output while preserving verified raster, layer, mask, clipping, blend, adjustment, and vector behavior.

Current policy:

- Prefer recovered CSP data and native-backed rules over tuned constants.
- For new native evidence, use the adjacent workspace's hybrid workflow:
  IDA/Hex-Rays for decompilation, xrefs, structure recovery, and database notes
  when useful; Windows/WSL r2 for stable CLI evidence and decompiler comparison.
  Arkana remains optional only.
- Treat WSL `pdg`/`pdd` and Windows `pdc` as reading aids; confirm behavior with disassembly/xrefs/strings or importer probes before changing semantics.
- Keep unsupported features conservative until a targeted sample or native trace supports them.
- Treat text/frame/material/gradation fallbacks as preview layers unless their native renderer path is proven.

Update this direction only when the project changes scope or a broad class of `.clip` features becomes supported.

## Direction 3: Agent Handoff Hygiene

Goal: Make new conversations start from the right state quickly.

Current policy:

- Keep `docs/AI_MEMORY.md` as the short current-state memory.
- Keep `docs/analysis.md` as the evidence log.
- Keep this file as durable direction, not a running checklist.

Update this direction only when the documentation structure changes.
