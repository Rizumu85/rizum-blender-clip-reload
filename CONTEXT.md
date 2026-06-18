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
