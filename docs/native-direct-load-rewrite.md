# Native Direct-Load Rewrite

Last updated: 2026-06-16

## Decision

Build a native `.clip` renderer that avoids sidecar PNGs and the Python
compositor. The renderer boundary is native; the Blender boundary depends on
what stock Blender exposes.

Chosen stack:

- Rust renderer core.
- `wgpu` GPU compositing.
- Small explicit C ABI for host adapters.
- Thin C++20 OpenImageIO `ImageInput` adapter for OIIO-level hosts and for a
  possible Blender ImBuf/source bridge.
- Blender image-datablock bridge for stock Blender if no public runtime
  filetype registration exists.
- Stock Blender image-datablock bridge stores the `.clip` source path plus
  freshness metadata as custom image properties, keeps rendered pixels in a
  generated image datablock, and packs dirty native images on explicit `Pack
  Now` or Blender `save_pre`.

When native direct-load is accepted, remove the Python compositor/loader and
sidecar PNG workflow. Do not keep compatibility paths or runtime fallbacks. A
Blender Python add-on may still own UI, installation, file watching, and image
datablock updates, but it must not parse `.clip`, composite pixels, or write
sidecar images.

For the stock Blender bridge, the accepted persistence model is:

- Create or update a generated `bpy.types.Image`.
- Upload native-rendered RGBA pixels into that image.
- Initial import packs after the completed image is shown. Reload renders mark
  the image as needing pack; `Pack Now` packs the current pixels immediately,
  and Blender `save_pre` packs dirty native images before the file is saved.
- Store source tracking properties on the image, including the original `.clip`
  path, source mtime, source size, source SHA-256, canvas dimensions, renderer
  version, and reload status.
- On `load_post`, scan images with these properties. If the source `.clip`
  exists and is newer/different, queue the native renderer and replace the image
  pixels when the background render finishes.
- If the source `.clip` is missing, keep the packed pixels visible and report the
  missing source instead of clearing the image.

This is still not a true file-backed Blender image format. `Image.reload()` and
generic external image auto-reload add-ons do not own this path; reload belongs
to this add-on unless Blender gains an ImBuf/source bridge.

Architecture details: `docs/native-code-architecture.md`.

## Completed First Milestone

Prove that OpenImageIO can recognize `.clip` as a native image format in the
Blender/OIIO path.

Scope:

- Build the smallest possible OIIO `ImageInput` plugin for `.clip`.
- Register the `.clip` extension.
- Open a `.clip` file and return a deterministic placeholder `ImageSpec` and
  pixel buffer.
- Verify whether Blender can discover the plugin and load a `.clip` path as an
  image without using a sidecar PNG.

This milestone is only about format plumbing. It should not port the Python
compositor, parse full layer semantics, or add fallback behavior.

Current artifact:

- `native/oiio/` contains a minimal C++20 `ImageInput` plugin and probe
  executable source.
- The plugin has been built against Blender 5.0.1's bundled OpenImageIO 3.0.9.1
  and verified through Blender's Python `OpenImageIO` module.
- Blender's stock image loader does not accept `.clip` through the external OIIO
  plugin alone. ImBuf uses a static file type table and only calls OIIO from
  explicit built-in format bridges such as PSD.

First milestone conclusion:

- OIIO-level `.clip` plugin loading is feasible.
- Blender ordinary `bpy.data.images.load(".clip")` direct-load is not achieved
  by an external OIIO plugin alone.
- Blender's runtime image-loader filetype table is static. There is no public
  add-on API found so far for registering a new ImBuf image filetype.
- The next milestone is a Blender image-datablock bridge proof. If true stock
  `bpy.data.images.load(".clip")` behavior is required later, the faithful path
  is a Blender source patch or upstream ImBuf bridge, not an external OIIO
  plugin alone.

## Completed Second Milestone

Prove that native-rendered RGBA bytes can update a Blender image datablock
without sidecar PNG output.

Scope:

- Use the OIIO placeholder plugin as the temporary native pixel source.
- Create a generated `bpy.types.Image` from the add-on side.
- Upload RGBA bytes with Blender's bulk pixel API.
- Measure upload cost for representative sizes.

This milestone does not parse `.clip` layers and does not reintroduce a Python
compositor. It only verifies the Blender API boundary for the accepted stock
Blender path.

Second milestone result:

- The spike at `native/spikes/blender-image-bridge/` opens the milestone OIIO
  placeholder plugin from Blender 5.0.1, reads RGBA bytes, creates a generated
  `bpy.types.Image`, and uploads pixels with `foreach_set`.
- No sidecar PNG is written.
- The bridge does not make `.clip` a true file-backed Blender image format.
  `Image.reload()` and external image auto-reload add-ons will not work unless
  the add-on owns reload and updates the datablock.
