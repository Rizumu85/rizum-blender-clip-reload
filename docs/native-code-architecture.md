# Native Code Architecture

Last updated: 2026-06-15

## Goal

Build the native `.clip` loader as a set of deep modules, not a translated
version of the current single-file Python compositor. The accepted renderer path
is Rust plus `wgpu` GPU compositing. Host integration is split by boundary:

- OpenImageIO `ImageInput` adapter for OIIO-level hosts and possible Blender
  ImBuf/source integration.
- Blender image-datablock bridge for stock Blender add-on integration when
  external image filetype registration is unavailable.

The Python loader remains only a temporary external reference while the native
path is under construction. It must not become a compatibility layer or runtime
fallback.

## Milestones

Completed first milestone: OIIO format recognition only.

- Build the smallest `.clip` `ImageInput` plugin.
- Make OpenImageIO discover `.clip` as an image format.
- Return deterministic placeholder image metadata and pixels.

Result: Blender's bundled Python `OpenImageIO` can open the plugin, but
Blender's stock image loader does not pass unknown extensions to external OIIO
plugins. ImBuf uses a static filetype table.

Completed second milestone: Blender image-datablock bridge.

- Use native/OIIO RGBA bytes as the source.
- Create or update a generated Blender `Image`.
- Upload pixels through Blender's bulk pixel API.
- Measure upload cost.

Result: the bridge works in stock Blender 5.0.1 without sidecar PNG output. It
is not a true file-backed image format; reload must be owned by the add-on unless
Blender gains an ImBuf/filetype bridge. The accepted persistence model is to
pack the latest rendered pixels into the `.blend` and store `.clip` source
tracking metadata on the image.

Completed third milestone foundation: native `.clip` data path.

- Implement Rust container probing and minimal metadata extraction.
- Return real canvas metadata and deterministic placeholder pixels through the
  runtime/C ABI boundary.
- Keep OIIO and Blender bridge adapters thin.

Result: `clip_file` owns `CSFCHUNK` walking and in-memory SQLite metadata reads,
`clip_runtime` owns session summary plus deterministic placeholder region reads,
and `clip_capi` exposes the minimal C ABI. `clip_cli` verifies both
`Test_Clipping.clip` and `Ref_Terra404_Live2D.clip` metadata.

Completed fourth milestone foundation: OIIO adapter wired to the Rust C ABI.

- The C++ adapter must call `clip_capi` for open, metadata, and placeholder
  pixels.
- The adapter must stop owning hard-coded image dimensions.
- Do not move parser or renderer behavior into C++.

Result: the adapter uses `clip_renderer_session_open`,
`clip_renderer_session_info`, and `clip_renderer_session_read_rgba8`. Blender
5.0.1's Python OpenImageIO module now sees real `.clip` metadata through the
plugin: `Test_Clipping.clip` opens as 512x512 and `Ref_Terra404_Live2D.clip`
opens as 4800x6100. Pixels are still deterministic native placeholders.

Completed fifth milestone foundation: native raster data extraction below
`clip_file`.

- Parse and decompress `CHNKExta` block payloads.
- Map SQLite layer rows into typed Rust records needed for graph planning.
- Decode one simple full-color raster layer into RGBA bytes for a targeted
  fixture.
- Keep compositing, blend modes, and GPU passes out of this milestone.

Result: `clip_file` can resolve raster layer sources, parse/decompress
`CHNKExta` block streams with `flate2`, decode full-color
`alpha plane + BGRA plane`, grayscale `alpha + gray`, and monochrome
`alpha bitplane + white bitplane` tiles into straight RGBA, and expose
`read_raster_layer_rgba(path, LayerId)` for raster layers. Raster source
metadata reads optional schema columns safely, including
`LayerColorTypeIndex` and `LayerRenderOffscrOffsetX/Y`, and the public decode
API crops/pastes decoded offscreen RGBA onto the canvas before returning.
Fixture coverage locks `Test_Clipping.clip` layer graph records plus layers
`10` and `11`, `Test_ Grayscale.clip` layer `5`,
`Test_Monochrome.clip` layer `7`, and the known negative-X render offsets in
`Ref_Terra404_Live2D`.

