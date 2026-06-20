# Rizum Clip Reload

Import Clip Studio Paint `.clip` files into Blender as packed, reloadable image
textures.

## What It Does

Rizum Clip Reload lets Blender open Clip Studio Paint artwork from `.clip` files
as a flattened image texture. It is made for artists who draw in Clip Studio
Paint and use Blender for materials, layout, animation, or reference work.

You can import a `.clip` file, keep the rendered image packed inside the
`.blend`, and reload it when the source file changes. The extension focuses on
the final visual result. It does not turn Clip Studio layers into editable
Blender layers.

## Install

1. Use Blender 4.2 or newer.
2. Open `Edit > Preferences > Get Extensions`.
3. Choose `Install from Disk...`.
4. Select `clip_studio_importer.zip`.
5. Enable `Rizum Clip Reload`.

## Use

1. In Blender, choose `File > Import > Clip Studio (.clip)`.
2. Select your `.clip` file.
3. Blender creates a generated image from the file.
4. The image is packed into the `.blend`.
5. After editing the `.clip` in Clip Studio Paint, save it again.
6. Blender can reload the image automatically, or you can press `Manual Reload`
   in the Image Editor sidebar.

If the original `.clip` file is missing later, Blender keeps showing the last
packed render.

## Current Status

Package version: `0.8.67`.

Works today:

- Raster layers and full-color artwork.
- Paper/background layers.
- Folders and clipped layers.
- Layer masks and opacity.
- Common blend modes.
- Adjustment and filter layers covered by current verification samples.
- Manual reload and non-blocking auto-reload.
- Packed image persistence inside `.blend` files.
- Clear status messages for missing sources, render errors, and pack state.
- UI translations for Simplified Chinese, Japanese, and Spanish.

Not in scope right now:

- Editable layer import.
- Vector strokes or fills.
- Text layers.
- Bubble/frame renderers.
- 3D layers or animation timelines.
- Writing changes back to `.clip` files.

## Build Package

Build the native renderer first:

```powershell
cd native\rust
cargo build --release -q -p clip_capi
cargo build --release -q -p clip_cli
cd ..\..
```

Then build the installable extension zip:

```powershell
python tools\build_blender_addon.py
```

The script writes `clip_studio_importer.zip`. The zip contains the Blender
extension files, `LICENSE`, `NOTICE.md`, and the native renderer files under
`native/<platform>/`.

By default the script packages the current host platform. Windows x64 is tested
on the maintainer's machine. Linux x64, macOS x64, and macOS arm64 package
support is present but maintainer-untested because the maintainer does not have
those devices. Build those packages on matching machines, or pass explicit
artifact directories:

```powershell
python tools\build_blender_addon.py --platform linux-x64
python tools\build_blender_addon.py --platform macos-x64
python tools\build_blender_addon.py --platform macos-arm64
python tools\build_blender_addon.py --platform linux-x64 --native-artifact-dir linux-x64=path\to\linux\artifacts
```

To let Blender perform the final extension build, pass a Blender executable:

```powershell
python tools\build_blender_addon.py --blender "C:\Program Files\Blender Foundation\Blender 4.2\blender.exe"
```

## Verification

Use the native CLI against CSP-exported PNG references:

```powershell
cd native\rust
cargo run -q -p clip_cli -- ..\..\img\Test_ToneCurve.clip --compare-png ..\..\img\Test_ToneCurve.png
cargo run -q -p clip_cli -- ..\..\img\Test_AddGlowMultiply.clip --compare-png ..\..\img\Test_AddGlowMultiply.png
```

For the convergence gate:

```powershell
scripts\verify_native_convergence.ps1 -SkipClipCompare
```

## Project Layout

- `clip_studio_importer/` - Blender extension source.
- `clip_studio_importer.zip` - installable extension package.
- `native/rust/` - Rust native renderer workspace.
- `scripts/` - verification and maintenance scripts.
- `tests/` - Python-side bridge/package tests.
- `docs/AI_MEMORY.md` - compact current project state.
- `docs/plan.md` - durable direction and open work.
- `docs/native-code-architecture.md` - native crate/module boundaries.
- `docs/native-tile-event-renderer.md` - current tile-event renderer model.
- `docs/native-performance-investigation.md` - compressed performance evidence.
- `docs/analysis.md` - append-only historical evidence and rejected hypotheses.

## Roadmap

- Keep the installable extension on the native renderer path.
- Improve visual fidelity only when a sample-backed semantic issue remains
  visible enough to justify the risk.
- Improve reload and rendering performance through measured renderer/cache work.
- Keep verification on CSP PNG exports and native `clip_cli --compare-png`.
- Explore true file-backed `.clip` integration later, without sidecar PNGs.