- Accepted UX persistence for this bridge is packed rendered pixels in the
  `.blend` plus `.clip` source-tracking custom properties. Reopening a `.blend`
  must show the packed last render immediately, then reload from source if the
  add-on detects that the `.clip` changed.
- Synthetic timing shows the bridge is viable, but Python-side `uint8 ->
  float32` conversion is the main large-image cost: 4096x4096 measured
  `125.927 ms` conversion and `58.130 ms` Blender upload on this machine.

## Completed Blender Add-on Bridge Milestone

Move the stock Blender datablock bridge from spike into the installable add-on
and remove the add-on Python sidecar runtime path.

Scope:

- Keep `.clip` parsing, tile decode, and compositing inside the native Rust
  runtime.
- Add a thin Python bridge that calls the Rust C ABI and uploads final RGBA8
  pixels into Blender.
- Create or update generated `bpy.types.Image` datablocks and mark successful
  renders as needing pack.
- Store `.clip` source/render metadata on native images so manual reload and the
  background watcher can update the same datablock.
- Remove the installable add-on's Python compositor/sidecar path once the native
  bridge owns import, reload, source tracking, and packaging.

Result:

- `clip_studio_importer/native_bridge.py` loads `clip_capi` with `ctypes`,
  checks ABI version `1`, opens native sessions, reads image metadata, renders
  full-canvas RGBA8 pixels through `clip_renderer_session_read_rgba8`, and
  converts those bytes to Blender float pixels for `foreach_set`.
- Native imports render through the packaged out-of-process `clip_cli` worker,
  create generated images without sidecar PNGs, auto-pack the first completed
  import after it is shown, mark reload renders as needing pack, and store
  source path, source mtime, source size, source SHA-256, canvas metadata,
  renderer ABI, renderer version, reload status, and pack status custom
  properties. The installable add-on zip includes the local
  release `clip_cli` worker plus `clip_capi` library under
  `clip_studio_importer/native/`; preferences expose reload/debug controls
  instead of successful packaged-worker status or a user renderer override. The direct
  `clip_capi` path remains internal development/test plumbing because
  in-process wgpu rendering can crash Blender's UI redraw path on Blender
  5.0.1/NVIDIA.
- Initial import, `Reload`, and the non-blocking watcher update
  images through the C ABI/generated-image path only. Initial import waits for
  the worker to return real canvas pixels before creating and showing the
  generated Blender image, then schedules initial pack as a separate main-thread
  timer, avoiding a confusing temporary placeholder while Blender remains
  responsive.
- Blender `load_post` now scans native images, checks the stored source
  mtime/size/hash against the current `.clip`, queues a native refresh when the
  source changed or stored freshness metadata is missing, and records
  `missing_source` while keeping packed pixels visible if the source is gone.
  A persistent `save_pre` handler packs dirty native images before saving the
  `.blend`, and the Image Editor panel exposes `Pack Now`.
- `clip_studio_importer/__init__.py` no longer imports `clip_loader`, no longer
  exposes a `Use native renderer` off switch, and no longer writes or reloads
  sidecar PNGs. `tools/build_blender_addon.py` packages only `__init__.py`,
  `native_bridge.py`, and native libraries under `clip_studio_importer/native/`.
  The duplicate `clip_studio_importer/clip_loader.py` package copy has been
  removed; the project-root `clip_loader.py` remains reference verification
  tooling outside the add-on runtime.
- Native reload diagnostics are image-level metadata. Background, import, and
  manual renders track elapsed/last render duration metadata. Render failures
  store `clip_reload_status=error` plus `clip_reload_error`, successful renders
  clear old errors, missing sources store `missing_source`, pack state is stored
  as `clip_pack_status`, and the Image Editor panel displays readable
  status/error/timing/pack messages. Renderer version and other native metadata
  are kept out of the normal UI and are available through copied/opened
  diagnostics when they help issue reports.
- `clip_renderer_session_support_info` exposes the runtime metadata-only support
  selector through the C ABI. The C report includes a summary line plus the
  unsupported layer/node list. The Python bridge stores support status and
  structured unsupported details on the Blender image, but the Image Editor
  panel only shows support UI when unsupported nodes exist: a compact locator
  summary, a short detail preview, and copy/open diagnostics actions. Full
  native-support summaries, source/resource counts, largest raster/mask
  metadata, and renderer version are not normal UI labels. Unsupported detail
  lines are parsed into structured layer records with layer id, optional layer
  name, node id, kind, and reason, so copied diagnostics can identify source
  layers for issue reports without adding Blender-side layer navigation.
- Unit coverage uses fake Blender image/data objects to lock image creation,
  pixel upload, source metadata, packing, size-mismatch rejection, and native
  source freshness states. A direct Python smoke against
  `native/rust/target/release/clip_capi.dll` and `img/Test_Clipping.clip`
  returned `512x512`, ABI `1`, and first pixel `[0,0,224,255]`.

