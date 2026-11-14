# AI Memory

Last reconciled: 2026-10-02

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
- `docs/glossary.md` - domain and architecture terms.

Open `docs/analysis.md` only when a task needs long-form historical evidence.

## Current Scope

In scope:

- Flattened raster layers.
- Folders, masks, clipping, opacity, blend modes, and THROUGH/container behavior.
- Adjustment/filter layers covered by current native strict support.
- Simple text layers are rasterized as flattened source pixels by
  `clip_runtime/src/text_render/*` (Skia CPU surfaces, unhinted grayscale,
  system fonts resolved through `fontdb`, no bundled fonts), then composited by
  the wgpu renderer like any raster source. Horizontal text builds a
  `HorizontalTextPlan` and draws positioned text blobs; vertical, CJK upright,
  tate-chu-yoko, arc path, underline, and strikethrough layouts use measured
  constants that live in code and tests, not in this file. The `Text_1`
  through `Text_15` guard matrix is a 99%-practical flattened text state; the
  remaining residuals are raster edge strength and mixed-script glyph placement.
  A shaped-text prototype (`RIZUM_CLIP_SHAPED_TEXT=1`, Skia RunHandler/TextBlob
  model matching CSP's observed `allocRunTextPos`/`drawTextBlob` path) is
  disabled by default because it regresses horizontal guards; CSP's observed
  `SkFont::setHinting(1)` was probed and rejected for the same reason. This is
  not editable text import. Probe history: `docs/analysis.md`.
- Blender generated-image import, reload, pack state, diagnostics, and i18n.
- Native CLI verification against CSP PNG exports.

Out of scope:

- Editable layer import.
- Vector strokes/fills, bubble/frame renderers, 3D layers, animation, and `.clip`
  write-back.
- Python compositor fallback, sidecar PNG workflows, post-processing bandages,
  and full-canvas per-layer caches.

## Accepted Runtime

- The installable Blender add-on is `Rizum Clip Reload`, version `0.9.2`.
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
- Unsupported out-of-scope layer kinds such as vector, bubble/frame, or future
  unknown layer records are skipped during flattened import and recorded in
  diagnostics; they do not abort the import. Simple text layers are rendered as
  flattened pixels by the native path.
- Native text rendering resolves the font names stored in `.clip` against the
  user's installed system fonts, then creates Skia typefaces from those matched
  font bytes. The add-on does not bundle fonts and does not expose extra font
  directory configuration. The renderer is split into focused modules:
  `text_render/font.rs`, `horizontal.rs`, `decoration.rs`, `vertical.rs`,
  `vertical/layout.rs`, `arc.rs`, and `shaped.rs`; `text_render.rs` keeps the
  shared style parser, orchestration, and regression tests.
- Normal UI shows source, status, pack state, `Manual Reload`, `Pack`, errors,
  and `Copy Diagnostic`. Developer Mode shows timing/diagnostic details.
  Funding links stay in README/repository metadata, not inside Blender UI.
- Manual import/reload render failures show a Blender error popup on the main
  thread; initial import failures must not fail silently just because no image
  datablock exists yet.
- UI translations exist for Simplified Chinese, Japanese, and Spanish. Add-on
  name and copied/opened diagnostics stay English.
- Windows x64 packaging is the current maintainer-tested release target.
  Linux x64 and macOS arm64 packages are built as separate zip files and should
  be treated as test candidates until real-device testing. macOS x64 is not a
  release target without an Intel Mac test device.
- Do not upload one universal extension zip containing several native platform
  binaries. Blender Extensions expects separate platform-specific zips.
  `tools/build_blender_addon.py --platform all --output-dir dist` writes one
  zip per platform instead of combining native binaries. The `Build extension
  package` GitHub Actions workflow builds Windows x64, Linux x64, and macOS
  arm64 native artifacts and uploads split platform zips.

## Native Renderer State

- The old Python compositor/loader has been removed from the installable path.
  Use CSP PNG exports plus `clip_cli --compare-png` as the verifier.
- `clip_runtime::ClipSession` keeps a `ClipContainer`, so main rendering does
  not reopen the `.clip` for every layer.
- The main source selector is `select_gpu_normal_render_stack`, a metadata-only
  selector. The old decoded selector remains only for trace/test/debug paths.
- Raster source placement uses the sum of `LayerRenderOffscrOffsetX/Y` and
  `LayerOffsetX/Y`. The metadata parser exposes that effective offset to every
  decoded, sparse-GPU, and reload-manifest path; absent columns default to zero.
- Brightness/Contrast filter payloads store signed brightness followed by signed
  contrast. Brightness is an additive byte-domain clamp before the contrast
  LUT. Do not restore the older white-point/black-point `linear_lut` brightness
  branches: positive brightness in those branches darkened real CSP layers and
  was already rejected by the historical isolated filter sample.
- Main rendering uses recursive provider streaming and a tile-event renderer
  with explicit barrier segments for semantics that are not safely tile-local.
- Tile-event lowering is now a convergence-gated area. Do not add new semantic
  coverage unless `docs/native-tile-event-scoreboard.md` shows the measured
  barrier, tests exist, and `planned_passes` or `barrier_segments` drops.
- `TILE_EVENT_ABI_VERSION` is `10`. Any shader-visible payload layout change
  must bump it and keep reload-manifest compatibility tests.

Strict native support currently covers the project scope: raster sources, simple
text rasterization, folders, masks, clipping, common blend modes, and the
supported adjustment/filter set. Unknown future filters remain explicitly
unsupported.

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
- The 4096x4096 `Tex_LiuJunlang_Clothes` work snapshot (private, not in
  `img/`) contains a masked
  Brightness/Contrast layer with payload `(+25, +20)`. Restoring additive
  brightness reduced its compare from `max=60 / mean=0.732963` to
  `max=7 / mean=0.122501`; the remaining extrema are sparse near-black/source
  quantization residuals, while representative adjusted colors are within one
  LSB of the CSP export.
- `Test_RealArt`, `Ref_Terra404_Live2D`, `Ref_MXL_Idol1`, and `Ref_Kabi_Live2D`
  (gitignored private samples) are visually usable with remaining low-level or known reference residuals.
  Do not retune broad blend formulas from one hotspot without native evidence
  and guard samples.

Rejected fidelity shortcuts:

- Do not reintroduce the broad Python compositor or CPU fallback.
- Do not replace ordinary Multiply/Overlay/SoftLight/ColorBurn with recovered
  formulas unless a targeted sample improves and guards remain stable.
- Do not relax the nested THROUGH guards
  (`planner_lowers_through_inside_nested_container_scope` and
  `planner_lowers_clipped_container_sibling_with_nested_container_through_child`
  in `clip_gpu/src/stream_program_tests.rs`, derived from `Ref_Terra404_Live2D`)
  that keep nested THROUGH groups with direct container children as explicit
  barriers unless a focused legacy-vs-tile test proves faithful output.

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
  evidence changes that. It is opt-in behind
  `RIZUM_CLIP_REGION_RASTER_RESIDENT_ATLAS=1`; the fixture is
  `scripts/benchmark_persistent_reload_cache.py`.

Diagnostic env vars: `RIZUM_CLIP_RENDER_PROFILE`, `RIZUM_CLIP_DECODE_PROFILE`,
and `RIZUM_CLIP_TEXT_PROFILE` (profiling output); `RIZUM_CLIP_PARALLEL_TILE_DECODE`,
`RIZUM_CLIP_SHAPED_TEXT`, and `RIZUM_CLIP_REGION_RASTER_RESIDENT_ATLAS` (opt-in
experiments, all off by default); `RIZUM_CLIP_DISABLE_PERSISTENT_WORKER` and
`RIZUM_CLIP_RENDERER_DLL` (add-on bridge overrides). See
`docs/native-performance-investigation.md` for performance context.

## Verification Bias

Run targeted checks only when the change needs them.

Useful commands:

```powershell
cd native\rust
cargo fmt --all --check
cargo test --workspace -- --test-threads=4
cargo run -q -p clip_cli -- ..\..\img\Test_AddGlowMultiply.clip --compare-png ..\..\img\Test_AddGlowMultiply.png
cargo run -q -p clip_cli -- ..\..\img\Test_ToneCurve.clip --compare-png ..\..\img\Test_ToneCurve.png
```

Python add-on tests (repo root): `python -m unittest discover -s tests`.

Local GPU runs need the thread cap above: every GPU test creates its own wgpu
device, and the default thread count creates dozens at once, which crashes or
deadlocks the `clip_gpu` test process, while CI's software adapter copes
without it. If `clip_gpu` still hangs, rerun it with `--test-threads=1`.

Convergence gate:

```powershell
scripts\verify_native_convergence.ps1 -SkipClipCompare
```