Completed sixth milestone foundation: first render-graph planning skeleton.

- Convert typed layer rows into a graph-facing node list.
- Preserve sibling/child order and visibility filtering.
- Identify paper and raster nodes without compositing them.
- Add a runtime/CLI probe for planned node order on `Test_Clipping.clip`.

Result: `clip_graph` owns the planning input and planned node types. It walks the
root layer, child chains, and sibling chains deterministically, filters hidden
subtrees with `LayerVisibility`, detects duplicate ids, missing links, and
cycles, and emits typed nodes for containers, paper, raster, filters, and
unsupported visible layers. `clip_runtime` converts `clip_file` metadata rows
into `clip_graph` inputs and stores a `RenderPlan` in each session. `clip_cli`
reports the plan for `Test_Clipping.clip` as root container `2`, paper `4`,
raster `10`, and clipped raster `11`.

Completed seventh milestone foundation: first `wgpu` resource/readback scaffold.

- Upload one decoded full-color raster layer into a GPU texture.
- Read that texture back through the `clip_gpu` and `clip_runtime` boundary for
  a targeted fixture.
- Keep the pass single-layer only; do not implement blend modes, folder
  isolation, clipping semantics, or CPU compositing fallback in this milestone.

Result: `clip_gpu` owns a real `wgpu 29.0.3` device/queue context and a
single-layer `Rgba8Unorm` texture roundtrip path. It uploads straight RGBA bytes
with `Queue::write_texture`, copies the texture into a padded readback buffer,
maps the buffer, strips row padding, and returns RGBA bytes. `clip_runtime`
exposes `ClipSession::read_raster_layer_rgba_via_gpu(LayerId)` as an explicit
developer probe, and `clip_cli --gpu-roundtrip-layer 10` verifies
`Test_Clipping.clip` layer `10` against the locked decode stats:
nonzero alpha `37151`, sums `[8507579, 5832707, 5832707, 8933976]`.

Completed eighth milestone foundation: planned raster GPU resource cache.

- Resolve visible raster nodes from `RenderPlan` to decoded raster sources.
- Upload each planned raster source as a GPU texture keyed by layer id and render
  mipmap id.
- Report the uploaded resource list through runtime/CLI for `Test_Clipping.clip`.
- Do not implement blend modes, folder isolation, clipping semantics, shader
  compositing, or CPU compositing fallback in this milestone.

Result: `clip_gpu::resource` owns the resource-cache skeleton. It accepts decoded
straight RGBA upload descriptors, pads rows to
`COPY_BYTES_PER_ROW_ALIGNMENT`, writes mapped staging buffers, copies into
`Rgba8Unorm` textures, waits for submission completion, and returns stable
resource info keyed by layer id plus render mipmap id. `clip_runtime` resolves
visible raster `RenderPlan` nodes through `clip_file` and calls the GPU upload
path; `clip_cli --gpu-upload-planned-rasters` reports `Test_Clipping.clip` as
two uploaded resources: node `2` layer `10` mipmap `15`, and node `3` layer
`11` mipmap `16`.

Completed ninth milestone foundation: first GPU output target and shader-pass
skeleton.

- Draw exactly one selected uploaded raster resource into an output texture.
- Read the output texture back through the existing GPU readback path.
- Keep the pass single-resource only; do not implement blend modes, folder
  isolation, clipping semantics, full-stack compositing, or CPU compositing
  fallback in this milestone.

Result: `clip_gpu::pass` owns the first shader pass. It draws one selected
uploaded raster resource with a WGSL full-screen triangle, uses `textureLoad` for
per-pixel source fetches, writes into an `Rgba8Unorm` output texture, and reads
back through the shared GPU readback helper. `clip_runtime` exposes
`ClipSession::draw_raster_layer_rgba_via_gpu(LayerId)` as a developer probe that
requires the layer to be a visible planned raster node and reports exact byte
differences against the decoded source. `clip_cli --gpu-draw-layer 10` and
`--gpu-draw-layer 11` on `Test_Clipping.clip` both return `differing_bytes=0`.

Completed tenth milestone foundation: native-backed simple raster stack pass.