Remaining bridge work:

- Improve user-facing diagnostics beyond the current image-level status/error
  and unsupported-node locators, especially clearer supported-but-imperfect
  fidelity residuals. Do not add layer navigation to the Blender add-on.
- `clip_cli --gpu-support-check` and `clip_cli --gpu-support-json` expose the
  metadata-only support check for developer diagnosis and automation. The text
  and JSON outputs include layer names for unsupported nodes and largest
  raster/mask resource layer ids when available. These are diagnostic outputs
  for automation and issue capture, not renderers or fallback paths.

## Completed Third Milestone Foundation

Start the native data path without touching the Python compositor:

- Implement Rust `.clip` container probing and minimal metadata extraction.
- Return real canvas dimensions and a deterministic native placeholder through
  the runtime/C ABI boundary.
- Keep OIIO and Blender bridge adapters thin; they should only call the native
  runtime and move pixels into the host.
- Do not port blend modes or layer compositing until file/container ownership is
  clean.

Third milestone result:

- `clip_file` walks `CSFCHUNK`, validates chunk bounds, extracts `CHNKSQLi`, and
  indexes `CHNKExta` external ids.
- `clip_file` opens the embedded SQLite database in memory with `rusqlite`
  `deserialize_read_exact`; it does not write a temporary SQLite file.
- `clip_runtime` opens a session, exposes real canvas metadata, and returns a
  deterministic RGBA placeholder region.
- `clip_capi` exposes `open`, `info`, `read_rgba8`, `close`, ABI version, and
  thread-local last-error functions.
- `clip_cli` verifies the path:
  `Test_Clipping.clip -> 512x512, root_layer=2, layers=4, external_data=7`;
  `Ref_Terra404_Live2D.clip -> 4800x6100, root_layer=2, layers=1212,
  external_data=2243`.

## Completed Fourth Milestone Foundation

Wire the C++ OIIO adapter to the Rust C ABI:

- Replace the OIIO adapter's hard-coded 64x64 placeholder with
  `clip_renderer_session_open` and `clip_renderer_session_info`.
- Return real `.clip` dimensions in `ImageSpec`.
- Read deterministic placeholder pixels through `clip_renderer_session_read_rgba8`.
- Keep all `.clip` parsing in Rust; the C++ adapter must remain glue only.
- Re-run the Blender/OIIO and Blender image-datablock bridge probes.

Fourth milestone result:

- `native/rust/crates/clip_capi/include/clip_capi.h` is the C ABI header used by
  the C++ adapter.
- The C++ OIIO adapter now opens `.clip` files through
  `clip_renderer_session_open`, fills `ImageSpec` from
  `clip_renderer_session_info`, and reads scanlines through
  `clip_renderer_session_read_rgba8`.
- The adapter no longer owns hard-coded image dimensions. It still returns
  deterministic placeholder pixels until the real renderer exists.
- `IOProxy` support is intentionally disabled for now because the Rust ABI opens
  filesystem paths. A future Blender ImBuf/source bridge should add an explicit
  memory/open-buffer ABI instead of pretending path-only code supports memory
  input.
- Verified with Blender 5.0.1 bundled OpenImageIO 3.0.9.1:
  `Test_Clipping.clip -> 512x512, root_layer=2, layers=4, external_data=7,
  first_pixel=[0,0,224,255]`.
- Verified with the large reference metadata path:
  `Ref_Terra404_Live2D.clip -> 4800x6100, root_layer=2, layers=1212,
  external_data=2243`.
- Re-ran the Blender image-datablock bridge spike after the Rust-backed adapter:
  it created a 512x512 generated image from OIIO bytes with no sidecar PNG.

## Completed Fifth Milestone Foundation

Start native raster data extraction without compositing:

- Parse `CHNKExta` block payloads in Rust and decompress raster/mask bodies.
- Map SQLite layer rows into typed Rust records for raster, folder, paper, and
  filter-layer graph planning.
- Decode a single simple full-color raster layer into RGBA bytes for a targeted
  fixture.
- Keep this below `clip_file`; do not add blend modes, folder compositing, or GPU
  passes in this milestone.

Fifth milestone result:

- `clip_file::container` exposes `external_data_body()` for indexed `CHNKExta`
  bodies.
- `clip_file::metadata` now maps SQLite layer rows into typed
  `LayerGraphRecord` values for graph planning: layer id, kind, visibility,
  clipping flag, opacity, raw composite id, sibling/child links, render mipmap,
  and mask mipmap.
