# Blender Extension Submission Draft

This document is a review draft for preparing Rizum Clip Reload for
extensions.blender.org. Do not treat the listing copy here as final until Rizum
has reviewed it.

## License Recommendation

Recommended project license: GPL-3.0-or-later.

Reason:

- Blender Extensions requires uploaded add-ons to be GPL-3.0-or-later
  compliant.
- The Blender add-on imports Blender's Python API directly, so using the same
  GPL-3.0-or-later expression avoids ambiguity for official distribution.
- Rust dependencies found through `cargo metadata` are permissive or
  GPL-compatible license families in the current dependency graph:
  MIT, Apache-2.0, Zlib, BSD-2-Clause, ISC, 0BSD, CC0-1.0, Unicode-3.0, and
  Unlicense combinations.
- Apache-2.0 is compatible with GPL-3.0, so GPL-3.0-or-later is the safer
  Blender-extension target than GPL-2.0-or-later.

This is an engineering license review, not legal advice.

## Reference Project Audit

- Avarel/silicate is MIT licensed and was used as an algorithm/architecture
  reference for tile-local GPU rendering. Current recommendation: credit it in
  `NOTICE.md` only, not in `blender_manifest.toml`, because no source code or
  assets are copied or bundled.
- Kazuhito00/clip_studio_paint_tool is MIT licensed and should receive direct
  credit. Keep the Kazuhito MIT notice in `NOTICE.md` and credit
  `2024 KazuhitoTakahashi` in `blender_manifest.toml`.

## Current Package Scope

Initial fully tested upload target: Windows x64.

The package builder can also produce Linux x64, macOS x64, and macOS arm64
native packages when matching `clip_cli` and `clip_capi` artifacts are present.
Those packages must be uploaded as separate platform-specific zip files, not as
one universal package. Mark Linux/macOS packages as test candidates until Rizum
or another tester verifies them on real Linux/macOS devices.

Declared permissions:

- `files`: required to import `.clip` sources, resolve Blender-relative paths,
  track source freshness, and reload changed sources.
- `clipboard`: required by the copy diagnostics actions.
- No `network` permission: the add-on does not contact remote services.

## Listing Copy Draft

Short summary:

Rizum Clip Reload imports Clip Studio Paint `.clip` artwork into Blender as a
flattened generated image, keeps the image packed in the `.blend`, and can
reload changed source files.

Description draft:

Rizum Clip Reload is an unofficial Clip Studio Paint `.clip` importer for
Blender. It renders supported raster artwork through a packaged native renderer
and uploads the result into a generated Blender image datablock. The rendered
pixels are packed into the `.blend`, so files keep showing the last successful
render after reopening.

The add-on focuses on flattened raster fidelity: raster layers, folders, masks,
clipping, blend modes, and adjustment/filter layers used by the supported
samples. It does not provide editable Clip Studio layers inside Blender, and it
does not currently support vector strokes, text, bubble/frame renderers, 3D
layers, animation timelines, or write-back to `.clip`.

The import menu entry is `File > Import > Clip Studio (.clip)`. Imported images
can be manually reloaded from the Image Editor panel, and optional diagnostics
can be copied for issue reports.

Reviewer notes:

- The Windows x64 extension is self-contained and tested on the maintainer's
  machine: it bundles the native renderer worker and C ABI library.
- Linux x64 and macOS packages are separate zip files and should be labeled as
  test candidates until real-device smoke tests pass.
- Platform native file names differ by OS:
  `windows-x64` uses `clip_cli.exe` and `clip_capi.dll`,
  `linux-x64` uses `clip_cli` and `libclip_capi.so`, and macOS uses `clip_cli`
  and `libclip_capi.dylib`.
- The extension does not download or execute remote code.
- The extension does not require internet access.
- The extension is not affiliated with Blender, CELSYS, or Clip Studio Paint.

## Reviewer Reply Draft

Hello.

Thank you for the review. I will not upload the previous universal package. I
will upload separate platform-specific zip files instead, with each zip
containing only the native files for its matching operating system. The Windows
x64 package contains `clip_cli.exe` and `clip_capi.dll`; Linux contains
`clip_cli` and `libclip_capi.so`; macOS contains `clip_cli` and
`libclip_capi.dylib`.

About the Windows `.exe`: it is not third-party software and it is not copied
from another application. `clip_cli.exe` is built from this repository's own
Rust source code, mainly `native/rust/crates/clip_cli`. The paired C ABI library
is built from `native/rust/crates/clip_capi`. The add-on uses this native worker
to parse and render `.clip` files out of Blender's UI process, then uploads the
rendered pixels into a Blender generated image.

The project source is here:
https://github.com/Rizumu85/rizum-blender-clip-reload

The package does not bundle Clip Studio Paint, CELSYS binaries, or any other
third-party executable application for processing `.clip` files. Third-party
code is limited to normal Rust crate dependencies used to build the project's
own binaries, with their license notices kept in the repository.

## Pre-upload Checklist

- Project/source URL is set to
  `https://github.com/Rizumu85/rizum-blender-clip-reload` in
  `blender_manifest.toml`.
- Build and smoke-test the Windows x64 extension package in Blender 4.2 or newer.
- Smoke-test macOS packages on matching Intel/Apple Silicon machines when
  possible.
- Smoke-test the Linux x64 package on Steam Deck or another x86_64 Linux
  machine.
- Do not upload a universal package containing native files for several
  operating systems.
- For Linux/macOS uploads, build one platform-specific zip per operating system
  and label packages honestly until matching-machine smoke tests pass.
- Run Blender's extension validator/build command if available on the release
  machine.
- Review `NOTICE.md` before upload.
- Add macOS/Linux platform packages only with matching native artifacts and
  real-device smoke tests.
