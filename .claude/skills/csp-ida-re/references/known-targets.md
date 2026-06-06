# Known CSP Reverse-Engineering Targets

Keep this file concise. This is historical IDA fallback material. The current default native workflow is the adjacent r2 command-line workflow:

`E:\Documents\Claude\Projects\rizum-clip-studio-paint\R2_COMMANDLINE_WORKFLOW.md`

If IDA/MCP fallback is explicitly re-enabled, verify addresses against the loaded IDA image base before use.

## iswCoreTG.dll

- `CSVec4Sampling::InitRasterOperation`
  - VA observed: `0x12477090`
  - Meaning: sets `CSVec4Sampling+0x60 = 1`, stores `RCPatternDraw*` at `+0x68`, tangent/rotate flag at `+0x70`.
  - Current status: exported and real, but no direct static caller found in checked `.text` paths.
- `CSVec4Sampling::SetLayer`
  - VA observed: `0x1247b180`
  - Meaning: clears pattern fields `+0x60/+0x68/+0x70`.
- `CSVec4Sampling::InitSamplingForRuler`
  - VA observed: `0x12482090`
  - Meaning: calls `SetLayer`, then starts ordinary ruler sampling.
- `CSVec4Sampling::Sampling`
  - VA observed: `0x1247a370`
  - Meaning: branches on sampler `+0x60`; nonzero path calls pattern plotting.
- `RCPatternDraw::DrawSinglePattern` import consumers
  - Calls appear in `PlotCurvePattern` and `PlotPreSamplePattern`.
  - No checked frame/text route re-enables pattern mode before these.
- `CSVectorSetting::SetAntiAliasSimple`
  - RVA observed: `0x273f0`
  - VA observed: `0x122673f0`
  - Body: stores incoming value at `CSVectorSetting+0x08`.
- `CreateLookUpTableGRAD`
  - VA observed: `0x122be240`
  - Meaning: gradient-map LUT builder from runtime nodes.

## iswCmnTG.dll

- Image base observed: `0x120d0000`.
- `RCPatternDraw::DrawSinglePattern`
  - RVA observed: `0x55740`
  - VA observed: `0x12125740`
  - Meaning: external stamp consumer used by `iswCoreTG.dll`.
- `RCPatternDraw::BeginDraw`
  - VA observed: `0x121243b0`
  - Meaning: consumes an already-built `RCPatternDrawParam*`; not the BrushStyle compiler.
- `RCPatternDrawParam`
  - Pattern block begins near `+0x08`.
  - Size fields around `+0x148`.
  - Color fields around `+0x154`.
  - Hardness around `+0x168`.
  - Rotation around `+0x19c`.
  - Interval around `+0x1ac`.
  - `CreateSumiPatternCore` is a self-contained built-in pattern island, not the document BrushStyle bridge.

## TGXPGPlugInCore.dll

- `XPGPlugInCore::ReadPageFile`
  - RVA observed: `0x5000`
  - VA observed: `0x15035000`
  - Calls `CSVectorSetting::SetAntiAliasSimple` through IAT; copies page/export context fields.
- `sub_1503a2e0`
  - Shared wrapper for text-frame/frame-folder `DrawToVector`.
  - It dispatches to imported core functions but does not set up `RCPatternDrawParam` or `InitRasterOperation`.
- Pattern/material island:
  - `sub_15047df0` calls `RCPatternDraw::CreateSumiPattern*`.
  - `sub_150477d0` / `sub_1503e290` build `CSPattern` objects.
  - Current status: useful for material package serialization, not the missing frame/text brush renderer bridge.

## ExportPSD.dll

- Useful for PSD packaging and adjustment payload tags.
- It maps adjustment kinds to Photoshop tags:
  - `1 -> brit`
  - `2 -> levl`
  - `3 -> curv`
  - `4 -> hue2`
  - `5 -> blnc`
  - `6 -> nvrt`
  - `7 -> post`
  - `8 -> thrs`
  - `9 -> grdm`
- It consumes host-created offscreens/runtime adjustment data; it is not the vector/brush renderer.

## Current SizePressure Target

- Open target: `Vector_SizePressure.clip`.
- Current importer result: `max=226`, `mean=0.024409`, `visible=151`.
- Residual shape: output-only hard-edge pixels on the vector layer, concentrated in several large tail dabs.
- `sub_142CB45F0` extra-curve composition is identity/pass-through for the reopened saved-file route.
- Preserve `SizeEffector=0x31` range semantics: `primary * (1.0 + (amount1 - 1.0) * secondary)`.
- Reject metric-only shortcuts unless new native evidence appears: `amount1=1.4`, radius scale, hard-span `-0.5`, distance-fraction attribute interpolation, duplicate-centre skips, and whole-dab accumulated-coverage gates.

## Current Negative Evidence

- `ExportPSD.dll` packages host-created offscreens and descriptors; it does not reconstruct vector/brush pixels.
- `TGXPGPlugInCore.dll` has official text/frame conversion routes, but traced `DrawToVector` paths use ordinary ruler/vector conversion.
- `CSFrameFolderLayer::DrawToVector`, `CSTextFrameV4Layer::DrawToVector`, and frame-line vector wrappers do not show BrushStyle/PatternStyle setup.
- `CSBitmap` polygon/segment primitives are plain geometry fill/draw helpers, not brush-material samplers.
- Static/dynamic scans so far found no module importing `CSVec4Sampling::InitRasterOperation` or `RCPatternDrawParam::Set*` as the missing bridge.

## Importer Verification Baselines

Use these after any renderer-related change:

- `python verify_one_clip.py img\Vector_SizePressure.clip`
  - Current: `max=226`, `mean=0.024409`, `visible=151`.
- `python verify_one_clip.py img\Vector_OpacityPressure.clip`
  - Current: pixel-exact.
- `python verify_one_clip.py img\Vector_OpacityRandom_50.clip`
  - Current: pixel-exact.
- `python verify_one_clip.py img\Test_Vector.clip`
  - Current: `max=187`, `mean=1.043658`, `visible=27806`.
- `python verify_one_clip.py img\test_Filters_Vector_Text.clip`
  - Current: `max=225`, `mean=0.569085`, `visible=48576`.
- `python verify_one_clip.py img\test_Filters_Vector_Text.clip --layers 5 --ref img\test_Filters_Vector_Text_Vector.png`
  - Current: `max=173`, `mean=0.159297`, `visible=8857`.
- `python verify_filter_exports.py img\test_Filters_Vector_Text.clip --max-max 8 --max-mean 0.06 --max-visible-px 2200`
  - Should pass.