- `clip_file::metadata` resolves a raster layer source through
  `Layer.LayerRenderMipmap -> Mipmap.BaseMipmapInfo -> MipmapInfo.Offscreen ->
  Offscreen.BlockData`, including offscreen pixel dimensions from the
  `Offscreen.Attribute` `Parameter` payload when present.
- `clip_file::tiles` parses `CHNKExta` block streams, handles
  `BlockDataBeginChunk`, `BlockStatus`, `BlockCheckSum`, and
  `BlockDataEndChunk`, and zlib-decompresses tile payloads with `flate2`.
- `clip_file::tiles` decodes full-color raster tiles from
  `alpha plane + BGRA plane` into straight RGBA bytes.
- `clip_file::read_raster_layer_rgba(path, LayerId)` is the first public native
  raster extraction API. It intentionally supports only full-color raster layers
  for now and errors on other layer color types.
- Targeted fixture coverage:
  - `Test_Clipping.clip` layer graph records: root folder, paper, and two raster
    layers.
  - Layer `10`: source `extrnlid7A4545CCDE9D4E579B1230B4DB88B130`, offscreen
    `62`, `512x512`, decoded blob length `1,310,720`, nonzero alpha `37,151`,
    channel sums `[8,507,579, 5,832,707, 5,832,707, 8,933,976]`.
  - Layer `11`: public API decode confirms sample pixels
    `(100,100)=[80,70,229,255]`, `(300,300)=[80,70,229,255]`, nonzero alpha
    `196,553`.

## Completed Sixth Milestone Foundation

Build the first render-graph planning skeleton:

- Move from per-layer extraction to a graph-facing list of typed nodes.
- Preserve sibling/child ordering and visibility filtering.
- Identify paper and raster nodes without compositing them yet.
- Add a runtime/CLI probe that can report the planned node order for
  `Test_Clipping.clip`.
- Do not implement blend modes, folder isolation, clipping semantics, or GPU
  passes in this milestone.

Sixth milestone result:

- `clip_graph` now defines `LayerGraphInput`, typed `RenderNode` values, and a
  `RenderPlan` builder independent of `.clip` storage.
- The planner walks the root, children, and siblings in stored order; filters
  hidden subtrees using the `LayerVisibility` bit flag; and reports duplicate
  ids, missing links, cycles, depth overflow, and node-count overflow as real
  errors.
- `clip_runtime` converts `clip_file` metadata rows into graph inputs and stores
  the plan on `ClipSession`.
- `clip_cli` reports `Test_Clipping.clip` as root container `2`, paper `4`,
  raster `10`, and clipped raster `11`.

## Completed Seventh Milestone Foundation

Build the first `wgpu` resource/readback scaffold:

- Upload one decoded full-color raster layer into a GPU texture.
- Read that texture back through the `clip_gpu` and `clip_runtime` boundary.
- Verify against the existing `Test_Clipping.clip` decoded layer fixture.
- Do not add blend modes, folder isolation, clipping semantics, or CPU
  compositing fallback in this milestone.

Seventh milestone result:

- `clip_gpu` now initializes a real `wgpu 29.0.3` device/queue context with no
  CPU fallback.
- `GpuRenderer::roundtrip_rgba8` uploads straight RGBA bytes into an
  `Rgba8Unorm` texture, copies the texture to a padded readback buffer, strips
  row padding, and returns RGBA bytes.
- `clip_runtime::ClipSession::read_raster_layer_rgba_via_gpu(LayerId)` decodes
  one full-color raster layer and roundtrips it through GPU memory.
- `clip_cli --gpu-roundtrip-layer 10` on `Test_Clipping.clip` verifies the GPU
  readback against the locked layer-10 decode stats: nonzero alpha `37151`,
  sums `[8507579, 5832707, 5832707, 8933976]`.

## Completed Eighth Milestone Foundation

Build the planned raster GPU resource cache:

- Resolve visible raster nodes from `RenderPlan` to decoded raster sources.
- Upload each planned raster source as a GPU texture keyed by layer id and render
  mipmap id.
- Report the uploaded resource list through runtime/CLI for `Test_Clipping.clip`.
- Do not add blend modes, folder isolation, clipping semantics, shader
  compositing, or CPU compositing fallback in this milestone.

Eighth milestone result:

- `clip_gpu::resource` now defines raster upload descriptors, resource keys,
  resource infos, and `GpuRasterResourceCache`.
- `GpuRenderer::upload_raster_resources` pads decoded straight RGBA rows to
  `COPY_BYTES_PER_ROW_ALIGNMENT`, uploads through mapped staging buffers, copies
  into `Rgba8Unorm` textures, waits for submission completion, and returns stable
  resource info keyed by layer id plus render mipmap id.
- `clip_runtime::ClipSession::upload_planned_raster_resources_via_gpu()` resolves
  visible raster nodes from the render plan through `clip_file` without moving
  `.clip` parsing into `clip_gpu`.
