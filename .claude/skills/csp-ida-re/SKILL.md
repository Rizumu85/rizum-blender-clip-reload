---
name: csp-ida-re
description: PAUSED fallback only. Historical project-specific IDA Pro workflow for Clip Studio Paint .clip importer work. Do not use by default during the radare2/r2 command-line trial; use only if the user explicitly re-enables IDA/Arkana/Hex-Rays MCP analysis or r2 proves inadequate.
---

# CSP IDA Reverse Engineering (Paused)

This skill is paused for normal work. The current native-analysis path is direct command-line radare2/r2 in:

`E:\Documents\Claude\Projects\rizum-clip-studio-paint\R2_COMMANDLINE_WORKFLOW.md`

Do not invoke `ida-pro-mcp`, Arkana MCP, Hex-Rays/IDA MCP skills, or IDA-domain scripting for CSP native analysis unless the user explicitly re-enables them or the r2 route is documented as insufficient.

## Tool Order

Fallback order only after MCP analysis is explicitly re-enabled:

1. Prefer a strict read-only `ida-pro-mcp` session against an open IDA GUI database.
2. Use Arkana/Ghidra only as a fallback or cross-check, especially for already-noted functions.
3. Use `hexrays-ida-domain-scripting-unsafe` only for trusted local CSP binaries/headless IDA tasks. It can execute code locally.
4. Use `hexrays-ida-domain-api`, `hexrays-ida-plugin-development`, and `hexrays-package-ida-plugin` only when building IDA plugins or ida-domain scripts.

Do not tell the user to restart IDA/Codex as the first fallback. First use the r2 workflow. If MCP analysis is re-enabled later, the project MCP config is `.mcp.json`.

## Safety Rules

- Treat IDA write operations as persistent database edits. Rename/comment/type only when it helps the investigation.
- Ask before saving an IDB after bulk script changes.
- Never manually convert address bases in your head; use MCP `int_convert` or a small script.
- Ground claims in decompilation/disassembly/xrefs/strings. Clearly mark inference.
- Do not replace importer behavior from one reverse-engineering clue unless a sample/PNG/PSD verification improves and guards stay stable.

## CSP Targets

Primary installed tree:

`C:\Program Files\CELSYS\CLIP STUDIO 1.5\CLIP STUDIO PAINT`

Priority modules:

- `iswCoreTG.dll`: vector/ruler/adjustment-layer runtime.
- `iswCmnTG.dll`: `RCPatternDraw*` low-level pattern engine.
- `TGXPGPlugInCore.dll`: XPG/material/text/frame export and conversion routes.
- `ExportPSD.dll`: Photoshop packaging and adjustment descriptor reference.
- `CLIPStudioPaint.exe`: Planeswalker BrushStyle/tool-state model and possible bridge code.

## Known Anchors

See `references/known-targets.md` only after deciding an IDA fallback pass is justified.

Current high-value open question:

Finish `Vector_SizePressure` native equivalence. `SizeEffector=0x31` extra-curve composition is closed: the saved-file route passes an identity/pass-through curve, so the remaining `151` output-only hard pixels should be chased through native size feedback, sample distribution, or plot/queue setup rather than by changing `amount1` or generic `0x31` semantics.

## Workflow

1. Record why r2 is insufficient or cite the user instruction that re-enabled IDA/MCP.
2. Identify the exact module and current IDA image base.
3. Resolve known anchors by name/RVA/VA and verify bytes/xrefs before trusting old addresses.
4. Walk xrefs in both directions:
   - caller path from document layer/frame/text/vector object
   - callee path into raster/vector/pattern engine
5. Do not add IDA comments/names or save IDB changes unless explicitly requested.
6. Before editing importer code, reproduce the candidate behavior with a temporary probe and compare using:
   - `python verify_one_clip.py ...`
   - `python verify_one_clip.py img\Vector_SizePressure.clip`
   - `python verify_one_clip.py img\Vector_OpacityPressure.clip`
   - `python verify_filter_exports.py img\test_Filters_Vector_Text.clip --max-max 8 --max-mean 0.06 --max-visible-px 2200`
7. Update `docs/analysis.md` and `docs/plan.md` with the result, including rejected hypotheses.
