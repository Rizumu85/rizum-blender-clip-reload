# Native Code Architecture

Last reconciled: 2026-06-20

## Goal

The native renderer is the accepted `.clip` flattening engine for Blender. It
should stay modular enough that parsing, semantic planning, GPU execution,
worker protocol, and C ABI concerns can evolve independently.

Non-goals:

- Python compositor compatibility.
- Runtime CPU compositor fallback.
- Sidecar PNG product workflow.
- Global full-canvas per-layer caches.
- Vector/text/bubble/frame renderer compatibility.

## Crate Layout

`native/rust` is the Rust workspace.

| Crate | Responsibility |
| --- | --- |
| `clip_model` | Shared IDs, geometry, color, layer, blend, and metadata types. |
| `clip_file` | `.clip` container access, SQLite metadata reads, external chunk reads, and tile decode helpers. |
| `clip_graph` | Visible render graph construction and source selection. |
| `clip_gpu` | `wgpu` device resources, shaders, render passes, tile-event execution, barrier execution, and stream encoding. |
| `clip_runtime` | `ClipSession`, persistent renderer, reload manifests, support reports, Blender worker JSON, and runtime orchestration. |
| `clip_capi` | Small C ABI surface for host integration. |
| `clip_cli` | CLI, compare tooling, Blender worker one-shot mode, and persistent render server mode. |

Root files such as `lib.rs` and `main.rs` are wiring only. Do not add renderer
behavior there.

## Data Flow

Normal render flow:

1. `ClipSession` opens and owns a `ClipContainer`.
2. `clip_file` reads metadata/resources from that container.
3. `clip_graph` selects the visible strict native render stack.
4. `clip_runtime` asks `clip_gpu` to plan and execute the stack.
5. `clip_gpu` streams sources through tile-local programs and explicit barrier
   segments.
6. `clip_cli`, the persistent worker, or `clip_capi` returns RGBA8 output plus
   diagnostics.

Reload flow:

1. A successful render emits a reload manifest.
2. The next render compares canvas/root/source order and raster/mask tile
   fingerprints.
3. The worker returns `no_change`, `patch`, or `full`.
4. Blender applies dirty patch rows to the existing generated image or replaces
   the whole image.

## `clip_file`

`clip_file` owns file format IO. Keep SQLite and external chunk details here.

Important boundaries:

- Metadata records live in focused `metadata/*` modules.
- `metadata.rs` is a small re-export/wiring surface.
- External chunk readers live under `external/`.
- From-container helpers are preferred when render code already owns a
  `ClipContainer`; do not reopen the same `.clip` per layer.

Useful helpers:

- `read_raster_layer_source_info_from_container`
- `read_raster_layer_source_rgba_from_container`
- `read_layer_mask_alpha_from_container`

## `clip_graph`

`clip_graph` maps `.clip` metadata to renderable semantic sources. It should
not decide GPU pass shapes or tile-event eligibility.

Current main selector:

- `select_gpu_normal_render_stack`

Decoded/debug selectors may remain for trace and tests, but product rendering
should use the metadata-first path.

## `clip_gpu`

`clip_gpu` owns GPU execution.

Major areas:

- `stream_program*`: render-program planning, segment classification, barrier
  reasons, and tile-local lowering decisions.
- `stream_sequence`: dispatches planned segments and handles provider fallback.
  It should not add lowering or barrier classification logic.
- `stream_tile_event*`: typed tile-event ABI, payload layout, buffer building,
  and event execution entry points.
- `pass*` and barrier modules: faithful legacy/barrier rendering for semantics
  not yet tile-local.
- WGSL shaders: execute the semantics encoded by the planner, not infer source
  graph structure on their own.

Every shader-visible tile-event payload layout change must bump
`TILE_EVENT_ABI_VERSION` and keep a reload-manifest compatibility test that
promotes old manifests to full render.

## `clip_runtime`

`clip_runtime` owns application-level native state.

Key responsibilities:

- `ClipSession` and container lifetime.
- `RuntimeGpuRenderer` and reusable GPU/session resources.
- Support reports and strict native support summaries.
- Reload manifests and dirty patch planning.
- Worker JSON emitted for the Blender bridge.
- Render/decode/profile diagnostics.

The runtime may cache sparse resources and checkpoints when keyed by source/tile
fingerprints. It must not cache full-canvas layer textures as a product shortcut.

## `clip_capi`

`clip_capi` is an integration boundary, not a renderer implementation. Keep it
small:

- ABI structs and versioning.
- Host-callable render/support functions.
- Error/result conversion.
- Focused tests for ABI behavior.

Formatting and support-report text belongs in helper modules, not in the crate
root.

## Blender Bridge

The Blender add-on uses generated images rather than a filetype loader:

- Native worker returns top-row-first RGBA8.
- `native_bridge.py` flips rows into Blender's bottom-row-first
  `Image.pixels` storage.
- The image is packed into `.blend` files.
- Persistent reload applies patch rows when the worker returns dirty rects.

Keep Blender state handling in Python, but keep `.clip` rendering semantics in
Rust.

## Dependency Direction

Allowed direction:

```text
clip_model
  <- clip_file
  <- clip_graph
  <- clip_gpu
  <- clip_runtime
  <- clip_capi / clip_cli
```

`clip_runtime` can coordinate lower crates; lower crates should not know about
Blender, C ABI packaging, or CLI details.

## Test Strategy

Use the smallest test that covers the changed boundary:

- File/metadata changes: `clip_file` tests and fixture reads.
- Graph semantics: `clip_graph` or runtime planner tests.
- Tile-event semantics: planner plus GPU legacy-vs-tile tests.
- Worker/reload behavior: runtime worker tests or CLI smoke tests.
- Blender image upload/patch behavior: Python unit tests.
- Product samples: `clip_cli --compare-png`.
