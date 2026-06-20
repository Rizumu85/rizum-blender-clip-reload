# AI Memory

Last reconciled: 2026-06-20

## Read First

This repository owns the Blender-facing `.clip` importer, native renderer bridge,
and verification harness.

Primary files:

- `clip_studio_importer/__init__.py` - Blender UI, operators, preferences, and
  async import/reload orchestration.
- `clip_studio_importer/native_bridge.py` - native worker/C ABI bridge and
  Blender image upload.
- `clip_studio_importer/image_state.py` - image IDProperties, status, timing,
  source freshness, pack state, and support locator state.
- `clip_studio_importer/worker_protocol.py` - one-shot and persistent native
  worker protocol records.
- `native/rust/` - Rust renderer workspace.
- `tools/build_blender_addon.py` - extension packaging.

Primary docs:

- `docs/plan.md` - current durable plan.
- `docs/native-code-architecture.md` - crate/module boundaries.
- `docs/native-direct-load-rewrite.md` - accepted Blender runtime path.
- `docs/native-tile-event-renderer.md` - current tile-event renderer model.
- `docs/native-tile-event-scoreboard.md` - measured convergence gate.
- `docs/native-performance-investigation.md` - compressed performance evidence.
- `docs/design.md` - Blender UX decisions.
- `docs/analysis.md` - append-only historical evidence and rejected hypotheses.

Open `docs/analysis.md` only when a task needs long-form historical evidence.

## Current Scope

In scope:

- Flattened raster layers.
- Folders, masks, clipping, opacity, blend modes, and THROUGH/container behavior.
- Adjustment/filter layers covered by current native strict support.
- Blender generated-image import, reload, pack state, diagnostics, and i18n.
- Native CLI verification against CSP PNG exports.

Out of scope:

- Editable layer import.
- Vector strokes/fills, text, bubble/frame renderers, 3D layers, animation, and
  `.clip` write-back.
- Python compositor fallback, sidecar PNG workflows, post-processing bandages,
  and full-canvas per-layer caches.

## Accepted Runtime

- The installable Blender add-on is `Rizum Clip Reload`, version `0.8.67`.
- The import menu remains `File > Import > Clip Studio (.clip)`.
- The packaged native worker renders flattened RGBA8 output outside Blender's UI
  process, and Blender uploads it into generated images.
- Initial import and manual reload are asynchronous. Initial import creates no
  placeholder image; the image appears only after real native pixels return.
- Initial import schedules a pack after the first visible image is created.
  Reloads mark the image `Needs Pack`; a `save_pre` handler packs dirty native
  images before saving the `.blend`.
- Missing source files keep the packed pixels visible and mark the image
  `missing_source`.
- Normal UI shows source, status, pack state, `Manual Reload`, `Pack`, errors,
  and `Copy Diagnostic`. Developer Mode shows timing/diagnostic details.
- UI translations exist for Simplified Chinese, Japanese, and Spanish. Add-on
  name and copied/opened diagnostics stay English.
- Windows x64 packaging is maintainer-tested. Linux x64, macOS x64, and macOS
  arm64 packaging support exists but is maintainer-untested.
- `tools/build_blender_addon.py --platform all` can build one universal zip when
  `native/artifacts/<platform>/` contains each platform's native artifacts. The
  `Build extension package` GitHub Actions workflow builds and uploads that
  universal zip.

## Native Renderer State

- The old Python compositor/loader has been removed from the installable path.
  Use CSP PNG exports plus `clip_cli --compare-png` as the verifier.
- `clip_runtime::ClipSession` keeps a `ClipContainer`, so main rendering does
  not reopen the `.clip` for every layer.
- The main source selector is `select_gpu_normal_render_stack`, a metadata-only
  selector. The old decoded selector remains only for trace/test/debug paths.
- Main rendering uses recursive provider streaming and a tile-event renderer
  with explicit barrier segments for semantics that are not safely tile-local.
- Tile-event lowering is now a convergence-gated area. Do not add new semantic
  coverage unless `docs/native-tile-event-scoreboard.md` shows the measured
  barrier, tests exist, and `planned_passes` or `barrier_segments` drops.
- `TILE_EVENT_ABI_VERSION` is `10`. Any shader-visible payload layout change
  must bump it and keep reload-manifest compatibility tests.

Strict native support currently covers the project scope: raster sources,
folders, masks, clipping, common blend modes, and the supported adjustment/filter
set. Unknown future filters remain explicitly unsupported.

## Fidelity Anchors

Representative current native results:

- `Test_Clipping`, `Test_ClippingEdge`, `Test_ToneCurve`, and the newer focused
  Tone Curve samples compare exact.
- `Test_AddGlowMultiply` is down to one-LSB invisible/low-level residuals
  (`raw_max=1`, `premul_max=1`).
- `Test_HSL2` hue-only is exact. `Test_HSL3/4/5` are max-1 visible-zero style
  residuals. Original `Test_HSL` still has low residuals.
- `Test_Gradiation` remains a known Gradient Map residual (`max=10`); prior
  fixed-point interpolation probes traded the max for worse aggregate error.
- `Test_RealArt`, `Ref_Terra404_Live2D`, `Ref_MXL_Idol1`, and `Ref_Kabi_Live2D`
  are visually usable with remaining low-level or known reference residuals.
  Do not retune broad blend formulas from one hotspot without native evidence
  and guard samples.

Rejected fidelity shortcuts:

- Do not reintroduce the broad Python compositor or CPU fallback.
- Do not replace ordinary Multiply/Overlay/SoftLight/ColorBurn with recovered
  formulas unless a targeted sample improves and guards remain stable.
- Do not relax the Terra guard that keeps nested THROUGH groups with direct
  container children as explicit barriers unless a focused legacy-vs-tile test
  proves faithful output.

## Performance State

- Selected-tile CHNKExta reads and sparse decode/upload are in place.
- Tile-local event execution is the main optimization model; barrier passes are
  explicit, counted, and explained.
- Persistent worker reloads keep native process/device state and can return
  full, patch, or no-change results from reload manifests.
- Decode/zlib parallelism was measured and is not the current bottleneck.
- libvips/OIIO/Chromium-inspired diagnostics now exist as profiling/task-graph
  evidence, but no broad task graph or demand pipeline should be added without
  a measured prototype.
- Resident sparse atlas inside `patch_renderer=region` was prototyped and did
  not hit useful cases in the fixed reload fixture; keep it disabled unless new
  evidence changes that.

Use `RIZUM_CLIP_RENDER_PROFILE=1` and
`docs/native-performance-investigation.md` for performance context.

## Verification Bias

Run targeted checks only when the change needs them.

Useful commands:

```powershell
cd native\rust
cargo fmt --all --check
cargo test --workspace
cargo run -q -p clip_cli -- ..\..\img\Test_AddGlowMultiply.clip --compare-png ..\..\img\Test_AddGlowMultiply.png
cargo run -q -p clip_cli -- ..\..\img\Test_ToneCurve.clip --compare-png ..\..\img\Test_ToneCurve.png
```

Convergence gate:

```powershell
scripts\verify_native_convergence.ps1 -SkipClipCompare
```
