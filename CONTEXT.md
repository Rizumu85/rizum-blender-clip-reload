# Project Context

Last updated: 2026-06-18

This repository owns the Blender-facing `.clip` importer, native renderer
bridge, and verification harness for flattened raster Clip Studio Paint files.

## Domain Terms

**Flattened raster texture**

The user-facing output of this project: a full-canvas RGBA image suitable for
Blender image textures. It includes raster layers, folders, masks, clipping,
THROUGH groups, blend modes, and supported adjustment/filter layers. It does not
include editable CSP layer navigation, vector strokes/fills, text, bubble/frame
rendering, animation, or write-back.

**Native renderer**

The Rust/wgpu implementation that reads `.clip` data and composites the
flattened raster texture. It is the accepted runtime path for the Blender add-on.
The project-root Python loader is reference tooling only, not a runtime fallback.

**Generated Blender image**

The Blender `Image` datablock created by the add-on after the native renderer
returns real pixels. It stores source tracking, reload status, pack status,
timing, support diagnostics, and reload manifest metadata as Blender custom
properties.

**Image state**

The durable state stored on a generated Blender image: source path/freshness,
native renderer markers, reload status, pack status, support diagnostics,
timing, and manifest metadata. Architecture work should treat this as one
domain module rather than a loose set of raw custom-property keys.

**Native worker protocol**

The JSON-and-RGBA-file contract between the Blender Python add-on and the
packaged `clip_cli` worker, including one-shot render files, persistent server
messages, reload manifests, dirty-rect patches, timing fields, and error
payloads.

**Reload manifest**

The compact native render manifest stored on a generated Blender image and sent
back to the worker on reload. It lets the worker compare old and new graph/source
state and return no-change, dirty-rect patches, or full image output.

**Support diagnostics**

Metadata-only native support information used for issue reports and developer
debugging. Normal Blender UI should show only actionable unsupported-node
locators when failures exist; full support summaries and resource statistics
belong in copied/opened diagnostics.

**CLI command runner**

The developer-facing `clip_cli` command execution layer. It should parse a
command, run the matching diagnostic/render/support action, and format output
without requiring `main.rs` to know every command's internal behavior.

**Metadata reader**

A `clip_file` module that maps SQLite rows and binary attributes into typed
records such as layer graph records, raster/render sources, mask sources, filter
sources, paper colour, and canvas summary. SQLite schema details should remain
inside `clip_file`.

**Streaming execution context**

The `clip_gpu` render-execution state that owns encoder lifecycle, ping-pong
texture selection, dirty bounds, region/split-region rendering, flush policy,
and provider resource retention for recursive streaming renders.

**Render program**

The native renderer's planned execution IR for a flattened raster texture. It
compiles a strict `GpuNormalStackSource` sequence into ordered render segments
before GPU encoding. A render program separates CSP semantic selection from
tile-local execution decisions, and carries planning statistics such as
tile-local segment count, barrier count, planned tile events, and planned pass
count.

**Render segment**

One ordered unit inside a render program. A segment is either tile-local, such
as an atlas-backed raster run or raster-only clipping run, or a barrier that
must currently execute through the faithful legacy source path. Future native
performance work should add new segment kinds instead of adding opportunistic
branches directly to streaming traversal code.

**Lowering decision**

The render-program planner's first-class answer for the next source range. It
states whether that range lowers to a tile-local render segment or remains a
barrier, plus the segment kind, source span, barrier reason, and cost hint. It
keeps eligibility logic behind the planner seam instead of spreading boolean
checks through the executor.

**Tile event ABI**

The versioned typed event contract for tile-local rendering. The current first
form models raster tile events with event headers and raster payloads. The
tile-silo shader consumes separate event-header and raster-payload storage
buffers while preserving the original raster, mask, clipped-raster, and
raster-only clipping-run semantics. Future work should add new event kinds only
after each semantic model is faithful.

**Performance plan diagnostic**

The metadata/block-level CLI report that explains the current native render
program without running a GPU render. It combines render-program segment stats,
typed barrier reason counts, compressed tile occupancy, and sparse atlas upload
estimates so native performance work can be compared by planner coverage rather
than intuition.

**Sparse atlas cache**

Session-level native renderer state that maps logical raster or mask source
tiles plus compressed tile fingerprints to sparse atlas slots. It decides which
tiles can be reused, which atlas slots need updated payloads, and which stale
slots can be reclaimed across reload generations.

**Sparse atlas texture pool**

The GPU-side pool of sparse atlas textures keyed by atlas format and atlas id.
It creates resident atlas textures on demand and applies in-place region updates
for changed atlas slots. Future dirty segment reload execution should feed this
pool from the sparse atlas cache and bind the resulting textures in tile-local
segment reruns.

**Sparse atlas executor**

The tile-local executor adapter that consumes resident sparse atlas slots and
binds the matching sparse atlas texture pool entries as shader inputs. It should
reuse the typed tile event ABI and tile-silo shader semantics instead of
creating a separate compositor path.
