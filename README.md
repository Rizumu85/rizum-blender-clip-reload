# Rizum Clip Reload

Import Clip Studio Paint `.clip` files into Blender as packed, reloadable image
textures.

- [English](#english)
- [中文](#中文)

---

## English

- [What It Does](#what-it-does)
- [Install](#install)
- [Use](#use)
- [Current Status](#current-status)
- [Build Package](#build-package)
- [Project Layout](#project-layout)
- [Roadmap](#roadmap)

### What It Does

Rizum Clip Reload lets Blender open Clip Studio Paint artwork from `.clip` files
as a flattened image texture.

It is made for artists who draw in Clip Studio Paint and use Blender for
materials, layout, animation, or reference work.

You can import a `.clip` file, keep the rendered image packed inside the
`.blend`, and reload it when the source file changes.

The extension focuses on the final visual result. It does not turn Clip Studio
layers into editable Blender layers.

### Install

1. Use Blender 4.2 or newer.
2. Open `Edit > Preferences > Get Extensions`.
3. Choose `Install from Disk...`.
4. Select `clip_studio_importer.zip`.
5. Enable `Rizum Clip Reload`.

### Use

1. In Blender, choose `File > Import > Clip Studio (.clip)`.
2. Select your `.clip` file.
3. Blender creates a generated image from the file.
4. The image is packed into the `.blend`.
5. After editing the `.clip` in Clip Studio Paint, save it again.
6. Blender can reload the image automatically, or you can press `Manual Reload`
   in the Image Editor sidebar.

If the original `.clip` file is missing later, Blender keeps showing the last
packed render.

### Current Status

Package version: `0.8.67`.

Works today:

- Raster layers and full-color artwork.
- Paper/background layers.
- Folders and clipped layers.
- Layer masks and opacity.
- Common blend modes.
- Adjustment and filter layers used by the current test files.
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

### Build Package

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
support is present but not maintainer-tested because the maintainer does not
have those devices. Build those packages on matching machines, or pass explicit
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

### Project Layout

- `clip_studio_importer/` - Blender extension source.
- `clip_studio_importer.zip` - installable extension package.
- `native/rust/` - native renderer workspace.
- `docs/AI_MEMORY.md` - compact current project state.
- `docs/analysis.md` - research notes and historical findings.
- `docs/design.md` - Blender user experience notes.
- `docs/plan.md` - durable direction and open work.

### Roadmap

- Keep the installable extension on the native renderer path.
- Improve visual fidelity for complex `.clip` files.
- Improve reload and rendering performance.
- Keep verification on CSP PNG exports and native `clip_cli --compare-png`.
- Explore true file-backed `.clip` integration later, without sidecar PNGs.

---

## 中文

- [它能做什么](#它能做什么)
- [安装](#安装)
- [使用](#使用)
- [当前状态](#当前状态)
- [构建插件包](#构建插件包)
- [项目结构](#项目结构)
- [路线图](#路线图)

### 它能做什么

Rizum Clip Reload 可以把 Clip Studio Paint 的 `.clip` 文件导入 Blender，
作为一张扁平化的图片纹理使用。

它适合这样的流程：你在 Clip Studio Paint 里画图，在 Blender 里做材质、
排版、动画或参考。

导入后，渲染出来的图片会被打包进 `.blend`。之后 `.clip` 源文件有变化时，
你可以重新加载，不需要手动导出 PNG 再替换。

这个插件关注的是“最后看起来对不对”。它不会把 Clip Studio 的图层变成
Blender 里可编辑的图层。

### 安装

1. 使用 Blender 4.2 或更新版本。
2. 打开 `Edit > Preferences > Get Extensions`。
3. 选择 `Install from Disk...`。
4. 选择 `clip_studio_importer.zip`。
5. 启用 `Rizum Clip Reload`。

### 使用

1. 在 Blender 中选择 `File > Import > Clip Studio (.clip)`。
2. 选择你的 `.clip` 文件。
3. Blender 会从这个文件创建一张 generated image。
4. 图片会被打包进 `.blend`。
5. 在 Clip Studio Paint 里修改并保存 `.clip` 后，Blender 可以自动重新加载。
6. 你也可以在 Image Editor 侧边栏点击 `Manual Reload` 手动刷新。

如果之后找不到原始 `.clip` 文件，Blender 仍然会显示上一次已经打包好的画面。

### 当前状态

包版本：`0.8.67`。

现在可以处理：

- 栅格图层和全彩图像。
- 纸张/背景图层。
- 文件夹和裁剪图层。
- 图层蒙版和透明度。
- 常见混合模式。
- 当前测试文件里用到的调整图层和滤镜图层。
- 手动重新加载和非阻塞自动重新加载。
- 把图片结果保存进 `.blend`。
- 缺失源文件、渲染错误、pack 状态等清晰提示。
- 简体中文、日文和西班牙文 UI 翻译。

目前不做：

- 导入可编辑图层。
- 矢量线条或填充。
- 文字图层。
- 对话框/分镜框渲染器。
- 3D 图层或动画时间轴。
- 把修改写回 `.clip` 文件。

### 构建插件包

先构建 native 渲染器：

```powershell
cd native\rust
cargo build --release -q -p clip_capi
cargo build --release -q -p clip_cli
cd ..\..
```

再构建可安装扩展 zip：

```powershell
python tools\build_blender_addon.py
```

脚本会写出 `clip_studio_importer.zip`。这个 zip 包含 Blender 扩展文件、
`LICENSE`、`NOTICE.md`，以及 `native/` 下的 native 渲染器文件。

如果想让 Blender 执行最终的 extension build，可以传入 Blender 可执行文件：

```powershell
python tools\build_blender_addon.py --blender "C:\Program Files\Blender Foundation\Blender 4.2\blender.exe"
```

### 项目结构

- `clip_studio_importer/` - Blender 扩展源码。
- `clip_studio_importer.zip` - 可安装扩展包。
- `native/rust/` - native 渲染器 workspace。
- `docs/AI_MEMORY.md` - 当前项目状态摘要。
- `docs/analysis.md` - 研究笔记和历史发现。
- `docs/design.md` - Blender 用户体验说明。
- `docs/plan.md` - 长期方向和 open work。

### 路线图

- 可安装扩展继续走 native renderer 路径。
- 继续提升复杂 `.clip` 文件的视觉还原度。
- 继续优化重新加载和渲染性能。
