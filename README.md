<div align="center">
  <img src="docs/assets/rizum-clip-reload-icon.png" alt="Rizum Clip Reload icon" width="148">

# Rizum Clip Reload

Import Clip Studio Paint `.clip` files into Blender as packed, reloadable image textures.

[Report Issues](https://github.com/Rizumu85/rizum-blender-clip-reload/issues) ·
[Releases](https://github.com/Rizumu85/rizum-blender-clip-reload/releases) ·
[Blender Extensions](https://extensions.blender.org/)

![Version](https://img.shields.io/badge/version-0.8.67-f2cfc7)
![Blender](https://img.shields.io/badge/Blender-4.2%2B-d4b6aa)
![Native renderer](https://img.shields.io/badge/native-renderer-4a3832)
![License](https://img.shields.io/badge/license-GPL--3.0--or--later-79675f)

</div>

---

## Why

Rizum Clip Reload is for artists who draw in Clip Studio Paint and use Blender
for materials, layout, animation, or reference work.

Instead of exporting a sidecar PNG every time, import the `.clip` directly into
Blender. The add-on renders the artwork as a flattened generated image, packs it
into the `.blend`, and reloads it when the source file changes.

This project focuses on the final visual result. It does not turn Clip Studio
layers into editable Blender layers.

## Features

- Import `.clip` files from `File > Import > Clip Studio (.clip)`.
- Render flattened artwork through the bundled native renderer.
- Auto-reload when the source `.clip` changes.
- Manual reload and pack controls in Blender's Image Editor sidebar.
- Keep packed pixels visible even when the original `.clip` source is missing.
- Supports raster layers, folders, masks, clipping, opacity, blend modes, and
  supported adjustment/filter layers.
- UI translations for Simplified Chinese, Japanese, and Spanish.

## Install

1. Use Blender 4.2 or newer.
2. Download the latest release zip from the
   [Releases](https://github.com/Rizumu85/rizum-blender-clip-reload/releases)
   page.
3. Open `Edit > Preferences > Get Extensions`.
4. Choose `Install from Disk...`.
5. Select the downloaded zip.
6. Enable `Rizum Clip Reload`.

Windows x64 is maintainer-tested. Linux x64, macOS Intel, and macOS Apple
Silicon native packages are included in the universal release package, but they
are currently maintainer-untested because I do not have those devices.

## Use

1. In Blender, choose `File > Import > Clip Studio (.clip)`.
2. Select your `.clip` file.
3. Blender creates a generated image from the rendered artwork.
4. The first import is packed into the `.blend`.
5. After editing the `.clip` in Clip Studio Paint, save it again.
6. Blender can reload the image automatically, or you can press `Manual Reload`
   in the Image Editor sidebar.

If the original `.clip` file is missing later, Blender keeps showing the last
packed render and marks the source as missing.

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

Not in scope:

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

By default the script packages the current host platform. To build one zip
containing all supported native packages, collect native artifacts under
`native/artifacts/<platform>/` and run:

```powershell
python tools\build_blender_addon.py --platform all --output clip_studio_importer-universal.zip
```

The `Build extension package` GitHub Actions workflow builds the platform
artifacts and uploads `clip_studio_importer-universal.zip` as a workflow
artifact.

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
- `native/rust/` - Rust native renderer workspace.
- `scripts/` - verification and maintenance scripts.
- `tests/` - Python-side bridge/package tests.
- `docs/AI_MEMORY.md` - compact current project state.
- `docs/plan.md` - durable direction and open work.
- `docs/analysis.md` - append-only historical evidence and rejected hypotheses.

## 中文简介

Rizum Clip Reload 可以把 Clip Studio Paint 的 `.clip` 文件导入 Blender，
作为打包在 `.blend` 里的可重载图片纹理使用。

它适合这样的流程：你在 Clip Studio Paint 里画图，在 Blender 里做材质、
排版、动画或参考。导入后，插件会用内置 native renderer 渲染 `.clip`，
然后在 Blender 里创建 generated image。图片会被打包进 `.blend`，之后
`.clip` 源文件变化时可以自动或手动重新加载。

插件关注最终画面是否正确，不会把 Clip Studio 的图层变成 Blender 里可编辑
的图层。

目前支持栅格图层、文件夹、裁剪图层、图层蒙版、透明度、常见混合模式，以及
当前验证样本覆盖的调整/滤镜图层。暂不支持可编辑图层导入、矢量、文字、对话框/
分镜框、3D 图层、动画时间轴，或写回 `.clip` 文件。

Windows x64 已在维护者机器上测试。Linux x64、macOS Intel、macOS Apple
Silicon 的 native 包已经包含在 universal release package 中，但因为没有对应
设备，目前标记为维护者未测试。

## License

GPL-3.0-or-later. See [LICENSE](LICENSE).
