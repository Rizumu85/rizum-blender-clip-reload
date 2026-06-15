# Plan

Last reconciled: 2026-06-15

## Purpose

This file records durable project directions. Do not update it for every probe, rejected hypothesis, address trace, or metric change.

- Current compact state: `docs/AI_MEMORY.md`
- Full evidence and rejected hypotheses: `docs/analysis.md`
- User-facing design context: `docs/design.md`
- Native rewrite direction: `docs/native-direct-load-rewrite.md`

## Direction 1: Raster Fidelity First

Goal: improve flattened `.clip` output for raster artwork without reopening vector, bubble/frame, or text rendering.

Current focus:

- Keep both loader entrypoints aligned: `clip_loader.py` and `clip_studio_importer/clip_loader.py`.
- Keep the main compositor on straight `uint8` RGBA buffers so transparent cache RGB, Add/Add Glow byte alpha, and future native byte-domain blend work are expressible.
- Continue raster layer semantics: folders, masks, clipping, THROUGH groups, blend modes, adjustment/filter layers, visibility bit flags, and sidecar PNG output.
- Keep `LayerVisibility` as a bit flag: values `0` and `2` are hidden; values `1` and `3` are visible.
- Use targeted sample improvements with guard samples before accepting compositor changes.
- Treat vector, bubble/frame, and text layers as unsupported/skipped content in this repo.

Open raster targets:

- `Ref_Terra404_Live2D`: complex clipped/grouped highlight stacks and bottom-edge clipped blend residuals are now close; remaining differences are low-level rounding-scale pixels.
- `Test_AddGlowMultiply`: Add Glow base plus clipped standard-preserve siblings now routes through the native GPU path; remaining residual is low-level (`raw_max=5`, `premul_max=3`).
- `Ref_Kabi_Live2D`: the former large white-eye residual is fixed; the remaining known max is a tiny local block around `(1738,799)`.
- Non-zero render or mask offsets should stay sample-driven and guarded.

## Direction 2: Blender Add-on Workflow

Goal: keep the Blender importer usable until the native direct-load rewrite replaces the Python sidecar workflow.

Current focus:

- Preserve the sidecar PNG cache path only for the current Python implementation.
- Keep manual reload and non-blocking auto-reload behavior stable.
- Rebuild `clip_studio_importer.zip` whenever package code changes.

## Direction 3: Native Image Loading Rewrite

Goal: replace the Python compositor/sidecar PNG workflow with a native GPU renderer and Blender image-datablock integration.

Current policy:

- Rust plus `wgpu` is the chosen renderer direction, with a thin C++ OpenImageIO plugin boundary and a stock Blender image-datablock bridge.
- External OpenImageIO plugin loading alone is not enough for stock Blender `bpy.data.images.load(".clip")`; true file-backed support requires a Blender ImBuf/source bridge or upstream source patch.
- Current native milestone: continue strict GPU coverage for raster adjustment/filter layers, remaining byte-domain blend quantization, and large-stack GPU performance. Ordinary raster blend modes `LayerComposite=1..26` plus `36` are enabled, isolated containers can resolve with supported non-NORMAL blend modes, clipping runs support non-NORMAL raster bases plus clipped raster siblings, THROUGH groups clear the clip base for following clipped layers, and LUT-style adjustment/filter layers now route through a dedicated GPU pass: Tone Curve (`FilterLayerInfo` type `3`) and Gradient Map (`type 9`). `IllustrationBlendModes.clip`, `IllustrationBlendModes2.clip`, `Test_AddGlowMultiply.clip`, `Test_ToneCurve.clip`, `Test_Gradiation.clip`, and `Test_RealArt.clip --gpu-support-check` are fully routed but still have residual formula/quantization or performance work; improve correctness only with source-backed native evidence and guard samples.
- Large-stack performance is now a throughput and scheduling issue rather than an OOM blocker. Strict GPU raster uploads use source-sized offscreen textures with shader-side canvas offsets and per-resource staging submission. The host-facing normal render path uses a recursive streaming GPU source provider: the main selector builds a metadata-only GPU source tree/resource plan, then raster/mask tile payloads are decoded and uploaded at point of use inside containers, clipping runs, THROUGH groups, and filters. `ClipSession` holds the opened `.clip` container and batches render-plan raster/mask source metadata, so support checks and the render provider reuse resolved sources instead of reopening the file or rerunning raster/mask source queries per layer. `Test_RealArt.clip` now full-renders and compares against `Test_RealArt.png` without wgpu OOM (`raw_max=5`, `premul_max=2`), but still takes about 89s on this machine. The next native performance step is reducing filter metadata reads, tile decode/upload overhead, and full-canvas intermediate cache cost without introducing CPU compositor fallback, post-processing, or a global all-layer texture cache.
- Native raster extraction now applies render offscreen placement through `LayerRenderOffscrOffsetX/Y`, matching the existing mask placement model and the known `Ref_Terra404_Live2D` negative-X render sources. This removes a structural decode gap before further large-reference GPU work.
- Native raster extraction now decodes full-color, grayscale, and monochrome raster tile streams. `Test_ Grayscale.clip` and `Test_Monochrome.clip` route through the strict GPU path and compare exactly against CSP PNGs.
- Native support diagnostics use a metadata-only strict selector. `clip_cli --gpu-support-check` validates graph, raster source, mask source, and LUT-filter support without tile decode, GPU initialization, or rendering; it must remain diagnostics only, not a fallback renderer.
- `clip_cli --gpu-trace-pixel <x> <y>` is available for native GPU prefix tracing and now includes per-source before/after/input pixels. Current open traces point to a Subtract alpha/rounding boundary feeding Color Dodge/Color Burn in `IllustrationBlendModes.clip`, plus a Pin Light/Hue/Saturation residual in `IllustrationBlendModes2.clip`; rejected broad fixes should remain in evidence, not shader code.
- If the OIIO/native direct-load path is accepted, remove the Python compositor/loader and sidecar PNG implementation instead of keeping compatibility or fallback paths.
- This direction is about flattened raster loading only; it does not restore vector, bubble/frame, or text renderer compatibility.

## Direction 4: Documentation Hygiene

Goal: make new conversations start from the right state quickly.

Current policy:

- Keep `docs/AI_MEMORY.md` as the short current-state memory.
- Keep `docs/analysis.md` as the append-only historical evidence log.
- Keep this file as durable direction, not a running checklist.
