<table align="center"><tr><td align="center" width="620">
  <img src="docs/assets/rizum-clip-reload-icon.png" alt="Rizum Clip Reload icon" width="140">
  <h1><strong>Rizum Clip Reload</strong></h1>
  <p style="margin-top: 0; margin-bottom: 10px;">Import Clip Studio Paint <code>.clip</code> files into Blender as packed, reloadable image textures.</p>

  <p style="margin-top: 0; margin-bottom: 10px;">
    <a href="https://github.com/Rizumu85/rizum-blender-clip-reload/issues">Report Issues</a> |
    <a href="https://github.com/Rizumu85/rizum-blender-clip-reload/releases">Releases</a> |
    <a href="https://extensions.blender.org/">Blender Extensions</a>
  </p>

  <p style="margin-top: 0; margin-bottom: 10px;">
    <img src="https://img.shields.io/badge/version-0.8.67-f2cfc7" alt="Version">
    <img src="https://img.shields.io/badge/Blender-4.2%2B-d4b6aa" alt="Blender 4.2+">
    <img src="https://img.shields.io/badge/Rust-native-4a3832" alt="Rust native renderer">
    <img src="https://img.shields.io/badge/wgpu-GPU%20renderer-79675f" alt="wgpu GPU renderer">
    <img src="https://img.shields.io/badge/OpenImageIO-adapter-d4b6aa" alt="OpenImageIO adapter">
    <img src="https://img.shields.io/badge/license-GPL--3.0--or--later-79675f" alt="License">
  </p>

  <p style="margin-top: 0; margin-bottom: 0;">
    <a href="#why">English</a> |
    <a href="#why-zh">中文</a>
  </p>
</td></tr></table>

---

<a id="why"></a>

## For What

Rizum Clip Reload is for artists who draw in Clip Studio Paint and use Blender
for materials, layout, animation, or reference work.

Instead of exporting a sidecar PNG every time, import the `.clip` directly into
Blender. The add-on renders the artwork as a flattened generated image, packs it
into the `.blend`, and reloads it when the source file changes.

This project focuses on the final visual result. It does not turn Clip Studio
layers into editable Blender layers.

## Features

- Import `.clip` files from `File > Import > Clip Studio (.clip)`.
- Render flattened artwork through the bundled Rust native renderer.
- Use `wgpu` for GPU compositing and tile-event rendering.
- Includes an experimental OpenImageIO `ImageInput` adapter for OIIO-level hosts
  and future integration work.
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

## License

GPL-3.0-or-later. See [LICENSE](LICENSE).

---

<a id="why-zh"></a>

<table align="center"><tr><td align="center" width="620">
  <img src="docs/assets/rizum-clip-reload-icon.png" alt="Rizum Clip Reload icon" width="140">
  <h1><strong>Rizum Clip Reload</strong></h1>
  <p style="margin-top: 0; margin-bottom: 10px;">把 Clip Studio Paint <code>.clip</code> 文件导入 Blender，作为可打包、可重载的图片纹理。</p>

  <p style="margin-top: 0; margin-bottom: 10px;">
    <a href="https://github.com/Rizumu85/rizum-blender-clip-reload/issues">反馈问题</a> |
    <a href="https://github.com/Rizumu85/rizum-blender-clip-reload/releases">下载发布版</a> |
    <a href="https://extensions.blender.org/">Blender Extensions</a>
  </p>

  <p style="margin-top: 0; margin-bottom: 10px;">
    <img src="https://img.shields.io/badge/version-0.8.67-f2cfc7" alt="Version">
    <img src="https://img.shields.io/badge/Blender-4.2%2B-d4b6aa" alt="Blender 4.2+">
    <img src="https://img.shields.io/badge/Rust-native-4a3832" alt="Rust native renderer">
    <img src="https://img.shields.io/badge/wgpu-GPU%20renderer-79675f" alt="wgpu GPU renderer">
    <img src="https://img.shields.io/badge/OpenImageIO-adapter-d4b6aa" alt="OpenImageIO adapter">
    <img src="https://img.shields.io/badge/license-GPL--3.0--or--later-79675f" alt="License">
  </p>

  <p style="margin-top: 0; margin-bottom: 0;">
    <a href="#why">English</a> |
    <a href="#why-zh">中文</a>
  </p>
</td></tr></table>

## 用于什么

Rizum Clip Reload 适合这样的流程：你在 Clip Studio Paint 里画图，在 Blender
里做材质、排版、动画或参考。

不用每次手动导出 sidecar PNG，直接把 `.clip` 导入 Blender。插件会把作品渲染成
一张扁平的 generated image，打包进 `.blend`，并在源文件变化后重新加载。

这个项目关注最终画面是否正确。它不会把 Clip Studio 的图层变成 Blender 里可编辑
的图层。