- `clip_cli --gpu-upload-planned-rasters` on `Test_Clipping.clip` reports two
  uploaded resources: node `2` layer `10` mipmap `15`, and node `3` layer `11`
  mipmap `16`.

## Completed Ninth Milestone Foundation

Build the first GPU output target and shader-pass skeleton:

- Draw exactly one selected uploaded raster resource into an output texture.
- Read the output texture back through the existing GPU readback path.
- Keep the pass single-resource only.
- Do not add blend modes, folder isolation, clipping semantics, full-stack
  compositing, or CPU compositing fallback in this milestone.

Ninth milestone result:

- `clip_gpu::pass` now draws one selected uploaded raster resource through a
  WGSL full-screen triangle.
- The shader uses `textureLoad` for per-pixel source fetches, writes into an
  `Rgba8Unorm` output texture, and reads back through the shared GPU readback
  helper.
- `clip_runtime::ClipSession::draw_raster_layer_rgba_via_gpu(LayerId)` requires
  the selected layer to be a visible planned raster node, uploads only that
  resource, draws it, and reports exact byte differences against the decoded
  source for the developer probe.
- `clip_cli --gpu-draw-layer 10` and `--gpu-draw-layer 11` on
  `Test_Clipping.clip` both return `differing_bytes=0`.

## Completed Tenth Milestone Foundation

Build a native-backed simple raster stack pass over the shader framework:

- Draw planned raster resources in render-plan order for the subset whose
  semantics are direct texture replacement/copy.
- Report unsupported nodes and unsupported semantics explicitly.
- Do not implement clipping, non-normal blend modes, folder isolation, masks, or
  CPU compositing fallback in this milestone.

Tenth milestone result:

- `clip_runtime::ClipSession::draw_simple_raster_stack_via_gpu()` builds a strict
  partial stack for the GPU developer path.
- It draws only visible planned raster nodes whose current semantics are direct
  texture replacement/copy.
- It reports unsupported nodes and semantics explicitly, including paper,
  clipping, masks, opacity, non-normal composite ids, non-canvas-sized rasters,
  alpha-compositing requirements, filters, and unsupported layer kinds.
- `clip_gpu::GpuRenderer::draw_raster_stack_to_rgba8` draws ordered resources
  through the shader framework.
- `clip_cli --gpu-simple-stack` on `Test_Clipping.clip` draws only node `2`
  layer `10`, reports paper node `1` and clipped raster node `3` as unsupported,
  and returns `differing_bytes_from_last_drawn=Some(0)`.

## Completed Eleventh Milestone Foundation

Implement a NORMAL alpha-over shader pass:

- Composite planned straight RGBA raster resources in render-plan order using
  NORMAL alpha-over.
- Keep the supported subset strict: no clipping, masks, non-normal blend modes,
  folder isolation semantics, or CPU compositing fallback.
- Continue reporting unsupported nodes and unsupported semantics explicitly.

Eleventh milestone result:

- `clip_gpu::GpuRenderer::draw_normal_raster_stack_to_rgba8()` composites
  ordered straight RGBA raster textures with a WGSL NORMAL alpha-over pass.
- The pass uses ping-pong `Rgba8Unorm` accumulation textures and shader-side
  straight RGBA alpha-over, instead of fixed-function premultiplied blending.
- `clip_runtime::ClipSession::draw_normal_raster_stack_via_gpu()` shares the
  strict raster selector with the direct-copy probe and reports unsupported
  semantics explicitly.
- Unsupported containers block their child subtree so folder semantics cannot be
  bypassed by drawing children directly.
- Verified probes:
  - `Test_Clipping.clip --gpu-normal-stack`: draws layer `10`, reports paper
    and clipped raster unsupported.
  - `Test_ToneCurve.clip --gpu-normal-stack`: draws two NORMAL raster layers and
    reports the filter layer unsupported.
  - `Illustration4K.clip --gpu-normal-stack`: draws three NORMAL raster layers
    with no unsupported nodes.

## Completed Twelfth Milestone Foundation

Add paper/background and `LayerOpacity` support to the strict NORMAL GPU path:

- Parse or surface paper/background colour metadata instead of treating paper as
  a placeholder.
- Apply `LayerOpacity` as an explicit shader/runtime input for otherwise
  supported NORMAL raster layers.
- Keep clipping, masks, non-normal blend modes, folder isolation semantics, and
  CPU compositing fallback unsupported until dedicated native models exist.

Twelfth milestone result:

- `clip_file::metadata` decodes Paper colour from `DrawColorMain*`, with
  thumbnail and palette fallback, and handles signed/repeated-byte colour
  storage.
- Paper colour is carried through `LayerGraphRecord`, `LayerGraphInput`, and
  `RenderNode`.