- Draw planned raster resources in render-plan order for the subset whose
  semantics are direct texture replacement/copy.
- Report unsupported nodes and unsupported semantics explicitly.
- Do not implement clipping, non-normal blend modes, folder isolation, masks, or
  CPU compositing fallback in this milestone.

Result: `clip_runtime` now has a strict partial-stack planner for the GPU
developer path. It draws only visible planned raster nodes whose current
semantics are direct texture replacement/copy, and it reports paper, clipping,
mask, opacity, non-normal composite, non-canvas-sized raster, alpha-compositing,
filter, and unsupported layer-kind requirements explicitly. The GPU side uses
`GpuRenderer::draw_raster_stack_to_rgba8` to draw ordered resources through the
shader framework. `clip_cli --gpu-simple-stack` on `Test_Clipping.clip` draws
only node `2` layer `10`, reports paper node `1` and clipped raster node `3` as
unsupported, and returns `differing_bytes_from_last_drawn=Some(0)`.

Completed eleventh milestone foundation: NORMAL alpha-over shader pass.

- Composite planned straight RGBA raster resources in render-plan order using
  NORMAL alpha-over.
- Keep the supported subset strict: no clipping, masks, non-normal blend modes,
  folder isolation semantics, or CPU compositing fallback.
- Continue reporting unsupported nodes and unsupported semantics explicitly.

Result: `clip_gpu::pass` now owns a WGSL NORMAL alpha-over pass that keeps
straight RGBA output by ping-ponging two `Rgba8Unorm` accumulation textures.
`clip_runtime` shares one strict selector between the direct-copy probe and the
NORMAL stack probe; the NORMAL path allows stacked alpha compositing while still
rejecting clipping, masks, opacity, non-normal composites, paper, filters,
unsupported layer kinds, non-canvas rasters, and non-trivial container semantics.
Unsupported containers also block their child subtree so folder semantics are
not bypassed. `clip_cli --gpu-normal-stack` verifies `Test_Clipping.clip`,
`Test_ToneCurve.clip`, and `Illustration4K.clip`; the 4K sample draws three
NORMAL raster layers with no unsupported nodes.

Completed twelfth milestone foundation: paper/background and LayerOpacity for
the strict NORMAL GPU path.

- Parse or surface the metadata needed for paper/background colour.
- Add shader/runtime inputs for `LayerOpacity` on otherwise supported NORMAL
  rasters.
- Keep clipping, masks, non-normal blend modes, folder isolation semantics, and
  CPU compositing fallback unsupported until dedicated native models exist.

Result: Paper colour is decoded in `clip_file::metadata` using the same observed
schema order as the Python importer: `DrawColorMain*` when enabled/nonzero, then
`LayerThumbnail.ThumbnailMainColor*`, then `LayerPalette*`. The colour travels
through graph nodes as `Rgba8`. `clip_gpu` now has `GpuNormalStackSource` for
ordered raster and solid-colour sources, and the NORMAL shader applies per-source
opacity before alpha-over. `Test_Clipping.clip --gpu-normal-stack` draws Paper
plus layer `10` and reports only the clipped raster unsupported.
`Test_Opacity.clip --gpu-normal-stack` draws Paper plus two NORMAL rasters,
including `LayerOpacity=128`, with no unsupported nodes.

Completed thirteenth milestone foundation: host-facing strict NORMAL read path.

- Replace deterministic placeholder region reads with native GPU output for the
  strict supported subset.
- Cache the rendered image inside `ClipSession` after the first read.
- Return an explicit unsupported-plan error instead of returning a partial image.

Result: `ClipSession::read_rgba8_region()` renders the strict NORMAL GPU stack
once, caches the full image, and copies requested regions from that cache.
If the plan contains unsupported semantics, it returns `UnsupportedRenderPlan`.
The C ABI test reads a real Paper pixel from `Test_Opacity.clip`
(`[226,226,226,255]`), while CLI probes continue to report unsupported plans
without blocking developer flags.

Completed fourteenth milestone foundation: layer masks for the strict NORMAL
GPU path.

