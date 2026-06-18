# AGENTS.md

Project-level instructions for Codex.

## Working Agreement

- Start with `docs/AI_MEMORY.md` for the current renderer state before reading the long research logs.
- Write project files, code comments, and technical docs in English. Summarize chat changes in Chinese.

## Current Scope

- This repo owns the Blender-facing importer, native renderer bridge, and verification harness.
- Keep `.clip` fidelity focused on flattened raster layers, folders, masks, clipping, blend modes, adjustment/filter layers, and Blender add-on behavior.
- Do not pursue vector, bubble/frame, or text renderer compatibility in this repo.
- Treat historical vector/native notes as archived evidence only unless Rizum explicitly reopens that scope.

## Doc Roles

- `docs/AI_MEMORY.md`: compact current state for agents.
- `docs/analysis.md`: append-only evidence, historical findings, and rejected hypotheses.
- `docs/design.md`: Blender UX and user-facing behavior.
- `docs/plan.md`: durable directions and open work.

## Engineering Rules

- Read the relevant docs before changing code.
- Keep changes surgical and traceable to the requested task.
- Prefer existing patterns in `clip_loader.py` for reference-verifier work and `clip_studio_importer/native_bridge.py` for add-on native bridge work.
- Do not replace importer behavior from one reverse-engineering clue unless a targeted sample improves and guard samples stay stable.
- Treat metric-only probes as diagnostic until sample or runtime evidence supports them.
- Avoid degradation handling, fallbacks, hacks, heuristics, local stabilizations, or post-processing bandages that are not faithful general algorithms.
- During refactors, do not preserve old interfaces for compatibility by default; rewrite the interface cleanly and let incorrect callers fail loudly.
- Preserve historical rejection notes in `docs/analysis.md`; update `docs/AI_MEMORY.md` when the current state changes.

## Reverse Engineering Rules

- IDA Pro symbol names (function names like `RenderBlendModeCall_0`, variable names, struct field names) in our IDB files are AI-assigned, not from official CSP debug info. They may be misleading or wrong.
- Verify behavior from the **actual disassembly and observed runtime values**, not from symbol names. Treat names as hypotheses to test, not ground truth.
- When a function name suggests one role but its callers, callees, or assembly contradicts that role, trust the assembly and runtime samples.
- When data-driven sample fitting (computing the formula on real pixel inputs and matching CSP output) gives clean results, prefer that evidence over plausible-looking IDA decompilation.
- Do not modify IDA databases (no `set_comments`, `rename`, `patch`, `define_*`) unless explicitly asked. Read-only investigation only.
- IDA MCP is stateful and uses a single active instance — only one agent at a time may run IDA tool calls. Do not parallelize IDA work across subagents.

## Native Rewrite Architecture Rules

- Native rewrite code lives under `native/`; do not extend the Python loader/compositor for the native path.
- Do not create monolithic renderer files, even during spikes or refactors.
- Crate/package root files such as `lib.rs`, `main.rs`, and C++ plugin entry files are wiring only: declarations, re-exports, and calls into named modules.
- Keep parsing, metadata mapping, tile decode, render graph construction, GPU scheduling, shader/pipeline code, C ABI, and OIIO adapter code in separate modules.
- If a Rust/C++ source file grows beyond roughly 500 lines, split it before adding new behavior unless the file is a generated artifact.
- Throwaway experiments must live under `native/spikes/<name>/` and must be deleted or promoted into the module architecture before the route is accepted.
- The accepted native path must remove the Python compositor/loader and sidecar PNG workflow; do not add compatibility shims or runtime fallbacks back to them.

## Verification

Run targeted checks only when the change needs them. Prefer raster/filter samples relevant to the change, for example:

```powershell
python verify_one_clip.py img/Test_AddGlowMultiply.clip
python verify_one_clip.py img/Test_ClippingEdge.clip
python verify_one_clip.py img/Test_Mask.clip
python verify_one_clip.py img/Test_ToneCurve.clip
```