- `clip_gpu::GpuNormalStackSource` represents ordered raster sources and
  solid-colour sources.
- The NORMAL shader applies each source's opacity before straight RGBA
  alpha-over.
- `Test_Clipping.clip --gpu-normal-stack` now draws Paper plus layer `10` and
  reports only clipped raster `11` unsupported.
- `Test_Opacity.clip --gpu-normal-stack` draws Paper plus two NORMAL raster
  layers, including `LayerOpacity=128`, with no unsupported nodes.

## Completed Thirteenth Milestone Foundation

Connect the host-facing region read path to the strict NORMAL GPU renderer:

- Remove deterministic placeholder pixels from `ClipSession::read_rgba8_region`.
- Render and cache the strict NORMAL GPU output on first region read.
- Return an explicit unsupported-plan error instead of returning a partial image
  when a file uses clipping, masks, non-normal blend modes, folder isolation, or
  other unsupported semantics.

Thirteenth milestone result:

- `ClipSession::read_rgba8_region()` renders the strict NORMAL GPU image once,
  caches it, and copies requested regions from the cached RGBA image.
- The C ABI read test now verifies a real Paper pixel from `Test_Opacity.clip`
  (`[226,226,226,255]`) instead of the former placeholder pattern.
- CLI host probes print `host first_pixel=...` for supported strict NORMAL files
  and `host first_pixel_unavailable=...` for unsupported plans while still
  allowing developer `--gpu-*` probes.

## Completed Fourteenth Milestone Foundation

Add layer-mask support to the strict NORMAL GPU path:

- Decode mask mipmap resources below `clip_file`.
- Upload mask textures alongside raster resources.
- Multiply source alpha by mask alpha in the NORMAL shader.
- Keep clipping, non-normal blend modes, folder isolation semantics, and CPU
  compositing fallback unsupported until dedicated native models exist.

Fourteenth milestone result:

- `clip_file::read_layer_mask_alpha()` resolves layer mask mipmaps from
  `LayerLayerMaskMipmap`.
- Single-channel mask tile blobs are decoded into canvas-size alpha images.
- Offscreen `InitColor` is used for omitted mask chunks, and
  `LayerMaskOffscrOffsetX/Y` is applied by paste/crop to the canvas.
- `clip_gpu` uploads masks as `R8Unorm` textures keyed by layer id plus mask
  mipmap id.
- The NORMAL shader multiplies source alpha by the sampled mask value.
- `clip_runtime` treats masked NORMAL rasters as supported in the strict NORMAL
  path.
- `Test_Mask.clip --gpu-normal-stack` reports `sources=3`,
  `raster_resources=2`, `mask_resources=1`, and `unsupported=0`.
- The Rust fixture locks the decoded layer-5 mask at nonzero `227309`, sum
  `57888516`.

## Completed Fifteenth Milestone Foundation

Add clipping-run support to the strict NORMAL GPU path:

- Model a base layer followed by clipped siblings as a native-owned clipping run
  rather than independent flattened siblings.
- Use the recovered native base-cache ownership model as the design constraint.
- Reuse strict support for Paper, opacity, and masks where the clipping run model
  permits it.
- Keep non-normal blend modes, folder isolation semantics, filters, and CPU
  compositing fallback unsupported until dedicated native models exist.

Fifteenth milestone result:

- `clip_runtime` groups a same-depth raster base and following raster clipped
  siblings into one `GpuNormalStackSource::ClippingRun`.
- `clip_gpu` renders the run fully on GPU: base alpha-over into a
  white-transparent cache, clipped NORMAL preserve-RGB passes that keep the
  base alpha, then NORMAL resolve back into the main stack.
- The NORMAL alpha-over shader now preserves destination RGB when output alpha
  remains zero, matching white-transparent straight RGBA cache semantics.
- `Test_Clipping.clip --gpu-normal-stack` reports `sources=2`,
  `raster_resources=2`, `unsupported=0`; host reads now return real pixels.
- `Test_ClippingEdge.clip --gpu-normal-stack` reports `sources=1`,
  `raster_resources=2`, `unsupported=0`; alpha matches the CSP PNG and raw RGB
  sums are within rounding scale.
- `--gpu-simple-stack` still reports clipping unsupported.

## Completed Sixteenth Milestone Foundation

Add strict NORMAL folder/container isolation:

- Render supported child stacks into an owned folder cache before resolving that
  cache into the parent stack.
- Allow Paper, opacity, masks, and clipping runs inside the folder cache when
  all children remain in the strict NORMAL subset.
- Keep THROUGH groups, non-normal blend modes, filters, unsupported container
  semantics, and CPU compositing fallback unsupported until dedicated native
  models exist.

Sixteenth milestone result:

- `GpuNormalStackSource::Container` recursively renders supported child sources
  into an owned white-transparent cache.