- Decode mask mipmap resources below `clip_file`.
- Upload mask textures alongside raster resources.
- Multiply source alpha by mask alpha in the NORMAL shader.
- Keep clipping, non-normal blend modes, folder isolation semantics, and CPU
  compositing fallback unsupported until dedicated native models exist.

Result: `clip_file::read_layer_mask_alpha()` resolves `LayerLayerMaskMipmap`,
decodes single-channel mask tiles, applies Offscreen `InitColor` for omitted
chunks, and pastes/crops through `LayerMaskOffscrOffsetX/Y` to canvas size.
`clip_gpu` uploads mask resources as `R8Unorm` textures keyed by layer id plus
mask mipmap id, and the NORMAL shader multiplies source alpha by the sampled
mask value. `clip_runtime` now treats masked NORMAL rasters as supported in the
strict NORMAL path, while `--gpu-simple-stack` still reports masks unsupported.
`Test_Mask.clip --gpu-normal-stack` reports three sources, two raster resources,
one mask resource, and zero unsupported nodes.

Completed fifteenth milestone foundation: clipping runs for the strict NORMAL
GPU path.

- Model a base layer followed by clipped siblings as a native-owned clipping
  run, not as independent flattened siblings.
- Reuse the existing strict source model for Paper, opacity, and masks inside
  the run where the native model allows it.
- Keep non-normal blend modes, folder isolation semantics, filters, and CPU
  compositing fallback unsupported until dedicated native models exist.

Result: `clip_runtime` now consumes a same-depth raster base plus following
raster clipped siblings as one `GpuNormalStackSource::ClippingRun`. `clip_gpu`
renders the base into a white-transparent clipping cache, applies NORMAL clipped
siblings with a preserve-RGB pass that does not grow the base alpha, and resolves
the cache back into the main stack through NORMAL alpha-over. `Test_Clipping.clip
--gpu-normal-stack` reports `sources=2`, two raster resources, and zero
unsupported nodes. `Test_ClippingEdge.clip --gpu-normal-stack` reports
`sources=1`, two raster resources, and zero unsupported nodes; alpha matches the
CSP PNG and transparent pixels retain white RGB.

Completed sixteenth milestone foundation: strict NORMAL folder/container
isolation.

- Render supported child stacks into an owned offscreen cache before resolving
  the folder into its parent stack.
- Reuse Paper, opacity, masks, and clipping runs inside the folder cache only
  when all children stay inside the strict NORMAL subset.
- Keep THROUGH groups, non-normal blend modes, filters, unsupported container
  semantics, and CPU compositing fallback unsupported until their dedicated
  native models exist.

Result: `GpuNormalStackSource` now has a recursive `Container` source.
`clip_gpu` renders container children into a white-transparent cache, then
resolves that cache into the parent with container opacity, optional container
mask, and a container blend mode. `clip_runtime` uses a recursive strict
selector, treats the root container as structural pass-through, and represents
non-root containers whose `LayerComposite` maps to a supported raster blend as
container sources. Verification covers the GPU container cache opacity path,
Multiply container resolve, the selector's folder source shape, and a
real-raster synthetic e2e test that wraps `Test_Clipping.clip`'s actual
Paper/raster/clipped-raster stack in a NORMAL folder and checks exact equality
with the flat output. `clip_cli --plan-only` now prints metadata and planned
nodes without triggering host render, so large references can be scanned safely.
Completed seventeenth milestone foundation: THROUGH groups for the strict GPU
path.

- Model THROUGH groups from native/Python evidence, not as ordinary NORMAL
  isolated folders.
- Render children against the parent stack contribution, then constrain the
  before/after delta with the THROUGH group mask and opacity.
- Keep non-normal blend modes, filters, unsupported container semantics, and CPU
  compositing fallback unsupported until dedicated native models exist.

Result: `GpuNormalStackSource` now has a recursive `ThroughGroup` source.
`clip_gpu` renders THROUGH children against the current parent contribution and
uses a dedicated before/after resolve pass so group opacity and masks affect only
the contribution delta. `clip_runtime` maps `LayerComposite=30` containers to
THROUGH sources instead of reporting them as unsupported containers. Verification
covers GPU opacity blending, runtime selector shape, and both existing folder
fixtures: `Test_FolderNested.clip` and `Test_FolderVisibility.clip` now report
`unsupported=0`, and C ABI full-image comparisons against CSP PNGs are
`raw_max=1` / `premul_max=1`.