## 功能特点

- 从 `File > Import > Clip Studio (.clip)` 直接导入 `.clip` 文件。
- 使用内置 Rust native renderer 渲染扁平化画面。
- 使用 `wgpu` 做 GPU 合成和 tile-event 渲染。
- 仓库包含实验性的 OpenImageIO `ImageInput` adapter，用于 OIIO 级宿主和后续集成
  工作。
- 源 `.clip` 文件变化后可以自动重载。
- 在 Blender Image Editor 侧边栏提供手动重载和打包控制。
- 即使原始 `.clip` 源文件丢失，也会继续显示上一次已打包的像素。
- 支持栅格图层、文件夹、蒙版、裁剪图层、透明度、混合模式，以及当前支持的调整/
  滤镜图层。
- UI 已支持简体中文、日语和西班牙语翻译。

## 安装

1. 使用 Blender 4.2 或更新版本。
2. 从 [Releases](https://github.com/Rizumu85/rizum-blender-clip-reload/releases)
   页面下载最新 release zip。
3. 打开 `Edit > Preferences > Get Extensions`。
4. 选择 `Install from Disk...`。
5. 选择下载的 zip。
6. 启用 `Rizum Clip Reload`。

Windows x64 已在维护者机器上测试。Universal release package 里也包含 Linux x64、
macOS Intel 和 macOS Apple Silicon 的 native 包，但因为我没有对应设备，目前这些
平台标记为维护者未测试。

## 使用

1. 在 Blender 中选择 `File > Import > Clip Studio (.clip)`。
2. 选择你的 `.clip` 文件。
3. Blender 会根据渲染结果创建一张 generated image。
4. 首次导入会把图片打包进 `.blend`。
5. 在 Clip Studio Paint 里修改并保存 `.clip` 后，Blender 可以自动重新加载。
6. 也可以在 Image Editor 侧边栏点击 `Manual Reload` 手动刷新。

如果之后找不到原始 `.clip` 文件，Blender 仍然会显示上一次已打包的画面，并标记源
文件缺失。

## 当前状态

包版本：`0.8.67`。

目前支持：

- 栅格图层和全彩图像。
- 纸张/背景图层。
- 文件夹和裁剪图层。
- 图层蒙版和透明度。
- 常见混合模式。
- 当前验证样本覆盖的调整图层和滤镜图层。
- 手动重载和非阻塞自动重载。
- 图片结果打包保存进 `.blend`。
- 缺失源文件、渲染错误、pack 状态等清晰提示。

目前不做：

- 导入可编辑图层。
- 矢量线条或填充。
- 文字图层。
- 对话框/分镜框渲染器。
- 3D 图层或动画时间轴。
- 把修改写回 `.clip` 文件。

## 构建插件包

先构建 native renderer：

```powershell
cd native\rust
cargo build --release -q -p clip_capi
cargo build --release -q -p clip_cli
cd ..\..
```

然后构建可安装扩展包：

```powershell
python tools\build_blender_addon.py
```

脚本会生成 `clip_studio_importer.zip`。这个 zip 包含 Blender 扩展文件、`LICENSE`、
`NOTICE.md`，以及 `native/<platform>/` 下的 native renderer 文件。

默认情况下脚本会打包当前主机平台。要生成包含所有支持平台 native 包的 zip，请先把
native artifacts 放到 `native/artifacts/<platform>/`，然后运行：

```powershell
python tools\build_blender_addon.py --platform all --output clip_studio_importer-universal.zip
```

`Build extension package` GitHub Actions workflow 会构建各平台 artifacts，并上传
`clip_studio_importer-universal.zip` workflow artifact。

## 验证

可以用 native CLI 对比 CSP 导出的 PNG reference：

```powershell
cd native\rust
cargo run -q -p clip_cli -- ..\..\img\Test_ToneCurve.clip --compare-png ..\..\img\Test_ToneCurve.png
cargo run -q -p clip_cli -- ..\..\img\Test_AddGlowMultiply.clip --compare-png ..\..\img\Test_AddGlowMultiply.png
```

收敛检查：

```powershell
scripts\verify_native_convergence.ps1 -SkipClipCompare
```

## 项目结构

- `clip_studio_importer/` - Blender 扩展源码。
- `native/rust/` - Rust native renderer workspace。
- `scripts/` - 验证和维护脚本。
- `tests/` - Python 侧 bridge/package 测试。
- `docs/AI_MEMORY.md` - 给 agent 使用的当前项目状态摘要。
- `docs/plan.md` - 稳定方向和后续计划。
- `docs/analysis.md` - 历史证据和被拒绝假设的 append-only 记录。

## 许可证

GPL-3.0-or-later。见 [LICENSE](LICENSE)。