- Container cache resolve applies container opacity and optional container mask
  through the same NORMAL alpha-over pass used by raster sources.
- `clip_runtime` now selects strict sources recursively, keeps the root
  container as structural pass-through, and turns non-root `LayerComposite=0`
  containers into container sources.
- A synthetic runtime test verifies that a NORMAL folder is represented as a
  container source rather than flattened into the parent.
- A real-raster e2e runtime test wraps `Test_Clipping.clip`'s actual
  Paper/raster/clipped-raster stack in a synthetic NORMAL folder and verifies
  exact output equality with the flat stack.
- `clip_cli --plan-only` prints summary and planned nodes without triggering
  host rendering, which keeps large-reference graph scans cheap.
- Existing folder fixtures exercise `LayerComposite=30` THROUGH and were kept
  for the next dedicated group-semantics milestone.

## Completed Seventeenth Milestone Foundation

Add THROUGH group support to the strict GPU path:

- Do not treat THROUGH groups as NORMAL isolated folders.
- Render children against the parent stack contribution, then constrain the
  before/after delta by the group mask and opacity.
- Use `Test_FolderNested.clip` and `Test_FolderVisibility.clip` as first guards.
- Keep non-normal blend modes, filters, unsupported container semantics, and CPU
  compositing fallback unsupported until dedicated native models exist.

Seventeenth milestone result:

- `GpuNormalStackSource::ThroughGroup` represents recursive THROUGH group
  sources separately from isolated `Container` sources.
- `clip_gpu` renders THROUGH children against the current parent contribution
  and resolves the before/after delta with group opacity and optional mask.
- `clip_runtime` maps `LayerComposite=30` containers to THROUGH sources.
- A GPU test locks opacity blending, and a runtime selector test locks that a
  THROUGH folder remains a through-group source instead of a NORMAL container.
- `Test_FolderNested.clip --gpu-normal-stack` reports `sources=3`,
  `raster_resources=2`, `unsupported=0`.
- `Test_FolderVisibility.clip --gpu-normal-stack` reports `sources=2`,
  `raster_resources=1`, `unsupported=0`.
- C ABI full-image checks against CSP PNGs report `raw_max=1` and
  `premul_max=1` for both folder fixtures.

## Current Eighteenth Milestone

Start non-NORMAL raster blend-mode support in the strict GPU path:

- Add GPU/runtime structure for ordinary raster sources whose `LayerComposite`
  is not NORMAL.
- Enable individual blend modes only after the `.clip LayerComposite` mapping
  and native behavior are verified for ordinary raster layers, not just nearby
  IDA switch cases.
- Use small blend fixtures first, then guard with clipping/folder/mask samples
  before broadening support.
- Keep filters, unsupported container semantics, and CPU compositing fallback
  unsupported until dedicated native models exist.

Eighteenth milestone progress:

- Ordinary, non-clipped raster `LayerComposite=1` Darken, `2` Multiply,
  `3` Color Burn, `4` Linear Burn, `5` Subtract, `6` Darker Color,
  `7` Lighten, `8` Screen, `9` Color Dodge, `10` Glow Dodge, `11` Add,
  `12` Add Glow, `13` Lighter Color, `14` Overlay, `15` Soft Light,
  `16` Hard Light, `17` Vivid Light, `18` Linear Light, `19` Pin Light,
  `20` Hard Mix, `21` Difference, `22` Exclusion, `23` Hue,
  `24` Saturation, `25` Color, `26` Brightness/Luminosity, and `36` Divide
  are supported.
- Byte-domain passes are used for Add Glow, Color Burn, Color Dodge, and
  Glow Dodge. The standard blend pass covers the remaining ordinary blend modes
  and quantizes the pure blend target to the u8 grid before alpha-over.
- Darker Color and Lighter Color compare source/destination with Rec.709 luma
  (`0.2126/0.7152/0.0722`). HSL modes keep the W3C-style luminosity function
  and quantize `set_sat` tiny spans (`max-min <= 2/255`) to min/max channel
  membership.
- The NORMAL alpha-over pass explicitly rounds to the u8 grid before writing
  `Rgba8Unorm`, which keeps later byte-domain blend passes from seeing
  backend-dependent UNORM truncation.
- `clip_cli --gpu-trace-pixel <x> <y>` now samples the real strict GPU renderer
  after each top-level source prefix for native-fidelity diagnosis. It is not a
  CPU compositor, fallback renderer, or image post-processing path. It also
  prints before/after RGBA plus the raw source RGBA and mask alpha at the traced
  pixel for each source.
- `clip_cli --compare-png <ref.png>` renders through
  `ClipSession::read_rgba8_region()` and compares the native output with a
  CSP-exported PNG in raw and premultiplied byte domains. It is developer
  verification tooling, not a CPU oracle or fallback renderer.