Current eighteenth milestone: non-NORMAL raster blend-mode support for the
strict GPU path.

- Add GPU/runtime structure for ordinary raster sources whose
  `LayerComposite` is not NORMAL.
- Enable each blend mode only after verifying the `.clip LayerComposite` to
  native behavior mapping and preserving guard samples.
- Keep filters, unsupported container semantics, and CPU compositing fallback
  unsupported until dedicated native models exist.

Progress:

- Ordinary, non-clipped raster `LayerComposite=1` Darken, `2` Multiply,
  `3` Color Burn, `4` Linear Burn, `5` Subtract, `6` Darker Color,
  `7` Lighten, `8` Screen, `9` Color Dodge, `10` Glow Dodge, `11` Add,
  `12` Add Glow, `13` Lighter Color, `14` Overlay, `15` Soft Light,
  `16` Hard Light, `17` Vivid Light, `18` Linear Light, `19` Pin Light,
  `20` Hard Mix, `21` Difference, `22` Exclusion, `23` Hue,
  `24` Saturation, `25` Color, `26` Brightness/Luminosity, and `36` Divide
  are supported.
- `GpuNormalRasterSource` carries a blend mode, and the renderer selects a
  byte-domain WGSL pass or the standard blend-over WGSL pass for supported
  non-NORMAL sources.
- The NORMAL alpha-over pass explicitly rounds to the u8 grid before writing
  `Rgba8Unorm`, so later byte-domain blend passes do not inherit
  backend-dependent UNORM truncation.
- The standard blend pass quantizes the pure blend target to the u8 grid before
  alpha-over.
- Darker Color and Lighter Color use Rec.709 luma
  (`0.2126/0.7152/0.0722`) for source/destination comparison. HSL modes keep
  the W3C-style luminosity function and quantize `set_sat` tiny spans
  (`max-min <= 2/255`) to min/max channel membership.
- Clipping runs can now use a non-NORMAL base plus clipped siblings for all
  currently supported raster blend modes. The base renders into its owned cache
  with NORMAL, clipped standard modes preserve the base alpha through the
  standard preserve shader, clipped Add Glow/Color Burn/Color Dodge/Glow Dodge
  use a byte-domain preserve shader, and the completed cache resolves into the
  parent with the base blend mode.
- Isolated containers can now use supported non-NORMAL blend modes when their
  completed cache resolves into the parent stack. The selector also tracks
  clip-base validity: THROUGH groups clear the clip base, and a clipped raster
  with no effective base is routed as an ordinary raster source.
- Strict GPU raster uploads now use source-sized offscreen textures with
  shader-side canvas offsets instead of expanding every decoded raster to the
  canvas. Upload staging buffers are submitted and released per resource, so
  staging memory is not retained for the whole stack.
- The host-facing normal render path now uses recursive provider streaming.
  Runtime builds a metadata-only GPU source tree and resource plan, then
  `clip_gpu` requests raster/mask resources from a provider at point of use
  inside containers, clipping runs, THROUGH groups, and filters. Each encoded
  source submits/polls before its temporary uploaded textures are dropped.
  `ClipSession` keeps the opened `ClipContainer`, and provider decode uses
  from-container raster/mask helpers instead of reopening the `.clip` file per
  layer. A render-only optimization can elide an initial terminal
  Normal/opacity=1/no-mask container when the parent contribution is empty;
  support checks, selector tests, and trace diagnostics keep the original
  container structure.
- `Test_RealArt.clip --gpu-support-check` now reports `unsupported=0` and the
  metadata-only resource stats are explicit: 343 raster sources total roughly
  `33.5GB` of RGBA texture data if held globally, plus six masks totalling about
  `151MB`. A release `--compare-png ..\..\img\Test_RealArt.png` probe now
  completes without wgpu OOM at `raw_max=5` / `premul_max=2`, but still takes
  about 89s on this machine. The next practical blocker is throughput: repeated
  per-layer SQLite metadata queries, tile decode/upload, and full-canvas
  intermediate caches are too slow for interactive use, so optimization should
  target faithful scheduling/resource reuse rather than CPU fallback or
  post-processing.
- `clip_cli --gpu-trace-pixel <x> <y>` samples the native GPU output after each
  top-level strict source prefix. This is developer instrumentation over the
  real GPU renderer, not a CPU compositor, oracle, fallback, or post-processing
  path. It also prints before/after RGBA plus the raw source RGBA and mask alpha
  at that pixel for the source being applied.
- `clip_cli --gpu-support-check` runs a metadata-only strict selector that
  validates graph, raster source, mask source, and LUT-filter support without
  decoding raster/mask tile pixels, creating a GPU device, or rendering an
  image. It is developer support diagnostics, not a renderer, fallback, or
  compositor path.
- `clip_cli --compare-png <ref.png>` renders through the same host-facing
  native GPU path as adapters and compares the result with a CSP-exported PNG in
  raw and premultiplied byte domains. This is developer verification tooling
  only; it does not introduce a CPU compositor, runtime fallback, or
  post-processing path.
- `Test_HardMix.clip`, `Test_ColorBurn.clip`, `Test_GlowDodge.clip`,
  `Test_ColorDodge.clip`, `Test_FolderNested.clip`,
  `Test_FolderVisibility.clip`, `Test_Clipping.clip`, and
  `Test_SoftLight.clip`, `Test_ Grayscale.clip`, and
  `Test_Monochrome.clip` compare exactly against CSP PNGs through the C ABI.
  `Test_Hue.clip`, `Test_VividLight.clip`, `Test_AddGlow.clip`, and
  `Test_Mask.clip` remain within `raw_max=1` / `premul_max=1`;
  `Test_Saturation.clip` and `Test_Color.clip` remain within `raw_max=2` /
  `premul_max=2`. `Test_AddGlowMultiply.clip` now routes without unsupported
  nodes and compares at `raw_max=5` / `premul_max=3`.
- `IllustrationBlendModes.clip --gpu-normal-stack` and
  `IllustrationBlendModes2.clip --gpu-normal-stack` now report
  `unsupported=0`. C ABI comparisons remain diagnostic rather than exact:
  `IllustrationBlendModes.png` is currently `raw_max=72`,
  `premul_max=72`, `premul_visible_px=16946`; `IllustrationBlendModes2.png`
  is currently `raw_max=8`, `premul_max=8`,
  `premul_visible_px=29259`. Current trace evidence points
  `IllustrationBlendModes.png` at a narrow Subtract -> Color Dodge -> Color Burn
  interaction around `(266,244)` and `IllustrationBlendModes2.png` at a
  Pin Light/Hue/Saturation chain around `(427,138)`.

Completed nineteenth milestone foundation: native LUT adjustment/filter GPU
pass.

- Read `FilterLayerInfo` records for filter layers without teaching `clip_gpu`
  about SQLite or `.clip` storage.
- Support the LUT-style filter types whose Python-side formulas reduce to a
  faithful 1D LUT or luminosity LUT: Brightness/Contrast, Level Correction,
  Tone Curve, Invert/Reverse Gradient, Posterization, and Gradient Map.
- Keep unsupported filter types explicit until each has a dedicated native
  parameter model and shader.

Result: `clip_file::metadata` exposes `read_filter_layer_source_from_sqlite`
for filter type and payload extraction. `clip_runtime/src/filter_lut.rs` owns
filter-payload parsing and byte-domain 256-entry LUT construction for
`FilterLayerInfo` type `1` Brightness/Contrast, type `2` Level Correction,
type `3` Tone Curve, type `6` Invert/Reverse Gradient, type `7`
Posterization, and type `9` Gradient Map. Runtime accepts those filters when
their composite/mask/opacity semantics are in the strict supported subset,
uploads any layer mask through the existing mask cache, and passes a LUT mode
to `clip_gpu`: RGB channel indexing for channel-wise filters and luminosity
indexing for Gradient Map. `clip_gpu` applies the LUT in one dedicated wgpu
filter pass against the accumulated straight RGBA image while preserving alpha.
HSL (`4`), Color Balance (`5`), and Threshold (`8`) remain unsupported because
they need dedicated native models/shaders rather than the existing channel LUT
pass. `Test_ToneCurve.clip --gpu-normal-stack` and
`Test_Gradiation.clip --gpu-normal-stack` report `unsupported=0` with one
filter mask each; C ABI comparisons match the Python verifier baselines:
`Test_ToneCurve` `raw_max=17` / `premul_max=17`, and `Test_Gradiation`
`raw_max=10` / `premul_max=10`.