- Runtime only maps those blend modes for ordinary unclipped raster nodes.
  Clipped non-NORMAL siblings and non-NORMAL clipping-run bases remain
  unsupported.
- `Test_AddGlow.clip --gpu-normal-stack` reports `sources=3`,
  `raster_resources=2`, `unsupported=0`.
- `Test_ColorDodge.clip --gpu-normal-stack` reports `sources=3`,
  `raster_resources=2`, `unsupported=0`.
- `Test_ColorBurn.clip --gpu-normal-stack` reports `sources=3`,
  `raster_resources=2`, `unsupported=0`.
- `Test_GlowDodge.clip --gpu-normal-stack` reports `sources=3`,
  `raster_resources=2`, `unsupported=0`.
- `Test_HardMix.clip --gpu-normal-stack` reports `sources=3`,
  `raster_resources=2`, `unsupported=0`.
- `Test_VividLight.clip --gpu-normal-stack` reports `sources=3`,
  `raster_resources=2`, `unsupported=0`.
- `Test_Hue.clip --gpu-normal-stack`, `Test_Saturation.clip --gpu-normal-stack`,
  and `Test_Color.clip --gpu-normal-stack` report `sources=3`,
  `raster_resources=2`, `unsupported=0`.
- `Test_SoftLight.clip --gpu-normal-stack` reports `sources=3`,
  `raster_resources=2`, `unsupported=0`.
- C ABI full-image comparison for `Test_AddGlow` against the CSP PNG reports
  `raw_max=1` and `premul_max=1`.
- C ABI full-image comparison for `Test_HardMix`, `Test_ColorBurn`,
  `Test_GlowDodge`, `Test_ColorDodge`, `Test_FolderNested`,
  `Test_FolderVisibility`, and `Test_Clipping` is exact. `Test_VividLight`,
  `Test_SoftLight`, `Test_Hue`, `Test_AddGlow`, and `Test_Mask` remain within
  `raw_max=1` and `premul_max=1`; `Test_Saturation` and `Test_Color` remain
  within `raw_max=2` and `premul_max=2`.
- `Test_AddGlowMultiply.clip` continues to report the clipped siblings
  unsupported, so the new Add Glow support is not being used as a clipping-run
  substitute.
- `IllustrationBlendModes.clip --gpu-normal-stack` now reports
  `unsupported=0`. C ABI comparison against `IllustrationBlendModes.png`
  is currently `raw_max=72`, `premul_max=72`, and
  `premul_visible_px=16946`. GPU prefix trace at `(266,244)` identifies the
  current largest residual as a narrow Color Dodge/Color Burn interaction:
  after Color Dodge the native GPU path has `[88,67,143,255]`, then Color Burn
  produces `[42,4,142,255]`, while CSP final is `[42,4,214,255]`. A simple
  near-white Color Dodge saturation threshold was rejected because exact
  counterexample pixels regress.
- `IllustrationBlendModes2.clip --gpu-normal-stack` now reports
  `unsupported=0` after adding Linear Burn, Darker/Lighter Color, Linear Light,
  Pin Light, Exclusion, Brightness/Luminosity, and Divide. C ABI comparison
  against `IllustrationBlendModes2.png` is currently `raw_max=8`,
  `premul_max=8`, and `premul_visible_px=29259`. GPU prefix trace at
  `(427,138)` identifies the residual as `PinLight -> Hue -> Saturation`.
  Raising HSL tiny-span quantization from `2/255` to `3/255` improves this
  image but regresses `Test_Saturation`, so that candidate is rejected.

## Renderer Direction After Format Proof

After the strict GPU path covers first non-NORMAL raster blend modes:

1. Build a tile scheduler over the graph.
2. Extend the pass graph toward filters and non-normal blend modes
   using native evidence.
3. Read final RGBA bytes back for the host adapter.
4. Compare output against CSP PNG exports and the current Python output while it
   still exists.
5. Replace the Python Blender add-on's compositor/sidecar path with native
   renderer calls and image datablock updates.
6. Delete the Python compositor/loader and sidecar PNG workflow.

## Performance Principles

- The user path is GPU-first.
- Do not maintain a native CPU compositor fallback or duplicate native CPU
  oracle.
- Use current Python output and CSP-exported PNGs only as slow external
  references during development.
- Avoid full-canvas temporary buffers when tile-local rendering is possible.
- Avoid PNG encode/decode round trips entirely in the accepted native path.

## Non-Goals

- No sidecar fallback after native direct-load is accepted.
- No Python compositor/loader compatibility layer after native direct-load is
  accepted.
- No vector, bubble/frame, text, 3D, animation, or write-back renderer.
- No local stabilizing hacks, post-processing patches, or visual-only GPU
  approximations.