## Repository Layout

```text
native/
  README.md
  rust/
    Cargo.toml
    crates/
      clip_model/      # Pure domain types: canvas, rects, layer ids, modes.
      clip_file/       # .clip container, SQLite metadata, external tile decode.
      clip_graph/      # Render graph and tile dependency planning.
      clip_gpu/        # wgpu device, resources, shaders, passes, readback.
      clip_runtime/    # Session orchestration and cache lifetime.
      clip_capi/       # Small C ABI exported to the C++ OIIO adapter.
      clip_cli/        # Developer CLI for benchmarks and reference comparisons.
  oiio/
    README.md          # C++ OIIO ImageInput adapter contract.
  spikes/
    blender-image-bridge/
      README.md        # Stock Blender generated Image upload probe.
```

## Module Contracts

### `clip_model`

Pure data model. It knows nothing about files, SQLite, GPU, OIIO, Blender, or
tests.

Owns:

- Canvas size and rectangles.
- Straight RGBA byte pixel conventions.
- Layer ids, layer kinds, visibility, opacity, blend mode names.
- Small value invariants shared by all other crates.

Does not own:

- File parsing.
- Render graph decisions.
- GPU resources.

### `clip_file`

The only module that understands `.clip` storage details.

Owns:

- `CSFCHUNK` walking.
- `CHNKSQLi` extraction.
- `CHNKExta` indexing and block decompression.
- SQLite row mapping into `clip_model` values.
- Source tile decode for raster, mask, grayscale, and monochrome tiles.
- Full-color raster extraction through `read_raster_layer_rgba`.
- Filter-layer payload extraction through typed metadata helpers.

Does not own:

- Layer compositing semantics.
- GPU pipeline creation.
- OIIO entry points.

### `clip_graph`

Turns model-level layers into a deterministic render plan.

Owns:

- Visibility filtering.
- Folder/group structure.
- Clipping run ownership.
- Tile dependency planning.
- Stable node ids for cacheable graph nodes.

Does not own:

- `.clip` bytes.
- GPU execution.
- Blend shader code.

### `clip_gpu`

The only module that talks to `wgpu`.

Owns:

- Adapter/device/queue setup.
- Texture and buffer allocation.
- Shader modules and pipeline layouts.
- Layer upload strategy.
- GPU compositing passes.
- GPU adjustment/filter passes.
- Final RGBA readback for OIIO.

Does not own:

- `.clip` metadata interpretation.
- OIIO plugin symbols.
- Python-compatible fallback rendering.

### `clip_runtime`

The orchestration module.

Owns:

- Opening a `.clip` session.
- Holding parsed file state, render graph, GPU renderer, and caches together.
- Public Rust interface for reading full images or regions.
- Error conversion into stable runtime errors.

Does not own:

- Low-level file parsing implementation.
- Shader code.
- C ABI details.

### `clip_capi`

Small, explicit ABI for C++.

Owns:

- Public C header at `native/rust/crates/clip_capi/include/clip_capi.h`.
- `extern "C"` functions.
- Opaque handles.
- Error code and error string conversion.
- ABI versioning.

Does not own:

- Rendering decisions.
- OIIO classes.

### `oiio`

C++ adapter only.

Owns:

- OpenImageIO plugin registration.
- `ImageInput` subclass and ImageSpec setup.
- Calls into `clip_capi`.

Does not own:

- `.clip` parsing.
- GPU implementation.
- Sidecar PNG output.

### Blender Datablock Bridge

This is the stock Blender integration boundary if Blender cannot be taught about
`.clip` through a public filetype registration API.

Owns:

- Creating and updating `bpy.types.Image` datablocks.
- Passing file paths and reload events to the native runtime.
- Uploading final RGBA bytes returned by the native runtime.
- Packing the latest rendered pixels into the `.blend` by default.
- Storing and reading `.clip` source-tracking custom properties on images.
- User-facing import/reload UI.

Does not own:

- `.clip` parsing.
- Tile decoding.
- Compositing.
- Sidecar PNG writing.
- Pixel post-processing to hide renderer defects.

Persistence rules:

- The image datablock is generated/packed, not file-backed to a sidecar PNG.
- The packed pixels are the last known rendered result and must remain visible
  after reopening the `.blend`, even if the `.clip` source is missing.
- Custom properties must identify the source `.clip` and render freshness. Use
  stable project-prefixed names such as:
  - `rizum_clip_source`
  - `rizum_clip_source_mtime_ns`
  - `rizum_clip_source_hash`
  - `rizum_clip_width`
  - `rizum_clip_height`
  - `rizum_clip_renderer_version`
  - `rizum_clip_reload_status`
- On Blender `load_post`, the add-on scans images with `rizum_clip_source`. If
  the source exists and is newer/different, it asks the native runtime to render
  and updates the image pixels. If the source is missing, it keeps the packed
  pixels and reports the missing source.
- Repacking after successful reload is the default so the `.blend` remains
  self-contained. A future user option may disable packing to reduce `.blend`
  size, but the default path is packed.

## Dependency Direction

```text
clip_model
  -> clip_file
  -> clip_graph -> clip_gpu

clip_file + clip_graph + clip_gpu
  -> clip_runtime
  -> clip_capi
  -> C++ OIIO adapter
```

Rules:

- `clip_model` has no project-local dependencies.
- `clip_file` and `clip_graph` may depend on `clip_model`, but not on each
  other unless a narrow interface is documented first.
- `clip_gpu` may depend on `clip_model` and `clip_graph`.
- `clip_runtime` is the only place allowed to combine file, graph, and GPU
  modules.
- `clip_capi` depends on `clip_runtime`; the C++ OIIO adapter depends only on
  the C ABI.

## Runtime Flows

Target OIIO/ImBuf flow if Blender gains an explicit `.clip` bridge:

1. C++ OIIO adapter receives a `.clip` path.
2. Adapter calls `clip_renderer_session_open`.
3. Rust runtime parses container metadata and builds a render graph.
4. Runtime initializes the selected `wgpu` backend.
5. OIIO asks for image pixels.
6. Runtime schedules tile decode and GPU compositing.
7. GPU renders into an offscreen texture/buffer.
8. Runtime reads back RGBA bytes.
9. Adapter returns pixels to OIIO/Blender.

Target stock Blender add-on flow:

1. Blender add-on receives a `.clip` path.
2. Add-on calls the native runtime through a small extension boundary.
3. Rust runtime parses metadata, builds the graph, and runs the `wgpu`
   compositor.
4. Runtime returns final RGBA bytes and image metadata.
5. Add-on creates or updates a generated `bpy.types.Image`.
6. Add-on uploads the RGBA bytes with Blender's bulk pixel API.
7. Add-on packs the updated rendered pixels into the `.blend` and records source
   freshness custom properties.

This flow is still a native renderer path. Python only owns Blender UI and
datablock wiring.

## Anti-Monolith Rules

- No source file owns more than one of: container parsing, SQLite mapping, tile
  decode, graph planning, GPU resource setup, shader dispatch, ABI conversion,
  OIIO class glue.
- Root files are wiring only.
- Spikes cannot land as production code until split into the module layout.
- No Python compatibility path may be introduced in native code.
- No sidecar PNG writer may be introduced in native code except a developer-only
  CLI output command for tests and benchmarks.

## Test Strategy

Native runtime tests compare against external references:

- CSP-exported PNGs.
- Current Python loader output while it exists.
- Small hand-built `.clip` fixtures for parser and graph invariants.

There is no native CPU compositor oracle. The GPU compositor is the only native
renderer path. Use `clip_cli --compare-png <ref.png>` for native/CSP PNG
smoke checks instead of ad-hoc local comparison scripts when the strict GPU plan
supports the file.
